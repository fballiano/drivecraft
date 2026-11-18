#!/usr/bin/env bash
# Builds DriveCraft for one target and packages it with the bundled rclone.
#
#   scripts/package.sh <target> <name> <version>
#   scripts/package.sh aarch64-apple-darwin macos-arm64 0.1.0
#
# The result goes to dist/:
#   macOS:   DriveCraft-<version>-<name>.zip (DriveCraft.app, ad-hoc signed)
#   Windows: DriveCraft-<version>-<name>.zip
#   Linux:   DriveCraft-<version>-<name>.tar.gz
set -euo pipefail

target=$1
name=$2
version=$3
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

cargo build --release --locked -p drivecraft --target "$target"

exe=""
case "$target" in *windows*) exe=".exe" ;; esac
bin="target/$target/release/drivecraft$exe"

# build.rs downloads rclone into its OUT_DIR, in a folder named after the
# rclone version.
rclone_version=$(sed -n 's/^const RCLONE_VERSION: &str = "\(.*\)";/\1/p' crates/app/build.rs)
rclone=$(find "target/$target/release/build" -type f -path "*/out/$rclone_version/rclone$exe" | head -n 1)
if [ -z "$rclone" ]; then
    echo "The rclone binary from build.rs is missing." >&2
    exit 1
fi

stage="target/package/$name"
rm -rf "$stage"
mkdir -p "$stage" dist
package="DriveCraft-$version-$name"

copy_licenses() {
    mkdir -p "$1"
    cp LICENSE-MIT LICENSE-APACHE ATTRIBUTION.md "$1/"
    cp licenses/rclone.txt "$1/rclone-LICENSE.txt"
    cp crates/app/assets/fonts/OFL-Inter.txt "$1/Inter-OFL.txt"
    cp crates/app/assets/icons/LICENSE-lucide.txt "$1/Lucide-LICENSE.txt"
}

case "$target" in
*apple-darwin)
    app="$stage/DriveCraft.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$bin" "$app/Contents/MacOS/DriveCraft"
    cp "$rclone" "$app/Contents/Resources/rclone"
    cp crates/app/assets/DriveCraft.icns "$app/Contents/Resources/DriveCraft.icns"
    copy_licenses "$app/Contents/Resources/licenses"
    # CFBundleShortVersionString takes only digits and dots.
    plain_version=$(echo "$version" | sed 's/[^0-9.].*//')
    cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>DriveCraft</string>
    <key>CFBundleDisplayName</key><string>DriveCraft</string>
    <key>CFBundleIdentifier</key><string>io.github.fballiano.drivecraft</string>
    <key>CFBundleExecutable</key><string>DriveCraft</string>
    <key>CFBundleIconFile</key><string>DriveCraft</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>${plain_version:-0.0.0}</string>
    <key>CFBundleVersion</key><string>${plain_version:-0.0.0}</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
    # Ad-hoc signature: Apple Silicon needs a signed bundle. This is not a
    # Developer ID signature, so Gatekeeper still asks on the first start.
    codesign --force --deep --sign - "$app"
    ditto -c -k --norsrc --noextattr --keepParent "$app" "dist/$package.zip"
    ;;
*windows*)
    dir="$stage/DriveCraft"
    mkdir -p "$dir"
    cp "$bin" "$dir/DriveCraft.exe"
    cp "$rclone" "$dir/rclone.exe"
    copy_licenses "$dir/licenses"
    (cd "$stage" && 7z a -tzip "$root/dist/$package.zip" DriveCraft >/dev/null)
    ;;
*linux*)
    dir="$stage/DriveCraft"
    mkdir -p "$dir"
    cp "$bin" "$dir/drivecraft"
    cp "$rclone" "$dir/rclone"
    copy_licenses "$dir/licenses"
    tar -C "$stage" -czf "dist/$package.tar.gz" DriveCraft
    ;;
*)
    echo "No package format for $target." >&2
    exit 1
    ;;
esac

echo "dist/$(ls dist | grep -F "$package")"
