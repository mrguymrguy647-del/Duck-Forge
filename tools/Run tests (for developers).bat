@echo off
rem ================================================================
rem  Runs every Duck Forge test. Double-click it in File Explorer.
rem
rem  1. The engine's tests, with everything on (window, graphics card
rem     and sound): the same as typing "cargo test".
rem  2. The engine's tests again with no window, the way a server runs
rem     them: "cargo test --no-default-features".
rem  3. Duck Forge Studio's tests, which build real games and play them:
rem     "cargo test --workspace" in the studio folder.
rem
rem  The first time takes a while (Rust builds everything once, and needs
rem  the internet for that). After that it's a few minutes.
rem ================================================================
setlocal
cd /d "%~dp0.."
title Duck Forge tests

call "tools\rust_check.bat"
if errorlevel 1 (
    pause
    exit /b 1
)

echo.
echo [1 of 3] The engine's tests, with everything on (window, graphics card and sound)...
echo.
cargo test
if errorlevel 1 goto failed

echo.
echo [2 of 3] The engine's tests again, without a window, the way a server runs them...
echo.
cargo test --no-default-features
if errorlevel 1 goto failed

rem A copy of Duck Forge made by the Studio (in Documents) has no Studio
rem code in it: there, the first two are all there is.
if not exist "studio\Cargo.toml" goto passed
echo.
echo [3 of 3] Duck Forge Studio's tests (they build real games and play them)...
echo.
pushd studio
cargo test --workspace
set "studio_result=%errorlevel%"
popd
if not "%studio_result%"=="0" goto failed

:passed

echo.
echo ================================================================
echo  All tests passed.
echo ================================================================
pause
exit /b 0

:failed
echo.
echo ================================================================
echo  Some tests FAILED. Scroll up to see which ones, and why: look for
echo  the word FAILED and the lines after "failures:".
echo ================================================================
pause
exit /b 1
