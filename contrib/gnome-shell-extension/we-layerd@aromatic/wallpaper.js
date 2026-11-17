import Clutter from 'gi://Clutter';
import GLib from 'gi://GLib';
import GObject from 'gi://GObject';
import Graphene from 'gi://Graphene';
import St from 'gi://St';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import {RoundedClipEffect, getRoundedClipBounds, watchRoundedClipBounds} from './roundedClipEffect.js';

const POLL_INTERVAL_MS = 1000;

export const LiveWallpaper = GObject.registerClass(
    class LiveWallpaper extends St.Widget {
        constructor(backgroundActor, matchWindow, monitorForWindow = metaWindow => metaWindow.get_monitor()) {
            super({
                reactive: false,
                // The clone is scaled to cover; never paint past our box.
                clip_to_allocation: true,
                x_expand: true,
                y_expand: true,
            });

            this._backgroundActor = backgroundActor;
            this._matchWindow = matchWindow;
            this._monitorForWindow = monitorForWindow;
            this._wallpaper = null;
            this._sourceDestroyId = 0;
            this._pollId = 0;
            this._roundedClip = null;

            // Mirror the rounded clip GNOME puts on this background's content
            // (workspace previews) so the clone gets the same rounded corners.
            const content = backgroundActor.content;
            this._unwatchClipBounds = content
                ? watchRoundedClipBounds(content, () => this._updateRoundedClip())
                : null;
            this._clipRadiusId = content?.connect('notify::rounded-clip-radius',
                () => this._updateRoundedClip()) ?? 0;

            backgroundActor.layout_manager = new Clutter.BinLayout();
            backgroundActor.add_child(this);

            this.connect('destroy', () => {
                this._unwatchClipBounds?.();
                this._unwatchClipBounds = null;
                if (this._clipRadiusId) {
                    this._backgroundActor.content?.disconnect(this._clipRadiusId);
                    this._clipRadiusId = 0;
                }
                if (this._pollId) {
                    GLib.source_remove(this._pollId);
                    this._pollId = 0;
                }
                this._disconnectCloneSource();
                if (this._wallpaper) {
                    this._wallpaper.destroy();
                    this._wallpaper = null;
                }
            });

            this._attachWhenReady();
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

        // Never influence the size of the background we decorate. GNOME's own
        // background actors report no natural size in workspace previews; the
        // clone would otherwise report the renderer window's full size and the
        // overview layout would widen every workspace preview to fit it.
        vfunc_get_preferred_width(_forHeight) {
            return [0, 0];
        }

        vfunc_get_preferred_height(_forWidth) {
            return [0, 0];
        }

        // Background actors are not always monitor-shaped: the app grid shrinks
        // the workspace background to a short strip. Scale the renderer clone
        // uniformly to cover the box (cropping the overflow) like GNOME's own
        // background content, instead of stretching it.
        vfunc_allocate(box) {
            this.set_allocation(box);
            this._updateRoundedClip();
            if (!this._wallpaper)
                return;

            const width = box.get_width();
            const height = box.get_height();
            const [sourceWidth, sourceHeight] = this._wallpaper.source?.get_size() ?? [0, 0];
            const childBox = new Clutter.ActorBox();
            if (sourceWidth > 0 && sourceHeight > 0 && width > 0 && height > 0) {
                const scale = Math.max(width / sourceWidth, height / sourceHeight);
                const childWidth = sourceWidth * scale;
                const childHeight = sourceHeight * scale;
                childBox.set_origin((width - childWidth) / 2, (height - childHeight) / 2);
                childBox.set_size(childWidth, childHeight);
            } else {
                childBox.set_origin(0, 0);
                childBox.set_size(width, height);
            }
            this._wallpaper.allocate(childBox);
        }

        _updateRoundedClip() {
            const content = this._backgroundActor.content;
            const bounds = content ? getRoundedClipBounds(content) : null;
            const monitor = Main.layoutManager.monitors[this._backgroundActor.monitor];
            const width = this.width;
            const height = this.height;
            if (!bounds || !monitor || width <= 0 || height <= 0) {
                if (this._roundedClip) {
                    this.remove_effect(this._roundedClip);
                    this._roundedClip = null;
                }
                return;
            }

            if (!this._roundedClip) {
                this._roundedClip = new RoundedClipEffect();
                this.add_effect(this._roundedClip);
            }
            // GNOME's bounds are in monitor coordinates; this actor spans the
            // whole (scaled) monitor, so map them per axis.
            const scaleX = width / monitor.width;
            const scaleY = height / monitor.height;
            const [x, y, clipWidth, clipHeight] = bounds;
            this._roundedClip.setBounds([
                x * scaleX,
                y * scaleY,
                (x + clipWidth) * scaleX,
                (y + clipHeight) * scaleY,
            ]);
            // The radius is in monitor pixels too; keep it proportionally
            // round on the (possibly scaled) preview actor.
            this._roundedClip.setClipRadius(
                (content.rounded_clip_radius ?? 0) * Math.min(scaleX, scaleY));
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
                // Match the window registered for this monitor's connector, not
                // the monitor Mutter happens to report for its current position.
                if (this._monitorForWindow(metaWindow) === monitorIndex)
                    return actor;
            }
            return null;
        }
    }
);
