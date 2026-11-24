# Build and run mxKeys on NixOS:
#   nix-shell
#   npm ci && cargo tauri build --no-bundle   (npx tauri's prebuilt binary does not load on NixOS)
#   ./target/release/mxkeys
# The firmware tools mxKeys downloads are generic Linux binaries: enable nix-ld
# (`programs.nix-ld.enable = true;`) so they can run.
{ pkgs ? import <nixpkgs> { } }:
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [ pkg-config gobject-introspection cargo rustc rustfmt clippy nodejs_22 cargo-tauri ];
  buildInputs = with pkgs; [
    webkitgtk_4_1 gtk3 librsvg libsoup_3 openssl glib glib-networking
    at-spi2-atk cairo gdk-pixbuf harfbuzz pango
    udev dbus git
  ];
  shellHook = ''
    export XDG_DATA_DIRS=${pkgs.gsettings-desktop-schemas}/share/gsettings-schemas/${pkgs.gsettings-desktop-schemas.name}:${pkgs.gtk3}/share/gsettings-schemas/${pkgs.gtk3.name}:$XDG_DATA_DIRS
    export GIO_MODULE_DIR=${pkgs.glib-networking}/lib/gio/modules
    export WEBKIT_DISABLE_DMABUF_RENDERER=1
  '';
}
