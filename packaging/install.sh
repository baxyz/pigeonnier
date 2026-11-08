#!/bin/sh
# Installs Pigeonnier for the current user. Usage: packaging/install.sh [--debug] [--no-build]
set -eu

profile=release
build=yes
for argument in "$@"; do
  case "$argument" in
    --debug) profile=debug ;;
    --no-build) build=no ;;
    *) echo "unknown option: $argument" >&2; exit 2 ;;
  esac
done

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
prefix=${PREFIX:-$HOME/.local}
bindir=$prefix/bin
datadir=$prefix/share
systemd_dir=${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user
uuid=pigeonnier@baxyz.github.io
extension_dir=$datadir/gnome-shell/extensions/$uuid

if [ "$build" = yes ]; then
  if [ "$profile" = release ]; then
    cargo build --manifest-path "$root/Cargo.toml" --release --locked
  else
    cargo build --manifest-path "$root/Cargo.toml" --locked
  fi
fi

target=${CARGO_TARGET_DIR:-$root/target}/$profile
mkdir -p "$bindir" "$datadir/applications" "$datadir/dbus-1/services" "$systemd_dir" "$extension_dir/schemas"
for binary in pigeonnier pigeonnierd pigeonnier-gnome; do
  install -m 755 "$target/$binary" "$bindir/$binary"
done

fill() {
  sed "s|@BINDIR@|$bindir|g" "$root/packaging/$1" > "$2"
}
fill pigeonnierd.service "$systemd_dir/pigeonnierd.service"
fill io.github.baxyz.Pigeonnier.service "$datadir/dbus-1/services/io.github.baxyz.Pigeonnier.service"
fill io.github.baxyz.Pigeonnier.Gnome.desktop "$datadir/applications/io.github.baxyz.Pigeonnier.Gnome.desktop"

for file in metadata.json extension.js model.js stylesheet.css; do
  install -m 644 "$root/extensions/gnome/$file" "$extension_dir/$file"
done
install -m 644 "$root"/extensions/gnome/schemas/*.gschema.xml "$extension_dir/schemas/"
if command -v glib-compile-schemas >/dev/null; then
  glib-compile-schemas "$extension_dir/schemas"
fi

echo "Installed in $prefix. To finish:"
echo "  systemctl --user daemon-reload && systemctl --user enable --now pigeonnierd"
echo "  gnome-extensions enable $uuid   # after logging out and back in"
