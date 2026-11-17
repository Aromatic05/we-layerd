import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {GnomeShellOverride} from './gnomeShellOverride.js';
import {WindowManager} from './windowManager.js';

const BUS_NAME = 'io.github.weLayerd.Gnome';
const OBJECT_PATH = '/io/github/weLayerd/Gnome';
const EXTENSION_RUNTIME_VERSION = 'gnome-window-bridge-v6-output-windows';
const PROTOCOL_XML_PATH = 'protocol/io.github.weLayerd.Gnome.xml';

function logDebug(message) {
    console.log(`[we-layerd][${EXTENSION_RUNTIME_VERSION}] ${message}`);
}

function loadProtocolXml(dir) {
    const file = dir.get_child(PROTOCOL_XML_PATH);
    const [, bytes] = file.load_contents(null);
    return new TextDecoder().decode(bytes);
}

// Renderer registrations outlive enable()/disable(). The extension declares
// the unlock-dialog session mode so it normally stays enabled while locked,
// but it can still be disabled and re-enabled (Extensions app, shell updates,
// older shells), and the daemon registers its windows only once. The
// module stays loaded for the life of the shell, so keeping the map here lets
// the extension take its existing renderer windows back after unlocking
// instead of leaving them as ordinary application windows.
const registeredTargets = new Map();

export default class WeLayerdExtension extends Extension {
    enable() {
        // Drop registrations whose renderer process exited while disabled.
        for (const [id, target] of registeredTargets) {
            if (target.pid > 0 && !GLib.file_test(`/proc/${target.pid}`, GLib.FileTest.EXISTS))
                registeredTargets.delete(id);
        }
        logDebug(`enable() restoring ${registeredTargets.size} renderer registration(s)`);
        this._targets = registeredTargets;
        this._override = new GnomeShellOverride(
            metaWindow => this._shouldHideWindow(metaWindow),
            () => this._targets.size > 0,
            metaWindow => this._monitorForWindow(metaWindow)
        );
        this._windowManager = new WindowManager(
            metaWindow => this._shouldManageWindow(metaWindow),
            metaWindow => this._placementForWindow(metaWindow)
        );
        this._override.enable();
        this._windowManager.enable();
        // Coming back from the lock screen with restored registrations: the
        // desktop, overview and Blur my Shell backgrounds lost their live
        // wallpaper when we were disabled, and the daemon will not register
        // again to trigger a rebuild. Without this the overview backdrop shows
        // the static GNOME background while only workspace previews (created
        // fresh each time) show the live wallpaper.
        if (this._targets.size > 0)
            this._override.reloadBackgrounds();
        // Re-pin every renderer window when monitors are added, removed,
        // rotated, or rearranged.
        this._monitorsChangedId = Main.layoutManager.connect('monitors-changed', () => {
            if (this._override?.emittingSyntheticMonitorsChanged)
                return;
            this._windowManager?.refreshPlacements();
            this._override?.reloadBackgrounds();
        });

        this._dbusImpl = Gio.DBusExportedObject.wrapJSObject(loadProtocolXml(this.dir), this);
        this._dbusConnection = Gio.bus_get_sync(Gio.BusType.SESSION, null);
        this._dbusImpl.export(this._dbusConnection, OBJECT_PATH);
        this._dbusNameId = Gio.bus_own_name_on_connection(
            this._dbusConnection,
            BUS_NAME,
            Gio.BusNameOwnerFlags.REPLACE,
            null,
            null
        );
    }

    disable() {
        logDebug('disable()');
        if (this._monitorsChangedId) {
            Main.layoutManager.disconnect(this._monitorsChangedId);
            this._monitorsChangedId = 0;
        }
        this._windowManager?.disable();
        this._windowManager = null;
        this._override?.disable();
        this._override = null;

        if (this._dbusImpl) {
            this._dbusImpl.unexport();
            this._dbusImpl = null;
        }
        if (this._dbusNameId) {
            Gio.bus_unown_name(this._dbusNameId);
            this._dbusNameId = 0;
        }
        this._dbusConnection = null;

        // Keep registeredTargets: see the comment above the class.
        this._targets = null;
    }

    Ping() {
        return `${EXTENSION_RUNTIME_VERSION};metadata=5`;
    }

    // Legacy single-window registration. The window is cloned into whichever
    // monitor Mutter reports for it and is not pinned to a monitor.
    RegisterWindow(xid, pid, title, wmClass) {
        logDebug(`RegisterWindow(xid=0x${xid.toString(16)}, pid=${pid}, title=${title ?? ''}, wmClass=${wmClass ?? ''})`);
        return this._registerTarget(xid, pid, title, wmClass, '');
    }

    // One renderer window per output: the window is pinned to the monitor
    // driving `connector` and cloned into that monitor's background only.
    RegisterOutputWindow(id, pid, title, wmClass, connector) {
        logDebug(`RegisterOutputWindow(id=${id}, pid=${pid}, title=${title ?? ''}, wmClass=${wmClass ?? ''}, connector=${connector ?? ''})`);
        return this._registerTarget(id, pid, title, wmClass, connector);
    }

    UnregisterWindow(xid) {
        logDebug(`UnregisterWindow(xid=0x${xid.toString(16)})`);
        if (!this._targets?.delete(xid))
            return false;

        this._windowManager?.refreshMatches();
        this._override?.reloadBackgrounds();
        return true;
    }

    _registerTarget(id, pid, title, wmClass, connector) {
        this._targets.set(id, {
            xid: id,
            pid,
            title: title ?? '',
            wmClass: wmClass ?? '',
            connector: connector ?? '',
        });

        this._windowManager?.refreshMatches();
        this._windowManager?.refreshPlacements();
        this._override?.reloadBackgrounds();
        return true;
    }

    StartVideo() {}

    StopVideo() {}

    PauseVideo() {}

    ResumeVideo() {}

    // Reports (output, activated, maximized, fullscreen) for every regular
    // toplevel so the daemon can drive its pause/mute window rules on GNOME,
    // which lacks the wlr foreign-toplevel protocol.
    GetWindowStates() {
        const states = [];
        if (!this._targets?.size)
            return states;
        // Apps are hidden behind the lock screen, which shows the live
        // wallpaper: no window should pause or mute it while locked.
        if (Main.sessionMode?.isLocked)
            return states;

        const focusWindow = global.display.get_focus_window();
        const workspace = global.workspace_manager.get_active_workspace();
        const monitorManager = global.backend?.get_monitor_manager?.() ?? null;
        const logicalMonitors = monitorManager?.get_logical_monitors?.() ?? [];
        // Meta.Window.get_monitor() reports a logical monitor index. Resolve
        // its connector through the logical monitor's primary physical
        // monitor — shell Monitor objects carry no connector property.
        const connectorFor = index => {
            const logical =
                logicalMonitors.find(monitor => monitor.get_number?.() === index) ??
                logicalMonitors[index];
            return logical?.get_monitors?.()?.[0]?.get_connector?.() ?? '';
        };
        // Borderless "fullscreen" games and players never set the fullscreen
        // state; they just size an undecorated window to the whole monitor.
        // Report any window whose frame covers its monitor as fullscreen.
        const coversMonitor = win => {
            const frame = win.get_frame_rect?.();
            const monitor = global.display.get_monitor_geometry?.(win.get_monitor());
            if (!frame || !monitor || monitor.width <= 0 || monitor.height <= 0)
                return false;
            return frame.x <= monitor.x && frame.y <= monitor.y &&
                frame.x + frame.width >= monitor.x + monitor.width &&
                frame.y + frame.height >= monitor.y + monitor.height;
        };

        for (const win of global.display.list_all_windows()) {
            if (!win || this._matches(win))
                continue;
            if (![Meta.WindowType.NORMAL, Meta.WindowType.DIALOG, Meta.WindowType.MODAL_DIALOG]
                .includes(win.get_window_type?.()))
                continue;
            if (!win.located_on_workspace(workspace) || !win.showing_on_its_workspace?.())
                continue;

            states.push([
                connectorFor(win.get_monitor()),
                win === focusWindow,
                Boolean(win.maximized_horizontally && win.maximized_vertically),
                Boolean(win.is_fullscreen?.() ?? win.fullscreen) || coversMonitor(win),
            ]);
        }
        return states;
    }

    _shouldManageWindow(metaWindow) {
        return this._matches(metaWindow);
    }

    _shouldHideWindow(metaWindow) {
        return this._shouldManageWindow(metaWindow);
    }

    _matches(metaWindow) {
        return this._targetFor(metaWindow) !== null;
    }

    _targetFor(metaWindow) {
        if (!metaWindow || !this._targets)
            return null;
        for (const target of this._targets.values()) {
            if (this._matchesTarget(metaWindow, target))
                return target;
        }
        return null;
    }

    // Logical monitor index the window renders for: its registered connector
    // when known, otherwise wherever Mutter placed it (legacy registration).
    _monitorForWindow(metaWindow) {
        const target = this._targetFor(metaWindow);
        if (!target)
            return -1;
        if (target.connector) {
            const index = this._monitorIndexForConnector(target.connector);
            if (index >= 0)
                return index;
        }
        return metaWindow.get_monitor?.() ?? -1;
    }

    // Exact monitor rectangle a registered output window must occupy, or null
    // when the window has no connector or its monitor is not present.
    _placementForWindow(metaWindow) {
        const target = this._targetFor(metaWindow);
        if (!target?.connector)
            return null;
        const index = this._monitorIndexForConnector(target.connector);
        if (index < 0)
            return null;
        const rect = global.display.get_monitor_geometry(index);
        return {monitor: index, x: rect.x, y: rect.y, width: rect.width, height: rect.height};
    }

    _monitorIndexForConnector(connector) {
        const monitorManager = global.backend?.get_monitor_manager?.() ?? null;
        const direct = monitorManager?.get_monitor_for_connector?.(connector);
        if (typeof direct === 'number' && direct >= 0)
            return direct;

        for (const logical of monitorManager?.get_logical_monitors?.() ?? []) {
            const connectors = (logical.get_monitors?.() ?? []).map(monitor => monitor.get_connector?.());
            if (connectors.includes(connector))
                return logical.get_number?.() ?? -1;
        }
        return -1;
    }

    _matchesTarget(metaWindow, target) {
        const pid = metaWindow.get_pid?.() ?? 0;
        const title = metaWindow.get_title?.() ?? '';
        const wmClass = metaWindow.get_wm_class?.() ?? '';

        if (target.pid > 0 && pid !== target.pid)
            return false;
        if (target.title && title !== target.title)
            return false;
        if (target.wmClass && wmClass !== target.wmClass)
            return false;

        if (target.pid > 0 || target.title || target.wmClass)
            return true;

        if (metaWindow.get_description) {
            const description = metaWindow.get_description() ?? '';
            if (description.includes(`0x${target.xid.toString(16)}`))
                return true;
        }

        return false;
    }
}
