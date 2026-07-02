# we-layerd（中文文档）

`we-layerd` 是一个基于 Rust 的 Wallpaper Engine Wayland 守护进程，当前主路径使用原生 `wallpaper-engine-renderer` 动态库。

## 功能

- 构建并使用仓库内的 `wallpaper-engine-renderer` git submodule
- 通过 Wayland layer-shell 展示壁纸
- 支持 DMA-BUF / SHM 帧呈现
- 将指针输入转发给交互壁纸
- 每个 output 一个 renderer session
- 提供 `we-gui` 图形界面与托盘控制
- 支持 `stop`、`pause`、`resume`、`reload`、`status`

## 构建

```bash
git submodule update --init --recursive
cargo build --workspace
```

构建 `we-layerd` 时会顺带构建上游动态库，并放到：

```text
target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so
~/.local/bin/lib/libwallpaper-engine-renderer.so
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
