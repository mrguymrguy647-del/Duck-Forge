@echo off
rem Runs every duckforge test. Double-click it in File Explorer, or run
rem "test.bat" in a terminal in this folder.
cd /d "%~dp0"

echo Running the tests with everything on (window, GPU and sound)...
cargo test
if errorlevel 1 goto failed

echo.
echo Running the tests again without a window, the way a server runs them...
cargo test --no-default-features
if errorlevel 1 goto failed

echo.
echo All tests passed.
pause
exit /b 0

:failed
echo.
echo Some tests FAILED. Scroll up to see which ones, and why.
pause
exit /b 1
