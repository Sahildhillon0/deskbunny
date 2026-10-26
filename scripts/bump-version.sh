#!/usr/bin/env bash
# Set the DeskBunny version everywhere it lives, so the next push to main
# publishes a release for it (see .github/workflows/release.yml).
#   scripts/bump-version.sh 0.2.0
set -euo pipefail

new=${1:-}
if ! [[ $new =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: $0 X.Y.Z" >&2
  exit 1
fi
cd "$(dirname "$0")/.."
old=$(sed -n 's/^  "version": "\(.*\)",$/\1/p' app/src-tauri/tauri.conf.json)

sed -i.bak "s/^  \"version\": \"$old\",$/  \"version\": \"$new\",/" app/src-tauri/tauri.conf.json
sed -i.bak "0,/^version = \"$old\"$/s//version = \"$new\"/" Cargo.toml
sed -i.bak "s/^#define DB_FW_VERSION \"$old\"$/#define DB_FW_VERSION \"$new\"/" firmware/deskbunny/deskbunny.ino
rm -f app/src-tauri/tauri.conf.json.bak Cargo.toml.bak firmware/deskbunny/deskbunny.ino.bak
(cd app && npm version "$new" --no-git-tag-version --allow-same-version >/dev/null)
cargo update --workspace --quiet

grep -q "\"version\": \"$new\"" app/src-tauri/tauri.conf.json
grep -q "^version = \"$new\"" Cargo.toml
grep -q "DB_FW_VERSION \"$new\"" firmware/deskbunny/deskbunny.ino
echo "Version $old -> $new. Commit and push to main to publish v$new:"
echo "  git commit -am \"release: v$new\" && git push"
