#define AppName "Raven Code"
#define AppVersion "0.2.0"
#define SourceDir AddBackslash(SourcePath) + "..\..\dist\raven-code-" + AppVersion + "-windows-x86_64"

[Setup]
AppId={{A084764B-5E96-4F75-8994-5D5C9388FBE2}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Raven Developers
DefaultDirName={localappdata}\Programs\Raven Code
DefaultGroupName=Raven Code
PrivilegesRequired=lowest
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
OutputDir=..\..\dist\installer
OutputBaseFilename=RavenCodeSetup-{#AppVersion}-x64
SetupIconFile=..\..\apps\raven-code\assets\raven-code.ico
UninstallDisplayIcon={app}\RavenCode.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Raven Code"; Filename: "{app}\RavenCode.exe"
Name: "{group}\Uninstall Raven Code"; Filename: "{ununinstallexe}"
Name: "{autodesktop}\Raven Code"; Filename: "{app}\RavenCode.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\.rvenc"; ValueType: string; ValueName: ""; ValueData: "RavenCode.Workspace"; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\RavenCode.Workspace"; ValueType: string; ValueName: ""; ValueData: "Raven Code Workspace"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\RavenCode.Workspace\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\file-icons\workspace.ico"
Root: HKCU; Subkey: "Software\Classes\RavenCode.Workspace\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\RavenCode.exe"" ""%1"""
