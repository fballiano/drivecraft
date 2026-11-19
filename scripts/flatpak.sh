#!/usr/bin/env bash
# The Flatpak of DriveCraft (packaging/flatpak/).
#
#   scripts/flatpak.sh sources        writes packaging/flatpak/cargo-sources.json
#   scripts/flatpak.sh build          builds and installs the Flatpak of this checkout
#   scripts/flatpak.sh flathub <tag>  writes target/flathub/: the manifest and the
#                                     sources for the Flathub repository
#
# Flathub builds offline, so cargo-sources.json lists each crate in
# Cargo.lock. Make it again after each change of Cargo.lock.
# "sources" needs Python 3. "build" needs flatpak and flatpak-builder (Linux).
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
app=io.github.fballiano.drivecraft
manifest="packaging/flatpak/$app.yml"
# A fixed commit of flatpak-builder-tools, so the generator does not change
# without a review.
tools_commit=9d86d336a39b9af38b86f114b6ce5820fe260f91

sources() {
    local tools="target/flatpak-tools"
    local generator="$tools/flatpak-cargo-generator.py"
    if [ ! -f "$generator" ]; then
        mkdir -p "$tools"
        curl -sSfL -o "$generator" \
            "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$tools_commit/cargo/flatpak-cargo-generator.py"
    fi
    if [ ! -d "$tools/venv" ]; then
        python3 -m venv "$tools/venv"
        "$tools/venv/bin/pip" install --quiet 'aiohttp>=3.9.5,<4' 'PyYAML>=6.0.2,<7' 'tomlkit>=0.13.3,<1'
    fi
    "$tools/venv/bin/python" -I "$generator" Cargo.lock -o "${1:-packaging/flatpak/cargo-sources.json}"
}

case "${1:-}" in
sources)
    sources
    ;;
build)
    [ -f packaging/flatpak/cargo-sources.json ] || sources
    flatpak-builder --user --install --force-clean --install-deps-from=flathub \
        target/flatpak-build "$manifest"
    echo "Start it with: flatpak run $app"
    ;;
flathub)
    tag=${2:?"Give the release tag, for example v0.1.2."}
    commit=$(git rev-parse "$tag^{commit}")
    out=target/flathub
    rm -rf "$out"
    mkdir -p "$out"
    sources "$out/cargo-sources.json"
    # Build the release tag instead of this checkout.
    python3 - "$manifest" "$out/$app.yml" "$tag" "$commit" <<'EOF'
import sys
src, dst, tag, commit = sys.argv[1:]
text = open(src).read()
start = text.index("      - type: dir\n")
end = text.index("      - cargo-sources.json\n")
git = (
    "      - type: git\n"
    "        url: https://github.com/fballiano/drivecraft.git\n"
    f"        tag: {tag}\n"
    f"        commit: {commit}\n"
)
text = text[:start] + git + text[end:]
text = text.replace("# scripts/flatpak.sh flathub replaces this source with the release tag.\n", "")
open(dst, "w").write(text)
EOF
    echo "Copy the files in $out to the Flathub repository."
    ;;
*)
    sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
    exit 1
    ;;
esac
