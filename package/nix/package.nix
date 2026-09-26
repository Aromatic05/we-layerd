{
  lib,
  stdenv,
  rustPlatform,
  rendererSrc,
  cmake,
  pkg-config,
  makeWrapper,
  cef-binary,
  directx-shader-compiler,
  fontconfig,
  freetype,
  gst_all_1,
  gtk3,
  libdrm,
  libglvnd,
  libva,
  libxkbcommon,
  libpulseaudio,
  lz4,
  mesa,
  pango,
  vulkan-headers,
  vulkan-loader,
  wayland,
  wayland-scanner,
  wayland-protocols,
  xdotool,
}:
let
  manifest = lib.importTOML ../../Cargo.toml;

  renderer = stdenv.mkDerivation {
    pname = "wallpaper-engine-renderer";
    version = "0-unstable";
    src = rendererSrc;

    nativeBuildInputs = [
      cmake
      pkg-config
      wayland-scanner
    ];

    buildInputs = [
      cef-binary
      directx-shader-compiler
      fontconfig
      freetype
      gst_all_1.gstreamer
      gst_all_1.gst-plugins-base
      gst_all_1.gst-plugins-bad
      gtk3
      libdrm
      libglvnd
      libva
      libxkbcommon
      lz4
      mesa
      pango
      vulkan-headers
      vulkan-loader
      wayland
      wayland-protocols
    ];

    cmakeFlags = [
      "-DBUILD_WEWEB=ON"
      "-DBUILD_QML=OFF"
      "-DBUILD_TESTING=OFF"
      "-DHANABI_BUILD_VENDORED_DXC=OFF"
      "-DCEF_ROOT=${cef-binary}"
      "-DCMAKE_INSTALL_LIBDIR=lib"
    ];
  };
in
rustPlatform.buildRustPackage {
  pname = "we-layerd";
  version = manifest.package.version;

  src = ../../.;

  cargoLock.lockFile = ./../../Cargo.lock;
  cargoBuildFlags = [
    "-p"
    "we-layerd"
    "-p"
    "we-gui"
  ];

  nativeBuildInputs = [
    makeWrapper
    pkg-config
  ];

  buildInputs = [
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

  preBuild = ''
    export WE_LAYERD_INSTALL_PREFIX=/usr
    export WE_LAYERD_PREBUILT_RENDERER_ROOT=${renderer}
  '';

  installPhase = ''
    mkdir -p $out/bin $out/lib $out/share/applications $out/share/icons/hicolor/scalable/apps
    mkdir -p $out/share/gnome-shell/extensions/we-layerd@aromatic $out/lib/systemd/user
    cargoTarget=target/${stdenv.hostPlatform.rust.rustcTarget}/release
    install -Dm755 "$cargoTarget/we-layerd" $out/bin/we-layerd
    install -Dm755 "$cargoTarget/we-gui" $out/bin/we-gui
    cp -a ${renderer}/lib/. $out/lib/
    cp -a contrib/gnome-shell-extension/we-layerd@aromatic/. \
      $out/share/gnome-shell/extensions/we-layerd@aromatic/
    install -Dm644 apps/we-gui/assets/we-gui.desktop $out/share/applications/we-gui.desktop
    install -Dm644 apps/we-gui/assets/we-gui-logo.svg \
      $out/share/icons/hicolor/scalable/apps/we-gui.svg
    install -Dm644 contrib/systemd/we-layerd.service $out/lib/systemd/user/we-layerd.service
    substituteInPlace $out/lib/systemd/user/we-layerd.service \
      --replace-fail /usr/bin/we-layerd $out/bin/we-layerd
  '';

  postFixup = ''
    gstreamerPluginPath="${gst_all_1.gst-plugins-base}/lib/gstreamer-1.0:${gst_all_1.gst-plugins-bad}/lib/gstreamer-1.0:${gst_all_1.gst-plugins-good}/lib/gstreamer-1.0:${gst_all_1.gst-libav}/lib/gstreamer-1.0"
    wrapProgram $out/bin/we-layerd \
      --set CEF_ROOT "${cef-binary}" \
      --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "$gstreamerPluginPath"
    wrapProgram $out/bin/we-gui \
      --set CEF_ROOT "${cef-binary}" \
      --prefix GST_PLUGIN_SYSTEM_PATH_1_0 : "$gstreamerPluginPath"
  '';

  meta = {
    description = "Wayland daemon and GUI for Wallpaper Engine wallpapers";
    homepage = "https://github.com/Aromatic05/we-layerd";
    platforms = [ "x86_64-linux" ];
    mainProgram = "we-layerd";
  };
}
