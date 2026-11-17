// Copyright (C) 2026 Jeff Shee <jeffshee8969@gmail.com> and contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.
//
// SPDX-License-Identifier: GPL-3.0-or-later
//
// Adapted for we-layerd from the RoundedCornersEffect in the Hanabi GNOME
// Shell extension. The debug border and debounced logging were dropped; the
// shader and the Mutter-derived texture mapping are kept as-is.

import Cogl from 'gi://Cogl';
import GObject from 'gi://GObject';
import Shell from 'gi://Shell';

// Adapted from the Mutter project.
// See <https://gitlab.gnome.org/GNOME/mutter/-/blob/main/src/compositor/meta-background-content.c>.
const DECLARATIONS = `
uniform vec4 bounds;           // x, y: top left; z, w: bottom right
uniform float clip_radius;
uniform vec2 pixel_step;

float rounded_rect_coverage (vec2 p) {
    float center_left = bounds.x + clip_radius;
    float center_right = bounds.z - clip_radius;
    float center_x;

    if (p.x < center_left)
        center_x = center_left;
    else if (p.x > center_right)
        center_x = center_right;
    else
        return 1.0; // The vast majority of pixels exit early here

    float center_top = bounds.y + clip_radius;
    float center_bottom = bounds.w - clip_radius;
    float center_y;

    if (p.y < center_top)
        center_y = center_top;
    else if (p.y > center_bottom)
        center_y = center_bottom;
    else
        return 1.0;

    vec2 delta = p - vec2(center_x, center_y);
    float dist_squared = dot(delta, delta);

    // Fully outside the circle
    float outer_radius = clip_radius + 0.5;
    if (dist_squared >= outer_radius * outer_radius)
        return 0.0;

    // Fully inside the circle
    float inner_radius = clip_radius - 0.5;
    if (dist_squared <= inner_radius * inner_radius)
        return 1.0;

    // Only pixels on the edge of the curve need expensive antialiasing
    return outer_radius - sqrt(dist_squared);
}
`;

const CODE = `
vec2 texture_coord = cogl_tex_coord0_in.xy / pixel_step;

bool inside = texture_coord.x >= bounds.x && texture_coord.x <= bounds.z
           && texture_coord.y >= bounds.y && texture_coord.y <= bounds.w;

if (clip_radius > 0.0 && !inside)
    cogl_color_out = vec4(0.0);
else if (clip_radius > 0.0)
    cogl_color_out *= rounded_rect_coverage(texture_coord);
`;

// Port of mutter's _clutter_actor_box_enlarge_for_effects: the stable, padded
// quantization applied to the paint box before it becomes the offscreen
// texture. Returns the enlarged box origin.
function enlargeForEffects(x1, y1, width, height) {
    if (width <= 0 || height <= 0)
        return [x1, y1];
    const x2 = Math.ceil(x1 + width + 0.75);
    const y2 = Math.ceil(y1 + height + 0.75);
    return [x2 - Math.round(width) - 3, y2 - Math.round(height) - 3];
}

// Replicates ClutterOffscreenEffect's fbo_offset: the offscreen texture's
// origin in actor-local coordinates, typically (-2, -2). See
// clutter_offscreen_effect_pre_paint in clutter-offscreen-effect.c.
function getFboOffset(actor) {
    const volume = actor.get_paint_volume();
    if (volume) {
        const origin = volume.get_origin();
        const [x1, y1] = enlargeForEffects(
            origin.x,
            origin.y,
            volume.get_width(),
            volume.get_height()
        );
        return [Math.trunc(x1), Math.trunc(y1)];
    }
    const box = actor.get_allocation_box();
    const [x1, y1] = enlargeForEffects(
        box.x1,
        box.y1,
        box.x2 - box.x1,
        box.y2 - box.y1
    );
    return [Math.trunc(x1 - box.x1), Math.trunc(y1 - box.y1)];
}

export const RoundedClipEffect = GObject.registerClass(
    class WeLayerdRoundedClipEffect extends Shell.GLSLEffect {
        constructor() {
            super();
            this._bounds = [0, 0, 0, 0];
            this._clipRadius = 0;
            this._uniformsDirty = true;
            this._textureWidth = 0;
            this._textureHeight = 0;
            this._textureScale = 0;
            this._textureOffsetX = 0;
            this._textureOffsetY = 0;
        }

        vfunc_build_pipeline() {
            this.add_glsl_snippet(
                Cogl.SnippetHook.FRAGMENT,
                DECLARATIONS,
                CODE,
                false
            );
        }

        // The shader compares texture coordinates (converted to texture pixels
        // via pixel_step) against bounds/clip_radius, so those uniforms must be
        // expressed in pixels of the offscreen texture this effect renders
        // into. Mutter sizes that texture from the actor's paint box, not its
        // allocation, and the relation between the two varies across Mutter
        // versions and monitor scales. Instead of predicting the size, read
        // the actual texture at paint time and mirror Mutter's actor-to-texture
        // mapping: texture_px = (logical_px - fbo_offset) * resource_scale.
        vfunc_paint_target(node, paintContext) {
            this._updateUniforms();
            super.vfunc_paint_target(node, paintContext);
        }

        _updateUniforms() {
            const texture = this.get_texture();
            const actor = this.get_actor();
            if (!texture || !actor)
                return;

            const width = texture.get_width();
            const height = texture.get_height();
            if (width <= 0 || height <= 0)
                return;

            const scale = actor.get_resource_scale();
            const [offsetX, offsetY] = getFboOffset(actor);

            if (!this._uniformsDirty &&
                width === this._textureWidth &&
                height === this._textureHeight &&
                scale === this._textureScale &&
                offsetX === this._textureOffsetX &&
                offsetY === this._textureOffsetY)
                return;

            this._uniformsDirty = false;
            this._textureWidth = width;
            this._textureHeight = height;
            this._textureScale = scale;
            this._textureOffsetX = offsetX;
            this._textureOffsetY = offsetY;

            this.set_uniform_float(
                this.get_uniform_location('pixel_step'),
                2,
                [1.0 / width, 1.0 / height]
            );
            const [x1, y1, x2, y2] = this._bounds;
            this.set_uniform_float(
                this.get_uniform_location('bounds'),
                4,
                [
                    (x1 - offsetX) * scale,
                    (y1 - offsetY) * scale,
                    (x2 - offsetX) * scale,
                    (y2 - offsetY) * scale,
                ]
            );
            this.set_uniform_float(
                this.get_uniform_location('clip_radius'),
                1,
                [this._clipRadius * scale]
            );
        }

        // Bounds are actor-local logical pixels: [x1, y1, x2, y2].
        setBounds(bounds) {
            this._bounds = bounds;
            this._uniformsDirty = true;
        }

        setClipRadius(radius) {
            this._clipRadius = radius;
            this._uniformsDirty = true;
        }
    }
);

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
