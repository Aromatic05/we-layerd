import Gio from 'gi://Gio';
import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import {GnomeShellOverride} from './gnomeShellOverride.js';
import {WindowManager} from './windowManager.js';

const BUS_NAME = 'io.github.weLayerd.Gnome';
const OBJECT_PATH = '/io/github/weLayerd/Gnome';
const EXTENSION_RUNTIME_VERSION = 'gnome-window-bridge-v3-hanabi-port';
const IFACE_XML = `
<node>
  <interface name="io.github.weLayerd.Gnome">
    <method name="Ping">
      <arg type="s" name="version" direction="out"/>
    </method>
    <method name="RegisterWindow">
      <arg type="u" name="xid" direction="in"/>
      <arg type="u" name="pid" direction="in"/>
      <arg type="s" name="title" direction="in"/>
      <arg type="s" name="wm_class" direction="in"/>
      <arg type="b" name="accepted" direction="out"/>
    </method>
    <method name="UnregisterWindow">
      <arg type="u" name="xid" direction="in"/>
      <arg type="b" name="removed" direction="out"/>
    </method>
  </interface>
</node>`;

function logDebug(message) {
    console.log(`[we-layerd][${EXTENSION_RUNTIME_VERSION}] ${message}`);
}

export default class WeLayerdExtension extends Extension {
    enable() {
        logDebug('enable()');
        this._target = null;
        this._override = new GnomeShellOverride(
            metaWindow => this._shouldHideWindow(metaWindow),
            () => this._target !== null
        );
        this._windowManager = new WindowManager(metaWindow => this._shouldManageWindow(metaWindow));
        this._override.enable();
        this._windowManager.enable();

        this._dbusImpl = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this);
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

        this._target = null;
    }

    Ping() {
        return `${EXTENSION_RUNTIME_VERSION};metadata=4`;
    }

    RegisterWindow(xid, pid, title, wmClass) {
        logDebug(`RegisterWindow(xid=0x${xid.toString(16)}, pid=${pid}, title=${title ?? ''}, wmClass=${wmClass ?? ''})`);
        this._target = {
            xid,
            pid,
            title: title ?? '',
            wmClass: wmClass ?? '',
        };

        this._windowManager?.refreshMatches();
        this._override?.reloadBackgrounds();
        return true;
    }

    UnregisterWindow(xid) {
        logDebug(`UnregisterWindow(xid=0x${xid.toString(16)})`);
        if (!this._target || this._target.xid !== xid)
            return false;

        this._target = null;
        this._windowManager?.refreshMatches();
        this._override?.reloadBackgrounds();
        return true;
    }

    _shouldManageWindow(metaWindow) {
        return this._matches(metaWindow);
    }

    _shouldHideWindow(metaWindow) {
        return this._shouldManageWindow(metaWindow);
    }

    _matches(metaWindow) {
        if (!metaWindow || !this._target)
            return false;

        const pid = metaWindow.get_pid?.() ?? 0;
        const title = metaWindow.get_title?.() ?? '';
        const wmClass = metaWindow.get_wm_class?.() ?? '';

        if (this._target.pid > 0 && pid !== this._target.pid)
            return false;
        if (this._target.title && title !== this._target.title)
            return false;
        if (this._target.wmClass && wmClass !== this._target.wmClass)
            return false;

        if (this._target.pid > 0 || this._target.title || this._target.wmClass)
            return true;

        if (metaWindow.get_description) {
            const description = metaWindow.get_description() ?? '';
            if (description.includes(`0x${this._target.xid.toString(16)}`))
                return true;
        }

        return false;
    }
}
