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
- `renderer.source`：单屏/默认回退使用的 workshop 壁纸目录。layer-shell 的输出绑定已经
  覆盖所有需要运行的屏幕时可以留空；它不是视频文件，也不是 Wallpaper Engine 的 CLI 参数。
- `renderer.assets_path`：Wallpaper Engine 的 `assets/` 目录

## 通用设置

```toml
[general]
interactive = true
show_fps = false
fps_report_interval_secs = 1
scale_mode = "cover"
force_scene_audio_loop = false
```

- 后端自动选择：GNOME 会话走 GNOME actor clone，其它桌面环境走 layer-shell
- `interactive`：为 `false` 时会设置空 input region，让壁纸不阻挡桌面鼠标交互
- `show_fps`：保留 FPS 统计开关
- `force_scene_audio_loop`：默认关闭；开启后会循环原本设为 `single`、可见且自动开始播放的 scene 声音，不改变 start-silent 声音和 `random` 播放。守护进程会把它安全合并到 `scene.audio.forceLoop`，并保留其它版本 1 source options。
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
- `max_fps`：所有壁纸 profile 的全局上限；单壁纸的 `fps` 不能超过它
- `speed`、`volume`、`muted`：直接传给 renderer ABI 的 source 参数

当前 DMA-BUF 协商范围：

- layer-shell 后端读取 linux-dmabuf v4 的 surface feedback，v3 compositor 使用全局 modifier 列表
- compositor 公布的全部 `(fourcc, modifier)` 组合会传给 `wallpaper-engine-renderer`
- scene、video、web 使用同一套消费者能力集合进行协商
- v4 feedback 运行中变化时会重新传入 renderer，并重新绑定输出或退回 SHM
- `prefer_dmabuf` 决定是否优先 DMA-BUF，`allow_shm_fallback` 决定无兼容组合时是否允许退回 SHM

## 多输出 layer-shell 运行时

Wayland 输出使用协议版本 4 提供的稳定 `wl_output.name` 标识。每个输出拥有独立的
layer surface、renderer session、buffer/presentation 状态以及播放列表 cursor/计时器。
某一屏重配置或失败不会销毁其它输出 worker。

```toml
[renderer]
# 未显式绑定的输出可以使用这个回退；也可以留空。
source = "/path/to/Steam/steamapps/workshop/content/431960/1111111111"

[outputs."DP-1"]
wallpaper_id = "2222222222"
source = "/path/to/Steam/steamapps/workshop/content/431960/2222222222"

[outputs."HDMI-A-1"]
playlist = "Daily"
```

一个输出只能绑定一张壁纸（`wallpaper_id` + `source`）或一个命名播放列表，不能同时
绑定两者。`renderer.source` 为空时，没有显式绑定的输出不会启动 worker。显示器移除时
只停止对应 worker，新显示器出现时也只启动新适用的 worker。无效配置或单屏运行失败会
写入该输出的状态，不会停止健康屏幕。

每个输出的播放列表状态相互独立，可单独控制：

```bash
we-layerd playlist play Daily --output DP-1
we-layerd playlist next --output DP-1
we-layerd playlist previous --output DP-1
we-layerd playlist stop --output DP-1
```

只有一个 output worker 时，`we-layerd ctl status` 继续输出兼容的 `[runtime]` /
`[presentation]`；多屏时使用 `[output_runtime."<name>".runtime]` 与
`[output_runtime."<name>".presentation]` 分别报告 source、播放列表 cursor 和呈现状态。

## 壁纸应用 Hook

可以配置一个通用命令 Hook，用于接入 DMS/Matugen 等主题生成器，而不让 `we-layerd`
直接依赖某个桌面 Shell：

```toml
[hooks.wallpaper_applied]
command = "~/.local/bin/we-theme-sync"
args = []
```

每个输出成功启动或切换壁纸后，首帧提交成功时都会独立异步启动一次该命令。例如
DP-1 的首帧不会抑制 HDMI-A-1 的 Hook。Hook 启动失败或退出码非零只会写入日志，
不会中止壁纸运行。`command` 会被直接执行，不会经过 Shell；管道、重定向等 Shell
语法应写进单独的可执行脚本。

Hook 会继承守护进程环境，并额外收到：

- `WE_LAYERD_EVENT=wallpaper_applied`
- `WE_LAYERD_SOURCE`：当前 workshop 壁纸目录，已经展开 `~`
- `WE_LAYERD_BACKEND`：当前后端名称
- `WE_LAYERD_GENERATION`：当前运行 generation

## 动态库查找顺序

如果 `renderer.library_path` 为空，或者指向的文件不存在，`we-layerd` 会按以下顺序继续查找：

1. `~/.local/lib/libwallpaper-engine-renderer.so`
2. `/usr/lib/libwallpaper-engine-renderer.so`
3. 可执行文件旁边的 `../lib/libwallpaper-engine-renderer.so`

## GUI 输出

`we-gui` 现在只生成 renderer-native 配置：

- 旧单屏模式下将 `renderer.source` 写成选中的 workshop 壁纸目录
- 多输出模式下只更新壁纸 profile 和 `[outputs]` 绑定，并保留全局 `renderer.source` 回退
- 从 Wallpaper Engine 安装目录推导 `renderer.assets_path`
- 合并 scene 用户属性和可选的音频循环覆盖，同时保留其它 `renderer.options_json` 字段
- 生成所选壁纸配置时保留 `hooks` 配置
- 遇到无效 JSON、非对象的 scene/audio 容器或不支持的 options 版本时会拒绝保存，不会覆盖原配置
- 不再生成 Wine、Proton、X11 capture、video-native 或 `openWallpaper` 参数

## GUI 语言

`we-gui` 默认使用英语。在 **Settings → Language → 简体中文** 中切换后，窗口和
Linux 托盘菜单会立即改用简体中文。语言选择独立于渲染器配置，保存在
`$XDG_CONFIG_HOME/we-layerd/gui.toml`；未设置 `XDG_CONFIG_HOME` 时使用
`~/.config/we-layerd/gui.toml`：

```toml
language = "zh-Hans"
```

支持的 BCP 47 语言标签为 `en` 和 `zh-Hans`。文件缺失、格式损坏或值不受支持时
都会回退到英语。GUI 使用同目录临时文件加重命名的方式原子写入此文件；切换语言
不会修改渲染器设置。

可用以下命令运行本地化、偏好设置持久化和无头状态测试：

```bash
cargo test -p we-gui
```
