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

# The runner image can already have the packages.
missing=()
for package in "$@"; do
    dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -q "install ok installed" \
        || missing+=("$package")
done
if [ ${#missing[@]} -eq 0 ]; then
    echo "All packages are installed: $*"
    exit 0
fi

azure=http://azure.archive.ubuntu.com/ubuntu/
shopt -s nullglob
for file in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
    [ -f "$file" ] || continue
    sudo sed -i -E \
        -e "s#mirror\+file:/etc/apt/apt-mirrors\.txt#$azure#g" \
        -e "s#https?://(security|archive)\.ubuntu\.com/ubuntu/?#$azure#g" \
        "$file"
done
echo "The sources of apt:"
grep -hE "^(deb |URIs:)" /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources 2>/dev/null | sort -u

apt=(sudo apt-get -o Acquire::Retries=5)
"${apt[@]}" update
"${apt[@]}" install -y "${missing[@]}"
