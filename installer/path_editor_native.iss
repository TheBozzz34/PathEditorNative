#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif

#define AppName "Path Editor Native"
#define AppExeName "PathEditorNative.exe"

[Setup]
AppId={{D4EA1C78-5947-438A-9D66-5B74634840A1}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Path Editor Native
DefaultDirName={autopf}\Path Editor Native
DefaultGroupName=Path Editor Native
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=PathEditorNative-Setup-{#AppVersion}
Compression=lzma
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=admin
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Files]
Source: "..\dist\PathEditorNative\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\dist\PathEditorNative\README.txt"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Description: "Launch {#AppName}"; Flags: nowait postinstall skipifsilent
