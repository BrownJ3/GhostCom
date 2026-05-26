$ErrorActionPreference = "Stop"

$Repo = if ($env:GHSTPRTCL_REPO) { $env:GHSTPRTCL_REPO } else { "BrownJ3/GhostCom" }
$Version = if ($env:GHSTPRTCL_VERSION) { $env:GHSTPRTCL_VERSION } else { "v0.1.0-alpha.6" }
$InstallDir = if ($env:GHSTPRTCL_INSTALL_DIR) { $env:GHSTPRTCL_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "ghstprtcl\bin" }

if (-not [Environment]::Is64BitOperatingSystem) {
  throw "Only 64-bit Windows is currently supported."
}

$Asset = "ghstprtcl-x86_64-pc-windows-msvc.zip"
$Base = "https://github.com/$Repo/releases"

if ($Version -eq "latest") {
  $Download = "$Base/latest/download/$Asset"
  $Sums = "$Base/latest/download/SHA256SUMS"
  $Sig = "$Base/latest/download/SHA256SUMS.sig"
} else {
  $Download = "$Base/download/$Version/$Asset"
  $Sums = "$Base/download/$Version/SHA256SUMS"
  $Sig = "$Base/download/$Version/SHA256SUMS.sig"
}

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ghstprtcl-" + [System.Guid]::NewGuid())
New-Item -ItemType Directory -Force $Tmp | Out-Null

try {
  $ArchivePath = Join-Path $Tmp $Asset
  $SumsPath = Join-Path $Tmp "SHA256SUMS"
  $SigPath = Join-Path $Tmp "SHA256SUMS.sig"

  Write-Host "Downloading $Asset"
  Invoke-WebRequest -Uri $Download -OutFile $ArchivePath
  Invoke-WebRequest -Uri $Sums -OutFile $SumsPath
  Invoke-WebRequest -Uri $Sig -OutFile $SigPath

  $PublicKeyPem = @'
-----BEGIN PUBLIC KEY-----
MFkwEwYHKoZIzj0CAQYIKoZIzj0DAQcDQgAE7ksrBPCPrcno8t3lh/5QY93tETqZ
uTTallJhWhFA/RBoIzHJjsopPxzToTP+JC13v7cvM47K4ni9TMjEEYm05w==
-----END PUBLIC KEY-----
'@

  if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw "PowerShell 7 or newer is required for release signature verification."
  }

  $SumsBytes = [System.IO.File]::ReadAllBytes($SumsPath)
  $SigBytes = [System.IO.File]::ReadAllBytes($SigPath)
  $Ecdsa = [System.Security.Cryptography.ECDsa]::Create()
  try {
    $Ecdsa.ImportFromPem($PublicKeyPem)
    $SignatureOk = $Ecdsa.VerifyData(
      $SumsBytes,
      $SigBytes,
      [System.Security.Cryptography.HashAlgorithmName]::SHA256,
      [System.Security.Cryptography.DSASignatureFormat]::Rfc3279DerSequence
    )
  } finally {
    $Ecdsa.Dispose()
  }

  if (-not $SignatureOk) {
    throw "Release signature verification failed."
  }

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
