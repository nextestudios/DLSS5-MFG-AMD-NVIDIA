; Instalador do DLSS5-FrameGen (Inno Setup 6). Gerado no workflow de release:
;   iscc /DAppVersion=1.2.0 /DExe=..\dist\DLSS5-FrameGen.exe /O..\dist installer\DLSS5-FrameGen.iss
; Instala por usuário (sem Administrador) em %LOCALAPPDATA%\Programs, onde o app consegue se atualizar sozinho.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\target\release\gpu-mods-app.exe"
#endif

[Setup]
; Fixed: identifies the app across versions (upgrade in place, one uninstall entry).
AppId={{412866E3-F72B-42E9-A96F-6085599E7C51}
AppName=DLSS5-FrameGen
AppVersion={#AppVersion}
AppVerName=DLSS5-FrameGen {#AppVersion}
AppPublisher=Nexte Studios
AppPublisherURL=https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA
AppSupportURL=https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/issues
AppUpdatesURL=https://github.com/nextestudios/DLSS5-MFG-AMD-NVIDIA/releases
DefaultDirName={localappdata}\Programs\DLSS5-FrameGen
DefaultGroupName=DLSS5-FrameGen
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputBaseFilename=DLSS5-FrameGen-Setup
UninstallDisplayIcon={app}\DLSS5-FrameGen.exe
LicenseFile=..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Languages]
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "DLSS5-FrameGen.exe"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\DLSS5-FrameGen"; Filename: "{app}\DLSS5-FrameGen.exe"
Name: "{autodesktop}\DLSS5-FrameGen"; Filename: "{app}\DLSS5-FrameGen.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\DLSS5-FrameGen.exe"; Description: "{cm:LaunchProgram,DLSS5-FrameGen}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Leftover of a self-update (the previous exe, renamed while it was running).
Type: files; Name: "{app}\DLSS5-FrameGen.old.exe"
