import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Graphene from 'gi://Graphene';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';

const POLL_INTERVAL_MS = 1000;

export const LiveWallpaper = GObject.registerClass(
    class LiveWallpaper extends St.Widget {
        constructor(backgroundActor, matchWindow) {
            super({
                layout_manager: new Clutter.BinLayout(),
                width: backgroundActor.width,
                height: backgroundActor.height,
                reactive: false,
                x_expand: true,
                y_expand: true,
            });

            this._backgroundActor = backgroundActor;
            this._matchWindow = matchWindow;
            this._wallpaper = null;
            this._sourceDestroyId = 0;
            this._pollId = 0;
            this._workareasChangedId = global.display.connect('workareas-changed', () => {
                this._updateWorkareaClip();
            });

            backgroundActor.layout_manager = new Clutter.BinLayout();
            backgroundActor.add_child(this);
            this._updateWorkareaClip();
            this.connect('notify::allocation', () => this._updateWorkareaClip());

            this.connect('destroy', () => {
                if (this._pollId) {
                    GLib.source_remove(this._pollId);
                    this._pollId = 0;
                }
                if (this._workareasChangedId) {
                    global.display.disconnect(this._workareasChangedId);
                    this._workareasChangedId = 0;
                }
                this._disconnectCloneSource();
                if (this._wallpaper) {
                    this._wallpaper.destroy();
                    this._wallpaper = null;
                }
            });

            this._attachWhenReady();
        }

        _updateWorkareaClip() {
            const monitorIndex = this._backgroundActor.monitor;
            const monitor = Main.layoutManager.monitors[monitorIndex];
            const workarea = Main.layoutManager.getWorkAreaForMonitor(monitorIndex);
            if (!monitor || monitor.height <= 0 || this.height <= 0)
                return;

            // Keep the clone inside the same panel-excluded area used by
            // GNOME's workspace background, accounting for actor allocation
            // changes while Overview animates the workspace previews.
            const panelOffset = (workarea.y - monitor.y) /
                monitor.height * this._backgroundActor.height;
            const clipTop = Math.max(0, Math.min(this.height, panelOffset));
            this.set_clip(0, clipTop, this.width, this.height - clipTop);
        }

        _attachWhenReady() {
            const tryAttach = () => {
                const renderer = this._findRenderer();
                if (!renderer)
                    return true;

                this._disconnectCloneSource();
                this._wallpaper = new Clutter.Clone({
                    reactive: false,
                    source: renderer,
                    x_expand: true,
                    y_expand: true,
                    pivot_point: new Graphene.Point({x: 0.5, y: 0.5}),
                });
                this._sourceDestroyId = renderer.connect('destroy', () => {
                    this._disconnectCloneSource();
                    if (this._wallpaper) {
                        this._wallpaper.destroy();
                        this._wallpaper = null;
                    }
                    this._attachWhenReady();
                });
                this.add_child(this._wallpaper);
                return GLib.SOURCE_REMOVE;
            };

            if (tryAttach() === GLib.SOURCE_REMOVE)
                return;

            if (this._pollId)
                GLib.source_remove(this._pollId);
            this._pollId = GLib.timeout_add(GLib.PRIORITY_DEFAULT, POLL_INTERVAL_MS, () => {
                const result = tryAttach();
                if (result === GLib.SOURCE_REMOVE)
                    this._pollId = 0;
                return result;
            });
        }

        _disconnectCloneSource() {
            if (this._sourceDestroyId && this._wallpaper?.source) {
                this._wallpaper.source.disconnect(this._sourceDestroyId);
            }
            this._sourceDestroyId = 0;
        }

        _findRenderer() {
            const monitorIndex = this._backgroundActor.monitor;
            for (const actor of global.get_window_actors(false)) {
                const metaWindow = actor?.meta_window;
                if (!metaWindow || !this._matchWindow(metaWindow))
                    continue;
                if (metaWindow.get_monitor() === monitorIndex)
                    return actor;
            }
            return null;
        }
    }
);
