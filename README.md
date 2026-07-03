# we-layerd

`we-layerd` is a Rust daemon for running Wallpaper Engine wallpapers on Wayland through the native `wallpaper-engine-renderer` library.

## What it does

- Builds the bundled `wallpaper-engine-renderer` submodule
- Presents renderer output through Wayland layer-shell
- Supports DMA-BUF and SHM frame presentation
- Forwards pointer input to interactive wallpapers
- Runs one renderer session per output
- Ships a GUI companion, `we-gui`, for workshop browsing and config generation

## Build

Initialize the bundled renderer submodule and build the workspace:

```bash
git submodule update --init --recursive
cargo build --workspace --release
WE_LAYERD_INSTALL_PREFIX=/usr cargo build --workspace --release
```

### Arch Linux packages

For this repository itself:

```bash
sudo pacman -S --needed \
  rustup gcc cmake pkgconf git \
  wayland wayland-protocols libxkbcommon \
  gtk3
```

For the renderer build:

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

## Install binaries

`cargo xtask install` follows the prefix recorded by the build. The default build prefix is `~/.local`.

```bash
cargo xtask install
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
