# The dev shell, shared by flake.nix (`nix develop`) and shell.nix (`nix-shell`)
# so the two entry points cannot drift apart.
{ pkgs }:

let
  # Linked at build time, found through pkg-config.
  buildDeps = with pkgs; [
    fontconfig
    freetype
    alsa-lib
    openssl
  ];

  # dlopen'd at run time, so they must also be on the loader path — a build that
  # links fine still dies at startup without them.
  runtimeDeps = with pkgs; [
    libxkbcommon
    wayland
    vulkan-loader
    libGL
    xorg.libX11
    xorg.libxcb
    xorg.libXcursor
    xorg.libXrandr
    xorg.libXi
  ];
in
pkgs.mkShell {
  packages =
    (with pkgs; [
      cargo
      rustc
      rustfmt
      clippy
      rust-analyzer
      pkg-config
    ])
    ++ buildDeps
    ++ runtimeDeps;

  # GPUI resolves its Vulkan driver, Wayland and xkb through dlopen, and `cargo
  # run` inherits this shell rather than a Nix wrapper, so the loader path has to
  # be set here.
  LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeDeps;

  shellHook = ''
    echo "qrate dev shell — cargo $(cargo --version | cut -d' ' -f2)"
    echo "PDF and video preview need ./scripts/fetch-binaries.sh"
  '';
}
