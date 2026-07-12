# Fedora / Ubuntu 构建与依赖

本文说明如何在 Fedora 和 Ubuntu 上从源码构建 `we-layerd`，以及如何生成对应的 RPM / DEB 包。

## 支持范围

当前打包配置以以下发行版为基准：

- Fedora 43，x86_64
- Ubuntu 26.04 LTS，amd64

Ubuntu 24.04 的系统 Rust 版本为 1.75，而当前锁定依赖需要 Rust 1.88 或更新版本。Ubuntu 24.04 可以使用 rustup 安装新工具链进行普通源码构建，但仓库中的 DEB 打包配置以 Ubuntu 26.04 为目标，因为它能直接使用发行版提供的 Rust 工具链。

`we-cef-helper` 当前只支持 Linux x86_64，因此 RPM 和 DEB 也只生成 x86_64/amd64 包。

## 依赖组成

项目同时包含 Rust、C++、Wayland、Vulkan、GStreamer、GTK 3、CEF 和 DXC 组件。

其中有两个需要特别说明的依赖：

- **CEF**：Web 壁纸后端由构建脚本强制启用。Fedora 提供 `cef` 和 `cef-devel`，RPM 使用系统 CEF。Ubuntu 26.04 没有可用的 CEF 开发包，DEB 构建脚本会下载固定版本的官方 CEF minimal binary distribution，并把运行时放入 `/usr/lib/cef`。
- **DXC**：当前着色器路径只使用 DirectX Shader Compiler。Fedora 和 Ubuntu 的基础仓库都没有对应开发包，因此两套打包脚本都会下载固定版本的 Microsoft DXC Linux 发行包，并把运行时放入 `/usr/lib/we-layerd/dxc`。

CEF 和 DXC 的版本、下载地址及 SHA-256 位于 `package/common/versions.env`。所有下载都会先校验哈希。

## Fedora 43

### 安装源码构建依赖

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

普通源码构建还需要准备 DXC：

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

### 安装 RPM 构建工具

```bash
sudo dnf install rpm-build rpmdevtools
```

### 构建 RPM

在仓库根目录运行：

```bash
package/fedora/build.sh
```

输出目录：

```text
package/fedora/out/RPMS/x86_64/
package/fedora/out/SRPMS/
```

安装生成的包：

```bash
sudo dnf install package/fedora/out/RPMS/x86_64/we-layerd-*.rpm
```

RPM 使用 Fedora 的系统 CEF，并把固定版本 DXC 随包安装。`package/common/renderer-system-cef.patch` 只放宽上游渲染器对系统 CEF 布局的检测，使 Fedora 的 `/usr/lib64/cef` 和 `/usr/src/cef-*` 布局可以被发行版自带的 `FindCEF.cmake` 处理。

## Ubuntu 26.04 LTS

### 安装源码构建依赖

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

普通源码构建需要准备 CEF 和 DXC：

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

### 安装 DEB 构建工具

```bash
sudo apt install \
  debhelper devscripts dpkg-dev fakeroot patchelf
```

### 构建 DEB

在仓库根目录运行：

```bash
package/ubuntu/build.sh
```

输出目录：

```text
package/ubuntu/out/we-layerd_*.deb
```

安装生成的包：

```bash
sudo apt install ./package/ubuntu/out/we-layerd_*.deb
```

DEB 会把 CEF 和 DXC 私有运行时放在项目自己的目录中，并为以下文件写入相对 RUNPATH：

```text
/usr/lib/libwallpaper-engine-renderer.so -> $ORIGIN/we-layerd/dxc
/usr/lib/we-cef-helper                  -> $ORIGIN/cef
```

这样生成的包不依赖构建机上的临时目录。

## Ubuntu 24.04 源码构建

Ubuntu 24.04 可以安装系统原生 C/C++ 依赖，但 Rust 必须通过 rustup 升级：

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

随后按照 Ubuntu 26.04 的 CEF/DXC 准备步骤构建即可。仓库不提供面向 Ubuntu 24.04 原生工具链的 DEB 构建保证。

## 包内容

两套包均安装：

```text
/usr/bin/we-layerd
/usr/bin/we-gui
/usr/lib/libwallpaper-engine-renderer.so
/usr/lib/we-cef-helper
/usr/share/applications/we-gui.desktop
/usr/share/icons/hicolor/scalable/apps/we-gui.svg
/usr/share/gnome-shell/extensions/we-layerd@aromatic/
```

Ubuntu 包额外包含 `/usr/lib/cef`，两套包都包含 `/usr/lib/we-layerd/dxc`。