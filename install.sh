#!/usr/bin/env bash
#
# Wisp — one-command installer for Arch Linux.
#
#   curl -fsSL https://raw.githubusercontent.com/Shixuuu/Wisp/main/install.sh | bash
#
# It installs the build and runtime dependencies, builds the wisp-git package
# from the latest commit and installs it with pacman. Run it again to upgrade.
# Remove it with:  sudo pacman -R wisp-git
#
# Run it as your normal user, not with sudo: makepkg refuses to run as root.

set -euo pipefail

REPO="https://github.com/Shixuuu/Wisp.git"
PKG="wisp-git"
BUILD_DEPS=(base-devel git)
RUNTIME_DEPS=(webkitgtk-6.0 gtk4 libadwaita hicolor-icon-theme)

say()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31merror:\033[0m %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -ne 0 ] || die "run this as your normal user, not as root — makepkg will not run as root"
command -v pacman >/dev/null 2>&1 || die "this installer is for Arch Linux — pacman was not found"
command -v sudo >/dev/null 2>&1 || die "sudo is needed to install packages"

if ! grep -qE '^(ID|ID_LIKE)=.*arch' /etc/os-release 2>/dev/null; then
  warn "this does not look like Arch Linux — carrying on anyway"
fi

# Rust comes from the Arch package, unless a toolchain is already on PATH.
deps=("${BUILD_DEPS[@]}" "${RUNTIME_DEPS[@]}")
if ! command -v cargo >/dev/null 2>&1; then
  deps+=(rust)
fi

say "Installing build and runtime dependencies"
sudo pacman -S --needed --noconfirm "${deps[@]}"

command -v makepkg >/dev/null 2>&1 || die "makepkg is still missing after installing base-devel"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

say "Fetching Wisp"
git clone --quiet --depth 1 "$REPO" "$work/wisp"
cd "$work/wisp/packaging/arch"

# makepkg insists that `cargo` be an installed package to satisfy its
# makedepends. A rustup toolchain provides cargo without one, and makepkg would
# then try to pull the `rust` package over it, so skip makepkg's dep check
# instead — the dependencies above are what it would be checking for.
makepkg_args=(-f)
pacman -Qq rust >/dev/null 2>&1 || makepkg_args+=(--nodeps)

say "Building $PKG"
makepkg "${makepkg_args[@]}"

pkgfile=$(find . -maxdepth 1 -name '*.pkg.tar.zst' -print -quit)
[ -n "$pkgfile" ] || die "the build finished but produced no package"

say "Installing $PKG"
sudo pacman -U --noconfirm "$pkgfile"

if command -v wisp >/dev/null 2>&1; then
  say "Installed $(wisp --version 2>/dev/null || printf 'wisp')"
fi
say "Start Wisp from your desktop menu, or run: wisp"
say "Remove it with: sudo pacman -R $PKG"
