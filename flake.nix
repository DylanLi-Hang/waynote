{
  description = "Wayland-native, markdown-based desktop sticky notes";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (system: {
        default = nixpkgs.legacyPackages.${system}.callPackage ./package.nix { };
        layer-shell = nixpkgs.legacyPackages.${system}.callPackage ./package.nix { layerShell = true; };
      });

      overlays.default = final: prev: {
        waynote = final.callPackage ./package.nix { };
      };

      devShells = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            inputsFrom = [ self.packages.${system}.default ];
            packages = with pkgs; [ cargo rustc rustfmt clippy rust-analyzer python3 glib gtk4-layer-shell ];
            env.RUST_SRC_PATH = "${pkgs.rustPlatform.rustLibSrc}";
          };
      });
    };
}
