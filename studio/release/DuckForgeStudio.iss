; Duck Forge Studio's Windows installer: DuckForgeStudioSetup.exe.
;
; Made with Inno Setup 6 (free, https://jrsoftware.org/isinfo.php) by the
; Windows workflow (.github/workflows/studio-release.yml), from the same
; folder the zip is made from:
;
;   ISCC.exe /DVersion=0.1.0 /DSource="dist\Duck Forge Studio" /O"dist" studio\release\DuckForgeStudio.iss
;
; What it does:
; - Installs for you alone, with no administrator rights, in
;   %LOCALAPPDATA%\Programs\Duck Forge Studio (a folder you can write to).
; - Adds Duck Forge Studio to the Start menu and the desktop, with the
;   duck icon, and to Windows Settings > Apps, to uninstall it.
; - Checks that Rust is there, and if it isn't, offers the official install
;   page. It never installs Rust itself.
; - Never touches your games or your settings. Your games are in your
;   Documents (Duck Forge), made there by the Studio the first time it
;   starts; your settings are in %APPDATA%\Duck Forge Studio. Installing a
;   newer version over this one, or uninstalling, leaves both as they are.
;   Uninstalling removes only the Studio, the copy of Duck Forge that came
;   with it, and its build folder (%LOCALAPPDATA%\Duck Forge\build: built
;   programs, made again whenever they're needed).

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Source
  #define Source "..\..\dist\Duck Forge Studio"
#endif
#ifndef SetupName
  #define SetupName "DuckForgeStudioSetup"
#endif

[Setup]
; The same id in every version, so a newer installer replaces an older
; install instead of adding a second one. Never change it.
AppId={{A3533C57-EC19-400A-8B60-754EDE424F1B}
AppName=Duck Forge Studio
AppVersion={#Version}
AppVerName=Duck Forge Studio {#Version}
AppPublisher=Duck Forge
AppPublisherURL=https://github.com/mrguymrguy647-del/Duck-Forge
AppSupportURL=https://github.com/mrguymrguy647-del/Duck-Forge
AppUpdatesURL=https://github.com/mrguymrguy647-del/Duck-Forge/releases
VersionInfoVersion={#Version}
; Just for you: no administrator rights, and {autopf} is then
; %LOCALAPPDATA%\Programs, which you can write to.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Duck Forge Studio
; Not asked: a folder that needs administrator rights (like C:\Program
; Files) wouldn't work.
DisableDirPage=yes
DisableProgramGroupPage=yes
OutputBaseFilename={#SetupName}
SetupIconFile=..\brand\duck-forge.ico
UninstallDisplayIcon={app}\DuckForgeStudio.exe
UninstallDisplayName=Duck Forge Studio
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; If the Studio is open (after pressing Update in it, say), close it first.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Put a Duck Forge Studio icon on the desktop"; GroupDescription: "Icons:"

[InstallDelete]
; The copy of Duck Forge that came with an older version is replaced whole,
; so no old file is left in it. Nothing of yours is ever in here.
Type: filesandordirs; Name: "{app}\duckforge"

[Files]
; Everything in the zip, except the zip's shortcut maker: the installer
; makes the shortcuts itself.
Source: "{#Source}\*"; DestDir: "{app}"; Excludes: "Create desktop shortcut.bat"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Duck Forge Studio"; Filename: "{app}\DuckForgeStudio.exe"; WorkingDir: "{app}"; Comment: "Make games without a terminal"
Name: "{autodesktop}\Duck Forge Studio"; Filename: "{app}\DuckForgeStudio.exe"; WorkingDir: "{app}"; Comment: "Make games without a terminal"; Tasks: desktopicon

[Run]
Filename: "{app}\DuckForgeStudio.exe"; Description: "Start Duck Forge Studio now"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Built programs only: made again whenever they're needed. Never your
; games (Documents\Duck Forge) or your settings (%APPDATA%\Duck Forge Studio).
Type: filesandordirs; Name: "{localappdata}\Duck Forge\build"
Type: dirifempty; Name: "{localappdata}\Duck Forge"

[Code]
const
  RustPage = 'https://www.rust-lang.org/tools/install';

function HasCargo(Folder: String): Boolean;
begin
  Result := (Folder <> '') and FileExists(AddBackslash(RemoveQuotes(Trim(Folder))) + 'cargo.exe');
end;

{ Rust is there if cargo.exe (Rust's build tool) is where rustup puts it,
  or anywhere on the PATH. }
function RustIsInstalled(): Boolean;
var
  Path: String;
  Semicolon: Integer;
begin
  Result := False;
  if GetEnv('CARGO_HOME') <> '' then
    Result := HasCargo(AddBackslash(GetEnv('CARGO_HOME')) + 'bin');
  if not Result then
    Result := HasCargo(ExpandConstant('{%USERPROFILE}') + '\.cargo\bin');
  Path := GetEnv('PATH') + ';';
  while (not Result) and (Path <> '') do
  begin
    Semicolon := Pos(';', Path);
    Result := HasCargo(Copy(Path, 1, Semicolon - 1));
    Delete(Path, 1, Semicolon);
  end;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  ErrorCode: Integer;
begin
  if (CurStep = ssPostInstall) and (not WizardSilent()) and (not RustIsInstalled()) then
  begin
    if MsgBox('Duck Forge Studio is installed.' + #13#10#13#10 +
      'It also needs Rust, the free tool that turns a game''s code into a program, ' +
      'and Rust isn''t on this computer yet. The installer doesn''t install it for you.' + #13#10#13#10 +
      'Open the official Rust page now? There, download rustup-init.exe, run it, and ' +
      'press Enter for the standard install. The Studio will find it afterwards.',
      mbConfirmation, MB_YESNO) = IDYES then
      ShellExec('open', RustPage, '', '', SW_SHOWNORMAL, ewNoWait, ErrorCode);
  end;
end;
