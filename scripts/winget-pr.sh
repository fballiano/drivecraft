#!/usr/bin/env bash
# Moves the open winget-pkgs pull request of DriveCraft to a new release.
#
#   scripts/winget-pr.sh 0.7.0
#
# The pull request (microsoft/winget-pkgs#484091, branch drivecraft-0.1.3 of
# fballiano/winget-pkgs) adds the first version of FabrizioBalliano.DriveCraft.
# Until Microsoft merges it, each release replaces its manifests: one commit
# on top of the master of the fork, with only the new version.
# After the merge, the release workflow sends new versions (winget-releaser).
#
# It needs gh with write access to fballiano/winget-pkgs.
set -euo pipefail

version=${1:?"Give the version, for example 0.7.0."}
root=$(cd "$(dirname "$0")/.." && pwd)
fork=fballiano/winget-pkgs
branch=drivecraft-0.1.3
pr=484091
dir=manifests/f/FabrizioBalliano/DriveCraft/$version

"$root/scripts/winget.sh" "$version" >/dev/null
src="$root/target/winget/$dir"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

base=$(gh api "repos/$fork/branches/master" -q .commit.sha)
base_tree=$(gh api "repos/$fork/git/commits/$base" -q .tree.sha)

entries=()
for file in "$src"/*.yaml; do
    name=$(basename "$file")
    base64 <"$file" | tr -d '\n' | jq -Rn '{encoding: "base64", content: input}' >"$tmp/blob.json"
    blob=$(gh api "repos/$fork/git/blobs" --input "$tmp/blob.json" -q .sha)
    entries+=("$(jq -n --arg p "$dir/$name" --arg s "$blob" '{path: $p, mode: "100644", type: "blob", sha: $s}')")
done
printf '%s\n' "${entries[@]}" | jq -s --arg base "$base_tree" '{base_tree: $base, tree: .}' >"$tmp/tree.json"
tree=$(gh api "repos/$fork/git/trees" --input "$tmp/tree.json" -q .sha)
commit=$(gh api "repos/$fork/git/commits" \
    -f message="New package: FabrizioBalliano.DriveCraft version $version" \
    -f tree="$tree" -f "parents[]=$base" -q .sha)
gh api -X PATCH "repos/$fork/git/refs/heads/$branch" -f sha="$commit" -F force=true >/dev/null

# Each file on GitHub must be the same as the local file.
for file in "$src"/*.yaml; do
    curl -sSf "https://raw.githubusercontent.com/$fork/$commit/$dir/$(basename "$file")" | cmp - "$file"
done

# The title, and the version in the description.
gh pr view "$pr" -R microsoft/winget-pkgs --json body -q .body >"$tmp/body.md"
sed -i.bak -E \
    -e "s/version \*\*[0-9.]+\*\*/version **$version**/" \
    -e "s#releases/tag/v[0-9.]+#releases/tag/v$version#" \
    "$tmp/body.md"
gh pr edit "$pr" -R microsoft/winget-pkgs \
    --title "New package: FabrizioBalliano.DriveCraft version $version" \
    --body-file "$tmp/body.md" >/dev/null
echo "https://github.com/microsoft/winget-pkgs/pull/$pr now has version $version (commit $commit)."
