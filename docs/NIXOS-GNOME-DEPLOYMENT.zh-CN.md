# NixOS GNOME 构建、部署与验证

本文给出使用远程 Nix 构建机部署 we-layerd 的通用流程。命令中的 `<...>` 项需要替换为自己的主机、用户和目录，远程连接使用构建机的 Tailscale IP。先将待部署源码同步到构建机，并确认系统 Flake 中的 `we-layerd` 输入指向这份源码。

## 构建项目包

在远程构建机上，用项目开发环境构建包：

```sh
ssh <用户>@<构建机的 Tailscale IP> \
  'cd <远程项目目录> && nix develop -c nix build .#default --print-build-logs'
```

构建完成后记录远程 `result` 指向的 store 路径：

```sh
ssh <用户>@<构建机的 Tailscale IP> 'readlink -f <远程项目目录>/result'
```

## 更新系统 Flake 输入

在系统 Flake 目录中更新 `we-layerd` 输入锁定信息。若输入是本地 `path:`，确保其指向当前源码目录：

```sh
run0 --pty /run/current-system/sw/bin/bash -euc '
  cd "$1"
  cp flake.lock "flake.lock.bak-we-layerd-$(date +%Y%m%d-%H%M%S)"
  nix flake update we-layerd
' -- '<系统 Flake 目录>'
```

## 构建 NixOS 系统

保留当前登录用户的 SSH agent，以普通用户执行远程构建：

```sh
nixos-rebuild build --flake <系统 Flake 目录>#<主机输出> \
  --build-host <用户>@<构建机的 Tailscale IP> --no-write-lock-file --print-build-logs
```

远端生成的闭包若没有本机信任的签名，Nix daemon 可能在构建成功后拒绝导入并提示缺少签名。优先让构建机使用本机信任的二进制缓存签名密钥。如果这是可信的自有构建机，也可按下一节只对这次导入使用 `--no-check-sigs`。从构建输出记录完整的 `*-nixos-system-<主机输出>-*` store 路径；签名错误不代表需要重新构建。

## 导入闭包并切换系统

将系统闭包路径填入 `system`。仅在本机 store 没有该闭包时从可信构建机复制；通过 `run0` 将当前登录会话的 SSH agent 和已知主机文件传给临时 root 命令，然后激活：

```sh
system=/nix/store/<完整的-nixos-system-路径>
builder=<用户>@<构建机的 Tailscale IP>

run0 --pty \
  --setenv=SSH_AUTH_SOCK="$SSH_AUTH_SOCK" \
  --setenv=NIX_SSHOPTS="-o UserKnownHostsFile=$HOME/.ssh/known_hosts -o StrictHostKeyChecking=yes" \
  /run/current-system/sw/bin/bash -euc '
    system="$1"
    builder="$2"
    if ! nix path-info "$system" >/dev/null 2>&1; then
      nix copy --from "ssh://$builder" --no-check-sigs "$system"
    fi
    nix-env --profile /nix/var/nix/profiles/system --set "$system"
    "$system/bin/switch-to-configuration" switch
  ' -- "$system" "$builder"
```

`--no-check-sigs` 会跳过该次复制的签名检查，只应用于你控制且信任的构建机；共享或不可信的构建机应配置受信任的签名密钥，不要跳过检查。激活后系统会按 NixOS 配置更新服务。

## 验证 GNOME 集成

we-layerd 的 systemd 用户服务仅在 `~/.config/we-layerd/config.toml` 存在时启动。若从旧版升级，且现有配置显式包含 `backend = "layer_shell"`，请将其手动改为 `backend = "gnome"`；环境默认值不会覆盖配置文件中已保存的值。新建配置会在 GNOME 会话中默认选择 `gnome`。

GNOME 后端通过 XWayland 窗口和 Shell 扩展将画面放入工作区背景，需启用 XWayland 且扩展处于活动状态。当前桥接使用共享内存而非 DMA-BUF，也不转发指针输入；所有显示器使用同一份壁纸配置。桥接监听 RandR 布局变化并重建渲染会话。扩展代码更新后注销并重新登录 GNOME，使 Shell 重新加载扩展 JavaScript。

登录图形会话后检查系统代次、扩展和服务状态：

```sh
readlink -f /run/current-system
readlink -f /run/current-system/sw/share/gnome-shell/extensions/we-layerd@aromatic
gnome-extensions info we-layerd@aromatic
systemctl --user show we-layerd.service -p ActiveState -p MainPID -p ExecStart
journalctl --user -u we-layerd.service -n 50 --no-pager
```

确认扩展状态为 `ACTIVE`、用户服务为 `active`，并检查日志没有启动错误。`SceneStartupFirstFrame` 表示场景 renderer 已输出首帧。工作区概览、切换动画、壁纸位置和桌面点击行为仍需在实际桌面会话中人工确认。
