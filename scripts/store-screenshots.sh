#!/usr/bin/env bash
# Makes the screenshots in docs/screenshots from the screenshots test: the
# light pages, 1600 pixels wide. The AppStream metadata (Flathub and other
# app stores) links to them on the website.
# It needs ImageMagick (brew install imagemagick).
#
#   scripts/store-screenshots.sh
set -euo pipefail

cd "$(dirname "$0")/.."
cargo test -p drivecraft screenshots -- --ignored
for page in overview add-account account-folders activity; do
    magick "target/screenshots/$page-light.png" -strip "docs/screenshots/$page.png"
done
ls -l docs/screenshots
