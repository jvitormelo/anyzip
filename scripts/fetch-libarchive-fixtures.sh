#!/usr/bin/env bash
# Downloads libarchive's test archives (BSD-2) for zip/7z/rar and uudecodes them into
# fixtures/libarchive/. Pinned to a commit so the golden files stay stable.
set -euo pipefail
COMMIT="${LIBARCHIVE_COMMIT:-d0bf2da07ebaa3964090ee936edea538d1602449}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/fixtures/libarchive"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
curl -fsSL "https://github.com/libarchive/libarchive/archive/$COMMIT.tar.gz" -o "$TMP/la.tar.gz"
tar -xzf "$TMP/la.tar.gz" -C "$TMP" --wildcards '*/libarchive/test/test_read_format_*' '*/COPYING'
mkdir -p "$OUT"
cp "$TMP"/libarchive-*/COPYING "$OUT/COPYING"
python3 -I "$ROOT/scripts/uudecode.py" "$OUT" "$TMP"/libarchive-*/libarchive/test/test_read_format_{zip,7zip,rar}*.uu
echo "$COMMIT" > "$OUT/COMMIT"
