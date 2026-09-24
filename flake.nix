{
  description = "Build and develop we-layerd on x86_64 Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    renderer = {
      url = "github:Aromatic05/wallpaper-engine-renderer/89dfcd86de2dc0ae537bc136046c5ed05733e7b7";
      flake = false;
    };
    rendererEigen = {
      url = "gitlab:libeigen/eigen/3147391d946bb4b6c68edd901f2add6ac1f31f8c";
      flake = false;
    };
    rendererSpirvReflect = {
      url = "github:KhronosGroup/SPIRV-Reflect/c6c0f5c9796bdef40c55065d82e0df67c38a29a4";
      flake = false;
    };
    rendererGlslang = {
      url = "github:KhronosGroup/glslang/9db8c369e6f49b5c00376040ba8c0cda6cbb7b4d";
      flake = false;
    };
    rendererMiniaudio = {
      url = "github:mackron/miniaudio/4a5b74bef029b3592c54b6048650ee5f972c1a48";
      flake = false;
    };
    rendererNlohmann = {
      url = "github:nlohmann/json/0457de21cffb298c22b629e538036bfeb96130b7";
      flake = false;
    };
    rendererQuickjs = {
      url = "github:quickjs-ng/quickjs/01bce21cb70c771b372ac17a8ef9920ee105e972";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, renderer, rendererEigen, rendererSpirvReflect
    , rendererGlslang, rendererMiniaudio, rendererNlohmann, rendererQuickjs }:
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
      projectSource = pkgs.lib.cleanSourceWith {
        src = self;
        filter = path: type:
          if toString path == toString self then true else
          let relative = pkgs.lib.removePrefix "${toString self}/" (toString path);
              topLevel = builtins.head (builtins.split "/" relative);
          in builtins.elem topLevel [
            ".gitmodules"
            "Cargo.lock"
            "Cargo.toml"
            "apps"
            "build.rs"
            "contrib"
            "crates"
            "src"
            "xtask"
          ];
      };
      package = pkgs.callPackage ./nix/package.nix {
        src = projectSource;
        inherit renderer cefSdk dxcSdk;
        rendererSubmodules = {
          Eigen = rendererEigen;
          "SPIRV-Reflect" = rendererSpirvReflect;
          glslang = rendererGlslang;
          miniaudio = rendererMiniaudio;
          nlohmann = rendererNlohmann;
          quickjs = rendererQuickjs;
        };
      };
    in {
      packages.${system} = {
        default = package;
        we-layerd = package;
      };

      nixosModules.default = import ./nix/module.nix {
        package = self.packages.${system}.default;
      };

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
          wayland-scanner
          wayland-protocols
          libxkbcommon
          gtk3
          pcre2
          libsysprof-capture
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
