param(
  [string]$Configuration = "release",
  [string]$Target = "x86_64-pc-windows-msvc",
  [string]$OutDir = ".build/Shenava-Windows",
  [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = Resolve-Path (Join-Path $ScriptDir "..\..\..")
$OverlayCrate = Join-Path $RepoRoot "desktop\linux\visualears-overlay"
$AssetRoot = Join-Path $ScriptDir "assets"
$FixtureRoot = Join-Path $ScriptDir "fixtures"
$ExeSource = Join-Path $OverlayCrate "target\$Target\$Configuration\visualears-overlay.exe"
$OutRoot = Join-Path $RepoRoot $OutDir
$ModelSource = Join-Path $AssetRoot "koochik_hd.onnx"
$ExpectedModelBytes = 138272920
$ExpectedModelSha256 = "64f5a3afbb5f603cdd44b56b3651d7a135562a69520e8a3386fe47b331c5a8f0"

if (!$SkipBuild) {
  Push-Location $OverlayCrate
  try {
    cargo xwin build --target $Target --release --features control-panel
  } finally {
    Pop-Location
  }
}

if (!(Test-Path $ExeSource)) {
  throw "Missing built Windows executable: $ExeSource"
}

if (!(Test-Path $ModelSource -PathType Leaf)) {
  throw "Missing Windows ASR model: $ModelSource. Run 'git lfs install' and 'git lfs pull'."
}
$ModelFile = Get-Item -LiteralPath $ModelSource
if ($ModelFile.Length -ne $ExpectedModelBytes) {
  throw "The Windows ASR model has the wrong size ($($ModelFile.Length) bytes, expected $ExpectedModelBytes). Run 'git lfs pull' and retry."
}
$ModelSha256 = (Get-FileHash -LiteralPath $ModelSource -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ModelSha256 -ne $ExpectedModelSha256) {
  throw "The Windows ASR model checksum does not match the contributor build ($ModelSha256). Run 'git lfs pull' and retry."
}

if (Test-Path $OutRoot) {
  Remove-Item -Recurse -Force $OutRoot
}

New-Item -ItemType Directory -Force -Path $OutRoot | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutRoot "Models\ShenavaStreaming") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutRoot "engine") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $OutRoot "fixtures") | Out-Null

Copy-Item $ExeSource (Join-Path $OutRoot "Shenava.exe")
Copy-Item (Join-Path $ScriptDir "Shenava.vbs") (Join-Path $OutRoot "Shenava.vbs")
Copy-Item (Join-Path $ScriptDir "Shenava.ico") (Join-Path $OutRoot "Shenava.ico")
Copy-Item (Join-Path $ScriptDir "run.ps1") (Join-Path $OutRoot "run.ps1")
Copy-Item (Join-Path $ScriptDir "README.md") (Join-Path $OutRoot "README.md")
Copy-Item (Join-Path $ScriptDir "*.cmd") $OutRoot
$PreoptModel = Join-Path $RepoRoot ".build\tract-preopt\koochik_hd.nnef.tgz"
if (Test-Path $PreoptModel) {
  Copy-Item $PreoptModel (Join-Path $OutRoot "Models\ShenavaStreaming\koochik_hd.nnef.tgz")
}
Copy-Item $ModelSource (Join-Path $OutRoot "Models\ShenavaStreaming\koochik_hd.onnx")
Copy-Item (Join-Path $AssetRoot "tokens.txt") (Join-Path $OutRoot "Models\ShenavaStreaming\tokens.txt")
Copy-Item (Join-Path $AssetRoot "hotwords_fa.txt") (Join-Path $OutRoot "Models\ShenavaStreaming\hotwords_fa.txt")
Copy-Item (Join-Path $AssetRoot "mel_filters_slaney_80x257.json") (Join-Path $OutRoot "engine\mel_filters_slaney_80x257.json")
Copy-Item (Join-Path $FixtureRoot "golha_clear.wav") (Join-Path $OutRoot "fixtures\golha_clear.wav")

$Zip = "$OutRoot.zip"
if (Test-Path $Zip) {
  Remove-Item -Force $Zip
}
Compress-Archive -Path (Join-Path $OutRoot "*") -DestinationPath $Zip

Write-Host "Wrote $OutRoot"
Write-Host "Wrote $Zip"
