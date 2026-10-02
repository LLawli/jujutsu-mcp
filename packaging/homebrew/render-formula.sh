#!/usr/bin/env bash
# Renders the Homebrew formula for VERSION from the release archives'
# .sha256 files in ARTIFACT_DIR, on stdout.
set -euo pipefail

if [ "$#" -ne 2 ]; then
  echo "usage: $0 VERSION ARTIFACT_DIR" >&2
  exit 2
fi
version=$1
artifacts=$2
template="$(dirname "$0")/jujutsu-mcp.rb.in"

formula=$(sed "s/@VERSION@/$version/g" "$template")
for target in aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-musl x86_64-unknown-linux-musl; do
  sum_file="$artifacts/jujutsu-mcp-$target.tar.gz.sha256"
  sha=$(awk '{print $1}' "$sum_file")
  if ! [[ "$sha" =~ ^[0-9a-f]{64}$ ]]; then
    echo "$0: no valid sha256 in $sum_file" >&2
    exit 1
  fi
  formula=${formula//"@SHA256_$target@"/$sha}
done

if grep -q "@[A-Z0-9_a-z-]*@" <<<"$formula"; then
  echo "$0: unfilled placeholder in the formula" >&2
  exit 1
fi
printf '%s\n' "$formula"
