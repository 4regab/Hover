; Inno Setup script for Hover.
; The same AppId, folder and Run value as the C# (2.x) installer had, so installing 3.x
; over a 2.x install replaces it in place (and uninstalls as one product). The exe is
; hoverai.exe, not 2.x's Hover.exe: Discord's game list matches any path ending in
; hover/hover.exe (Hover: Revolt of Gamers) and showed Hover as that game.
;   .\build.ps1 installer   (iscc /DMyAppVersion=<version> /DExeDir=<publish> /O<dir> packaging\windows\Hover.iss)
; build.ps1 installer passes the version from Cargo.toml. Needs Inno Setup 6 or 7.

#ifndef MyAppVersion
  #define MyAppVersion "3.0.0"
#endif
#ifndef ExeDir
  #define ExeDir "..\..\publish"
#endif

#define MyAppName "Hover"
#define MyAppExe "hoverai.exe"
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
OutputBaseFilename=Hover-Setup-{#MyAppVersion}
SetupIconFile=..\..\app\assets\hover.ico
UninstallDisplayIcon={app}\{#MyAppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
CloseApplications=yes

[InstallDelete]
; The old exe, so Discord stops seeing it (CloseApplications closes it if it runs).
Type: files; Name: "{app}\{#OldAppExe}"

[Files]
Source: "{#ExeDir}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
; The Go build draws the office with wgpu-native, which it looks for beside the exe.
#if FileExists(ExeDir + "\wgpu_native.dll")
Source: "{#ExeDir}\wgpu_native.dll"; DestDir: "{app}"; Flags: ignoreversion
#endif
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion

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
function RunsOldExe: Boolean;
var
  Value: String;
begin
  Result := RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#MyAppName}', Value)
    and (CompareText(Value, '"' + ExpandConstant('{app}\{#OldAppExe}') + '"') = 0);
end;
