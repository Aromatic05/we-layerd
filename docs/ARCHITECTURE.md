# Architecture

## Runtime path

```text
renderer dynamic library
-> renderer session
-> renderer tick / acquire
-> frame-callback paced present
-> in-flight buffer backpressure
-> acquire dmabuf/shm frame
-> Wayland layer-shell present
-> pointer input forwarding
-> IPC control
```

`we-layerd` no longer launches Wallpaper Engine through Wine and no longer captures X11 windows.

## Workspace layout

- `we-layerd` (root crate)
  - daemon entrypoint
  - config loading
  - IPC control server
  - Wayland renderer runtime
  - build integration for the renderer submodule

- `crates/we-renderer-sys`
  - raw ABI mapping for `wallpaper/abi/WeRenderer.h`
  - dynamic symbol loading

- `crates/we-renderer`
  - safe Rust wrapper for renderer sessions
  - frame ownership and fd duplication
  - input event encoding

- `crates/we-core`
  - shared config model
  - Steam/workshop discovery
  - wallpaper metadata scanning

- `apps/we-gui`
  - workshop browser
  - renderer-native config generation
  - daemon lifecycle and tray controls

## Wayland runtime

The active runtime is centered in:

- `src/wayland/renderer.rs`
- `src/wayland/state.rs`
- `src/wayland/wayland.rs`
- `src/wayland/geometry.rs`
- `src/wayland/diagnostics.rs`

Current behavior:

- one renderer session for the selected output
- one layer-shell surface bound to the first advertised `wl_output`
- one presenter path that supports DMA-BUF and SHM
- presentation paced by `wl_surface.frame` callbacks
- acquire throttled by an in-flight buffer cap
- pointer input forwarding when `general.interactive = true`
- live runtime status exported through `we-layerd ctl status`

Current non-goals:

- no linux-dmabuf feedback handling
- no format/modifier intersection negotiation
- no cross-GPU import matching
- no scanout/import device probing

## Renderer source of truth

Layer-shell behavior is intentionally aligned with:

```text
third_party/wallpaper-engine-renderer/standalone_layer_view/layerwallpaper.cpp
```

The key reference flow is:

```text
onRegistryGlobal
-> initWayland
-> onLayerSurfaceConfigure
-> createBufferForFrame
-> attach frame callback
-> presentFrame
-> main loop
```
