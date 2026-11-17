import Clutter from 'gi://Clutter';
import Cogl from 'gi://Cogl';
import GObject from 'gi://GObject';

// GNOME draws workspace previews by giving its own Meta.BackgroundContent a
// rounded clip (set_rounded_clip_bounds + rounded_clip_radius). The live
// wallpaper clone is a sibling actor that the content clip does not reach, so
// it would paint square corners and spill into areas GNOME leaves empty. This
// effect applies the same rounded-rectangle clip to the clone.

const DECLARATIONS =
    'uniform vec2 actor_size; ' +
    'uniform vec4 clip_bounds; ' + // x1, y1, x2, y2 in actor pixels
    'uniform float clip_radius; ';

// Signed distance to a rounded rectangle. GNOME antialiases its own clip over
// dist -0.5..0.5, so the static desktop background underneath is partially
// visible there. Stay fully opaque through that fringe and fade just outside
// it (the extra pixel allows for scaled previews); otherwise the original
// background bleeds through as a thin coloured line on the corners.
const CODE =
    'vec2 p = cogl_tex_coord_in[0].xy * actor_size; ' +
    'vec2 center = (clip_bounds.xy + clip_bounds.zw) * 0.5; ' +
    'vec2 half_size = (clip_bounds.zw - clip_bounds.xy) * 0.5; ' +
    'float r = min(clip_radius, min(half_size.x, half_size.y)); ' +
    'vec2 q = abs(p - center) - (half_size - vec2(r)); ' +
    'float dist = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r; ' +
    'cogl_color_out *= clamp(2.0 - dist, 0.0, 1.0); ';

// Rounded clip bounds GNOME assigned to each background content, keyed by the
// content object. Meta.BackgroundContent has a setter but no readable property.
const clipBounds = new WeakMap();
const clipListeners = new WeakMap();

export function recordRoundedClipBounds(content, rect) {
    if (!content)
        return;
    if (rect)
        clipBounds.set(content, [rect.origin.x, rect.origin.y, rect.size.width, rect.size.height]);
    else
        clipBounds.delete(content);
    for (const listener of clipListeners.get(content) ?? [])
        listener();
}

export function getRoundedClipBounds(content) {
    return clipBounds.get(content) ?? null;
}

export function watchRoundedClipBounds(content, listener) {
    let listeners = clipListeners.get(content);
    if (!listeners) {
        listeners = new Set();
        clipListeners.set(content, listeners);
    }
    listeners.add(listener);
    return () => listeners.delete(listener);
}

export const RoundedClipEffect = GObject.registerClass(
class WeLayerdRoundedClipEffect extends Clutter.ShaderEffect {
    vfunc_get_static_snippet() {
        return Cogl.Snippet.new(Cogl.SnippetHook.FRAGMENT, DECLARATIONS, CODE);
    }

    // bounds: [x1, y1, x2, y2] in the actor's own pixel coordinates.
    setClip(width, height, bounds, radius) {
        this.set_uniform_float('actor_size', 2, [width, height]);
        this.set_uniform_float('clip_bounds', 4, bounds);
        this.set_uniform_float('clip_radius', 1, [radius]);
        this.queue_repaint();
    }
});
