#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="$root/dist"

if command -v gsed >/dev/null 2>&1; then
    sed_cmd=gsed
else
    sed_cmd=sed
fi

rm -rf "$out"
mkdir -p "$out/preview"
cp -R "$root/static" "$out/static"
cp "$root/preview/mock.js" "$out/preview/mock.js"
cp -R "$root/preview/fixtures" "$out/preview/fixtures"

"$sed_cmd" -e 's#^\( *\)<script type="module" src="/static/index.js"></script>#\1<script src="/preview/mock.js"></script>\n&#' \
    "$root/static/index.html" >"$out/index.html"

grep -q '/preview/mock.js' "$out/index.html"
