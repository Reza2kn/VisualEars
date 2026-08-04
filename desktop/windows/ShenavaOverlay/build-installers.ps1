[CmdletBinding()]
param(
  [ValidateSet("x64", "arm64", "x86")]
  [string[]]$Architecture = @("x64", "arm64", "x86"),
  [ValidateSet("Debug", "Release")]
  [string]$Configuration = "Release",
  [string]$AppVersion = "",
  [string]$OutputDir = ".build/installers",
  [switch]$SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptDir "..\..\..")).Path
$OverlayCrate = Join-Path $RepoRoot "desktop\linux\visualears-overlay"
$BuildNativeScript = Join-Path $ScriptDir "build-native.ps1"
$InstallerScript = Join-Path $ScriptDir "ShenavaOverlay.iss"

$ArchitectureSpecs = @{
  "x64" = @{
    PackageDir = ".build/Shenava-Windows-x64"
    PeMachine = "x64"
    CompilerDefine = "/DARCH_X64"
  }
  "arm64" = @{
    PackageDir = ".build/Shenava-Windows-arm64"
    PeMachine = "arm64"
    CompilerDefine = "/DARCH_ARM64"
  }
  "x86" = @{
    PackageDir = ".build/Shenava-Windows-x86"
    PeMachine = "x86"
    CompilerDefine = "/DARCH_X86"
  }
}

function Get-InnoSetupCompiler {
  $Command = Get-Command "ISCC.exe" -ErrorAction SilentlyContinue
  if ($null -ne $Command) {
    return $Command.Source
  }

  $Candidates = @(
    (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe")
  )
  foreach ($Candidate in $Candidates) {
    if (Test-Path $Candidate -PathType Leaf) {
      return $Candidate
    }
  }
  return $null
}

function Get-InnoSetupVersion {
  param([Parameter(Mandatory = $true)][string]$CompilerPath)

  $UninstallKeys = @(
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1",
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\Inno Setup 6_is1"
  )
  foreach ($UninstallKey in $UninstallKeys) {
    $Properties = Get-ItemProperty $UninstallKey -ErrorAction SilentlyContinue
    if ($null -eq $Properties) {
      continue
    }
    foreach ($PropertyName in @("Inno Setup: Setup Version", "DisplayVersion")) {
      $Property = $Properties.PSObject.Properties[$PropertyName]
      if ($null -eq $Property) {
        continue
      }
      $VersionMatch = [regex]::Match(
        [string]$Property.Value,
        '(?<!\d)([1-9]\d*\.\d+\.\d+(?:\.\d+)?)(?!\d)'
      )
      if ($VersionMatch.Success) {
        return [Version]::Parse($VersionMatch.Groups[1].Value)
      }
    }
  }

  $VersionInfo = (Get-Item $CompilerPath).VersionInfo
  foreach ($VersionText in @($VersionInfo.ProductVersion, $VersionInfo.FileVersion)) {
    $VersionMatch = [regex]::Match(
      [string]$VersionText,
      '(?<!\d)([1-9]\d*\.\d+\.\d+(?:\.\d+)?)(?!\d)'
    )
    if ($VersionMatch.Success) {
      return [Version]::Parse($VersionMatch.Groups[1].Value)
    }
  }

  throw "Could not determine the Inno Setup version from $CompilerPath."
}

function Get-NativeWindowsPowerShell {
  $SystemDirectory = if ([Environment]::Is64BitProcess) { "System32" } else { "Sysnative" }
  $PowerShellPath = Join-Path $env:SystemRoot "$SystemDirectory\WindowsPowerShell\v1.0\powershell.exe"
  if (!(Test-Path $PowerShellPath -PathType Leaf)) {
    throw "The 64-bit Windows PowerShell executable was not found: $PowerShellPath"
  }
  return $PowerShellPath
}

function Get-PeMachine {
  param([Parameter(Mandatory = $true)][string]$Path)

  $Stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
  $Reader = $null
  try {
    $Reader = New-Object IO.BinaryReader($Stream)
    if ($Reader.ReadUInt16() -ne 0x5A4D) {
      throw "Not a PE executable (missing MZ header): $Path"
    }
    $Stream.Position = 0x3C
    $PeOffset = $Reader.ReadInt32()
    if ($PeOffset -lt 0 -or $PeOffset -gt ($Stream.Length - 6)) {
      throw "Invalid PE header offset in $Path"
    }
    $Stream.Position = $PeOffset
    if ($Reader.ReadUInt32() -ne 0x00004550) {
      throw "Not a PE executable (missing PE header): $Path"
    }
    $Machine = $Reader.ReadUInt16()
  } finally {
    if ($null -ne $Reader) {
      $Reader.Dispose()
    } else {
      $Stream.Dispose()
    }
  }

  switch ($Machine) {
    0x014C { return "x86" }
    0x8664 { return "x64" }
    0xAA64 { return "arm64" }
    default { return ("unknown-0x{0:X4}" -f $Machine) }
  }
}

function Invoke-Checked {
  param(
    [Parameter(Mandatory = $true)][string]$FilePath,
    [Parameter(Mandatory = $true)][string[]]$Arguments,
    [Parameter(Mandatory = $true)][string]$Description
  )

  Write-Host "==> $Description"
  & $FilePath @Arguments
  if ($LASTEXITCODE -ne 0) {
    throw "$Description failed with exit code $LASTEXITCODE."
  }
}

if ($env:OS -ne "Windows_NT") {
  throw "The setup installers must be built inside the Windows VM."
}
if (![Environment]::Is64BitOperatingSystem) {
  throw "The three-architecture build pipeline requires a 64-bit Windows build host."
}
if ([IO.Path]::IsPathRooted($OutputDir)) {
  throw "OutputDir must be relative to the repository root."
}
if (!(Test-Path $BuildNativeScript -PathType Leaf)) {
  throw "Missing native build script: $BuildNativeScript"
}
if (!(Test-Path $InstallerScript -PathType Leaf)) {
  throw "Missing Inno Setup script: $InstallerScript"
}

if ([string]::IsNullOrWhiteSpace($AppVersion)) {
  $CargoToml = Join-Path $OverlayCrate "Cargo.toml"
  $VersionMatch = [regex]::Match(
    (Get-Content $CargoToml -Raw),
    '(?m)^\s*version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+(?:\.[0-9]+)?)"'
  )
  if (!$VersionMatch.Success) {
    throw "Could not read a numeric package version from $CargoToml. Pass -AppVersion explicitly."
  }
  $AppVersion = $VersionMatch.Groups[1].Value
}
if ($AppVersion -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(?:\.[0-9]+)?$') {
  throw "AppVersion must contain three or four numeric components (for example, 1.2.3 or 1.2.3.4)."
}

$InnoSetupCompiler = Get-InnoSetupCompiler
if (!$InnoSetupCompiler) {
  throw "Inno Setup is missing. Run bootstrap-windows.ps1 from an Administrator PowerShell."
}
$InnoVersion = Get-InnoSetupVersion -CompilerPath $InnoSetupCompiler
if ($InnoVersion -lt [Version]"6.3.0") {
  throw "Inno Setup 6.3 or newer is required. Found $InnoVersion at $InnoSetupCompiler."
}
$WindowsPowerShell = Get-NativeWindowsPowerShell

$InstallerRoot = Join-Path $RepoRoot $OutputDir
New-Item -ItemType Directory -Force -Path $InstallerRoot | Out-Null
$InstallerRoot = (Resolve-Path $InstallerRoot).Path
$Architectures = @($Architecture | Select-Object -Unique)
$BuiltArtifacts = @()

foreach ($CurrentArchitecture in $Architectures) {
  $Spec = $ArchitectureSpecs[$CurrentArchitecture]
  $PackageRelative = $Spec.PackageDir
  $PackageRoot = Join-Path $RepoRoot $PackageRelative

  if (!$SkipBuild) {
    Write-Host ""
    Invoke-Checked `
      -FilePath $WindowsPowerShell `
      -Arguments @(
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy", "Bypass",
        "-File", $BuildNativeScript,
        "-Configuration", $Configuration,
        "-Architecture", $CurrentArchitecture,
        "-OutDir", $PackageRelative
      ) `
      -Description "Build and package Shenava for $CurrentArchitecture in a fresh PowerShell process"
  }

  $RequiredPackageFiles = @(
    "Shenava.exe",
    "Shenava.vbs",
    "Shenava.ico",
    "Shenava.cmd",
    "run.ps1",
    "README.md",
    "Models\ShenavaStreaming\koochik_hd.onnx",
    "Models\ShenavaStreaming\tokens.txt",
    "Models\ShenavaStreaming\hotwords_fa.txt",
    "engine\mel_filters_slaney_80x257.json"
  )
  $MissingPackageFiles = @(
    $RequiredPackageFiles |
      Where-Object { !(Test-Path (Join-Path $PackageRoot $_) -PathType Leaf) }
  )
  if ($MissingPackageFiles.Count -gt 0) {
    throw "The $CurrentArchitecture package is incomplete at $PackageRoot. Missing:`n$($MissingPackageFiles -join "`n")"
  }

  $PackagedExe = Join-Path $PackageRoot "Shenava.exe"
  $ActualMachine = Get-PeMachine -Path $PackagedExe
  if ($ActualMachine -ne $Spec.PeMachine) {
    throw "Refusing to build a mislabeled installer: expected $($Spec.PeMachine) payload, found $ActualMachine in $PackagedExe."
  }

  $OutputBaseName = "Shenava-Setup-$CurrentArchitecture"
  $ExpectedInstaller = Join-Path $InstallerRoot "$OutputBaseName.exe"
  if (Test-Path $ExpectedInstaller -PathType Leaf) {
    Remove-Item -Force $ExpectedInstaller
  }

  $CompilerArguments = @(
    $Spec.CompilerDefine,
    "/DSourceDir=$PackageRoot",
    "/DAppVersion=$AppVersion",
    "/O$InstallerRoot",
    "/F$OutputBaseName",
    $InstallerScript
  )
  Invoke-Checked `
    -FilePath $InnoSetupCompiler `
    -Arguments $CompilerArguments `
    -Description "Compile $CurrentArchitecture setup executable"

  if (!(Test-Path $ExpectedInstaller -PathType Leaf)) {
    throw "Inno Setup completed without producing $ExpectedInstaller."
  }
  $Artifact = Get-Item $ExpectedInstaller
  if ($Artifact.Length -le 0) {
    throw "Inno Setup produced an empty artifact: $ExpectedInstaller"
  }
  $BuiltArtifacts += $Artifact
  Write-Host "Verified $($Spec.PeMachine) payload: $PackagedExe"
  Write-Host "Wrote $ExpectedInstaller"
}

$HashLines = foreach ($Artifact in $BuiltArtifacts) {
  $Hash = (Get-FileHash -Algorithm SHA256 -Path $Artifact.FullName).Hash.ToLowerInvariant()
  "$Hash  $($Artifact.Name)"
}
$ChecksumPath = Join-Path $InstallerRoot "SHA256SUMS.txt"
$HashLines | Set-Content -Path $ChecksumPath -Encoding Ascii

Write-Host ""
Write-Host "Setup installers ready (Inno Setup $InnoVersion):"
foreach ($Artifact in $BuiltArtifacts) {
  Write-Host "  $($Artifact.FullName)"
}
Write-Host "  $ChecksumPath"
