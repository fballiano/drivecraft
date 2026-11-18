#!/usr/bin/env bash
# Has Apple notarize a signed DriveCraft.app, staples the ticket to it, and
# makes its zip again.
#
#   scripts/notarize.sh <path/DriveCraft.app> <dist/DriveCraft-....zip>
#
# Needs an App Store Connect API key in the environment:
#   NOTARY_KEY_P8_BASE64   the .p8 key, in base64
#   NOTARY_KEY_ID          the ID of that key
#   NOTARY_ISSUER_ID       the issuer ID of that key
set -euo pipefail

app=$1
zip=$2
: "${NOTARY_KEY_P8_BASE64:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER_ID:?}"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
key="$work/notary-key.p8"
echo "$NOTARY_KEY_P8_BASE64" | base64 --decode >"$key"
auth=(--key "$key" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID")

ditto -c -k --norsrc --noextattr --keepParent "$app" "$work/submit.zip"
set +e
xcrun notarytool submit "$work/submit.zip" "${auth[@]}" \
    --wait --timeout 45m --output-format json >"$work/result.json"
status=$?
set -e
cat "$work/result.json"
id=$(plutil -extract id raw "$work/result.json" 2>/dev/null || true)
result=$(plutil -extract status raw "$work/result.json" 2>/dev/null || true)
if [ $status -ne 0 ] || [ "$result" != "Accepted" ]; then
    # The log names each refused file and the reason.
    if [ -n "$id" ]; then
        xcrun notarytool log "$id" "${auth[@]}"
    fi
    echo "::error::Notarization did not succeed: ${result:-no answer}" >&2
    exit 1
fi

# The stapled ticket lets Gatekeeper accept the app also without internet.
xcrun stapler staple "$app"
spctl --assess --type execute --verbose=4 "$app"
rm -f "$zip"
ditto -c -k --norsrc --noextattr --keepParent "$app" "$zip"
echo "$zip"
