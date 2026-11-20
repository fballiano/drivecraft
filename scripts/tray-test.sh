#!/usr/bin/env bash
# Tests the Linux tray icon in Docker, from macOS or Linux:
#
#   scripts/tray-test.sh [wayland|x11]
#
# It builds DriveCraft for Linux in a container (the target folder is the
# Docker volume drivecraft-target, so the build of this computer does not
# change), then runs scripts/tray-test/run.sh. The screenshots and logs go
# to target/tray-test.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
docker build -q -t drivecraft-tray-test "$root/scripts/tray-test" >/dev/null
mkdir -p "$root/target/tray-test"
docker run --rm \
    -v "$root":/src:ro \
    -v "$root/target/tray-test":/out \
    -v drivecraft-target:/target \
    -v drivecraft-cargo:/root/.cargo/registry \
    -e CARGO_TARGET_DIR=/target -w /src \
    drivecraft-tray-test \
    bash -c 'cargo build -p drivecraft && scripts/tray-test/run.sh /target/debug/drivecraft /out "$@"' -- "$@"
