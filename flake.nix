{
  description = "Development environment for we-layerd";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      system = "x86_64-linux";
      pkgs = import nixpkgs { inherit system; };
      cefVersion = "144.0.30+g9e70dde+chromium-144.0.7559.257";
      cefSdkArchive = pkgs.fetchurl {
        url = "https://cef-builds.spotifycdn.com/cef_binary_${cefVersion}_linux64_minimal.tar.bz2";
        hash = "sha256-4M2NWQwy2gFGM9J7nujSE6FsR+5WP+5UUDPLCJXJidc=";
      };
      cefSdk = pkgs.runCommand "cef-sdk-${cefVersion}" {
        nativeBuildInputs = [ pkgs.gnutar pkgs.bzip2 ];
      } ''
        mkdir -p "$out"
        tar -xjf ${cefSdkArchive} -C "$out" --strip-components=1
      '';
      dxcVersion = "1.9.2602.24";
      dxcSdkArchive = pkgs.fetchurl {
        url = "https://github.com/microsoft/DirectXShaderCompiler/releases/download/v${dxcVersion}/linux_dxc_2026_05_26.x86_64.tar.gz";
        hash = "sha256-kos+mYbRHcQnkFDgI0CVDCm8vR5e+5097ZZp2t43Y50=";
      };
      dxcSdk = pkgs.runCommand "dxc-sdk-${dxcVersion}" {
        nativeBuildInputs = [ pkgs.gnutar ];
      } ''
        mkdir -p "$out"
        tar -xzf ${dxcSdkArchive} -C "$out"
      '';
    in {
      devShells.${system}.default = pkgs.mkShell {
        packages = with pkgs; [
          rustc
          cargo
          rustfmt
          clippy
          gcc
          cmake
          pkg-config
          git
          curl
          cacert
          gnumake
          binutils
          patchelf

          wayland
          wayland-protocols
          libxkbcommon
          gtk3
          xdotool
          libdrm
          libva
          libpulseaudio
          vulkan-headers
          vulkan-loader
          mesa
          libglvnd

          lz4
          pango
          fontconfig
          freetype

          gst_all_1.gstreamer
          gst_all_1.gst-plugins-base
          gst_all_1.gst-plugins-good
          gst_all_1.gst-plugins-bad
          gst_all_1.gst-libav
          gst_all_1.gst-plugins-ugly

          cefSdk
          dxcSdk
        ];

        shellHook = ''
          export PKG_CONFIG_PATH="${pkgs.wayland.dev}/lib/pkgconfig:${pkgs.wayland-protocols}/share/pkgconfig:${pkgs.libxkbcommon.dev}/lib/pkgconfig:${pkgs.gtk3.dev}/lib/pkgconfig:${pkgs.libdrm.dev}/lib/pkgconfig:${pkgs.libva.dev}/lib/pkgconfig:${pkgs.libpulseaudio.dev}/lib/pkgconfig:${pkgs.lz4.dev}/lib/pkgconfig:${pkgs.pango.dev}/lib/pkgconfig:${pkgs.fontconfig.dev}/lib/pkgconfig:${pkgs.freetype.dev}/lib/pkgconfig:''${PKG_CONFIG_PATH}"

          export CEF_ROOT="${cefSdk}"
          export CMAKE_PREFIX_PATH="${dxcSdk}''${CMAKE_PREFIX_PATH:+:$CMAKE_PREFIX_PATH}"
          export PATH="${dxcSdk}/bin:$PATH"

          if [ ! -f third_party/wallpaper-engine-renderer/CMakeLists.txt ]; then
            printf 'Initialize the renderer submodule with: git submodule update --init --recursive\n'
          fi
        '';
      };
    };
}
