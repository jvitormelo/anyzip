#!/usr/bin/env bash
# Replaces crates/unrar-sys/vendor/unrar with an official RARLAB source release, after checking
# its sha256 (take it from a second source: the release notes, a distro package, or a download
# on another machine), then runs everything that exercises UnRAR.
#
#   scripts/update-unrar.sh 7.3.2 f61a1160d6e2ebbda2ad353c8e82f62ab4312f97fde23c6c2c982d25e0014f99
#
# The build patches ErrHandler to thread_local (crates/unrar-sys/build.rs) and asserts the pattern
# is there: if a release changes it, the build fails loudly instead of going thread-unsafe.
set -euo pipefail
VERSION="${1:?usage: update-unrar.sh VERSION SHA256}"
SHA256="${2:?usage: update-unrar.sh VERSION SHA256}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR="$ROOT/crates/unrar-sys/vendor"
TARBALL="unrarsrc-$VERSION.tar.gz"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "https://www.rarlab.com/rar/$TARBALL" -o "$TMP/$TARBALL"
echo "$SHA256  $TMP/$TARBALL" | sha256sum -c -
tar -xzf "$TMP/$TARBALL" -C "$TMP"
test -f "$TMP/unrar/dll.hpp"

rm -rf "$VENDOR/unrar"
mv "$TMP/unrar" "$VENDOR/unrar"
printf '%s\n%s sha256 %s\n' "$VERSION" "$TARBALL" "$SHA256" > "$VENDOR/VERSION"
cp "$VENDOR/unrar/license.txt" "$ROOT/crates/unrar-sys/LICENSE-UNRAR.txt"

cd "$ROOT"
cargo test --workspace # struct layout probes, golden (rar fixtures)
cargo build --release -p anyzip --example dump
python3 -I scripts/diff-bsdtar.py fixtures/libarchive/*.rar
if [ -d fixtures/real ]; then python3 -I scripts/diff-bsdtar.py fixtures/real/*; fi
(cd npm && bun run build && bun test && node --test test/*.test.ts)
echo "UnRAR $VERSION vendored. Review: git diff --stat crates/unrar-sys"
