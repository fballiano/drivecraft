#!/usr/bin/env bash
# Makes docs/demo.gif from the frames of the demo_frames test.
# It needs ImageMagick (brew install imagemagick).
#
#   scripts/demo-gif.sh
set -euo pipefail

cd "$(dirname "$0")/.."
rm -rf target/demo-frames
cargo test -p drivecraft demo_frames -- --ignored
# One frame each 2.5 seconds; the GIF repeats forever. Each frame gets its
# own palette and no dithering, so no text of one frame shows in the next.
# (gifski blends near frames, and left faint text of other frames.)
magick -delay 250 -loop 0 target/demo-frames/*.png -resize 1080x \
    -dither None -colors 256 -layers OptimizeTransparency docs/demo.gif
ls -l docs/demo.gif
