# Development shell for NixOS (contributors only — the app itself never needs Nix).
# Untested so far: see docs/PROGRESS.md.
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ pkg-config cargo rustc rustfmt clippy nodejs_22 ];
  buildInputs = with pkgs; [ webkitgtk_4_1 gtk3 librsvg libsoup_3 openssl glib-networking udev ];
  shellHook = ''
    export XDG_DATA_DIRS=${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS
  '';
}
