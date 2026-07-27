# we-layerd

[简体中文](./docs/README.zh-CN.md)

A native Wallpaper Engine runtime for Wayland. `we-layerd` renders **scene**, **video**, and **web** wallpapers without Wine, while `we-gui` provides the desktop interface for browsing, configuring, and controlling them.

It supports compositors implementing the layer-shell protocol, including niri, Hyprland, and KDE Plasma, as well as GNOME through the bundled Shell extension.

## Screenshots

### Wallpaper library

![we-gui wallpaper library](./screenshot/we-gui-library.png)

<table>
  <tr>
    <td><img src="./screenshot/scene-wallpaper.png" alt="A scene wallpaper running on Wayland"></td>
    <td><img src="./screenshot/web-wallpaper.png" alt="A web wallpaper running on Wayland"></td>
  </tr>
  <tr>
    <td align="center">Scene wallpaper</td>
    <td align="center">Web wallpaper</td>
  </tr>
</table>

## Features

### Wallpaper compatibility

- Runs Wallpaper Engine **scene**, **video**, and **web** wallpapers through a native Linux renderer.
- Automatically discovers common Steam and Wallpaper Engine Workshop locations.
- Scans subscribed Workshop items and displays their titles, types, and static or animated previews.
- Provides title search and type filters for large wallpaper libraries.
- Reads wallpaper-defined user properties and exposes supported toggles, sliders, choices, colors, text fields, files, and directories in the GUI.
- Stores settings per wallpaper, so switching back restores its playback, presentation, and user-property values.

### Playback and presentation

- Applies and switches wallpapers directly from `we-gui` without manually restarting the runtime.
- Provides play, pause, resume, stop, and tray controls.
- Configures frame rate, playback speed, audio volume, and mute state per wallpaper.
- Can follow the output resolution or use a fixed rendering resolution.
- Supports cover, fit, stretch, and center scaling modes, plus 0°, 90°, 180°, and 270° rotation.
- Forwards pointer movement, clicks, and scrolling to interactive wallpapers.
- Includes an optional compatibility setting for looping visible scene audio authored as a one-shot sound.

### Wayland-native rendering

- Presents wallpapers as desktop surfaces through layer-shell rather than ordinary application windows.
- Uses DMA-BUF presentation for a zero-copy path when the renderer and compositor share compatible formats and modifiers.
- Falls back to shared-memory presentation when DMA-BUF is unavailable or unsuitable, including common hybrid-GPU configurations.
- Handles output size, integer and fractional scaling, viewport cropping, and dynamic renderer resizing.
- Uses Wayland frame callbacks and bounded in-flight buffers to avoid uncontrolled frame production.
- Supports GNOME through the bundled extension while keeping rendering in the native runtime.

### Desktop integration

- Provides an adaptive wallpaper grid with animated GIF previews.
- Follows the desktop light or dark appearance.
- Switches immediately between English and Simplified Chinese.
- Exposes runtime state and renderer settings from the GUI.
- Keeps playback accessible from a system tray menu after the main window is closed.

## Start

After installing the project, launch the graphical interface:

```bash
we-gui
```

The GUI detects common Steam paths, opens the Workshop library, saves wallpaper settings, and starts or switches the runtime when a wallpaper is applied.

## Requirements

- Linux with a Wayland session.
- A compositor with layer-shell support, or GNOME with the bundled extension enabled.
- A local Wallpaper Engine installation and downloaded Workshop wallpapers.
- Linux x86_64 for the current CEF-based web wallpaper helper.

## Documentation

- [Building and installation](./docs/BUILDING.md)
- [Manual configuration](./docs/CONFIGURATION.md)
- [Daemon, IPC, and command-line controls](./docs/ADVANCED.md)
- [Architecture](./docs/ARCHITECTURE.md)
- [Troubleshooting](./docs/TROUBLESHOOTING.md)
- [Chinese documentation](./docs/README.zh-CN.md)
