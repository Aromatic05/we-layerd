import GLib from 'gi://GLib';

function logDebug(message) {
    console.log(`[we-layerd][gnome-window-bridge-v3-hanabi-port] ${message}`);
}

class ManagedWindow {
    constructor(window, placementFor = () => null) {
        this._window = window;
        this._placementFor = placementFor;
        this._placing = false;
        this._signals = [];
        this._disposed = false;
        this._lowerLaterId = 0;
        this._hiddenFromWindowList = false;

        this._signals.push(window.connect_after('shown', () => {
            if (this._disposed)
                return;
            this._refresh();
        }));

        // Clicking the desktop clicks this window, and Mutter raises it above
        // every other window. Lower it again inside the signal, before the next
        // frame is painted; a deferred lower let the wallpaper cover all windows
        // for a frame, which looked like the active window flashing.
        this._signals.push(window.connect_after('raised', () => {
            if (this._disposed)
                return;
            this._window.lower();
        }));

        this._signals.push(window.connect('notify::above', () => {
            if (this._disposed)
                return;
            if (this._window.above)
                this._window.unmake_above();
        }));

        this._signals.push(window.connect('notify::minimized', () => {
            if (this._disposed)
                return;
            if (this._window.minimized)
                this._window.unminimize();
        }));

        this._signals.push(window.connect('notify::fullscreen', () => {
            if (this._disposed)
                return;
            if (this._window.fullscreen)
                this._window.unmake_fullscreen();
            this._place();
        }));

        // Mutter places a non-fullscreen window wherever it likes (often the
        // top-left of the whole layout, spanning monitors). Keep each output
        // window pinned to its own monitor's exact rectangle.
        for (const signal of ['position-changed', 'size-changed']) {
            this._signals.push(window.connect(signal, () => {
                if (!this._disposed)
                    this._place();
            }));
        }

        this._refresh();
    }

    _refresh() {
        if (this._disposed || !this._window)
            return;

        // skip_from_window_list keeps the window mapped when the user toggles
        // "show desktop" (e.g. Super+D): mutter only hides regular windows.
        if (this._window.hide_from_window_list && this._window.show_in_window_list) {
            this._window.hide_from_window_list();
            this._hiddenFromWindowList = true;
        }
        this._window.unmake_above();
        if (this._window.fullscreen)
            this._window.unmake_fullscreen();
        this._window.stick();
        this._place();
        this._queueLower();
    }

    _place() {
        if (this._disposed || !this._window || this._placing)
            return;
        const placement = this._placementFor(this._window);
        if (!placement || !this._window.move_resize_frame)
            return;

        const frame = this._window.get_frame_rect?.();
        if (frame &&
            frame.x === placement.x && frame.y === placement.y &&
            frame.width === placement.width && frame.height === placement.height)
            return;

        this._placing = true;
        try {
            const maximized = this._window.is_maximized?.() ?? Boolean(this._window.get_maximized?.());
            if (maximized) {
                try {
                    this._window.unmaximize(); // Mutter 49+
                } catch {
                    this._window.unmaximize(3 /* Meta.MaximizeFlags.BOTH */);
                }
            }
            if (this._window.get_monitor?.() !== placement.monitor)
                this._window.move_to_monitor?.(placement.monitor);
            // user_op=true: normal windows are otherwise kept inside the work
            // area, which would push the renderer below the top bar.
            this._window.move_resize_frame(
                true, placement.x, placement.y, placement.width, placement.height);
        } finally {
            this._placing = false;
        }
    }

    refreshPlacement() {
        this._place();
    }

    _queueLower() {
        if (this._disposed || !this._window)
            return;

        if (this._lowerLaterId)
            GLib.source_remove(this._lowerLaterId);

        this._lowerLaterId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 50, () => {
            this._lowerLaterId = 0;
            if (!this._disposed && this._window)
                this._window.lower();
            return GLib.SOURCE_REMOVE;
        });
    }

    disconnect() {
        this._disposed = true;

        if (this._lowerLaterId) {
            GLib.source_remove(this._lowerLaterId);
            this._lowerLaterId = 0;
        }

        this._signals.forEach(signal => {
            this._window.disconnect(signal);
        });
        this._signals = [];
        if (this._hiddenFromWindowList) {
            this._window.show_in_window_list();
            this._hiddenFromWindowList = false;
        }
        this._window = null;
    }
}

export class WindowManager {
    constructor(matchWindow, placementFor = () => null) {
        this._matchWindow = matchWindow;
        this._placementFor = placementFor;
        this._windows = new Set();
        this._mapId = 0;
    }

    enable() {
        logDebug('windowManager.enable()');
        this._mapId = global.window_manager.connect_after('map', (_wm, actor) => {
            this._tryAddWindow(actor?.get_meta_window?.());
        });

        for (const actor of global.get_window_actors(false))
            this._tryAddWindow(actor.get_meta_window());
    }

    disable() {
        logDebug('windowManager.disable()');
        for (const window of this._windows)
            this._clearWindow(window);
        this._windows.clear();

        if (this._mapId) {
            global.window_manager.disconnect(this._mapId);
            this._mapId = 0;
        }
    }

    refreshMatches() {
        for (const actor of global.get_window_actors(false))
            this._tryAddWindow(actor.get_meta_window());

        for (const window of [...this._windows]) {
            if (!this._matchWindow(window)) {
                this._clearWindow(window);
                this._windows.delete(window);
            }
        }
    }

    refreshPlacements() {
        for (const window of this._windows)
            window.managed?.refreshPlacement();
    }

    _tryAddWindow(window) {
        if (!window || this._windows.has(window) || !this._matchWindow(window))
            return;

        logDebug(`windowManager.manage(title=${window.get_title?.() ?? ''}, pid=${window.get_pid?.() ?? 0})`);
        window.managed = new ManagedWindow(window, this._placementFor);
        this._windows.add(window);
        window.managed._unmanagedId = window.connect('unmanaged', unmanagedWindow => {
            this._clearWindow(unmanagedWindow);
            this._windows.delete(unmanagedWindow);
        });
    }

    _clearWindow(window) {
        if (!window?.managed)
            return;

        if (window.managed._unmanagedId)
            window.disconnect(window.managed._unmanagedId);
        window.managed.disconnect();
        window.managed = null;
    }
}
