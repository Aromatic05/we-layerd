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

Daemon-managed playlists keep progressing when `we-gui` is closed:

```bash
we-layerd playlist play "Daily"
we-layerd playlist next
we-layerd playlist previous
we-layerd playlist stop
```

On layer-shell, target one output's independent playlist runtime with `--output`:

```bash
we-layerd playlist play "Daily" --output DP-1
we-layerd playlist next --output DP-1
we-layerd playlist previous --output DP-1
we-layerd playlist stop --output DP-1
```

`playlist stop` stops playlist progression and leaves the currently rendered wallpaper running.
Playlist position is restored after daemon restart; the restored entry starts with a fresh timer.
That persistence currently applies to the global playlist runtime. Output-scoped playlist workers
own independent in-memory cursors and timers.

The GUI **Playlists** sidebar controls these commands directly. Closing the wallpaper library window
does not drive playlist timing: progression remains daemon-owned. GUI playlist edits are persisted in
the normal configuration and a running edited playlist is reloaded by the daemon.

Other commands:
```bash
we-layerd doctor
we-layerd print-config --config ~/.config/we-layerd/config.toml
```

See [CONFIGURATION.md](./CONFIGURATION.md) for the active renderer-native config model.
