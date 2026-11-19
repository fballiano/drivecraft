#!/usr/bin/env bash
# Writes the winget manifests of a published release into target/winget/,
# in the layout of microsoft/winget-pkgs. It downloads the two installers
# of the release to compute their SHA-256 checksums.
#
#   scripts/winget.sh 0.1.2
#
# Use it for the first submission to winget-pkgs. Later releases go there
# from the release workflow (winget-releaser).
set -euo pipefail

version=${1:?"Give the version, for example 0.1.2."}
root=$(cd "$(dirname "$0")/.." && pwd)
id=FabrizioBalliano.DriveCraft
out="$root/target/winget/manifests/f/FabrizioBalliano/DriveCraft/$version"
base="https://github.com/fballiano/drivecraft/releases/download/v$version"
# The AppId in packaging/windows/drivecraft.iss, with the suffix of Inno Setup.
product_code='{3A7893EC-F309-4316-BD01-E6193C68A893}_is1'
schema=1.12.0
release_date=$(gh release view "v$version" --repo fballiano/drivecraft --json publishedAt -q '.publishedAt[:10]')

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
sha() {
    curl -sSfL -o "$tmp/$1" "$base/$1"
    shasum -a 256 "$tmp/$1" | cut -d' ' -f1 | tr '[:lower:]' '[:upper:]'
}
x64="DriveCraft-$version-windows-x64-setup.exe"
arm64="DriveCraft-$version-windows-arm64-setup.exe"
x64_sha=$(sha "$x64")
arm64_sha=$(sha "$arm64")

rm -rf "$out"
mkdir -p "$out"

cat >"$out/$id.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.version.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
EOF

cat >"$out/$id.installer.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.installer.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $version
InstallerType: inno
Scope: user
UpgradeBehavior: install
ReleaseDate: $release_date
ProductCode: '$product_code'
Installers:
- Architecture: x64
  InstallerUrl: $base/$x64
  InstallerSha256: $x64_sha
- Architecture: arm64
  InstallerUrl: $base/$arm64
  InstallerSha256: $arm64_sha
ManifestType: installer
ManifestVersion: $schema
EOF

cat >"$out/$id.locale.en-US.yaml" <<EOF
# yaml-language-server: \$schema=https://aka.ms/winget-manifest.defaultLocale.$schema.schema.json

PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: Fabrizio Balliano
PublisherUrl: https://github.com/fballiano
PublisherSupportUrl: https://github.com/fballiano/drivecraft/issues
Author: Fabrizio Balliano
PackageName: DriveCraft
PackageUrl: https://drivecraft.fabrizioballiano.com/
License: MIT OR Apache-2.0
LicenseUrl: https://github.com/fballiano/drivecraft/blob/main/LICENSE-MIT
ShortDescription: Open source client for OneDrive, Dropbox, Box, and more, built on rclone.
Description: |-
  DriveCraft keeps real copies of your cloud files in a folder on your computer, and syncs them in both directions.
  You select the folders that sync. The other folders stay only online.
  It works with OneDrive, Dropbox, Box, pCloud, kDrive, HiDrive, and Yandex Disk, and with more than one account at a time.
  DriveCraft uses rclone for the sync, and includes it.
Tags:
- box
- cloud
- dropbox
- onedrive
- pcloud
- rclone
- sync
ReleaseNotesUrl: https://github.com/fballiano/drivecraft/releases/tag/v$version
ManifestType: defaultLocale
ManifestVersion: $schema
EOF

echo "$out"
