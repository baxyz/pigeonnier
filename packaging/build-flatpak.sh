#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Builds pigeonnier.flatpak in the devcontainer, with one command:
#
#   packaging/build-flatpak.sh
#
# The app is compiled here, then packed for the GNOME runtime without flatpak-builder, whose sandbox
# the devcontainer refuses. That works because the binary only needs GTK, libadwaita and the C
# library, which the GNOME runtime carries in newer, compatible versions.
#
# The bundle lands in /workspaces/.dev/dist, which the host sees as ~/projects/baxyz/.dev/dist. Set
# PIGEONNIER_DIST to put it elsewhere.

set -euo pipefail

APP_ID=io.github.baxyz.Pigeonnier
RUNTIME_VERSION=51
FLATHUB=https://dl.flathub.org/repo/flathub.flatpakrepo

root=$(cd "$(dirname "$0")/.." && pwd -P)
dist=${PIGEONNIER_DIST:-/workspaces/.dev/dist}
work="$root/target/flatpak"

# What a rebuilt devcontainer lacks.
missing=()
pkg-config --exists gtk4 || missing+=(libgtk-4-dev)
pkg-config --exists libadwaita-1 || missing+=(libadwaita-1-dev)
command -v flatpak >/dev/null || missing+=(flatpak)
if ((${#missing[@]})); then
  echo "build-flatpak: installing ${missing[*]}"
  sudo apt-get update -qq
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends "${missing[@]}"
fi

cargo build --release --locked -p pigeonnier --manifest-path "$root/Cargo.toml"

rm -rf "$work"
mkdir -p "$work/app/files" "$dist"
files="$work/app/files"
install -Dm755 "$root/target/release/pigeonnier" -t "$files/bin"
install -Dm644 "$root/packaging/$APP_ID.desktop" -t "$files/share/applications"
install -Dm644 "$root/packaging/$APP_ID.metainfo.xml" -t "$files/share/metainfo"
install -Dm644 "$root/packaging/$APP_ID.svg" -t "$files/share/icons/hicolor/scalable/apps"

cat >"$work/app/metadata" <<EOF
[Application]
name=$APP_ID
runtime=org.gnome.Platform/$(uname -m)/$RUNTIME_VERSION
sdk=org.gnome.Sdk/$(uname -m)/$RUNTIME_VERSION
EOF

# GitHub and Todoist over the network, Todoist's sign-in coming back to 127.0.0.1, and the tokens
# in the system keyring.
flatpak build-finish "$work/app" --command=pigeonnier >/dev/null \
  --share=ipc --share=network --socket=fallback-x11 --socket=wayland --device=dri \
  --talk-name=org.freedesktop.secrets
# The icon check runs unsandboxed: bwrap is refused here, and the icon is ours.
flatpak build-export --disable-sandbox "$work/repo" "$work/app" >/dev/null
flatpak build-bundle "$work/repo" "$dist/pigeonnier.flatpak" "$APP_ID" --runtime-repo="$FLATHUB"

echo
echo "build-flatpak: $dist/pigeonnier.flatpak is ready. On the host:"
echo "  flatpak install --user --reinstall -y ~/projects/baxyz/.dev/dist/pigeonnier.flatpak"
