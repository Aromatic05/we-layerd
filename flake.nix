{
  description = "Build and package we-layerd on x86_64 Linux";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

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

  outputs =
    {
      self,
      nixpkgs,
      renderer,
      rendererEigen,
      rendererSpirvReflect,
      rendererGlslang,
      rendererMiniaudio,
      rendererNlohmann,
      rendererQuickjs,
    }:
    let
      systems = [ "x86_64-linux" ];
      forEachSystem =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f {
            inherit system;
            pkgs = import nixpkgs { inherit system; };
          }
        );
      rendererSubmodules = {
        Eigen = rendererEigen;
        "SPIRV-Reflect" = rendererSpirvReflect;
        glslang = rendererGlslang;
        miniaudio = rendererMiniaudio;
        nlohmann = rendererNlohmann;
        quickjs = rendererQuickjs;
      };
      mkRendererSource =
        pkgs:
        pkgs.runCommand "wallpaper-engine-renderer-source-89dfcd8" { } ''
          mkdir -p "$out"
          cp -a ${renderer}/. "$out/"
          chmod -R u+w "$out"
          ${nixpkgs.lib.concatStringsSep "\n" (
            nixpkgs.lib.mapAttrsToList (name: source: ''
              rm -rf "$out/third_party/${name}"
              mkdir -p "$out/third_party/${name}"
              cp -a ${source}/. "$out/third_party/${name}/"
            '') rendererSubmodules
          )}
        '';
    in
    {
      packages = forEachSystem (
        { system, pkgs }:
        let
          rendererSrc = mkRendererSource pkgs;
          package = pkgs.callPackage ./package/nix/package.nix { inherit rendererSrc; };
        in
        {
          we-layerd = package;
          default = self.packages.${system}.we-layerd;
        }
      );

      checks = forEachSystem (
        { system, pkgs }:
        let
          moduleSystem = nixpkgs.lib.nixosSystem {
            inherit system;
            modules = [
              self.nixosModules.default
              { services.we-layerd.enable = true; }
            ];
          };
        in
        {
          we-layerd = self.packages.${system}.we-layerd;
          nixos-module = pkgs.runCommand "we-layerd-nixos-module-check" { } ''
            test -n "${moduleSystem.config.systemd.user.services.we-layerd.serviceConfig.ExecStart}"
            test "${moduleSystem.config.systemd.user.services.we-layerd.unitConfig."X-Managed-By"}" = \
              "NixOS services.we-layerd.enable"
            touch "$out"
          '';
        }
      );

      devShells = forEachSystem (
        { pkgs, ... }:
        {
          default = pkgs.callPackage ./package/nix/shell.nix { };
        }
      );

      nixosModules.default = import ./package/nix/module.nix {
        defaultPackage = self.packages.x86_64-linux.default;
      };

      formatter = forEachSystem ({ pkgs, ... }: pkgs.nixfmt);
    };
}
