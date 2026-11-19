#!/usr/bin/env bash
# Builds a Windows installer from each Windows zip in a folder, with Inno
# Setup 6.3 or later (iscc in the PATH). It runs in Git Bash on Windows.
#
#   scripts/windows-installer.sh dist
#
# For dist/DriveCraft-<version>-windows-<arch>.zip, it writes
# dist/DriveCraft-<version>-windows-<arch>-setup.exe.
set -euo pipefail

dist=${1:?"Give the folder with the Windows zip files."}
root=$(cd "$(dirname "$0")/.." && pwd)
shopt -s nullglob
zips=("$dist"/DriveCraft-*-windows-*.zip)
if [ ${#zips[@]} -eq 0 ]; then
    echo "No Windows zip file in $dist." >&2
    exit 1
fi

for zip in "${zips[@]}"; do
    name=$(basename "$zip" .zip)          # DriveCraft-0.1.2-windows-x64
    arch=${name##*-windows-}              # x64 or arm64
    version=${name#DriveCraft-}
    version=${version%-windows-*}         # 0.1.2
    work="$root/target/installer/$arch"
    rm -rf "$work"
    mkdir -p "$work"
    unzip -q "$zip" -d "$work"
    # Git Bash changes arguments that look like paths. /D is not a path.
    MSYS_NO_PATHCONV=1 iscc /Q \
        "/DVersion=$version" "/DArch=$arch" \
        "/DSource=$(cygpath -w "$work/DriveCraft")" \
        "/DOutput=$(cygpath -w "$(cd "$dist" && pwd)")" \
        "$(cygpath -w "$root/packaging/windows/drivecraft.iss")"
    echo "$dist/$name-setup.exe"
done
