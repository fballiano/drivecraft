#!/usr/bin/env bash
# Installs Ubuntu packages on a GitHub runner.
#
#   scripts/ci-apt.sh <package>...
#
# GitHub runs in Azure, so the Azure mirror of the Ubuntu archive is near.
# The x64 runners also use archive.ubuntu.com and security.ubuntu.com, and
# a failure there stops the job. This script uses only the Azure mirror, and
# retries each download. The Arm runners use ports.ubuntu.com; for them,
# nothing changes.
set -euo pipefail

azure=http://azure.archive.ubuntu.com/ubuntu/
sources=/etc/apt/sources.list.d/ubuntu.sources
if [ -f "$sources" ]; then
    sudo sed -i \
        -e "s#mirror+file:/etc/apt/apt-mirrors.txt#$azure#" \
        -e "s#http://security.ubuntu.com/ubuntu/\{0,1\}#$azure#" \
        -e "s#http://archive.ubuntu.com/ubuntu/\{0,1\}#$azure#" \
        "$sources"
fi
apt=(sudo apt-get -o Acquire::Retries=5)
"${apt[@]}" update
"${apt[@]}" install -y "$@"
