; Inno Setup script for Hover.
; The same AppId, folder and Run value as the C# (2.x) installer had, so installing over a 2.x
; or a 5.x (Go) install replaces it in place (and uninstalls as one product). The exe is
; hoverai.exe, not 2.x's Hover.exe: Discord's game list matches any path ending in
; hover/hover.exe (Hover: Revolt of Gamers) and showed Hover as that game.
;   .\build.ps1 installer          (iscc /DMyAppVersion=<version> /DExeDir=<publish> /O<dir> packaging\windows\Hover.iss)
;   .\build.ps1 legacy installer   the same with /DExeDir=<publish-legacy> /DLegacy: the Go app's setup
; build.ps1 passes the version from VERSION. Needs Inno Setup 6 or 7.
;
; The setup takes everything in ExeDir: the C# app's publish folder holds hoverai.exe, the
; .NET and Windows App SDK files it runs on, hover-backend.exe, LICENSE and
; THIRD-PARTY-NOTICES.txt. The Go app's holds hoverai.exe, wgpu_native.dll, LICENSE and the notices.

#ifndef MyAppVersion
  #define MyAppVersion "3.0.0"
#endif
#ifndef ExeDir
  #define ExeDir "..\..\publish"
#endif

#define MyAppName "Hover"
#define MyAppExe "hoverai.exe"
#define BackendExe "hover-backend.exe"
; What an install from before the rename left (2.x, or 3.x before it).
#define OldAppExe "Hover.exe"
#define MyAppPublisher "Hover"

[Setup]
AppId={{B7E2B4C1-6E3A-4E1F-9A2C-1D0F5A7C9E20}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
VersionInfoVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
#ifdef Legacy
OutputBaseFilename=Hover-Legacy-Setup-{#MyAppVersion}
#else
OutputBaseFilename=Hover-Setup-{#MyAppVersion}
#endif
SetupIconFile=..\..\windows\Hover\Assets\hover.ico
UninstallDisplayIcon={app}\{#MyAppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
; The app targets Windows 10 version 2004 (build 19041) and newer.
MinVersion=10.0.19041
CloseApplications=yes

[InstallDelete]
; The 2.x exe, so Discord stops seeing it (CloseApplications closes it if it runs).
Type: files; Name: "{app}\{#OldAppExe}"
; The Go app (5.x) drew the office with wgpu-native; the C# app draws it with Direct3D.
Type: files; Name: "{app}\wgpu_native.dll"

[Files]
Source: "{#ExeDir}\*"; DestDir: "{app}"; Excludes: "*.pdb"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"
Name: "{userdesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked
Name: "startup"; Description: "Start {#MyAppName} when Windows starts"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
  ValueName: "{#MyAppName}"; ValueData: """{app}\{#MyAppExe}"""; \
  Flags: uninsdeletevalue; Tasks: startup
; Launch at login switched on in Settings points at the old exe, which is gone now.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; \
  ValueName: "{#MyAppName}"; ValueData: """{app}\{#MyAppExe}"""; \
  Tasks: not startup; Check: RunsOldExe

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Launch {#MyAppName}"; \
  Flags: nowait postinstall skipifsilent

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';

function RunsOldExe: Boolean;
var
  Value: String;
begin
  Result := RegQueryStringValue(HKCU, RunKey, '{#MyAppName}', Value)
    and (CompareText(Value, '"' + ExpandConstant('{app}\{#OldAppExe}') + '"') = 0);
end;

{ Ends a running Hover (the C# app, the Go app or 2.x) and the backend it started, so its files
  can be replaced or removed. Only one Hover runs per user, so a copy that runs is the one that
  matters wherever it was started from. The backend dies with the app (a job object), but the
  tree is ended here too in case a copy of an older build left one. taskkill finds nothing to end
  when Hover is not running, which is fine. }
procedure StopHover;
var
  Code: Integer;
begin
  { The C# app is asked to end in order first (its agents are told, the history and the settings are written): its own
    hoverai.dll beside it says which program this is, because the Go app and 2.x do not know --quit. Up to 20 seconds; a
    program that is still there after that is ended below. }
  if FileExists(ExpandConstant('{app}\hoverai.dll')) and FileExists(ExpandConstant('{app}\{#MyAppExe}')) then
    Exec(ExpandConstant('{app}\{#MyAppExe}'), '--quit', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /T /IM {#MyAppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /T /IM {#OldAppExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /T /IM {#BackendExe}', '', SW_HIDE, ewWaitUntilTerminated, Code);
  { A killed program lets go of its files a moment later. }
  Sleep(1000);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopHover;
  Result := '';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Value: String;
begin
  if CurUninstallStep = usUninstall then
    StopHover
  else if CurUninstallStep = usPostUninstall then
  begin
    { Launch at login, switched on in Settings, writes the Run value too, so the setup's own
      uninsdeletevalue does not cover it. It goes with the app it starts; another Hover's stays. }
    if RegQueryStringValue(HKCU, RunKey, '{#MyAppName}', Value)
      and (CompareText(Value, '"' + ExpandConstant('{app}\{#MyAppExe}') + '"') = 0) then
      RegDeleteValue(HKCU, RunKey, '{#MyAppName}');
  end;
end;
