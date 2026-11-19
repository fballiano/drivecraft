#!/usr/bin/env bash
# Installs Ubuntu packages on a GitHub runner.
#
#   scripts/ci-apt.sh <package>...
#
# The x64 runners use archive.ubuntu.com, security.ubuntu.com, and the
# Azure mirror, and an outage of one of them stops the job. This script
# gives apt a list of mirrors instead: apt tries the next one when a mirror
# fails. apt verifies the signature of each InRelease file, so any mirror is
# safe. The Arm runners use ports.ubuntu.com; for them, nothing changes.
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

list=/etc/apt/drivecraft-mirrors.txt
sudo tee "$list" >/dev/null <<'EOF'
http://azure.archive.ubuntu.com/ubuntu/	priority:1
http://mirrors.edge.kernel.org/ubuntu/	priority:2
http://mirror.us.leaseweb.net/ubuntu/	priority:3
http://mirrors.mit.edu/ubuntu/	priority:4
http://ftp.halifax.rwth-aachen.de/ubuntu/	priority:5
EOF
shopt -s nullglob
for file in /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources; do
    [ -f "$file" ] || continue
    sudo sed -i -E \
        -e "s#mirror\+file:/etc/apt/apt-mirrors\.txt#mirror+file:$list#g" \
        -e "s#https?://(security|archive|azure\.archive)\.ubuntu\.com/ubuntu/?#mirror+file:$list#g" \
        "$file"
done
echo "The sources of apt:"
grep -hE "^(deb |URIs:)" /etc/apt/sources.list /etc/apt/sources.list.d/*.list /etc/apt/sources.list.d/*.sources 2>/dev/null | sort -u

apt=(sudo apt-get -o Acquire::Retries=3 -o Acquire::http::Timeout=20)
"${apt[@]}" update
"${apt[@]}" install -y "${missing[@]}"
