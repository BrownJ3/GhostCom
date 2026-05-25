pub const INSTALL_SH: &str = r#"#!/bin/sh
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
"#;

pub const INSTALL_PS1: &str = r#"$ErrorActionPreference = "Stop"

$Repo = if ($env:GHSTPRTCL_REPO) { $env:GHSTPRTCL_REPO } else { "BrownJ3/GhostCom" }
$Version = if ($env:GHSTPRTCL_VERSION) { $env:GHSTPRTCL_VERSION } else { "latest" }
$InstallDir = if ($env:GHSTPRTCL_INSTALL_DIR) { $env:GHSTPRTCL_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "ghstprtcl\bin" }

if (-not [Environment]::Is64BitOperatingSystem) {
  throw "Only 64-bit Windows is currently supported."
}

$Asset = "ghstprtcl-x86_64-pc-windows-msvc.zip"
$Base = "https://github.com/$Repo/releases"

if ($Version -eq "latest") {
  $Download = "$Base/latest/download/$Asset"
  $Sums = "$Base/latest/download/SHA256SUMS"
} else {
  $Download = "$Base/download/$Version/$Asset"
  $Sums = "$Base/download/$Version/SHA256SUMS"
}

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ghstprtcl-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Force $Tmp | Out-Null

try {
  $ArchivePath = Join-Path $Tmp $Asset
  $SumsPath = Join-Path $Tmp "SHA256SUMS"

  Write-Host "Downloading $Asset"
  Invoke-WebRequest -Uri $Download -OutFile $ArchivePath
  Invoke-WebRequest -Uri $Sums -OutFile $SumsPath

  $Line = Get-Content $SumsPath | Where-Object { $_ -match "\s$([regex]::Escape($Asset))$" } | Select-Object -First 1
  if (-not $Line) {
    throw "Checksum for $Asset was not found."
  }

  $Expected = ($Line -split "\s+")[0].ToLowerInvariant()
  $Actual = (Get-FileHash -Algorithm SHA256 $ArchivePath).Hash.ToLowerInvariant()

  if ($Expected -ne $Actual) {
    throw "Checksum verification failed."
  }

  Expand-Archive -Path $ArchivePath -DestinationPath $Tmp -Force
  New-Item -ItemType Directory -Force $InstallDir | Out-Null
  Copy-Item (Join-Path $Tmp "ghstprtcl.exe") (Join-Path $InstallDir "ghstprtcl.exe") -Force

  $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
  if (($UserPath -split ";") -notcontains $InstallDir) {
    [Environment]::SetEnvironmentVariable("Path", "$UserPath;$InstallDir", "User")
    Write-Host "Added $InstallDir to your user PATH. Open a new terminal before running ghstprtcl."
  }

  Write-Host "Installed ghstprtcl to $InstallDir"
  Write-Host "Run: ghstprtcl"
} finally {
  Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
"#;
