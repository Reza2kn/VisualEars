[CmdletBinding()]
param(
  [switch]$SkipWebView2,
  [switch]$SkipInstallerTools
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$Toolchain = "stable-x86_64-pc-windows-msvc"
$Targets = @(
  "x86_64-pc-windows-msvc",
  "aarch64-pc-windows-msvc",
  "i686-pc-windows-msvc"
)
$RequiredVsComponents = @(
  "Microsoft.VisualStudio.Component.VC.Tools.x86.x64",
  "Microsoft.VisualStudio.Component.VC.Tools.ARM64",
  "Microsoft.VisualStudio.Component.VC.Llvm.Clang"
)

function Test-IsAdministrator {
  $Identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $Principal = New-Object Security.Principal.WindowsPrincipal($Identity)
  return $Principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Refresh-ProcessPath {
  $MachinePath = [Environment]::GetEnvironmentVariable("Path", "Machine")
  $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
  $CargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
  $env:Path = (@($env:Path, $MachinePath, $UserPath, $CargoBin) | Where-Object { $_ }) -join ";"
}

function Get-CommandSource {
  param([Parameter(Mandatory = $true)][string]$Name)

  $Command = Get-Command $Name -ErrorAction SilentlyContinue
  if ($null -eq $Command) {
    return $null
  }
  return $Command.Source
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

function Install-WingetPackage {
  param(
    [Parameter(Mandatory = $true)][string]$WingetPath,
    [Parameter(Mandatory = $true)][string]$Id,
    [string]$Override = "",
    [switch]$Force
  )

  $Arguments = @(
    "install",
    "--exact",
    "--id", $Id,
    "--source", "winget",
    "--accept-package-agreements",
    "--accept-source-agreements",
    "--silent"
  )
  if ($Force) {
    $Arguments += "--force"
  }
  if ($Override) {
    $Arguments += @("--override", $Override)
  }

  Invoke-Checked -FilePath $WingetPath -Arguments $Arguments -Description "Install $Id"
  Refresh-ProcessPath
}

function Get-VsWherePath {
  $Bundled = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $Bundled -PathType Leaf) {
    return $Bundled
  }
  return Get-CommandSource -Name "vswhere.exe"
}

function Get-VCToolsInstallPath {
  param(
    [string[]]$RequiredComponents = @("Microsoft.VisualStudio.Component.VC.Tools.x86.x64")
  )

  $VsWhere = Get-VsWherePath
  if (!$VsWhere) {
    return $null
  }

  $Arguments = @("-latest", "-products", "*", "-requires")
  $Arguments += $RequiredComponents
  $Arguments += @("-property", "installationPath")
  $InstallPath = & $VsWhere @Arguments
  if ($LASTEXITCODE -ne 0 -or !$InstallPath) {
    return $null
  }
  return ($InstallPath | Select-Object -First 1).Trim()
}

function Get-InnoSetupCompiler {
  $Command = Get-CommandSource -Name "ISCC.exe"
  if ($Command) {
    return $Command
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

function Get-WebView2Version {
  $UninstallRoots = @(
    "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*",
    "HKCU:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*"
  )

  foreach ($Root in $UninstallRoots) {
    $Runtime = Get-ItemProperty $Root -ErrorAction SilentlyContinue |
      Where-Object {
        $_.PSObject.Properties.Name -contains "DisplayName" -and
        $_.DisplayName -like "*WebView2 Runtime*"
      } |
      Select-Object -First 1
    if ($null -ne $Runtime) {
      return $Runtime.DisplayVersion
    }
  }
  return $null
}

if ($env:OS -ne "Windows_NT") {
  throw "This bootstrap must run inside the Windows VM."
}
if (![Environment]::Is64BitOperatingSystem) {
  throw "Shenava's native package currently requires 64-bit Windows."
}
if (!(Test-IsAdministrator)) {
  throw "Open PowerShell as Administrator, then rerun this script."
}

$Winget = Get-CommandSource -Name "winget.exe"
if (!$Winget) {
  throw "winget is missing. Install or update Microsoft App Installer, then rerun this script."
}

Write-Host "Bootstrapping Shenava native Windows development tools..."

if (!(Get-CommandSource -Name "git.exe")) {
  Install-WingetPackage -WingetPath $Winget -Id "Git.Git"
} else {
  Write-Host "==> Git is already installed"
}

if (!(Get-CommandSource -Name "rustup.exe")) {
  Install-WingetPackage `
    -WingetPath $Winget `
    -Id "Rustlang.Rustup" `
    -Override "-y --profile minimal --default-host x86_64-pc-windows-msvc --default-toolchain none"
} else {
  Write-Host "==> rustup is already installed"
}

if (!(Get-VCToolsInstallPath -RequiredComponents $RequiredVsComponents)) {
  Install-WingetPackage `
    -WingetPath $Winget `
    -Id "Microsoft.VisualStudio.2022.BuildTools" `
    -Force `
    -Override "--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended --add Microsoft.VisualStudio.Component.VC.Tools.ARM64 --add Microsoft.VisualStudio.Component.VC.Llvm.Clang --addProductLang En-us"
} else {
  Write-Host "==> Visual Studio C++ x64/x86, ARM64, and LLVM Build Tools are already installed"
}

if (!$SkipInstallerTools) {
  if (!(Get-InnoSetupCompiler)) {
    Install-WingetPackage -WingetPath $Winget -Id "JRSoftware.InnoSetup"
  } else {
    Write-Host "==> Inno Setup is already installed"
  }
}

if (!$SkipWebView2) {
  if (!(Get-WebView2Version)) {
    Install-WingetPackage -WingetPath $Winget -Id "Microsoft.EdgeWebView2Runtime"
  } else {
    Write-Host "==> Microsoft Edge WebView2 Runtime is already installed"
  }
}

Refresh-ProcessPath
$Rustup = Get-CommandSource -Name "rustup.exe"
if (!$Rustup) {
  throw "rustup was installed but is not visible in PATH. Open a new Administrator PowerShell and rerun this script."
}

Invoke-Checked `
  -FilePath $Rustup `
  -Arguments @("toolchain", "install", $Toolchain, "--profile", "default") `
  -Description "Install or update Rust $Toolchain"
foreach ($Target in $Targets) {
  Invoke-Checked `
    -FilePath $Rustup `
    -Arguments @("target", "add", $Target, "--toolchain", $Toolchain) `
    -Description "Install Rust target $Target"
}
Invoke-Checked `
  -FilePath $Rustup `
  -Arguments @("component", "add", "rustfmt", "clippy", "--toolchain", $Toolchain) `
  -Description "Install Rust developer components"

$VsInstallPath = Get-VCToolsInstallPath -RequiredComponents $RequiredVsComponents
if (!$VsInstallPath) {
  throw "The Visual Studio C++ x64/x86, ARM64, and LLVM toolsets were not detected after installation. Reboot the VM and rerun this script."
}

$Git = Get-CommandSource -Name "git.exe"
if (!$Git) {
  throw "Git was not detected after installation. Open a new Administrator PowerShell and rerun this script."
}

Write-Host ""
Write-Host "Bootstrap complete."
Invoke-Checked -FilePath $Git -Arguments @("--version") -Description "Verify Git"
Invoke-Checked -FilePath $Rustup -Arguments @("run", $Toolchain, "rustc", "--version") -Description "Verify rustc"
Invoke-Checked -FilePath $Rustup -Arguments @("run", $Toolchain, "cargo", "--version") -Description "Verify Cargo"
Write-Host "Visual Studio Build Tools: $VsInstallPath"
if (!$SkipInstallerTools) {
  $InnoSetupCompiler = Get-InnoSetupCompiler
  if (!$InnoSetupCompiler) {
    throw "Inno Setup was not detected after installation. Open a new Administrator PowerShell and rerun this script."
  }
  $InnoVersion = Get-InnoSetupVersion -CompilerPath $InnoSetupCompiler
  if ($InnoVersion -lt [Version]"6.3.0") {
    throw "Inno Setup 6.3 or newer is required for the architecture-specific installers. Found $InnoVersion."
  }
  Write-Host "Inno Setup compiler: $InnoSetupCompiler ($InnoVersion)"
}
if (!$SkipWebView2) {
  $WebView2Version = Get-WebView2Version
  if (!$WebView2Version) {
    throw "WebView2 Runtime was not detected after installation. Reboot the VM and rerun this script."
  }
  Write-Host "WebView2 Runtime: $WebView2Version"
}
Write-Host ""
Write-Host "Next: run desktop\windows\ShenavaOverlay\build-installers.ps1 from the repository root."
