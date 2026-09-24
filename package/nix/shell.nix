{ pkgs }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    cmake
    gcc
    git
    pkg-config
    rustc
    cargo
    wayland-scanner
  ];

  buildInputs = with pkgs; [
    cef-binary
    directx-shader-compiler
    fontconfig
    freetype
    gst_all_1.gstreamer
    gst_all_1.gst-libav
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-bad
    gst_all_1.gst-plugins-good
    gtk3
    libdrm
    libglvnd
    libva
    libxkbcommon
    libpulseaudio
    lz4
    mesa
    pango
    vulkan-headers
    vulkan-loader
    wayland
    wayland-protocols
    xdotool
  ];

  CEF_ROOT = "${pkgs.cef-binary}";
}
