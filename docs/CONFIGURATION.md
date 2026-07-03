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
backend = "layer_shell"
interactive = true
show_fps = false
fps_report_interval_secs = 1
scale_mode = "cover"
```

- `backend`: currently `layer_shell`
- `interactive`: when `false`, `we-layerd` sets an empty input region so the wallpaper does not consume pointer input
- `show_fps`: keeps the renderer FPS counters enabled in config/status
- `scale_mode`: `fit`, `cover`, or `stretch`

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
```

- `cache_path`: renderer cache directory
- `prefer_dmabuf`: prefer DMA-BUF buffers when the compositor and renderer support them
- `allow_shm_fallback`: allow SHM presentation when DMA-BUF is unavailable
- `fps`: target update rate passed to the renderer
- `speed`, `volume`, `muted`: source parameters forwarded directly to the renderer ABI

## Library search order

If `renderer.library_path` is empty or does not resolve to an existing file, `we-layerd` falls back to these locations:

1. `~/.local/lib/libwallpaper-engine-renderer.so`
2. `/usr/lib/libwallpaper-engine-renderer.so`
3. `../lib/libwallpaper-engine-renderer.so` relative to the executable

## GUI output

`we-gui` generates renderer-native config only:

- it writes `renderer.source` as the selected workshop item directory
- it derives `renderer.assets_path` from the Wallpaper Engine install path
- it no longer generates Wine, Proton, X11 capture, video-native, or `openWallpaper` arguments
