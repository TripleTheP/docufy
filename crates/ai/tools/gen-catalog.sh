#!/usr/bin/env bash
# Prints Rust `Variant` entries for model folders on Hugging Face, pinned to the current commit:
# every file (no Python, no README) with its size and SHA-256. LFS files carry their SHA-256 in the
# API; small files are downloaded and hashed here.
# Usage: gen-catalog.sh <repo> <dir or .>
set -euo pipefail
repo=$1
dir=$2
[ "$dir" = "." ] && dir=""
rev=$(curl -sS "https://huggingface.co/api/models/$repo" | grep -o '"sha":"[^"]*"' | head -1 | cut -d'"' -f4)
url="https://huggingface.co/api/models/$repo/tree/$rev"
[ -n "$dir" ] && url="$url/$dir"
json=$(curl -sS "$url")
tmp=$(mktemp -d)
echo "        // https://huggingface.co/$repo/tree/$rev/$dir"
echo "        repo: \"$repo\","
echo "        revision: \"$rev\","
echo "        dir: \"$dir\","
echo "        files: &["
# One object per file: split on "},{" at top level is fragile; use the order type,oid,size,(lfs),path.
echo "$json" | sed 's/},{"type"/}\n{"type"/g' | while IFS= read -r obj; do
  echo "$obj" | grep -q '"type":"file"' || continue
  path=$(echo "$obj" | grep -o '"path":"[^"]*"' | cut -d'"' -f4)
  size=$(echo "$obj" | grep -o '"size":[0-9]*' | head -1 | cut -d: -f2)
  name=${path##*/}
  case "$name" in *.py|*.md|.gitattributes|*.png|*.jpg|LICENSE*|*.jinja) continue;; esac
  sha=$(echo "$obj" | grep -o '"lfs":{"oid":"[^"]*"' | cut -d'"' -f6 || true)
  if [ -z "$sha" ]; then
    curl -sSL -o "$tmp/f" "https://huggingface.co/$repo/resolve/$rev/$path"
    sha=$(sha256sum "$tmp/f" | cut -d' ' -f1)
    size=$(stat -c %s "$tmp/f")
  fi
  echo "            ModelFile { name: \"$name\", size: $size, sha256: \"$sha\" },"
done
echo "        ],"
rm -rf "$tmp"
