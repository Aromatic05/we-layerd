# Troubleshooting

## The renderer library cannot be loaded

Check these paths in order:

1. `renderer.library_path`
2. `~/.local/lib/libwallpaper-engine-renderer.so`
3. `/usr/lib/libwallpaper-engine-renderer.so`
4. standard system library paths

For local repo builds, rerun:

```bash
git submodule update --init --recursive
cargo build --workspace
```

## The submodule builds but the library is missing

Expected locations after a successful build:

```text
~/.local/lib/libwallpaper-engine-renderer.so
/usr/lib/libwallpaper-engine-renderer.so
```

If the build fails inside the submodule, inspect the CMake step under:

```text
third_party/wallpaper-engine-renderer
```

## No wallpaper is shown

- Confirm the compositor exposes `zwlr_layer_shell_v1`
- Confirm `renderer.source` points to a real workshop item directory
- Confirm `renderer.assets_path` points to the Wallpaper Engine `assets/` directory
- Run `we-layerd doctor --config ~/.config/we-layerd/config.toml`
- Check `we-layerd ctl status` for:
  - `phase`
  - `last_error`
  - `render_width` / `render_height`
  - `last_present_backend`
  - `in_flight_buffers`

## Pointer interaction does not work

- Ensure `general.interactive = true`
- Confirm the wallpaper itself is interactive
- If the compositor never sends pointer focus to the wallpaper surface, events will not be forwarded

## DMA-BUF does not work

- Leave `renderer.prefer_dmabuf = true`
- Keep `renderer.allow_shm_fallback = true` unless you explicitly want DMA-BUF only
- Some compositor/GPU combinations will fall back to SHM even when DMA-BUF is preferred
- `we-layerd` currently does not implement linux-dmabuf feedback negotiation
- On hybrid or PRIME-offload setups, prefer `renderer.prefer_dmabuf = false` or keep SHM fallback enabled

## `ctl` cannot connect

Check:

- the daemon is running
- `XDG_RUNTIME_DIR` matches the active login session
- the command is running as the same user as the daemon

## `doctor` reports missing globals

- `ERR global wl_compositor` or `ERR global zwlr_layer_shell_v1` means the active Wayland session cannot host the current layer-shell backend
- `WARN global zwp_linux_dmabuf_v1` means the runtime can still run, but presentation is limited to SHM
- `WARN global wp_viewporter` or `WARN global wp_fractional_scale_manager_v1` means high-DPI geometry will be more limited
