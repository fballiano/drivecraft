; The Windows installer of DriveCraft, for Inno Setup 6.3 or later.
; scripts/windows-installer.sh builds it from a release zip:
;
;   iscc /DVersion=0.1.2 /DArch=x64 /DSource=<folder with DriveCraft.exe> /DOutput=dist drivecraft.iss
;
; It installs for the current user (no administrator rights), into
; %LOCALAPPDATA%\Programs\DriveCraft, with a shortcut in the Start menu.
; It never touches the synced folders or the settings.

#ifndef Version
  #error Give the version, for example /DVersion=0.1.2
#endif
#ifndef Arch
  #error Give the architecture: /DArch=x64 or /DArch=arm64
#endif
#ifndef Source
  #error Give the folder with DriveCraft.exe: /DSource=...
#endif
#ifndef Output
  #define Output "."
#endif

[Setup]
AppId={{3A7893EC-F309-4316-BD01-E6193C68A893}
AppName=DriveCraft
AppVersion={#Version}
AppVerName=DriveCraft {#Version}
AppPublisher=Fabrizio Balliano
AppPublisherURL=https://drivecraft.fabrizioballiano.com/
AppSupportURL=https://github.com/fballiano/drivecraft/issues
AppUpdatesURL=https://github.com/fballiano/drivecraft/releases
DefaultDirName={autopf}\DriveCraft
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
#if Arch == "arm64"
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#else
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
#endif
OutputDir={#Output}
OutputBaseFilename=DriveCraft-{#Version}-windows-{#Arch}-setup
SetupIconFile=..\..\crates\app\assets\DriveCraft.ico
UninstallDisplayIcon={app}\DriveCraft.ico
UninstallDisplayName=DriveCraft
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

[Files]
Source: "{#Source}\DriveCraft.exe"; DestDir: "{app}"; Flags: ignoreversion
; DriveCraft looks for rclone next to its own executable.
Source: "{#Source}\rclone.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Source}\licenses\*"; DestDir: "{app}\licenses"; Flags: ignoreversion recursesubdirs
Source: "..\..\crates\app\assets\DriveCraft.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\DriveCraft"; Filename: "{app}\DriveCraft.exe"; IconFilename: "{app}\DriveCraft.ico"

[Run]
Filename: "{app}\DriveCraft.exe"; Description: "Start DriveCraft"; Flags: nowait postinstall skipifsilent

[Code]
{ Stops DriveCraft and its rclone in the folder Dir. The filter on the path
  never stops a different rclone of the user. A stop during a sync is like a
  crash: the next start of DriveCraft cleans up. }
procedure StopDriveCraft(Dir: String);
var
  ResultCode: Integer;
begin
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "Get-Process -Name DriveCraft,rclone -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -like ''' + AddBackslash(Dir) + '*'' } | Stop-Process -Force"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopDriveCraft(WizardDirValue());
  Result := '';
end;

function InitializeUninstall(): Boolean;
begin
  StopDriveCraft(ExpandConstant('{app}'));
  Result := True;
end;
