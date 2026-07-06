import GLib from 'gi://GLib';
import Meta from 'gi://Meta';
import Shell from 'gi://Shell';
import {
    InjectionManager,
} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Background from 'resource:///org/gnome/shell/ui/background.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as Workspace from 'resource:///org/gnome/shell/ui/workspace.js';
import * as WorkspaceThumbnail from 'resource:///org/gnome/shell/ui/workspaceThumbnail.js';
import {LiveWallpaper} from './wallpaper.js';

function logDebug(message) {
    console.log(`[we-layerd][gnome-window-bridge-v3-hanabi-port] ${message}`);
}

export class GnomeShellOverride {
    constructor(matchWindow, isWallpaperActive) {
        this._injectionManager = new InjectionManager();
        this._matchWindow = matchWindow;
        this._isWallpaperActive = isWallpaperActive;
        this._wallpaperActors = new Set();
    }

    enable() {
        logDebug('gnomeShellOverride.enable()');
        const matchWindow = this._matchWindow;
        const isWallpaperActive = this._isWallpaperActive;
        const self = this;

        this._injectionManager.overrideMethod(
            Background.BackgroundManager.prototype,
            '_createBackgroundActor',
            originalMethod => {
                return function () {
                    const backgroundActor = originalMethod.call(this);
                    if (!isWallpaperActive())
                        return backgroundActor;

                    const wallpaper = new LiveWallpaper(backgroundActor, matchWindow);
                    this.videoActor = wallpaper;
                    self._wallpaperActors.add(wallpaper);
                    wallpaper.connect('destroy', actor => {
                        self._wallpaperActors.delete(actor);
                        if (this.videoActor === actor)
                            this.videoActor = null;
                    });
                    return backgroundActor;
                };
            }
        );

        this._injectionManager.overrideMethod(
            Shell.Global.prototype,
            'get_window_actors',
            originalMethod => {
                return function (hideManaged = true) {
                    const windowActors = originalMethod.call(this);
                    if (!hideManaged)
                        return windowActors;
                    return windowActors.filter(actor => !actor?.meta_window || !matchWindow(actor.meta_window));
                };
            }
        );

        this._injectionManager.overrideMethod(
            Workspace.Workspace.prototype,
            '_isOverviewWindow',
            originalMethod => {
                return function (window) {
                    if (matchWindow(window))
                        return false;
                    return originalMethod.apply(this, [window]);
                };
            }
        );

        this._injectionManager.overrideMethod(
            WorkspaceThumbnail.WorkspaceThumbnail.prototype,
            '_isOverviewWindow',
            originalMethod => {
                return function (window) {
                    if (matchWindow(window))
                        return false;
                    return originalMethod.apply(this, [window]);
                };
            }
        );

        this._injectionManager.overrideMethod(
            Meta.Display.prototype,
            'get_tab_list',
            originalMethod => {
                return function (type, workspace) {
                    const metaWindows = originalMethod.apply(this, [type, workspace]);
                    return metaWindows.filter(metaWindow => !matchWindow(metaWindow));
                };
            }
        );

        this._injectionManager.overrideMethod(
            Shell.WindowTracker.prototype,
            'get_window_app',
            originalMethod => {
                return function (window) {
                    if (matchWindow(window))
                        return null;
                    return originalMethod.apply(this, [window]);
                };
            }
        );

        this._injectionManager.overrideMethod(
            Shell.App.prototype,
            'get_windows',
            originalMethod => {
                return function () {
                    const metaWindows = originalMethod.call(this);
                    return metaWindows.filter(metaWindow => !matchWindow(metaWindow));
                };
            }
        );

        this._injectionManager.overrideMethod(
            Shell.App.prototype,
            'get_n_windows',
            _originalMethod => {
                return function () {
                    return this.get_windows().length;
                };
            }
        );

        this._injectionManager.overrideMethod(
            Shell.AppSystem.prototype,
            'get_running',
            originalMethod => {
                return function () {
                    const runningApps = originalMethod.call(this);
                    return runningApps.filter(app => app.get_n_windows() > 0);
                };
            }
        );
    }

    disable() {
        logDebug('gnomeShellOverride.disable()');
        for (const actor of this._wallpaperActors)
            actor.destroy();
        this._wallpaperActors.clear();
        this._injectionManager.clear();
    }

    reloadBackgrounds() {
        for (const actor of this._wallpaperActors)
            actor.destroy();
        this._wallpaperActors.clear();

        const laters = this._getLaters();
        if (!laters)
            return;

        laters.add(Meta.LaterType.BEFORE_REDRAW, () => {
            try {
                Main.layoutManager._updateBackgrounds();
            } catch (error) {
                console.error(error);
            }

            try {
                Main.screenShield?._dialog?._updateBackgrounds?.();
            } catch (error) {
                console.error(error);
            }

            try {
                Main.overview?._overview?._controls?._workspacesDisplay?._updateWorkspacesViews?.();
            } catch (error) {
                console.error(error);
            }

            return GLib.SOURCE_REMOVE;
        });
    }

    _getLaters() {
        if (global.compositor?.get_laters)
            return global.compositor.get_laters();
        if (Meta.Laters?.get)
            return Meta.Laters.get();
        return null;
    }
}
