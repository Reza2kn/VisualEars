#define MyAppName "Shenava"
#define MyAppPublisher "Shenava"
#define MyAppUrl "https://shenava.app"

#ifndef SourceDir
  #error SourceDir must point to an architecture-specific portable package
#endif

#ifndef AppVersion
  #define AppVersion "0.1.1"
#endif

#if (Defined(ARCH_X86) + Defined(ARCH_X64) + Defined(ARCH_ARM64)) != 1
  #error Define exactly one of ARCH_X86, ARCH_X64, or ARCH_ARM64
#endif

#ifdef ARCH_X86
  #define ArchitectureName "x86"
  #define ArchitectureLabel "32-bit x86"
  #define AllowedArchitectures "x86compatible and not x64compatible and not arm64"
#elif Defined(ARCH_X64)
  #define ArchitectureName "x64"
  #define ArchitectureLabel "64-bit x64"
  #define AllowedArchitectures "x64compatible and not arm64"
  #define InstallIn64BitMode "x64compatible and not arm64"
#elif Defined(ARCH_ARM64)
  #define ArchitectureName "arm64"
  #define ArchitectureLabel "64-bit ARM64"
  #define AllowedArchitectures "arm64"
  #define InstallIn64BitMode "arm64"
#endif

[Setup]
AppId={{BDF31FA8-87A3-404A-B245-99850FB45867}
AppName={#MyAppName}
AppVersion={#AppVersion}
AppVerName={#MyAppName} {#AppVersion} ({#ArchitectureLabel})
UninstallDisplayName={#MyAppName}
UninstallDisplayName={#MyAppName}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppUrl}
AppSupportURL={#MyAppUrl}
AppUpdatesURL={#MyAppUrl}
ArchitecturesAllowed={#AllowedArchitectures}
#ifdef InstallIn64BitMode
ArchitecturesInstallIn64BitMode={#InstallIn64BitMode}
#endif
MinVersion=10.0
DefaultDirName={autopf}\Shenava
DefaultGroupName=Shenava
DisableProgramGroupPage=yes
PrivilegesRequired=admin
OutputDir=.
OutputBaseFilename=Shenava-Setup-{#ArchitectureName}
Compression=lzma2/normal
SolidCompression=yes
WizardStyle=modern
SetupIconFile={#SourceDir}\Shenava.ico
SetupLogging=yes
CloseApplications=yes
RestartApplications=no
UninstallDisplayIcon={app}\Shenava.ico
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription={#MyAppName} Setup ({#ArchitectureLabel})
VersionInfoProductName={#MyAppName}
VersionInfoProductVersion={#AppVersion}
VersionInfoVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Excludes: "caption-smoke.png"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Shenava"; Filename: "{sys}\wscript.exe"; Parameters: """{app}\Shenava.vbs"""; WorkingDir: "{app}"; IconFilename: "{app}\Shenava.ico"
Name: "{autodesktop}\Shenava"; Filename: "{sys}\wscript.exe"; Parameters: """{app}\Shenava.vbs"""; WorkingDir: "{app}"; IconFilename: "{app}\Shenava.ico"; Tasks: desktopicon

[Run]
Filename: "{sys}\wscript.exe"; Parameters: """{app}\Shenava.vbs"""; Description: "Launch Shenava"; WorkingDir: "{app}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\caption-smoke.png"

[Code]
const
  WebView2ClientKey = 'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  WebView2BootstrapperUrl = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703';
  WebView2DownloadPageUrl = 'https://developer.microsoft.com/microsoft-edge/webview2/';

function HasWebView2Version(const RootKey: Integer): Boolean;
var
  Version: String;
begin
  Version := '';
  Result := RegQueryStringValue(RootKey, WebView2ClientKey, 'pv', Version) and
    (Trim(Version) <> '') and (CompareText(Trim(Version), '0.0.0.0') <> 0);
  if Result then
    Log('Detected Microsoft Edge WebView2 Runtime ' + Version);
end;

function IsWebView2RuntimeInstalled: Boolean;
begin
  { Microsoft documents the per-machine value in the 32-bit registry view on
    64-bit Windows. Check both per-user views as well for user-scoped installs. }
  Result := HasWebView2Version(HKLM32) or HasWebView2Version(HKCU32);
  if (not Result) and IsWin64 then
    Result := HasWebView2Version(HKCU64);
end;

function OnWebView2DownloadProgress(const Url, FileName: String;
  const Progress, ProgressMax: Int64): Boolean;
begin
  WizardForm.StatusLabel.Caption := 'Downloading Microsoft Edge WebView2 Runtime...';
  Result := True;
end;

function WebView2InstallFailure(const Detail: String): String;
begin
  Result := 'Microsoft Edge WebView2 Runtime is required, but Setup could not install it automatically.' + #13#10 + #13#10 +
    'Check this computer''s internet access, or install the Evergreen Runtime from:' + #13#10 +
    WebView2DownloadPageUrl + #13#10 + #13#10 + 'Details: ' + Detail;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  BootstrapperPath: String;
  ResultCode: Integer;
begin
  Result := '';
  if IsWebView2RuntimeInstalled then
    Exit;

  Log('Microsoft Edge WebView2 Runtime is missing; downloading the official Evergreen bootstrapper.');
  BootstrapperPath := ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe');
  try
    DownloadTemporaryFile(
      WebView2BootstrapperUrl,
      'MicrosoftEdgeWebview2Setup.exe',
      '',
      @OnWebView2DownloadProgress);
  except
    Result := WebView2InstallFailure(GetExceptionMessage);
    Log(Result);
    Exit;
  end;

  WizardForm.StatusLabel.Caption := 'Installing Microsoft Edge WebView2 Runtime...';
  if not Exec(
    BootstrapperPath,
    '/silent /install',
    ExpandConstant('{tmp}'),
    SW_HIDE,
    ewWaitUntilTerminated,
    ResultCode) then
  begin
    Result := WebView2InstallFailure('The bootstrapper could not start (Windows error ' + IntToStr(ResultCode) + ').');
    Log(Result);
    Exit;
  end;

  if ResultCode <> 0 then
  begin
    Result := WebView2InstallFailure('The bootstrapper exited with code ' + IntToStr(ResultCode) + '.');
    Log(Result);
    Exit;
  end;

  if not IsWebView2RuntimeInstalled then
  begin
    Result := WebView2InstallFailure('The bootstrapper completed, but the Runtime registry entry is still missing.');
    Log(Result);
    Exit;
  end;

  Log('Microsoft Edge WebView2 Runtime prerequisite installed successfully.');
end;
