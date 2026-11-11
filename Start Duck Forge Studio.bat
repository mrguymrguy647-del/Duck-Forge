@echo off
rem ================================================================
rem  Double-click this file to open Duck Forge Studio.
rem
rem  It builds the Studio from the code in this folder, then opens it.
rem  The first time takes several minutes and needs the internet: Rust
rem  downloads the parts the Studio is made from and builds them once.
rem  After that it opens in a few seconds.
rem
rem  (If you installed Duck Forge Studio with DuckForgeStudioSetup.exe,
rem  you don't need this file: use the Start menu or the desktop icon.)
rem ================================================================
setlocal
cd /d "%~dp0"
title Duck Forge Studio

call "tools\rust_check.bat"
if errorlevel 1 (
    pause
    exit /b 1
)

echo.
if exist "studio\target\release\DuckForgeStudio.exe" (
    echo Getting Duck Forge Studio ready. This takes a few seconds,
    echo or a minute or two if this folder's code has changed...
) else (
    echo Building Duck Forge Studio for the first time.
    echo This takes several minutes, and needs the internet. You'll see
    echo a line for each part Rust builds. You can leave it running.
)
echo.

pushd studio
cargo build --release -p studio_app
set "build_result=%errorlevel%"
popd
if not "%build_result%"=="0" goto failed

echo.
echo Opening Duck Forge Studio...
start "" "studio\target\release\DuckForgeStudio.exe"
exit /b 0

:failed
echo.
echo ================================================================
echo  Duck Forge Studio couldn't be built. The lines above say why:
echo  look for "error". If it says it couldn't download something,
echo  check the internet connection and double-click this file again.
echo ================================================================
pause
exit /b 1
