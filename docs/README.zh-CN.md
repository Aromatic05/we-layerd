# we-layerd（中文文档）

[English](../README.md)

`we-layerd` 是面向 Wayland 的原生 Wallpaper Engine 运行时，支持 **场景（Scene）**、**视频（Video）** 和 **网页（Web）** 三类壁纸，不需要通过 Wine 运行 Wallpaper Engine。`we-gui` 提供壁纸浏览、设置与播放控制界面。

项目支持实现 layer-shell 协议的合成器，包括 niri、Hyprland 和 KDE Plasma；GNOME 则通过随项目提供的 Shell 扩展接入。

## 截图

### 壁纸库

![we-gui 壁纸库](../screenshot/we-gui-library.png)

<table>
  <tr>
    <td><img src="../screenshot/scene-wallpaper.png" alt="在 Wayland 上运行的场景壁纸"></td>
    <td><img src="../screenshot/web-wallpaper.png" alt="在 Wayland 上运行的网页壁纸"></td>
  </tr>
  <tr>
    <td align="center">场景壁纸</td>
    <td align="center">网页壁纸</td>
  </tr>
</table>

## 功能

### 壁纸兼容性

- 通过原生 Linux 渲染器运行 Wallpaper Engine 的场景、视频和网页壁纸。
- 自动探测常见的 Steam、Wallpaper Engine 与创意工坊目录。
- 扫描已订阅的创意工坊项目，显示标题、壁纸类型以及静态或动态预览。
- 提供标题搜索和类型筛选，便于管理较大的壁纸库。
- 读取壁纸声明的用户属性，并在 GUI 中提供受支持的开关、滑块、选项、颜色、文本、文件和目录控件。
- 按壁纸保存播放、画面和用户属性设置，切换回来时可以恢复原有配置。

### 播放与画面控制

- 直接在 `we-gui` 中应用和切换壁纸，不需要手动重启运行时。
- 提供播放、暂停、恢复、停止和托盘控制。
- 可为每张壁纸分别设置帧率、播放速度、音量和静音状态。
- 渲染分辨率可以跟随输出，也可以使用固定分辨率。
- 支持覆盖、适应、拉伸、居中四种缩放方式，以及 0°、90°、180°、270° 旋转。
- 将鼠标移动、点击和滚轮输入转发给交互式壁纸。
- 可选地让原本以单次模式播放的可见场景音频循环播放，用于兼容部分壁纸。

### Wayland 原生呈现

- 通过 layer-shell 把壁纸作为桌面表面呈现，而不是普通应用窗口。
- 当渲染器与合成器具有兼容的格式和 modifier 时，使用 DMA-BUF 实现零拷贝呈现。
- DMA-BUF 不可用或不适合时自动回退到共享内存路径，包括常见的混合显卡环境。
- 处理输出尺寸、整数与分数缩放、视口裁剪以及渲染器动态调整大小。
- 使用 Wayland frame callback 和有限的在途缓冲区，避免无节制地产生帧。
- GNOME 通过随包扩展完成桌面集成，实际壁纸渲染仍由原生运行时负责。

### 桌面集成

- 提供自适应壁纸网格和 GIF 动态预览。
- 跟随桌面的亮色或暗色外观。
- 英语与简体中文可以即时切换。
- 在 GUI 中显示运行状态和渲染设置。
- 主窗口关闭后，仍可通过系统托盘控制播放。

## 启动

安装后只需启动图形界面：

```bash
we-gui
```

GUI 会探测常见 Steam 路径、打开创意工坊壁纸库、保存壁纸设置，并在应用壁纸时启动或切换运行时。

## 运行要求

- Linux Wayland 会话。
- 支持 layer-shell 的合成器，或者启用了随包扩展的 GNOME。
- 本地安装 Wallpaper Engine，并已下载创意工坊壁纸。
- 当前基于 CEF 的网页壁纸 helper 仅支持 Linux x86_64。

## 其他文档

- [构建与安装](./BUILDING.zh-CN.md)
- [手动配置](./CONFIGURATION.zh-CN.md)
- [守护进程、IPC 与命令行控制](./ADVANCED.zh-CN.md)
- [架构说明](./ARCHITECTURE.md)
- [故障排查](./TROUBLESHOOTING.zh-CN.md)
