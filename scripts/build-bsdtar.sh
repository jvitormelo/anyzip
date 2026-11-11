#!/usr/bin/env bash
# Builds bsdtar from the exact commit in fixtures/libarchive/COMMIT (with xz 5.8.1 static so 7z
# RISC-V / ARM64 BCJ filters match sevenz-rust2 and liblzma) for CI differential testing.
set -euo pipefail

OUT="${1:?usage: build-bsdtar.sh OUTPUT_BINARY_PATH}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
COMMIT="$(tr -d '[:space:]' < "$ROOT/fixtures/libarchive/COMMIT")"

for pkg in zlib1g-dev libbz2-dev libzstd-dev libssl-dev cmake ninja-build pkg-config; do
  if ! dpkg -s "$pkg" >/dev/null 2>&1; then
    timeout 2 getent hosts azure.archive.ubuntu.com >/dev/null 2>&1 \
      || printf 'https://mirrors.edge.kernel.org/ubuntu/\n' | sudo tee /etc/apt/apt-mirrors.txt >/dev/null
    sudo apt-get update -qq
    sudo apt-get install -y --no-install-recommends -qq zlib1g-dev libbz2-dev libzstd-dev libssl-dev cmake ninja-build pkg-config
    break
  fi
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

curl -fsSL "https://github.com/tukaani-project/xz/releases/download/v5.8.1/xz-5.8.1.tar.gz" | tar -xz -C "$TMP"
cmake -S "$TMP/xz-5.8.1" -B "$TMP/xz-build" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DBUILD_SHARED_LIBS=OFF \
  -DXZ_TOOL_XZ=OFF -DXZ_TOOL_XZDEC=OFF -DXZ_TOOL_LZMADEC=OFF -DXZ_TOOL_LZMAINFO=OFF -DXZ_DOC=OFF \
  -DCMAKE_INSTALL_PREFIX="$TMP/prefix" >/dev/null
ninja -C "$TMP/xz-build" install >/dev/null

curl -fsSL "https://github.com/libarchive/libarchive/archive/$COMMIT.tar.gz" | tar -xz -C "$TMP"
cmake -S "$TMP/libarchive-$COMMIT" -B "$TMP/build" -G Ninja \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_PREFIX_PATH="$TMP/prefix" \
  -DBUILD_SHARED_LIBS=OFF \
  -DENABLE_TEST=OFF -DENABLE_CAT=OFF -DENABLE_CPIO=OFF -DENABLE_UNZIP=OFF -DENABLE_INSTALL=OFF >/dev/null
ninja -C "$TMP/build" bsdtar >/dev/null

mkdir -p "$(dirname "$OUT")"
cp "$TMP/build/bin/bsdtar" "$OUT"
"$OUT" --version
