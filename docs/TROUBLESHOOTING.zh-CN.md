# 故障排查

## 无法加载 renderer 动态库

按这个顺序检查：

1. `renderer.library_path`
2. `~/.local/bin/lib/libwallpaper-engine-renderer.so`
3. `target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so`
4. 系统标准库路径

如果是本仓库本地构建，请重新执行：

```bash
git submodule update --init --recursive
cargo build --workspace
```

## submodule 已构建，但找不到库

成功构建后应当能看到：

```text
target/we-renderer-upstream/build/src/libwallpaper-engine-renderer.so
target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so
~/.local/bin/lib/libwallpaper-engine-renderer.so
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

## 指针交互无效

- 确认 `general.interactive = true`
- 确认壁纸本身支持交互
- 多输出模式下，指针事件会转发给当前获得焦点的 layer surface 对应 session

## DMA-BUF 不工作

- 保持 `renderer.prefer_dmabuf = true`
- 建议同时保留 `renderer.allow_shm_fallback = true`
- 某些 compositor / GPU 组合会自动退回 SHM，这不一定是错误

## `ctl` 无法连接

确认：

- daemon 正在运行
- `XDG_RUNTIME_DIR` 与当前登录会话一致
- 控制命令和 daemon 运行在同一用户下
