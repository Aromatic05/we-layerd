# Native packages

The packaging tree contains reproducible local package builds for the supported distribution baselines.

```text
package/
├── appimage/ portable x86_64 AppImage build
├── common/   shared source-vendoring, dependency pins, and patches
├── fedora/   Fedora 43 RPM/SRPM build
└── ubuntu/   Ubuntu 26.04 DEB build
```

Run the build entry points from any directory:

```bash
package/appimage/build.sh
package/fedora/build.sh
package/ubuntu/build.sh
```

Both scripts resolve the repository root themselves, verify external downloads, and place all temporary build state under their own `out/` directory. See [`docs/BUILDING.md`](../docs/BUILDING.md) or [`docs/BUILDING.zh-CN.md`](../docs/BUILDING.zh-CN.md) for dependency installation and distrobox validation.

The repository currently has no project license file. These definitions are therefore local test packages, not submission-ready Fedora or Ubuntu packages.