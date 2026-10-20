# 故障排查

## 无法加载 renderer 动态库

按这个顺序检查：

1. `renderer.library_path`
2. `~/.local/lib/libwallpaper-engine-renderer.so`
3. `/usr/lib/libwallpaper-engine-renderer.so`
4. 系统标准库路径

如果是本仓库本地构建，请重新执行：

```bash
git submodule update --init --recursive
cargo build --workspace
```

## submodule 已构建，但找不到库

成功构建后应当能看到：

```text
~/.local/lib/libwallpaper-engine-renderer.so
/usr/lib/libwallpaper-engine-renderer.so
```

如果失败，请优先检查：

```text
third_party/wallpaper-engine-renderer
```

里的 CMake 构建输出。

## 没有显示壁纸

- 确认合成器暴露了 `zwlr_layer_shell_v1`
- 确认 `renderer.source` 指向真实 workshop 壁纸目录
- 确认 `renderer.assets_path` 指向 Wallpaper Engine 的 `assets/` 目录
- 使用 `we-layerd ctl status` 查看当前 source 和 error 字段

## GNOME 错误选择 layer-shell 或壁纸不显示

- `[general]` 中使用默认 `backend = "auto"`；旧版 `backend = "layer_shell"` 按自动选择解释。
- 执行 `we-layerd doctor --config ~/.config/we-layerd/config.toml`；GNOME 会话应看到 `OK backend = gnome` 和 `OK gnome_extension`，无需 `zwlr_layer_shell_v1`。
- 确保已安装并启用 GNOME Shell 扩展，其 D-Bus 名称为 `io.github.weLayerd.Gnome`；可用 `backend = "gnome"` 显式指定。

## 指针交互无效

- 确认 `general.interactive = true`
- 确认壁纸本身支持交互
- 多输出模式下，指针事件会转发给当前获得焦点的 layer surface 对应 session
- Niri 使用 `place-within-backdrop` 时可配置 `general.input_mode = "background_proxy"`，Niri 的 layer-rule 只能匹配 `wallpaper-engine-renderer`，不要将 `we-layerd-input-proxy` 也放入 Backdrop。详见[配置](./CONFIGURATION.zh-CN.md#niri-backdrop-交互式壁纸)。

## DMA-BUF 不工作

- 保持 `renderer.prefer_dmabuf = true`
- 建议同时保留 `renderer.allow_shm_fallback = true`
- 某些 compositor / GPU 组合会自动退回 SHM，这不一定是错误
- 用 `we-layerd ctl status` 检查 `dmabuf_formats_known` 和 `dmabuf_format_count`
- linux-dmabuf v4 使用每个 surface 的 feedback，v3 使用全局 modifier 列表
- PRIME render offload 场景会强制使用 SHM

## `ctl` 无法连接

确认：

- daemon 正在运行
- `XDG_RUNTIME_DIR` 与当前登录会话一致
- 控制命令和 daemon 运行在同一用户下
