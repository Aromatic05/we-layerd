function logDebug(message) {
    console.log(`[we-layerd][gnome-window-bridge-v3-hanabi-port] ${message}`);
}

class ManagedWindow {
    constructor(window) {
        this._window = window;
        this._signals = [];
        this._disposed = false;

        this._signals.push(window.connect_after('shown', () => {
            if (this._disposed)
                return;
            this._refresh();
        }));

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
            if (!this._window.minimized)
                this._window.minimize();
        }));

        this._refresh();
    }

    _refresh() {
        if (this._disposed || !this._window)
            return;

        this._window.unmake_above();
        // Keep the renderer out of Mutter's workspace window stack. GNOME's
        // BackgroundManager clones its compositor actor into each workspace.
        if (!this._window.minimized)
            this._window.minimize();
    }

    disconnect() {
        this._disposed = true;

        this._signals.forEach(signal => {
            this._window.disconnect(signal);
        });
        this._signals = [];
        this._window = null;
    }
}

export class WindowManager {
    constructor(matchWindow) {
        this._matchWindow = matchWindow;
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

    _tryAddWindow(window) {
        if (!window || this._windows.has(window) || !this._matchWindow(window))
            return;

        logDebug(`windowManager.manage(title=${window.get_title?.() ?? ''}, pid=${window.get_pid?.() ?? 0})`);
        window.managed = new ManagedWindow(window);
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
