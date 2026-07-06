# GNOME Shell Extension

`cargo xtask install` now installs the extension into the matching prefix automatically:

- `~/.local/share/gnome-shell/extensions/we-layerd@aromatic/`
- `/usr/share/gnome-shell/extensions/we-layerd@aromatic/`

If you need a manual install, copy the extension directory into:

```bash
~/.local/share/gnome-shell/extensions/we-layerd@aromatic/
```

Then restart GNOME Shell or log out/in, and enable `we-layerd`.

This extension exposes the D-Bus name `io.github.weLayerd.Gnome` and keeps the
matched Wallpaper Engine XWayland window pinned to the desktop bottom layer.
