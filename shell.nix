# Development convenience shell; Cargo.lock pins application dependencies.
# For a pinned Nix release derivation, see the remaining packaging gates.
{ pkgs ? import <nixpkgs> {} }:
pkgs.mkShell {
  packages = with pkgs; [ cargo rustc rustfmt clippy gcc ];
}
