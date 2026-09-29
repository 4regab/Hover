; Inno Setup script for the native (Rust) Hover: installer/Hover.iss with the Rust exe.
; The same AppId, folder, exe name and Run value as the C# installer, so installing
; this over a C# install replaces it in place (and uninstalls as one product).
;   iscc /DMyAppVersion=<version> /DExeDir=<native\target\release> /O<dir> native\installer\Hover.iss
; Build pending: needs Windows and Inno Setup 6 or 7 (RUN-ON-WINDOWS.md, phase 4).

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#ifndef ExeDir
  #define ExeDir "..\target\release"
#endif

#define MyAppName "Hover"
#define MyAppExe "Hover.exe"
#define MyAppPublisher "Hover"

[Setup]
AppId={{B7E2B4C1-6E3A-4E1F-9A2C-1D0F5A7C9E20}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
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
; cargo names it hover.exe; installed as Hover.exe so shortcuts and the Run value an
; earlier install wrote still point at it.
Source: "{#ExeDir}\hover.exe"; DestDir: "{app}"; DestName: "{#MyAppExe}"; Flags: ignoreversion
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
