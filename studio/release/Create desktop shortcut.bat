@echo off
rem ================================================================
rem  Double-click this file to put a "Duck Forge Studio" shortcut,
rem  with the duck icon, on your desktop. It points at
rem  DuckForgeStudio.exe in this folder, so leave the folder where it
rem  is afterwards.
rem ================================================================
cd /d "%~dp0"

if not exist "DuckForgeStudio.exe" (
    echo DuckForgeStudio.exe isn't in this folder.
    echo Unzip the whole download first, then double-click this file inside the unzipped folder.
    echo.
    pause
    exit /b 1
)

powershell -NoProfile -Command "$s = (New-Object -ComObject WScript.Shell).CreateShortcut([Environment]::GetFolderPath('Desktop') + '\Duck Forge Studio.lnk'); $s.TargetPath = '%~dp0DuckForgeStudio.exe'; $s.WorkingDirectory = '%~dp0'; $s.IconLocation = '%~dp0DuckForgeStudio.exe,0'; $s.Save()"
if errorlevel 1 (
    echo.
    echo Could not create the shortcut.
) else (
    echo.
    echo Done! There is now a "Duck Forge Studio" shortcut on your desktop.
    echo Tip: right-click it and choose "Pin to taskbar" if you like.
)
echo.
pause
