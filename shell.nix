# Non-flake entry point. `nix develop` is preferred — it pins nixpkgs through
# flake.lock, where this takes whatever channel the caller has.
{ pkgs ? import <nixpkgs> { } }:

import ./nix/devshell.nix { inherit pkgs; }
