#!/bin/sh
set -eu

REPO="${GHSTPRTCL_REPO:-BrownJ3/GhostCom}"
VERSION="${GHSTPRTCL_VERSION:-latest}"
INSTALL_DIR="${GHSTPRTCL_INSTALL_DIR:-$HOME/.local/bin}"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

need curl
need tar

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS:$ARCH" in
  Darwin:arm64) TARGET="aarch64-apple-darwin" ;;
  Linux:x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
  *) echo "Unsupported platform: $OS $ARCH" >&2; exit 1 ;;
esac

ASSET="ghstprtcl-$TARGET.tar.gz"
BASE="https://github.com/$REPO/releases"

if [ "$VERSION" = "latest" ]; then
  DOWNLOAD="$BASE/latest/download/$ASSET"
  SUMS="$BASE/latest/download/SHA256SUMS"
else
  DOWNLOAD="$BASE/download/$VERSION/$ASSET"
  SUMS="$BASE/download/$VERSION/SHA256SUMS"
fi

TMP="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

echo "Downloading $ASSET"
curl -fL "$DOWNLOAD" -o "$TMP/$ASSET"
curl -fL "$SUMS" -o "$TMP/SHA256SUMS"

EXPECTED="$(grep " $ASSET\$" "$TMP/SHA256SUMS" | awk '{print $1}')"
if [ -z "$EXPECTED" ]; then
  echo "Checksum for $ASSET was not found" >&2
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$TMP/$ASSET" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL="$(shasum -a 256 "$TMP/$ASSET" | awk '{print $1}')"
else
  echo "Missing sha256sum or shasum for checksum verification" >&2
  exit 1
fi

if [ "$EXPECTED" != "$ACTUAL" ]; then
  echo "Checksum verification failed" >&2
  exit 1
fi

mkdir -p "$INSTALL_DIR"
tar -xzf "$TMP/$ASSET" -C "$TMP"
install -m 0755 "$TMP/ghstprtcl" "$INSTALL_DIR/ghstprtcl"

echo "Installed ghstprtcl to $INSTALL_DIR/ghstprtcl"
echo "Run: ghstprtcl"
