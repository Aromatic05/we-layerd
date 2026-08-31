# Configuration

Start from:

```bash
cp config.example.toml ~/.config/we-layerd/config.toml
```

## Required fields

```toml
[renderer]
library_path = ""
source = "/path/to/Steam/steamapps/workshop/content/431960/<wallpaper-id>"
assets_path = "/path/to/Steam/steamapps/common/wallpaper_engine/assets"
```

- `renderer.library_path`: leave it empty for automatic lookup, or set an explicit path to `libwallpaper-engine-renderer.so`
- `renderer.source`: workshop item directory used as the single-output/default fallback. It may be
  empty when layer-shell output bindings provide every wallpaper/playlist the daemon should run.
  It is a directory, not a video file and not a Wallpaper Engine CLI argument list.
- `renderer.assets_path`: Wallpaper Engine `assets/` directory

## General settings

```toml
[general]
interactive = true
show_fps = false
fps_report_interval_secs = 1
scale_mode = "cover"
force_scene_audio_loop = false
```

- backend selection is automatic: GNOME sessions use the GNOME actor-clone path; other desktops use layer-shell
- `interactive`: when `false`, `we-layerd` sets an empty input region so the wallpaper does not consume pointer input
- `show_fps`: keeps the renderer FPS counters enabled in config/status
- `force_scene_audio_loop`: opt-in override that loops visible, automatically started scene sounds authored as `single`; start-silent sounds and `random` playback are unchanged. The daemon merges this value into `scene.audio.forceLoop` without replacing other version-1 source options.
- `scale_mode`: `fit`, `cover`, or `stretch`
  - `stretch`: fill the logical surface and allow non-uniform scaling
  - `cover`: fill the logical surface and crop the source region when buffer and viewport aspects differ
  - `fit`: preserve aspect ratio and shrink the viewport destination when needed
  - current `fit` limitation: the runtime uses one layer-surface plus `wp_viewporter`, so any empty area stays on the bottom/right edges instead of being centered like a full letterbox compositor scene

## Renderer settings

```toml
[renderer]
cache_path = "~/.cache/we-layerd/renderer"
prefer_dmabuf = true
allow_shm_fallback = true
fps = 60
speed = 1.0
volume = 1.0
muted = false
msaa_samples = 1
options_json = '''
{
  "example": true
}
'''
```

- `cache_path`: renderer cache directory
- `prefer_dmabuf`: prefer DMA-BUF buffers when the current simple presenter path can use them
- `allow_shm_fallback`: allow SHM presentation when DMA-BUF is unavailable or explicitly disabled
- `fps`: target update rate passed to the renderer
- `max_fps`: global upper bound for all wallpaper profiles; per-wallpaper `fps` values are clamped to it
- `speed`, `volume`, `muted`: source parameters forwarded directly to the renderer ABI
- `msaa_samples`: final-output MSAA request. `1` disables MSAA. Values above 1 are supported by
  scene wallpapers only; the renderer resolves unsupported sample counts downward to the highest
  supported count and reports the decision through renderer diagnostics. Video and web sources
  reject values above 1 instead of silently ignoring them.
- `options_json`: optional raw JSON string forwarded to `we_renderer::Source.options_json`; invalid JSON stops runtime startup with a clear error

The GUI stores MSAA per wallpaper and exposes common 1x/2x/4x/8x choices only for scene
wallpapers. For video and web wallpapers it shows the setting as unsupported. Renderer diagnostics
are collected outside the frame hot path, exposed by `we-layerd ctl status`, and shown separately in
the GUI Settings panel. Loading an older renderer library that lacks the diagnostics ABI does not
prevent playback; status reports diagnostics as unavailable instead.

## Host integrations and application rules

```toml
[integrations]
media = true
audio_spectrum = false
audio_source = "@DEFAULT_MONITOR@"
audio_sample_rate = 48000
audio_update_hz = 30

[rules]
focused = "keep"
maximized = "keep"
fullscreen = "pause"
```

- `media` reads MPRIS players from the user session bus and forwards playback state plus track,
  artist, album and genre metadata to compatible scene wallpapers. Playing players are preferred.
- `audio_spectrum` captures stereo float PCM from a PulseAudio source and exports the renderer
  contract `[left 64 bins][right 64 bins]`. PipeWire's PulseAudio compatibility works directly;
  `@DEFAULT_MONITOR@` follows the current default sink monitor. This is opt-in because capture is
  continuous. Scene and web wallpapers can consume it; video wallpapers do not receive it.
- window rules use `keep`, `mute`, or `pause`, are evaluated per output, and rely on
  `zwlr_foreign_toplevel_manager_v1` for focused/maximized/fullscreen state.
- manual Pause/Resume and rule pause are independent. Removing a rule condition never resumes a
  wallpaper that the user manually paused.
- missing MPRIS/Pulse/foreign-toplevel capabilities are reported through status and do not stop
  wallpaper playback.

`we-layerd ctl status` includes `[integration_runtime]` with collector availability/errors. Output
runtime diagnostics report media/audio renderer capability and current rule mute state. Collectors
run outside the frame loop, and changing integration/rule settings does not recreate output workers.

Current DMA-BUF scope:

- the layer-shell backend reads linux-dmabuf v4 surface feedback and falls back to v3 global modifier events
- all advertised `(fourcc, modifier)` pairs are forwarded to `wallpaper-engine-renderer`
- scene, video, and web outputs negotiate against the same compositor capability set
- v4 feedback changes are forwarded while the renderer is running, allowing output rebinding or SHM fallback
- `prefer_dmabuf` selects policy; `allow_shm_fallback` controls behavior when no compatible pair exists

## Multi-output layer-shell runtime

Wayland outputs are addressed by the stable `wl_output.name` value exposed by protocol version 4.
Each configured output owns an independent layer surface, renderer session, buffer/presentation
state, and playlist cursor/timer. Reconfiguring or failing one output does not tear down the other
output workers.

```toml
[renderer]
# Optional fallback for discovered outputs without an explicit binding.
source = "/path/to/Steam/steamapps/workshop/content/431960/1111111111"

[outputs."DP-1"]
wallpaper_id = "2222222222"
source = "/path/to/Steam/steamapps/workshop/content/431960/2222222222"

[outputs."HDMI-A-1"]
playlist = "Daily"
```

An output may bind either one wallpaper (`wallpaper_id` plus `source`) or one named playlist, but
not both. If `renderer.source` is empty, discovered outputs without an explicit binding are left
unused. If a named output disappears, only that worker is stopped; when a new named output appears,
only the newly applicable worker is started. Invalid or failed output workers are reported through
status without stopping healthy outputs.

Per-output playlists use independent playback state. The global playlist commands remain available
for the legacy/shared playlist runtime; target a layer-shell output explicitly with:

```bash
we-layerd playlist play Daily --output DP-1
we-layerd playlist next --output DP-1
we-layerd playlist previous --output DP-1
we-layerd playlist stop --output DP-1
```

`we-layerd ctl status` keeps the legacy `[runtime]` / `[presentation]` form when exactly one output
worker is present. With multiple workers it reports independent
`[output_runtime."<name>".runtime]` and `[output_runtime."<name>".presentation]` tables, including
each output's source and playlist cursor.

## Playlists

Named playlists are part of the daemon configuration, so timing and progression do not depend on
the GUI process. Entries are ordered references to Workshop item directories and may repeat the
same wallpaper more than once.

```toml
[playlists]
active = "Daily"

[playlists.definitions.Daily]
mode = "repeat"
default_duration_ms = 1800000

[[playlists.definitions.Daily.items]]
wallpaper_id = "1234567890"
source = "/path/to/Steam/steamapps/workshop/content/431960/1234567890"

[[playlists.definitions.Daily.items]]
wallpaper_id = "9876543210"
source = "/path/to/Steam/steamapps/workshop/content/431960/9876543210"
duration_ms = 300000
```

Modes are `sequential`, `repeat`, `shuffle`, and `manual`. Sequential playback stops progressing
after the final entry, repeat wraps in order, shuffle uses a bag so an entry is not reused until the
current bag is exhausted, and manual only changes through explicit next/previous control. Missing
Workshop items are skipped rather than terminating a repeat/shuffle playlist.

Playlist runtime state is stored separately from the renderer configuration. On daemon restart the
current playlist entry and shuffle history are restored, while that entry's timer restarts from zero.

### GUI playlist workflow

`we-gui` exposes the same daemon playlist model from the **Playlists** sidebar. It can create,
rename, and delete named playlists; append wallpapers directly from the library; preserve duplicate
entries; reorder or remove entries; set playback mode; and set either a playlist-wide duration or an
entry-specific duration override. Play, previous, next, and stop actions are sent to the running
daemon rather than implemented by a GUI timer.

Playlist edits are written without replacing unrelated configuration sections. Output bindings are
updated atomically with playlist edits when required. When an edited playlist is currently running,
the GUI asks the daemon to reload the updated configuration. Global playlist stop clears
`playlists.active` on disk; per-output playlist control acts on the selected output worker. Manually
playing a single wallpaper deactivates the global playlist only in legacy/single-output mode; in
multi-output mode it changes only the selected output bindings.

Older `we-gui` random-playback preferences are migration-only. On the first library scan after this
version, an enabled legacy shuffle configuration is converted once into a `Migrated shuffle`
playlist if no playlist definitions already exist. The old source-type filters and interval become
that playlist's entries and default duration; the GUI no longer runs its own shuffle timer.

## Wallpaper applied hook

Use an optional command hook to integrate theme generators such as DMS/Matugen without making
`we-layerd` depend on a specific desktop shell:

```toml
[hooks.wallpaper_applied]
command = "~/.local/bin/we-theme-sync"
args = []
```

The command starts asynchronously once the first frame has been submitted for each successful
output startup or wallpaper switch. Multi-output workers are tracked independently, so the first
presented frame on DP-1 does not suppress the hook for HDMI-A-1. Hook startup or exit failures are
logged and do not stop the wallpaper runtime. `command` is executed directly, not through a shell;
put pipelines, redirection, or other shell syntax in an executable script instead.

The hook inherits the daemon environment and receives:

- `WE_LAYERD_EVENT=wallpaper_applied`
- `WE_LAYERD_SOURCE`: the active workshop item directory with `~` expanded
- `WE_LAYERD_BACKEND`: the active backend name
- `WE_LAYERD_GENERATION`: the runtime generation number

## Library search order

If `renderer.library_path` is empty or does not resolve to an existing file, `we-layerd` falls back to these locations:

1. `~/.local/lib/libwallpaper-engine-renderer.so`
2. `/usr/lib/libwallpaper-engine-renderer.so`
3. `../lib/libwallpaper-engine-renderer.so` relative to the executable

## GUI output

`we-gui` generates renderer-native config only:

- in legacy/single-output mode it writes `renderer.source` as the selected workshop item directory
- in multi-output mode it patches wallpaper profiles and `[outputs]` bindings while preserving the
  global `renderer.source` fallback
- it derives `renderer.assets_path` from the Wallpaper Engine install path
- it preserves `renderer.options_json` when re-saving an existing config
- it preserves `hooks` when generating the selected wallpaper config
- it merges scene user properties and the optional audio-loop override without discarding other `renderer.options_json` fields
- it refuses invalid JSON, non-object scene/audio containers, and unsupported options versions instead of overwriting them
- it no longer generates Wine, Proton, X11 capture, video-native, or `openWallpaper` arguments

## GUI language

`we-gui` starts in English. Choose **Settings → Language → 简体中文** to switch the
window and Linux tray menu immediately. The selection is stored separately from the
renderer configuration at `$XDG_CONFIG_HOME/we-layerd/gui.toml` (or
`~/.config/we-layerd/gui.toml` when `XDG_CONFIG_HOME` is unset):

```toml
language = "zh-Hans"
```

Supported BCP 47 language tags are `en` and `zh-Hans`. A missing, malformed, or
unsupported value falls back to English. The GUI writes this file atomically with a
same-directory temporary file and rename; it does not modify renderer settings when
the language changes.

Run the localization, preference persistence, and headless state tests with:

```bash
cargo test -p we-gui
```
