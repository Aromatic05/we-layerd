# 配置

复制示例配置：

```bash
cp config.example.toml ~/.config/we-layerd/config.toml
```

## 必填字段

```toml
[renderer]
library_path = ""
source = "/path/to/Steam/steamapps/workshop/content/431960/<wallpaper-id>"
assets_path = "/path/to/Steam/steamapps/common/wallpaper_engine/assets"
```

- `renderer.library_path`：留空表示自动查找，也可以显式指定 `libwallpaper-engine-renderer.so` 的路径
- `renderer.source`：workshop 壁纸目录，不是视频文件，也不是 Wallpaper Engine 的 CLI 参数
- `renderer.assets_path`：Wallpaper Engine 的 `assets/` 目录

## 通用设置

```toml
[general]
interactive = true
show_fps = false
fps_report_interval_secs = 1
scale_mode = "cover"
```

- 后端自动选择：GNOME 会话走 GNOME actor clone，其它桌面环境走 layer-shell
- `interactive`：为 `false` 时会设置空 input region，让壁纸不阻挡桌面鼠标交互
- `show_fps`：保留 FPS 统计开关
- `scale_mode`：`fit`、`cover`、`stretch`

## Renderer 设置

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

- `cache_path`：renderer 缓存目录
- `prefer_dmabuf`：优先使用 DMA-BUF
- `allow_shm_fallback`：DMA-BUF 不可用时允许退回 SHM
- `fps`：传给 renderer 的目标更新频率
- `speed`、`volume`、`muted`：直接传给 renderer ABI 的 source 参数

## 动态库查找顺序

如果 `renderer.library_path` 为空，或者指向的文件不存在，`we-layerd` 会按以下顺序继续查找：

1. `~/.local/lib/libwallpaper-engine-renderer.so`
2. `/usr/lib/libwallpaper-engine-renderer.so`
3. 可执行文件旁边的 `../lib/libwallpaper-engine-renderer.so`

## GUI 输出

`we-gui` 现在只生成 renderer-native 配置：

- 将 `renderer.source` 写成选中的 workshop 壁纸目录
- 从 Wallpaper Engine 安装目录推导 `renderer.assets_path`
- 不再生成 Wine、Proton、X11 capture、video-native 或 `openWallpaper` 参数
