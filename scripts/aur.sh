#!/usr/bin/env bash
# Writes the AUR package drivecraft-bin (PKGBUILD and .SRCINFO) for a
# version, from the two Linux packages of that version.
#
#   scripts/aur.sh <version> <folder with the Linux tar.gz files> <output folder>
#   scripts/aur.sh 0.1.2 dist target/aur
#
# It needs makepkg (Arch Linux) for .SRCINFO.
set -euo pipefail

version=${1:?"Give the version, for example 0.1.2."}
dist=${2:?"Give the folder with the Linux tar.gz files."}
out=${3:?"Give the output folder."}
root=$(cd "$(dirname "$0")/.." && pwd)

# makepkg does not accept "-" in pkgver.
case "$version" in *-*)
    echo "The version $version has a \"-\". The AUR takes only release versions." >&2
    exit 1
    ;;
esac

sha() { sha256sum "$dist/DriveCraft-$version-$1.tar.gz" | cut -d' ' -f1; }
x64=$(sha linux-x64)
arm64=$(sha linux-arm64)

mkdir -p "$out"
sed -e "s/@VERSION@/$version/" -e "s/@SHA256_X64@/$x64/" -e "s/@SHA256_ARM64@/$arm64/" \
    "$root/packaging/aur/PKGBUILD.in" >"$out/PKGBUILD"
(cd "$out" && makepkg --printsrcinfo >.SRCINFO)
echo "$out/PKGBUILD"
