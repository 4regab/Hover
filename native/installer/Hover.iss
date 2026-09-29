; Inno Setup script for Hover.
; The same AppId, folder, exe name and Run value as the C# (2.x) installer had, so
; installing 3.x over a 2.x install replaces it in place (and uninstalls as one product).
;   .\build.ps1 installer   (iscc /DMyAppVersion=<version> /DExeDir=<publish> /O<dir> native\installer\Hover.iss)
; build.ps1 installer passes the version from native/Cargo.toml. Needs Inno Setup 6 or 7.

#ifndef MyAppVersion
  #define MyAppVersion "3.0.0"
#endif
#ifndef ExeDir
  #define ExeDir "..\..\publish"
#endif

#define MyAppName "Hover"
#define MyAppExe "Hover.exe"
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
SetupIconFile=..\apps\hover\assets\hover.ico
UninstallDisplayIcon={app}\{#MyAppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
CloseApplications=yes

[Files]
; build.ps1 publish copies cargo's hover.exe to publish\Hover.exe, the C# build's name, so
; shortcuts and the Run value an earlier install wrote still point at it.
Source: "{#ExeDir}\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
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

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Launch {#MyAppName}"; \
  Flags: nowait postinstall skipifsilent
