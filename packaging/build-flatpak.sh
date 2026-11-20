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
# The bundle lands in /workspaces/.dev/dist (set PIGEONNIER_DIST to put it elsewhere). That folder is
# a bind mount of the Docker host, which need not be the desktop that runs flatpak: the closing
# message names the Docker host and the folder's path there.

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

# Where the bundle sits on the Docker host: the mount holding $dist, read from mountinfo (its fourth
# field is the folder's path in the host filesystem, the host path when that filesystem is the
# host's root). Empty off Docker, e.g. on Codespaces.
bundle="$dist/pigeonnier.flatpak"
docker_host=$(docker info --format '{{.Name}}' 2>/dev/null || true)
host_path=$(awk -v d="$(cd "$dist" && pwd -P)" '
  index(d "/", $5 "/") == 1 && length($5) > best { best = length($5); path = $4 substr(d, length($5) + 1) }
  END { print path }' /proc/self/mountinfo)

echo
echo "build-flatpak: $bundle is ready."
if [[ -n $docker_host && -n $host_path && $host_path != "$dist" ]]; then
  # flatpak maps the bundle into memory, so it cannot read it from a pipe: it needs a real file.
  echo "It sits on $docker_host at $host_path/pigeonnier.flatpak. On the desktop:"
  echo "  f=\$(mktemp --suffix=.flatpak) && scp $docker_host:$host_path/pigeonnier.flatpak \"\$f\" \\"
  echo "    && flatpak install --user --reinstall -y \"\$f\"; rm -f \"\$f\""
fi
