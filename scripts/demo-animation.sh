#!/usr/bin/env bash
# Makes docs/demo.webp, the animation on the website and in the README, from
# the frames of the demo_frames test. It needs ImageMagick
# (brew install imagemagick).
#
#   scripts/demo-animation.sh
set -euo pipefail

cd "$(dirname "$0")/.."
rm -rf target/demo-frames
cargo test -p drivecraft demo_frames -- --ignored
# One frame each 2.5 seconds; the animation repeats forever. Lossless WebP
# at the full size of the frames (2 pixels for each point), so the text is
# sharp on high-density displays, with all colors. The page shows it at
# half of its size.
magick -delay 250 -loop 0 target/demo-frames/*.png -define webp:lossless=true docs/demo.webp
ls -l docs/demo.webp
