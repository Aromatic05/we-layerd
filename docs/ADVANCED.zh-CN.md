# 高级主题

## IPC 与单实例

- Linux 下控制 IPC 默认使用抽象 Unix socket 名称（`we-layerd.control.<uid>`）。
- 同时保留文件 socket 回退逻辑以兼容更多环境。
- 守护进程启动时会获取单实例锁；同一用户重复启动会返回 `already running`。

## 运行时控制

控制运行中的守护进程：
```bash
we-layerd ctl stop
we-layerd ctl pause
we-layerd ctl resume
we-layerd ctl reload
we-layerd ctl status
```

其他命令：
```bash
we-layerd doctor
we-layerd print-config --config ~/.config/we-layerd/config.toml
```

具体配置块见 [CONFIGURATION.zh-CN.md](./CONFIGURATION.zh-CN.md)；当前主路径已经是 renderer-native 配置。
