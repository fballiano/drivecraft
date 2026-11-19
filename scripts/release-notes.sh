#!/usr/bin/env bash
# Prints the release notes of one version as Markdown, from the <release>
# element in the AppStream metadata. Flathub requires that element, so the
# notes are written only once.
#
#   scripts/release-notes.sh 0.8.0
set -euo pipefail

version=${1:?"Give the version, for example 0.8.0."}
root=$(cd "$(dirname "$0")/.." && pwd)
metainfo="$root/packaging/linux/io.github.fballiano.drivecraft.metainfo.xml"

python3 - "$metainfo" "$version" <<'EOF'
import sys
import xml.etree.ElementTree as ET

path, version = sys.argv[1], sys.argv[2]
for release in ET.parse(path).getroot().iter("release"):
    if release.get("version") != version:
        continue
    items = [" ".join("".join(li.itertext()).split()) for li in release.iter("li")]
    if not items:
        sys.exit(f"The release {version} has no <li> items in {path}.")
    print("## Changes\n")
    for item in items:
        print(f"- {item}")
    break
else:
    sys.exit(f"No <release version=\"{version}\"> in {path}.")
EOF
