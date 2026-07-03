# we-layerd（中文文档）

`we-layerd` 是一个基于 Rust 的 Wallpaper Engine Wayland 守护进程，当前主路径使用原生 `wallpaper-engine-renderer` 动态库。

## 功能

- 构建仓库内置的 `wallpaper-engine-renderer` submodule
- 通过 Wayland layer-shell 展示壁纸
- 支持 DMA-BUF / SHM 帧呈现
- 将指针输入转发给交互壁纸
- 每个 output 一个 renderer session
- 提供 `we-gui` 图形界面与托盘控制
- 支持 `stop`、`pause`、`resume`、`reload`、`status`

## 构建

```bash
git submodule update --init --recursive
cargo build --workspace --release
WE_LAYERD_INSTALL_PREFIX=/usr cargo build --workspace --release
```

### Arch Linux 依赖包

仓库本体构建依赖：

```bash
sudo pacman -S --needed \
  rustup gcc cmake pkgconf git \
  wayland wayland-protocols libxkbcommon \
  gtk3
```

renderer 构建依赖：

```bash
sudo pacman -S --needed \
  vulkan-headers vulkan-icd-loader mesa libglvnd \
  gstreamer gst-plugins-base-libs \
  lz4 pango fontconfig freetype2 \
  directx-shader-compiler
```

说明：

- 构建时会强制 `BUILD_WEWEB=ON`。
- `gtk3` 是当前 Linux 托盘 / GUI 栈需要的包。
- `directx-shader-compiler` 用来满足上游 renderer 对 DXC 的探测。

正式安装使用：

`cargo xtask install` 会遵循构建阶段记录下来的 prefix。默认构建 prefix 是 `~/.local`。

```bash
cargo xtask install
sudo cargo xtask install --prefix /usr
DESTDIR="$pkgdir" cargo xtask install --prefix /usr
```

## 配置

复制示例配置：

```bash
cp config.example.toml ~/.config/we-layerd/config.toml
```

配置细节见：

- [CONFIGURATION.zh-CN.md](./CONFIGURATION.zh-CN.md)
- [ADVANCED.zh-CN.md](./ADVANCED.zh-CN.md)
- [TROUBLESHOOTING.zh-CN.md](./TROUBLESHOOTING.zh-CN.md)
