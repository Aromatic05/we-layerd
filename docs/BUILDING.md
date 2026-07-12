# Fedora and Ubuntu builds

This document covers source builds and native RPM/DEB package builds for `we-layerd`.

## Supported baseline

The packaging files are currently maintained against:

- Fedora 43, x86_64
- Ubuntu 26.04 LTS, amd64

Ubuntu 24.04 ships Rust 1.75, while the current locked dependency graph requires Rust 1.88 or newer. Ubuntu 24.04 can still be used for a source build with a recent rustup toolchain, but the DEB packaging targets Ubuntu 26.04 so it can use the distribution Rust packages.

`we-cef-helper` currently supports Linux x86_64 only, so the RPM and DEB definitions are also restricted to x86_64/amd64.

## Dependency model

The workspace combines Rust, C++, Wayland, Vulkan, GStreamer, GTK 3, CEF, and DXC.

Two dependencies need special handling:

- **CEF:** The build forces the web wallpaper backend on. Fedora provides `cef` and `cef-devel`, so the RPM uses the system CEF runtime. Ubuntu 26.04 has no suitable CEF development package, so the DEB build downloads a pinned official CEF minimal binary distribution and installs its private runtime under `/usr/lib/cef`.
- **DXC:** The current shader path is DirectX Shader Compiler only. Neither Fedora nor Ubuntu provides a suitable package in its base repositories, so both package builds download a pinned Microsoft DXC Linux release and install its private runtime under `/usr/lib/we-layerd/dxc`.

Pinned versions, URLs, and SHA-256 digests are defined in `package/common/versions.env`. Every download is verified before use.

## Fedora 43

### Source build dependencies

```bash
sudo dnf install \
  gcc-c++ cmake pkgconf-pkg-config git curl ca-certificates \
  rust cargo \
  wayland-devel wayland-protocols-devel libxkbcommon-devel \
  gtk3-devel \
  lz4-devel pango-devel fontconfig-devel freetype-devel \
  vulkan-loader-devel vulkan-headers mesa-libGL-devel \
  libatomic libdrm-devel libva-devel \
  gstreamer1-devel gstreamer1-plugins-base-devel \
  gstreamer1-plugins-bad-free-devel \
  cef cef-devel libxdo-devel xdotool patchelf
```

Prepare DXC and build:

```bash
package/common/fetch-dependencies.sh dxc
source package/common/versions.env

mkdir -p .deps/dxc
tar -xzf "${WE_LAYERD_DOWNLOAD_CACHE}/${DXC_ARCHIVE}" -C .deps/dxc

export CMAKE_PREFIX_PATH="$PWD/.deps/dxc"
export PATH="$PWD/.deps/dxc/bin:$PATH"
export WE_LAYERD_INSTALL_PREFIX=/usr

git submodule update --init --recursive
cargo build --locked --workspace --release
```

### RPM build

Install the packaging tools:

```bash
sudo dnf install rpm-build rpmdevtools
```

Run from the repository root:

```bash
package/fedora/build.sh
```

Artifacts are written to:

```text
package/fedora/out/RPMS/x86_64/
package/fedora/out/SRPMS/
```

Install the result with:

```bash
sudo dnf install package/fedora/out/RPMS/x86_64/we-layerd-*.rpm
```

The RPM uses Fedora's system CEF and bundles the pinned DXC runtime. `package/common/renderer-system-cef.patch` only relaxes the renderer's system-CEF layout detection so Fedora's own `FindCEF.cmake` can handle `/usr/lib64/cef` and `/usr/src/cef-*`.

## Ubuntu 26.04 LTS

### Source build dependencies

```bash
sudo apt update
sudo apt install \
  build-essential cmake pkg-config git curl ca-certificates \
  rustc cargo \
  libwayland-dev wayland-protocols libxkbcommon-dev \
  libgtk-3-dev \
  liblz4-dev libpango1.0-dev libfontconfig1-dev libfreetype-dev \
  libvulkan-dev libgl-dev \
  libdrm-dev libva-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libgstreamer-plugins-bad1.0-dev \
  libxdo-dev xdotool patchelf
```

Prepare CEF and DXC and build:

```bash
package/common/fetch-dependencies.sh all
source package/common/versions.env

mkdir -p .deps/cef .deps/dxc
tar -xjf "${WE_LAYERD_DOWNLOAD_CACHE}/${CEF_ARCHIVE}" \
  -C .deps/cef --strip-components=1
tar -xzf "${WE_LAYERD_DOWNLOAD_CACHE}/${DXC_ARCHIVE}" \
  -C .deps/dxc

export CEF_ROOT="$PWD/.deps/cef"
export CMAKE_PREFIX_PATH="$PWD/.deps/dxc"
export PATH="$PWD/.deps/dxc/bin:$PATH"
export WE_LAYERD_INSTALL_PREFIX=/usr

git submodule update --init --recursive
cargo build --locked --workspace --release
```

### DEB build

Install the packaging tools:

```bash
sudo apt install \
  debhelper devscripts dpkg-dev fakeroot patchelf
```

Run from the repository root:

```bash
package/ubuntu/build.sh
```

Artifacts are written to:

```text
package/ubuntu/out/we-layerd_*.deb
```

Install the result with:

```bash
sudo apt install ./package/ubuntu/out/we-layerd_*.deb
```

The DEB installs private CEF and DXC runtimes and rewrites the runtime search paths to:

```text
/usr/lib/libwallpaper-engine-renderer.so -> $ORIGIN/we-layerd/dxc
/usr/lib/we-cef-helper                  -> $ORIGIN/cef
```

No build-directory path remains in the resulting package.

## Ubuntu 24.04 source build

Ubuntu 24.04 can use the same native dependencies, but Rust must be upgraded with rustup:

```bash
sudo apt install curl build-essential cmake pkg-config git \
  libwayland-dev wayland-protocols libxkbcommon-dev libgtk-3-dev \
  liblz4-dev libpango1.0-dev libfontconfig1-dev libfreetype-dev \
  libvulkan-dev libgl-dev libdrm-dev libva-dev \
  libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev \
  libgstreamer-plugins-bad1.0-dev libxdo-dev xdotool patchelf

curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup toolchain install stable
rustup default stable
```

Then follow the Ubuntu CEF/DXC preparation steps above. The repository does not promise a native-toolchain DEB build for Ubuntu 24.04.

## Installed files

Both packages install:

```text
/usr/bin/we-layerd
/usr/bin/we-gui
/usr/lib/libwallpaper-engine-renderer.so
/usr/lib/we-cef-helper
/usr/share/applications/we-gui.desktop
/usr/share/icons/hicolor/scalable/apps/we-gui.svg
/usr/share/gnome-shell/extensions/we-layerd@aromatic/
```

The Ubuntu package additionally owns `/usr/lib/cef`, and both packages own `/usr/lib/we-layerd/dxc`.