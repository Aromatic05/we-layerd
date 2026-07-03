# we-layerd

`we-layerd` is a Rust daemon for running Wallpaper Engine wallpapers on Wayland through the native `wallpaper-engine-renderer` library.

## What it does

- Builds and stages `wallpaper-engine-renderer` from `../we-new/wallpaper-engine-renderer`
- Presents renderer output through Wayland layer-shell
- Supports DMA-BUF and SHM frame presentation
- Forwards pointer input to interactive wallpapers
- Runs one renderer session per output
- Ships a GUI companion, `we-gui`, for workshop browsing and config generation

## Build

`cargo build` expects the upstream renderer checkout at:

```text
../we-new/wallpaper-engine-renderer
```

and stages the renderer runtime under:

```text
target/we-renderer-upstream/install
```

Build the workspace with:

```bash
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

For the upstream `wallpaper-engine-renderer` build:

```bash
sudo pacman -S --needed \
  vulkan-headers vulkan-icd-loader mesa libglvnd \
  gstreamer gst-plugins-base-libs \
  lz4 pango fontconfig freetype2 \
  directx-shader-compiler
```

Notes:

- `BUILD_WEWEB=ON` is forced during the upstream configure step.
- `gtk3` is needed by the current tray/GUI stack on Linux.
- `directx-shader-compiler` satisfies the upstream DXC probe used by the renderer submodule.

During `cargo build`, `build.rs` stages these runtime artifacts into `target/we-renderer-upstream/install/lib` and strips them there:

- `libwallpaper-engine-renderer.so`
- `we-cef-helper`

## Install binaries

```bash
cargo xtask install --prefix ~/.local
sudo cargo xtask install --prefix /usr
DESTDIR="$pkgdir" cargo xtask install --prefix /usr
```

Installed layout:

```text
$prefix/bin/we-layerd
$prefix/bin/we-gui
$prefix/lib/libwallpaper-engine-renderer.so
$prefix/lib/we-cef-helper
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

Leave `renderer.library_path = ""` to enable automatic lookup.

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
