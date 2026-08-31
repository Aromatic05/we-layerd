# 高级主题

## IPC 与单实例

- Linux 下控制 IPC 默认使用抽象 Unix socket 名称（`we-layerd.control.<uid>`）。
- 同时保留文件 socket 回退逻辑以兼容更多环境。
- 守护进程启动时会获取单实例锁；同一用户重复启动会返回 `already running`。

## 登录时自动启动

原生软件包会安装 systemd 用户服务 `we-layerd.service`。要让它在后续图形会话登录时
自动启动，可以执行：

```bash
systemctl --user enable we-layerd.service
```

如果当前没有 daemon 正在运行，并且希望本次会话立即启动，再执行
`systemctl --user start we-layerd.service`。

服务会读取 `~/.config/we-layerd/config.toml`，因此 `we-gui` 最近保存的壁纸与输出绑定会在
下一次图形会话登录时自动恢复。GUI 中的 **设置 → 行为 → 登录时自动启动壁纸** 管理同一个
enable/disable 状态。AppImage 或本地二进制没有随包 unit 时，启用该设置会在
`~/.config/systemd/user/` 下生成当前启动器对应的用户 unit。

关闭 `we-gui` 不会停止已经由 systemd 或其他外部 owner 启动的守护进程；只有 GUI 自己
直接拉起的 fallback 子进程会随 GUI 回收。显式的 **停止** 操作仍会停止当前守护进程，
不区分 owner。

## 运行时控制

控制运行中的守护进程：
```bash
we-layerd ctl stop
we-layerd ctl pause
we-layerd ctl resume
we-layerd ctl reload
we-layerd ctl status
```

守护进程管理的播放列表在 `we-gui` 关闭后仍会继续推进：

```bash
we-layerd playlist play "Daily"
we-layerd playlist next
we-layerd playlist previous
we-layerd playlist stop
```

layer-shell 下可以用 `--output` 只控制指定输出自己的播放列表运行时：

```bash
we-layerd playlist play "Daily" --output DP-1
we-layerd playlist next --output DP-1
we-layerd playlist previous --output DP-1
we-layerd playlist stop --output DP-1
```

全局播放列表位置会在守护进程重启后恢复；per-output 播放列表各自维护独立的内存
cursor 与计时器。

其他命令：
```bash
we-layerd doctor
we-layerd print-config --config ~/.config/we-layerd/config.toml
```

具体配置块见 [CONFIGURATION.zh-CN.md](./CONFIGURATION.zh-CN.md)；当前主路径已经是 renderer-native 配置。
