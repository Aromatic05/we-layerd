{ lib
, stdenv
, rustPlatform
, runCommand
, makeWrapper
, autoPatchelfHook
, pkg-config
, cmake
, gcc
, git
, patchelf
, gtk3
, libayatana-appindicator
, xdotool
, wayland
, wayland-protocols
, wayland-scanner
, libxkbcommon
, libdrm
, libva
, libpulseaudio
, vulkan-loader
, vulkan-headers
, libglvnd
, mesa
, lz4
, pango
, libsysprof-capture
, fontconfig
, pcre2
, freetype
, gst_all_1
, alsa-lib
, nspr
, nss
, cups
, atk
, libX11
, libXcursor
, libXcomposite
, libXdamage
, libXext
, libXfixes
, libXi
, libXrandr
, libXScrnSaver
, libXtst
, libxcb
, xcbutilwm
, xcbutilimage
, xcbutilkeysyms
, libxshmfence
, cefSdk
, dxcSdk
, src
, renderer
, rendererSubmodules
}:
let
  gstPluginPath = lib.makeSearchPath "lib/gstreamer-1.0" [
    gst_all_1.gstreamer.out
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gst_all_1.gst-plugins-bad
    gst_all_1.gst-libav
    gst_all_1.gst-plugins-ugly
  ];
  assembledSource = runCommand "we-layerd-source" { } ''
    mkdir -p "$out"
    cp -a ${src}/. "$out/"
    chmod -R u+w "$out"
    rm -rf "$out/third_party/wallpaper-engine-renderer"
    mkdir -p "$out/third_party/wallpaper-engine-renderer"
    cp -a ${renderer}/. "$out/third_party/wallpaper-engine-renderer/"
    chmod -R u+w "$out/third_party/wallpaper-engine-renderer"
    ${lib.concatStringsSep "\n" (lib.mapAttrsToList (name: source: ''
      rm -rf "$out/third_party/wallpaper-engine-renderer/third_party/${name}"
      mkdir -p "$out/third_party/wallpaper-engine-renderer/third_party/${name}"
      cp -a ${source}/. "$out/third_party/wallpaper-engine-renderer/third_party/${name}/"
    '') rendererSubmodules)}
  '';
in
rustPlatform.buildRustPackage {
  pname = "we-layerd";
  version = "0.2.9";
  src = assembledSource;
  cargoLock.lockFile = ../Cargo.lock;
  patches = [ ../package/common/build-without-git-metadata.patch ];

  nativeBuildInputs = [
    autoPatchelfHook
    makeWrapper
    pkg-config
    cmake
    gcc
    git
    patchelf
    wayland-scanner
  ];
  buildInputs = [
    gtk3
    libayatana-appindicator
    xdotool
    gcc.cc.lib
    wayland
    wayland-protocols
    libxkbcommon
    libdrm
    libva
    libpulseaudio
    vulkan-loader
    vulkan-headers
    libglvnd
    mesa
    lz4
    pango
    libsysprof-capture
    pcre2
    fontconfig
    freetype
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
    gst_all_1.gst-plugins-bad
    gst_all_1.gst-libav
    gst_all_1.gst-plugins-ugly
    alsa-lib
    nspr
    nss
    cups
    atk
    libX11
    libXcursor
    libXcomposite
    libXdamage
    libXext
    libXfixes
    libXi
    libXrandr
    libXScrnSaver
    libXtst
    libxcb
    xcbutilwm
    xcbutilimage
    xcbutilkeysyms
    libxshmfence
  ];

  cargoBuildFlags = [ "-p" "we-layerd" "-p" "we-gui" ];
  doCheck = false;

  CEF_ROOT = "${cefSdk}";
  CMAKE_PREFIX_PATH = "${dxcSdk}";
  WE_LAYERD_INSTALL_PREFIX = "/usr";
  CMAKE_BUILD_PARALLEL_LEVEL = "4";

  preBuild = ''
    export PATH="${dxcSdk}/bin:$PATH"
    export PKG_CONFIG_PATH="${gtk3.dev}/lib/pkgconfig:${wayland.dev}/lib/pkgconfig:${wayland-protocols}/share/pkgconfig:${libxkbcommon.dev}/lib/pkgconfig:${libdrm.dev}/lib/pkgconfig:${libva.dev}/lib/pkgconfig:${libpulseaudio.dev}/lib/pkgconfig:${lz4.dev}/lib/pkgconfig:${pango.dev}/lib/pkgconfig:${fontconfig.dev}/lib/pkgconfig:${freetype.dev}/lib/pkgconfig''${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
  '';

  postInstall = ''
    rm -f "$out/bin/we-layerd" "$out/bin/we-gui"
    install -Dm0755 target/${stdenv.hostPlatform.rust.rustcTarget}/release/we-layerd \
      "$out/libexec/we-layerd/we-layerd"
    install -Dm0755 target/${stdenv.hostPlatform.rust.rustcTarget}/release/we-gui \
      "$out/libexec/we-layerd/we-gui"
    install -Dm0755 target/we-renderer-upstream/install/lib/libwallpaper-engine-renderer.so \
      "$out/lib/libwallpaper-engine-renderer.so"
    install -Dm0755 target/we-renderer-upstream/install/lib/we-cef-helper \
      "$out/libexec/we-layerd/we-cef-helper"

    mkdir -p "$out/lib/we-layerd/cef" "$out/lib/we-layerd/dxc"
    cp -a ${cefSdk}/Release/. "$out/lib/we-layerd/cef/"
    chmod -R u+w "$out/lib/we-layerd/cef"
    cp -a ${cefSdk}/Resources/. "$out/lib/we-layerd/cef/"
    chmod -R u+w "$out/lib/we-layerd/cef"
    rm -f "$out/lib/we-layerd/cef/chrome-sandbox"
    install -m0755 ${dxcSdk}/lib/libdxcompiler.so "$out/lib/we-layerd/dxc/libdxcompiler.so"
    install -m0755 ${dxcSdk}/lib/libdxil.so "$out/lib/we-layerd/dxc/libdxil.so"

    install -d "$out/share/gnome-shell/extensions/we-layerd@aromatic"
    cp -a contrib/gnome-shell-extension/we-layerd@aromatic/. \
      "$out/share/gnome-shell/extensions/we-layerd@aromatic/"
    install -Dm0644 apps/we-gui/assets/we-gui.desktop \
      "$out/share/applications/we-gui.desktop"
    install -Dm0644 apps/we-gui/assets/we-gui-logo.svg \
      "$out/share/icons/hicolor/scalable/apps/we-gui.svg"
    install -Dm0644 ${cefSdk}/LICENSE.txt "$out/share/licenses/we-layerd/CEF-LICENSE.txt"
    install -Dm0644 ${dxcSdk}/LICENSE-MS.txt "$out/share/licenses/we-layerd/DXC-LICENSE-MS.txt"
    install -Dm0644 ${dxcSdk}/LICENSE-LLVM.txt "$out/share/licenses/we-layerd/DXC-LICENSE-LLVM.txt"

    mkdir -p "$out/bin" "$out/lib/systemd/user"
    makeWrapper "$out/libexec/we-layerd/we-layerd" "$out/bin/we-layerd" \
      --prefix LD_LIBRARY_PATH : "${libglvnd}/lib:${mesa}/lib:$out/lib:$out/lib/we-layerd/dxc:$out/lib/we-layerd/cef" \
      --prefix XDG_DATA_DIRS : "/run/opengl-driver/share" \
      --set GST_PLUGIN_SYSTEM_PATH_1_0 "${gstPluginPath}" \
      --set GST_PLUGIN_PATH_1_0 "${gstPluginPath}" \
      --set GST_PLUGIN_SCANNER_1_0 "${gst_all_1.gstreamer.out}/libexec/gstreamer-1.0/gst-plugin-scanner" \
      --set WE_LAYERD_RENDERER_LIBRARY_PATH "$out/lib/libwallpaper-engine-renderer.so" \
      --set WE_CEF_HELPER_PATH "$out/libexec/we-layerd/we-cef-helper" \
      --set WE_CEF_RESOURCES_DIR "$out/lib/we-layerd/cef" \
      --set WE_CEF_LOCALES_DIR "$out/lib/we-layerd/cef/locales" \
      --run 'export WE_CEF_CACHE_DIR="''${XDG_CACHE_HOME:-$HOME/.cache}/we-layerd/cef"'
    makeWrapper "$out/libexec/we-layerd/we-gui" "$out/bin/we-gui" \
      --prefix LD_LIBRARY_PATH : "${libayatana-appindicator}/lib:${libglvnd}/lib:${mesa}/lib:$out/lib:$out/lib/we-layerd/dxc:$out/lib/we-layerd/cef" \
      --prefix XDG_DATA_DIRS : "/run/opengl-driver/share" \
      --set GST_PLUGIN_SYSTEM_PATH_1_0 "${gstPluginPath}" \
      --set GST_PLUGIN_PATH_1_0 "${gstPluginPath}" \
      --set GST_PLUGIN_SCANNER_1_0 "${gst_all_1.gstreamer.out}/libexec/gstreamer-1.0/gst-plugin-scanner" \
      --set WE_LAYERD_RENDERER_LIBRARY_PATH "$out/lib/libwallpaper-engine-renderer.so" \
      --set WE_CEF_HELPER_PATH "$out/libexec/we-layerd/we-cef-helper" \
      --set WE_CEF_RESOURCES_DIR "$out/lib/we-layerd/cef" \
      --set WE_CEF_LOCALES_DIR "$out/lib/we-layerd/cef/locales" \
      --run 'export WE_CEF_CACHE_DIR="''${XDG_CACHE_HOME:-$HOME/.cache}/we-layerd/cef"'

    cat > "$out/lib/systemd/user/we-layerd.service" <<EOF
    [Unit]
    Description=we-layerd wallpaper daemon
    PartOf=graphical-session.target
    After=graphical-session-pre.target
    ConditionPathExists=%h/.config/we-layerd/config.toml

    [Service]
    Type=simple
    ExecStart=$out/bin/we-layerd run --config %h/.config/we-layerd/config.toml
    Restart=on-failure
    RestartSec=2

    [Install]
    WantedBy=graphical-session.target
    EOF
  '';

  preFixup = ''
    addAutoPatchelfSearchPath "$out/lib/we-layerd/cef" "$out/lib/we-layerd/dxc" "$out/lib"
  '';

  postFixup = ''
    patchelf --add-rpath '$ORIGIN/we-layerd/dxc' "$out/lib/libwallpaper-engine-renderer.so"
    patchelf --add-rpath '$ORIGIN/../../lib/we-layerd/cef' "$out/libexec/we-layerd/we-cef-helper"
  '';

  meta = {
    description = "Wallpaper Engine daemon and GUI";
    homepage = "https://github.com/Aromatic05/we-layerd";
    platforms = [ "x86_64-linux" ];
    mainProgram = "we-layerd";
  };
}
