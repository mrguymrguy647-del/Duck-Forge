@echo off
rem new_game: start a new game from a starter, ready to build and verify.
rem
rem   tools\new_game.bat mygame topdown      (or platformer, shooter, tiny)
rem
rem It copies the starter (and its tune file) to examples\mygame, renames
rem the game's main type (TopDown -> Mygame), sets the window title, points
rem the playthrough bot at playthroughs\mygame.json, adds the [[example]]
rem entry to Cargo.toml, records a first playthrough with the starter's bot
rem (if it has one), and runs tools\verify.bat mygame, which must pass.
rem Set NEW_GAME_NO_VERIFY=1 to skip those last two steps. The text edits
rem are done by tools\new_game.ps1 (PowerShell comes with Windows).
rem tools/new_game.sh does the same on Linux and macOS.

setlocal EnableDelayedExpansion
set "name=%~1"
set "kind=%~2"
cd /d "%~dp0.."
if "%name%"=="" goto usage
if "%kind%"=="" goto usage

rem The name: lowercase letters, digits and _, starting with a letter.
rem (findstr's [a-z] also matches capitals, so the letters are spelt out.)
set "lower=abcdefghijklmnopqrstuvwxyz"
echo %name%| findstr /r /x "[%lower%][%lower%0123456789_]*" > nul
if errorlevel 1 goto usage

set "starter="
if /i "%kind%"=="platformer" (
    set "starter=template"
    set "type=Template"
    set "title=Template - duckforge"
)
if /i "%kind%"=="topdown" (
    set "starter=starter_topdown"
    set "type=TopDown"
    set "title=Top-down starter - duckforge"
)
if /i "%kind%"=="shooter" (
    set "starter=starter_shooter"
    set "type=Shooter"
    set "title=Shooter starter - duckforge"
)
if /i "%kind%"=="tiny" (
    set "starter=quickstart"
    set "type=Catch"
    set "title="
)
if "%starter%"=="" goto usage

set "taken=0"
if exist "examples\%name%.rs" set "taken=1"
if exist "examples\%name%\" set "taken=1"
findstr /x /c:"name = \"%name%\"" Cargo.toml > nul && set "taken=1"
if "%taken%"=="1" (
    echo new_game: there's already a game called %name% ^(examples\%name% or in Cargo.toml^); pick another name
    exit /b 1
)

rem The files: one file, or a folder with main.rs, tests.rs and bot.rs.
if exist "examples\%starter%\main.rs" (
    xcopy /e /i /q "examples\%starter%" "examples\%name%" > nul
) else (
    copy /y "examples\%starter%.rs" "examples\%name%.rs" > nul
)
if errorlevel 1 (
    echo new_game: could not copy examples\%starter%
    exit /b 1
)
if exist "examples\%starter%.tune.json" copy /y "examples\%starter%.tune.json" "examples\%name%.tune.json" > nul

rem -Title only when there is one: Windows PowerShell 5 can't take an
rem empty "" argument with -File.
set "titlearg="
if defined title set titlearg=-Title "%title%"
powershell -NoProfile -ExecutionPolicy Bypass -File "tools\new_game.ps1" -Name "%name%" -Kind "%kind%" -Starter "%starter%" -Type "%type%" %titlearg%
if errorlevel 1 (
    echo new_game: renaming inside the new game failed ^(see above^)
    exit /b 1
)

if "%NEW_GAME_NO_VERIFY%"=="1" exit /b 0

rem A first playthrough, recorded by the starter's bot, so verify has
rem something to replay (and to compare with later).
if exist "examples\%name%\bot.rs" (
    echo new_game: recording playthroughs\%name%.json with its bot
    cargo test -q --no-default-features --example %name% -- --ignored record_playthrough > nul 2>&1
    if errorlevel 1 echo new_game: the bot couldn't record a playthrough ^(run: cargo test --no-default-features --example %name% -- --ignored^)
)
echo new_game: checking it with tools\verify.bat %name%
call "tools\verify.bat" %name%
exit /b %errorlevel%

:usage
echo usage: tools\new_game.bat ^<name^> ^<platformer^|topdown^|shooter^|tiny^>
echo   the name is lowercase letters, digits and _, starting with a letter, like bee_maze
exit /b 2
