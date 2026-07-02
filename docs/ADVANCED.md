# Advanced

## IPC And Single-Instance

- On Linux, control IPC uses an abstract Unix socket name (`we-layerd.control.<uid>`).
- File-socket fallback is kept for compatibility.
- Daemon startup acquires an instance lock; launching a second instance under the same user returns an `already running` error.

## Runtime Control

Control a running daemon:
```bash
we-layerd ctl stop
we-layerd ctl pause
we-layerd ctl resume
we-layerd ctl reload
we-layerd ctl status
```

Other commands:
```bash
we-layerd doctor
we-layerd print-config --config ~/.config/we-layerd/config.toml
```

See [CONFIGURATION.md](./CONFIGURATION.md) for the active renderer-native config model.
