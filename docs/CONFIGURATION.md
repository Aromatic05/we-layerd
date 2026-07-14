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
- `renderer.source`: workshop item directory, not a video file and not a Wallpaper Engine CLI argument list
- `renderer.assets_path`: Wallpaper Engine `assets/` directory

## General settings

```toml
[general]
interactive = true
show_fps = false
fps_report_interval_secs = 1
scale_mode = "cover"
```

- backend selection is automatic: GNOME sessions use the GNOME actor-clone path; other desktops use layer-shell
- `interactive`: when `false`, `we-layerd` sets an empty input region so the wallpaper does not consume pointer input
- `show_fps`: keeps the renderer FPS counters enabled in config/status
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
- `speed`, `volume`, `muted`: source parameters forwarded directly to the renderer ABI
- `options_json`: optional raw JSON string forwarded to `we_renderer::Source.options_json`; invalid JSON stops runtime startup with a clear error

Current DMA-BUF scope:

- the layer-shell backend reads linux-dmabuf v4 surface feedback and falls back to v3 global modifier events
- all advertised `(fourcc, modifier)` pairs are forwarded to `wallpaper-engine-renderer`
- scene, video, and web outputs negotiate against the same compositor capability set
- v4 feedback changes are forwarded while the renderer is running, allowing output rebinding or SHM fallback
- `prefer_dmabuf` selects policy; `allow_shm_fallback` controls behavior when no compatible pair exists

## Library search order

If `renderer.library_path` is empty or does not resolve to an existing file, `we-layerd` falls back to these locations:

1. `~/.local/lib/libwallpaper-engine-renderer.so`
2. `/usr/lib/libwallpaper-engine-renderer.so`
3. `../lib/libwallpaper-engine-renderer.so` relative to the executable

## GUI output

`we-gui` generates renderer-native config only:

- it writes `renderer.source` as the selected workshop item directory
- it derives `renderer.assets_path` from the Wallpaper Engine install path
- it preserves `renderer.options_json` when re-saving an existing config
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
