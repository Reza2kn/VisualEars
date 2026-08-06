[CmdletBinding()]
param(
  [ValidateSet("Debug", "Release")]
  [string]$Configuration = "Release",
  [ValidateSet("x64", "arm64", "x86")]
  [string]$Architecture = "x64",
  [string]$Toolchain = "stable-x86_64-pc-windows-msvc",
  [string]$OutDir = "",
  [switch]$SkipPackage
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$RepoRoot = (Resolve-Path (Join-Path $ScriptDir "..\..\..")).Path
$OverlayCrate = Join-Path $RepoRoot "desktop\linux\visualears-overlay"
$PackageScript = Join-Path $ScriptDir "package.ps1"

switch ($Architecture) {
  "x64" {
    $Target = "x86_64-pc-windows-msvc"
    $VsArchitecture = "amd64"
    $VsToolsComponent = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64"
  }
  "arm64" {
    $Target = "aarch64-pc-windows-msvc"
    $VsArchitecture = "arm64"
    $VsToolsComponent = "Microsoft.VisualStudio.Component.VC.Tools.ARM64"
  }
  "x86" {
    $Target = "i686-pc-windows-msvc"
    $VsArchitecture = "x86"
    $VsToolsComponent = "Microsoft.VisualStudio.Component.VC.Tools.x86.x64"
  }
}

if ([string]::IsNullOrWhiteSpace($OutDir)) {
  # Preserve the original no-argument x64 package path. Other explicit architectures
  # get an unambiguous suffix even when build-native.ps1 is invoked directly.
  $OutDir = if ($Architecture -eq "x64") {
    ".build/Shenava-Windows"
  } else {
    ".build/Shenava-Windows-$Architecture"
  }
}

function Get-CommandSource {
  param([Parameter(Mandatory = $true)][string]$Name)

  $Command = Get-Command $Name -ErrorAction SilentlyContinue
  if ($null -eq $Command) {
    return $null
  }
  return $Command.Source
}

function Get-VCToolsInstallPath {
  param([Parameter(Mandatory = $true)][string]$RequiredComponent)

  $VsWhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (!(Test-Path $VsWhere -PathType Leaf)) {
    throw "Visual Studio Build Tools were not found. Run bootstrap-windows.ps1 from an Administrator PowerShell."
  }

  $InstallPath = & $VsWhere -latest -products * -requires $RequiredComponent -property installationPath
  if ($LASTEXITCODE -ne 0 -or !$InstallPath) {
    throw "The MSVC component $RequiredComponent was not found. Run bootstrap-windows.ps1 again."
  }
  return ($InstallPath | Select-Object -First 1).Trim()
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

function Enable-TractArm64ClangAssembly {
  param(
    [Parameter(Mandatory = $true)][string]$RustupPath,
    [Parameter(Mandatory = $true)][string]$RustToolchain,
    [Parameter(Mandatory = $true)][string]$ManifestPath,
    [Parameter(Mandatory = $true)][string]$TargetTriple,
    [Parameter(Mandatory = $true)][string]$CargoProfile
  )

  # The pinned tract revision assumes every native Windows MSVC build uses
  # MASM syntax. That is correct for x64, but ARM64's generated GNU-style .S
  # kernels must stay untouched for clang-cl's integrated assembler.
  $MetadataJson = @(
    & $RustupPath run $RustToolchain cargo metadata `
      --locked `
      --format-version 1 `
      --manifest-path $ManifestPath
  )
  if ($LASTEXITCODE -ne 0) {
    throw "Cargo metadata failed while locating tract-linalg."
  }
  $Metadata = ($MetadataJson -join "`n") | ConvertFrom-Json
  $TractPackage = @(
    $Metadata.packages |
      Where-Object {
        $_.name -eq "tract-linalg" -and
        [string]$_.source -like "git+https://github.com/sonos/tract*"
      }
  ) | Select-Object -First 1
  if ($null -eq $TractPackage) {
    throw "The pinned tract-linalg Git dependency was not found in Cargo metadata."
  }

  $TractBuildScript = Join-Path (Split-Path -Parent $TractPackage.manifest_path) "build.rs"
  $OriginalCondition = 'env::var("CARGO_CFG_TARGET_ENV") == Ok("msvc".to_string()) && var("HOST").contains("-windows-")'
  $Arm64SafeCondition = "$OriginalCondition && var(`"CARGO_CFG_TARGET_ARCH`") == `"x86_64`""
  $BuildScriptContents = [IO.File]::ReadAllText($TractBuildScript)
  $PatchApplied = $false
  if ($BuildScriptContents.Contains($Arm64SafeCondition)) {
    Write-Host "==> tract-linalg ARM64 assembly workaround is already applied"
  } elseif (!$BuildScriptContents.Contains($OriginalCondition)) {
    throw "The pinned tract-linalg MASM condition changed; refusing to patch an unknown build script: $TractBuildScript"
  } else {
    $PatchedContents = $BuildScriptContents.Replace($OriginalCondition, $Arm64SafeCondition)
    [IO.File]::WriteAllText($TractBuildScript, $PatchedContents, (New-Object Text.UTF8Encoding($false)))
    $PatchApplied = $true
    Write-Host "==> Applied tract-linalg ARM64 clang assembly workaround"
  }

  if (!$PatchApplied) {
    Write-Host "==> Reusing the cached patched tract-linalg build script"
    return
  }

  # Cargo treats Git sources as immutable, and `cargo clean -p tract-linalg`
  # does not remove the host build-script executable used by a cross build.
  # Remove only tract-linalg's host/target build + fingerprint directories so
  # Cargo must compile the patched build.rs and regenerate the ARM64 sources.
  $TargetDirectory = [IO.Path]::GetFullPath([string]$Metadata.target_directory)
  $CacheRoots = @(
    (Join-Path $TargetDirectory "$CargoProfile\build"),
    (Join-Path $TargetDirectory "$CargoProfile\.fingerprint"),
    (Join-Path $TargetDirectory "$TargetTriple\$CargoProfile\build"),
    (Join-Path $TargetDirectory "$TargetTriple\$CargoProfile\.fingerprint")
  )
  $RemovedCacheDirectories = 0
  foreach ($CacheRoot in $CacheRoots) {
    if (!(Test-Path -LiteralPath $CacheRoot -PathType Container)) {
      continue
    }
    foreach ($CacheDirectory in @(Get-ChildItem -LiteralPath $CacheRoot -Directory -Filter "tract-linalg-*")) {
      Remove-Item -LiteralPath $CacheDirectory.FullName -Recurse -Force
      $RemovedCacheDirectories += 1
    }
  }
  $StaleBuildScripts = @(
    Get-ChildItem `
      -LiteralPath (Join-Path $TargetDirectory "$CargoProfile\build") `
      -Filter "build-script-build.exe" `
      -File `
      -Recurse `
      -ErrorAction SilentlyContinue |
      Where-Object { $_.Directory.Name -like "tract-linalg-*" }
  )
  if ($StaleBuildScripts.Count -gt 0) {
    throw "Could not invalidate cached tract-linalg build scripts:`n$($StaleBuildScripts.FullName -join "`n")"
  }
  Write-Host "==> Invalidated $RemovedCacheDirectories cached tract-linalg directories"
}

if ($env:OS -ne "Windows_NT") {
  throw "This is the native Windows build entry point. Run it inside the Windows VM."
}
if (![Environment]::Is64BitOperatingSystem) {
  throw "Shenava's native package currently requires 64-bit Windows."
}
if ([IO.Path]::IsPathRooted($OutDir)) {
  throw "OutDir must be relative to the repository root because package.ps1 resolves it there."
}

$Rustup = Get-CommandSource -Name "rustup.exe"
if (!$Rustup) {
  throw "rustup is missing. Run bootstrap-windows.ps1 first."
}
if (!(Get-CommandSource -Name "git.exe")) {
  throw "Git is missing. Run bootstrap-windows.ps1 first."
}

$VsInstallPath = Get-VCToolsInstallPath -RequiredComponent $VsToolsComponent
$VsDevShell = Join-Path $VsInstallPath "Common7\Tools\Launch-VsDevShell.ps1"
if (!(Test-Path $VsDevShell -PathType Leaf)) {
  throw "Visual Studio Developer PowerShell launcher is missing: $VsDevShell"
}

$RequiredSources = @(
  (Join-Path $OverlayCrate "Cargo.toml"),
  (Join-Path $OverlayCrate "Cargo.lock")
)
if (!$SkipPackage) {
  $RequiredSources += @(
    (Join-Path $ScriptDir "assets\koochik_hd.onnx"),
    (Join-Path $ScriptDir "assets\tokens.txt"),
    (Join-Path $ScriptDir "assets\hotwords_fa.txt"),
    (Join-Path $ScriptDir "assets\mel_filters_slaney_80x257.json"),
    (Join-Path $ScriptDir "fixtures\golha_clear.wav"),
    $PackageScript
  )
}
$MissingSources = @($RequiredSources | Where-Object { !(Test-Path $_ -PathType Leaf) })
if ($MissingSources.Count -gt 0) {
  throw "Required build/package inputs are missing:`n$($MissingSources -join "`n")"
}

Write-Host "==> Initialize Visual Studio $VsArchitecture developer environment (x64 host)"
& $VsDevShell -Arch $VsArchitecture -HostArch amd64 -SkipAutomaticLocation

if ($Architecture -eq "arm64") {
  # tract-linalg generates GNU-style ARM64 .S kernels. MSVC's cl.exe ignores
  # those files, while Visual Studio's clang-cl assembles them into COFF objects.
  $LlvmBin = Join-Path $VsInstallPath "VC\Tools\Llvm\x64\bin"
  $ClangCl = Join-Path $LlvmBin "clang-cl.exe"
  $LlvmLib = Join-Path $LlvmBin "llvm-lib.exe"
  if (!(Test-Path $ClangCl -PathType Leaf) -or !(Test-Path $LlvmLib -PathType Leaf)) {
    throw "Visual Studio LLVM tools are required for the ARM64 tract kernels. Run bootstrap-windows.ps1 again."
  }
  $env:CC_aarch64_pc_windows_msvc = $ClangCl
  $env:CXX_aarch64_pc_windows_msvc = $ClangCl
  $env:AR_aarch64_pc_windows_msvc = $LlvmLib
  Write-Host "==> Use Visual Studio clang-cl for ARM64 assembly kernels"
  Enable-TractArm64ClangAssembly `
    -RustupPath $Rustup `
    -RustToolchain $Toolchain `
    -ManifestPath (Join-Path $OverlayCrate "Cargo.toml") `
    -TargetTriple $Target `
    -CargoProfile ($Configuration.ToLowerInvariant())
}

$InstalledTargets = & $Rustup target list --installed --toolchain $Toolchain
if ($LASTEXITCODE -ne 0 -or $InstalledTargets -notcontains $Target) {
  throw "Rust target $Target is missing for $Toolchain. Run bootstrap-windows.ps1 first."
}

$CargoArguments = @(
  "run", $Toolchain,
  "cargo", "build",
  "--locked",
  "--target", $Target,
  "--features", "control-panel"
)
if ($Configuration -eq "Release") {
  $CargoArguments += "--release"
}

Push-Location $OverlayCrate
try {
  Invoke-Checked `
    -FilePath $Rustup `
    -Arguments $CargoArguments `
    -Description "Build Shenava natively for $Target ($Configuration)"
} finally {
  Pop-Location
}

$ProfileDir = $Configuration.ToLowerInvariant()
$BuiltExe = Join-Path $OverlayCrate "target\$Target\$ProfileDir\shenava.exe"
if (!(Test-Path $BuiltExe -PathType Leaf)) {
  throw "Cargo completed without producing the expected executable: $BuiltExe"
}

Write-Host "Built $BuiltExe ($Architecture)"
if ($SkipPackage) {
  Write-Host "Packaging skipped."
  exit 0
}

Write-Host "==> Package native Windows build"
& $PackageScript `
  -Configuration $ProfileDir `
  -Target $Target `
  -OutDir $OutDir `
  -SkipBuild

$PackageRoot = Join-Path $RepoRoot $OutDir
$PackageZip = "$PackageRoot.zip"
if (!(Test-Path $PackageRoot -PathType Container) -or !(Test-Path $PackageZip -PathType Leaf)) {
  throw "Packaging did not produce both expected artifacts: $PackageRoot and $PackageZip"
}

Write-Host "Native Windows package ready:"
Write-Host "  $PackageRoot"
Write-Host "  $PackageZip"
