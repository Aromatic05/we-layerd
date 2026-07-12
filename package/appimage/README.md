# AppImage package

Build the x86_64 AppImage from the repository root:

```bash
package/appimage/build.sh
```

The result is written to `package/appimage/out/we-layerd-<version>-x86_64.AppImage`.
The AppImage starts `we-gui` by default. The daemon CLI remains available through:

```bash
./we-layerd-*.AppImage --cli --help
```

For GNOME, explicitly install the bundled extension into the current user's data directory:

```bash
./we-layerd-*.AppImage --install-gnome-extension
```

The build bundles CEF, DXC, GTK-related libraries, GStreamer and its installed plugins. It deliberately does not bundle glibc, the ELF dynamic loader, or the host OpenGL/Vulkan/VA-API driver stack. `build.sh` audits both the AppDir and the final extracted AppImage and fails if a glibc component is present.

Build on the oldest supported distribution baseline. Ubuntu 24.04 is the currently tested baseline; building on a newer glibc raises the minimum host requirement even though glibc itself is not bundled.
