{
  description = "we-layerd NixOS package";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" ];
      forEachSystem = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));
    in
    {
      packages = forEachSystem (pkgs: {
        we-layerd = pkgs.callPackage ./package/nix/package.nix { };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.we-layerd;
      });

      checks = forEachSystem (pkgs: {
        we-layerd = self.packages.${pkgs.stdenv.hostPlatform.system}.we-layerd;
      });

      devShells = forEachSystem (pkgs: {
        default = pkgs.callPackage ./package/nix/shell.nix { };
      });

      formatter = forEachSystem (pkgs: pkgs.nixfmt);
    };
}
