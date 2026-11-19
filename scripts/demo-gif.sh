#!/usr/bin/env bash
# Makes docs/demo.gif from the frames of the demo_frames test.
# It needs gifski (brew install gifski).
#
#   scripts/demo-gif.sh
set -euo pipefail

cd "$(dirname "$0")/.."
rm -rf target/demo-frames
cargo test -p drivecraft demo_frames -- --ignored
# One frame each 2.5 seconds; the GIF repeats forever.
gifski --fps 0.4 --width 1080 --quality 100 -o docs/demo.gif target/demo-frames/*.png
ls -l docs/demo.gif
