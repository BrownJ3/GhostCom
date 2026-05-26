#!/bin/sh
set -eu

REPO="${GHSTPRTCL_REPO:-BrownJ3/GhostCom}"
VERSION="${GHSTPRTCL_VERSION:-v0.1.0-alpha.10}"
INSTALL_DIR="${GHSTPRTCL_INSTALL_DIR:-$HOME/.local/bin}"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "Missing required command: $1" >&2
    exit 1
  }
}

need curl
need openssl
need tar

path_contains() {
  case ":$PATH:" in
    *":$1:"*) return 0 ;;
    *) return 1 ;;
  esac
}

default_profile() {
  shell_name="$(basename "${SHELL:-}")"
  case "$shell_name" in
    zsh) printf '%s\n' "$HOME/.zshrc" ;;
    bash) printf '%s\n' "$HOME/.bashrc" ;;
    *) printf '%s\n' "$HOME/.profile" ;;
  esac
}

shell_quote() {
  printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
}

ensure_path_entry() {
  if path_contains "$INSTALL_DIR"; then
    return 0
  fi

  profile="${GHSTPRTCL_PROFILE:-$(default_profile)}"
  marker="# ghstprtcl installer"
  mkdir -p "$(dirname "$profile")"
  touch "$profile"

  if ! grep -F "$marker" "$profile" >/dev/null 2>&1; then
    quoted_install_dir="$(shell_quote "$INSTALL_DIR")"
    {
      printf '\n%s\n' "$marker"
      printf 'ghstprtcl_dir=%s\n' "$quoted_install_dir"
      printf 'case ":$PATH:" in\n'
      printf '  *":$ghstprtcl_dir:"*) ;;\n'
      printf '  *) export PATH="$ghstprtcl_dir:$PATH" ;;\n'
      printf 'esac\n'
    } >> "$profile"
  fi

  ADDED_PATH_PROFILE="$profile"
}

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
  SIG="$BASE/latest/download/SHA256SUMS.sig"
else
  DOWNLOAD="$BASE/download/$VERSION/$ASSET"
  SUMS="$BASE/download/$VERSION/SHA256SUMS"
  SIG="$BASE/download/$VERSION/SHA256SUMS.sig"
fi

TMP="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

echo "Downloading $ASSET"
curl -fL "$DOWNLOAD" -o "$TMP/$ASSET"
curl -fL "$SUMS" -o "$TMP/SHA256SUMS"
curl -fL "$SIG" -o "$TMP/SHA256SUMS.sig"

cat > "$TMP/release-signing-public-key.pem" <<'EOF'
-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE7ksrBPCPrcno8t3lh/5QY93tETqZ
uTTallJhWhFA/RBoIzHJjsopPxzToTP+JC13v7cvM47K4ni9TMjEEYm05w==
-----END PUBLIC KEY-----
EOF

if ! openssl dgst -sha256 \
  -verify "$TMP/release-signing-public-key.pem" \
  -signature "$TMP/SHA256SUMS.sig" \
  "$TMP/SHA256SUMS" >/dev/null 2>&1; then
  echo "Release signature verification failed" >&2
  exit 1
fi

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
ADDED_PATH_PROFILE=""
ensure_path_entry

echo "Installed ghstprtcl to $INSTALL_DIR/ghstprtcl"
if [ -n "$ADDED_PATH_PROFILE" ]; then
  echo "Added $INSTALL_DIR to PATH in $ADDED_PATH_PROFILE"
  echo "Open a new terminal, then run: ghstprtcl"
else
  echo "Run: ghstprtcl"
fi
