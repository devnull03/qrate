{
  description = "qrate — a spreadsheet for archival cataloguing, built on GPUI";

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
      devShells = forAllSystems (system: {
        default = import ./nix/devshell.nix { pkgs = nixpkgs.legacyPackages.${system}; };
      });
    };
}
