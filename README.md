# we-layerd

`we-layerd` is a Rust daemon for running Wallpaper Engine wallpapers on Wayland through the native `wallpaper-engine-renderer` library.

## What it does

- Builds and uses `wallpaper-engine-renderer` as a git submodule
- Presents renderer output through Wayland layer-shell
- Supports DMA-BUF and SHM frame presentation
- Forwards pointer input to interactive wallpapers
- Runs one renderer session per output
- Ships a GUI companion, `we-gui`, for workshop browsing and config generation

## Build

The repository vendors `wallpaper-engine-renderer` as a git submodule:

```bash
git submodule update --init --recursive
cargo build --workspace
```

### Arch Linux packages

For this repository itself:

```bash
sudo pacman -S --needed \
  rustup gcc cmake pkgconf git \
  wayland wayland-protocols libxkbcommon \
  gtk3
```

For the bundled `wallpaper-engine-renderer` submodule build:

```bash
sudo pacman -S --needed \
  vulkan-headers vulkan-icd-loader mesa libglvnd \
  gstreamer gst-plugins-base-libs \
  lz4 pango fontconfig freetype2 \
  directx-shader-compiler
```

Notes:

- `BUILD_WEWEB` is left at the upstream default. With the current upstream default, CEF is not required.
- `gtk3` is needed by the current tray/GUI stack on Linux.
- `directx-shader-compiler` satisfies the upstream DXC probe used by the renderer submodule.

During `cargo build`, `we-layerd` also builds the upstream renderer library from:

```text
third_party/wallpaper-engine-renderer
```

and stages it in both of these locations:

```text
target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so
~/.local/bin/lib/libwallpaper-engine-renderer.so
```

`we-layerd` and `we-gui` prefer `~/.local/bin/lib/libwallpaper-engine-renderer.so`, then the staged `target/...` copy, then standard system library paths.

## Install binaries

```bash
install -Dm755 target/release/we-layerd ~/.local/bin/we-layerd
install -Dm755 target/release/we-gui ~/.local/bin/we-gui
```

## Config

Start from:

```bash
cp config.example.toml ~/.config/we-layerd/config.toml
```

The important paths are:

- `renderer.library_path`
- `renderer.source`
- `renderer.assets_path`
- `renderer.cache_path`

See [docs/CONFIGURATION.md](./docs/CONFIGURATION.md) for the config model.

## Runtime commands

```bash
we-layerd ctl stop
we-layerd ctl pause
we-layerd ctl resume
we-layerd ctl reload
we-layerd ctl status
```

## More docs

- [docs/CONFIGURATION.md](./docs/CONFIGURATION.md)
- [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md)
- [docs/TROUBLESHOOTING.md](./docs/TROUBLESHOOTING.md)
