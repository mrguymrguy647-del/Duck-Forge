@echo off
rem rust_check: makes sure Rust is installed before anything is built.
rem
rem   call "tools\rust_check.bat"
rem
rem "Start Duck Forge Studio.bat" and "tools\Run tests (for developers).bat"
rem call it first. If cargo (Rust's build tool) is there, it says nothing
rem and returns 0. If Rust was installed but this window doesn't know yet
rem (the PATH changes when you sign in again), it finds it in the usual
rem place. If Rust isn't installed, it says so in plain words, offers to
rem open the official install page, and returns 1. It never installs
rem anything by itself.

where cargo > nul 2> nul
if not errorlevel 1 exit /b 0

if defined CARGO_HOME if exist "%CARGO_HOME%\bin\cargo.exe" (
    set "PATH=%CARGO_HOME%\bin;%PATH%"
    exit /b 0
)
if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    exit /b 0
)

echo.
echo   Rust isn't installed on this computer yet.
echo.
echo   Duck Forge is written in Rust, a free programming language, and it
echo   needs Rust to build the Studio, your games and the tests.
echo.
echo   Get it from the official page:
echo       https://www.rust-lang.org/tools/install
echo.
echo   There, download rustup-init.exe (64-bit), run it, and press Enter to
echo   choose the standard install. If it asks for the Visual Studio C++
echo   tools, let it install them too. When it says Rust is installed,
echo   double-click this file again.
echo.
choice /c YN /n /m "  Open that page in your web browser now? Press Y for yes or N for no: "
if errorlevel 2 exit /b 1
start "" "https://www.rust-lang.org/tools/install"
exit /b 1
