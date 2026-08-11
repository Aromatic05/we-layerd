# Architecture

## Runtime path

```text
Wayland output discovery (`wl_output.name`)
-> layer-shell orchestrator
-> one isolated output worker / Wayland connection per applicable output
-> renderer dynamic library + renderer session
-> renderer tick / acquire
-> frame-callback paced present
-> in-flight buffer backpressure
-> acquire dmabuf/shm frame
-> output-bound Wayland layer-shell surface
-> pointer input forwarding
-> per-output status / playlist state
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

- `src/backend/layer_shell/orchestrator.rs`
- `src/backend/layer_shell/event_loop.rs`
- `src/backend/layer_shell/state.rs`
- `src/backend/layer_shell/surface.rs`
- `src/backend/layer_shell/presenter.rs`
- `src/backend/wayland_common/`
- `src/runtime/status.rs`

Current behavior:

- stable output identity from `wl_output.name` (protocol version 4)
- one independent renderer session and layer-shell surface per active output worker
- independent worker lifecycle, playlist cursor/timer, presentation state, and error status
- hotplug reconciliation that starts/stops only affected workers
- output-local reconfiguration/failure isolation instead of tearing down unrelated displays
- one presenter path that supports DMA-BUF and SHM
- presentation paced by `wl_surface.frame` callbacks
- acquire throttled by an in-flight buffer cap
- pointer input forwarding when `general.interactive = true`
- live per-output runtime status exported through `we-layerd ctl status`
- linux-dmabuf v4 surface feedback, with v3 global modifier fallback
- exact `(fourcc, modifier)` capability forwarding to the renderer before output binding
- dynamic renderer output renegotiation when v4 surface feedback changes

Current non-goals:

- no independent GPU-device selection in `we-layerd`; the renderer performs export capability intersection
- no scanout-device preference policy beyond the compositor-provided tranche ordering

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
