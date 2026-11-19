#!/usr/bin/env bash
# Makes docs/social-preview.png (1280 x 640) for the GitHub social preview
# and the website. It needs ImageMagick 7 (brew install imagemagick).
#
#   scripts/social-preview.sh
set -euo pipefail

cd "$(dirname "$0")/.."
cargo test -p drivecraft screenshots -- --ignored
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fonts=crates/app/assets/fonts

magick target/screenshots/overview-dark.png -resize '800x533!' -alpha off "$tmp/shot-rgb.png"
magick -size 800x533 xc:black -fill white -draw "roundrectangle 0,0,799,532,18,18" "$tmp/mask.png"
magick "$tmp/shot-rgb.png" "$tmp/mask.png" -alpha off -compose CopyOpacity -composite "$tmp/shot.png"
magick "$tmp/shot.png" \( +clone -background black -shadow 60x24+0+16 \) +swap \
  -background none -layers merge +repage "$tmp/shot-shadow.png"
magick -size 2400x1600 radial-gradient:'#3A3270'-'#111112' -crop 1280x640+950+600 +repage "$tmp/bg.png"
magick "$tmp/bg.png" \
  \( docs/icon.png -resize 104x104 \) -geometry +68+146 -composite \
  -font "$fonts/Inter-SemiBold.ttf" -fill '#ECECF0' -pointsize 68 -annotate +70+330 'DriveCraft' \
  -font "$fonts/Inter-Regular.ttf" -fill '#A0A0AA' -pointsize 26 \
  -annotate +72+385 'Light, private, open source.' -annotate +72+421 'Replaces the official cloud apps.' \
  -font "$fonts/Inter-Medium.ttf" -fill '#D6D0FF' -pointsize 22 -annotate +72+490 'macOS  ·  Windows  ·  Linux' \
  "$tmp/shot-shadow.png" -geometry +590+40 -composite \
  -depth 8 docs/social-preview.png
ls -l docs/social-preview.png
