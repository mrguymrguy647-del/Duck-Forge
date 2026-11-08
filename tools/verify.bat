@echo off
rem verify: the whole check for one game, in one command.
rem
rem   tools\verify.bat dodge          (50 fuzz runs)
rem   tools\verify.bat dodge 200      (200 fuzz runs)
rem   tools\verify.bat dodge 0        (no fuzzing: the step says "skipped")
rem   tools\verify.bat --list-playthroughs dodge   (only list what it would replay)
rem
rem It builds the example headless, runs its play tests, replays every
rem playthrough of the game (playthroughs\<game>.json, <game>-*.json and
rem <game>_*.json, like duck_craft_night.json) with a contact sheet and the
rem debug overlay, fuzzes it, and prints one short report. A file named
rem after another example is that example's: mini_craft.json isn't replayed
rem for a game called mini.
rem Everything it saves goes in out\verify\<game>\. It stops with exit code 1
rem if anything failed. tools/verify.sh does the same on Linux and macOS.

setlocal EnableDelayedExpansion
set "list_only=0"
if /i "%~1"=="--list-playthroughs" set "list_only=1"
if "%list_only%"=="1" shift
set "game=%~1"
set "runs=%~2"
if "%runs%"=="" set "runs=50"
cd /d "%~dp0.."
if "%game%"=="" goto usage
rem The number of fuzz runs: digits only.
echo %runs%| findstr /r /x "[0-9][0-9]*" > nul || goto usage
if not exist "examples\%game%.rs" if not exist "examples\%game%\main.rs" goto usage

rem --list-playthroughs: only say which playthroughs would be replayed.
if "%list_only%"=="1" (
    for %%S in ("playthroughs\%game%.json" "playthroughs\%game%-*.json" "playthroughs\%game%_*.json") do (
        if exist "%%~S" call :owner "%%~nS"
        if exist "%%~S" if "!skip!"=="0" echo playthroughs\%%~nxS
    )
    exit /b 0
)

set "out=out\verify\%game%"
if exist "%out%" rmdir /s /q "%out%"
mkdir "%out%"
rem Short failure messages: the one-line panic, not a backtrace.
set "RUST_BACKTRACE=0"
set /a problems=0
set /a warnings=0

echo verify %game%

rem 1. Build. Nothing else can run if this fails.
cargo build -q --no-default-features --example %game% > "%out%\build.txt" 2>&1
if errorlevel 1 (
    rem The errors, with their file and line (the "-->" lines).
    findstr /r /c:"^error" /c:"^ *--> " "%out%\build.txt" > "%out%\build-errors.txt"
    set "msg=see %out%\build.txt"
    set "detail=%out%\build-errors.txt"
    call :report build FAILED
    echo VERIFY FAILED: %game% does not build
    exit /b 1
)
set "msg=built without a window"
set "detail="
call :report build ok

rem 2. Play tests.
cargo test -q --no-default-features --example %game% > "%out%\tests.txt" 2>&1
set "code=!errorlevel!"
set "result=no tests"
for /f "delims=" %%L in ('findstr /b /c:"test result" "%out%\tests.txt"') do set "result=%%L"
set "result=!result:test result: ok. =!"
set "result=!result:test result: FAILED. =!"
if not "!result!"=="no tests" for /f "tokens=1-3 delims=;" %%a in ("!result!") do set "result=%%a;%%b;%%c"
if not "!code!"=="0" if "!result!"=="no tests" set "result=did not run"
if "!code!"=="0" (
    set "msg=!result!"
    set "detail="
    call :report "play tests" ok
) else (
    rem Each failing test's place and message (the line after "panicked"),
    rem and compile errors with their file and line. PowerShell copies the
    rem lines exactly: cmd would drop the "!" in messages like assert_eq!.
    powershell -NoProfile -Command "$l = @(Get-Content -LiteralPath '%out%\tests.txt'); for ($i = 0; $i -lt $l.Count; $i++) { if ($l[$i] -match 'panicked') { $l[$i]; if ($i + 1 -lt $l.Count) { $l[$i + 1] } } elseif ($l[$i] -match '^\s*(left|right): |^error|^\s*--> ') { $l[$i] } }" > "%out%\tests-failed.txt"
    set "msg=!result!; all in %out%\tests.txt"
    set "detail=%out%\tests-failed.txt"
    call :report "play tests" FAILED
)

rem 3. Every playthrough, with a contact sheet of its events and the debug overlay.
set "found=0"
for %%S in ("playthroughs\%game%.json" "playthroughs\%game%-*.json" "playthroughs\%game%_*.json") do (
    if exist "%%~S" call :owner "%%~nS"
    if exist "%%~S" if "!skip!"=="0" (
        set "found=1"
        set "name=%%~nS"
        set "folder=%out%\%%~nS"
        cargo run -q --no-default-features --example %game% -- --headless --inputs "%%~S" --sheet-events --debug --quiet --out "!folder!" > "!folder!.txt" 2>&1
        set "failed=!errorlevel!"
        set "changed=0"
        for %%A in ("!folder!.txt") do if %%~zA GTR 0 (
            set "failed=1"
            rem Only "playthrough changed" or "playthrough tuned" lines: a
            rem warning, not a failure.
            findstr /v /b /c:"duckforge: playthrough changed: " /c:"duckforge: playthrough tuned: " "!folder!.txt" > nul
            if errorlevel 1 set "changed=1"
        )
        if "!changed!"=="1" (
            set "label=!name!.json                            "
            for /f "usebackq delims=" %%L in ("!folder!.txt") do (
                set "line=%%L"
                echo   !label:~0,28! !line:duckforge: playthrough =!
            )
            echo       if that's right: cargo run --no-default-features --example %game% -- --headless --inputs %%~S --accept
            echo       ^(or record it again: F10 in the game, or its record_playthrough test^)
            set /a warnings+=1
        ) else if "!failed!"=="0" (
            set "final=nothing reported"
            for /f "tokens=1,* delims= " %%a in ('findstr /b /c:"final: " "!folder!\summary.txt"') do set "final=%%b"
            set "frames=?"
            for /f "usebackq delims=" %%L in ("!folder!\summary.txt") do if "!frames!"=="?" (
                set "line=%%L"
                set "line=!line:* for =!"
                for /f "tokens=1" %%n in ("!line!") do set "frames=%%n"
            )
            set "msg=!frames! frames; !final!"
            set "detail="
            call :report "!name!.json" ok
        ) else (
            set "msg=see !folder!\summary.txt"
            set "detail=!folder!.txt"
            call :report "!name!.json" FAILED
        )
        rem Tune values that aren't the ones in the code: never hidden.
        if exist "!folder!\summary.txt" (
            for /f "delims=" %%T in ('findstr /b /c:"TUNED" "!folder!\summary.txt"') do echo       %%T
        )
    )
)
if "!found!"=="0" (
    set "msg=none (save one as playthroughs\%game%.json)"
    set "detail="
    call :report playthroughs ok
)

rem 4. A short fuzz: random key presses, looking for crashes, failed checks
rem    and freezes. 0 runs skips it.
if "%runs%"=="0" goto skipfuzz
cargo run -q --no-default-features --example %game% -- --fuzz %runs% --out "%out%\fuzz" > "%out%\fuzz.txt" 2>&1
set "code=!errorlevel!"
findstr /c:"no problems found" "%out%\fuzz.txt" > nul
if "!code!!errorlevel!"=="00" (
    set "msg=no problems found"
    set "detail="
    call :report "fuzz %runs% runs" ok
) else (
    findstr /v /c:"fuzzed " /c:"duckforge: coverage: " "%out%\fuzz.txt" > "%out%\fuzz-problems.txt"
    set /a count=0
    for /f %%L in ('findstr /b /c:"duckforge: seed" "%out%\fuzz-problems.txt"') do set /a count+=1
    set "msg=!count! different problem(s); replay scripts in %out%\fuzz"
    set "detail=%out%\fuzz-problems.txt"
    call :report "fuzz %runs% runs" FAILED
)

rem What the random runs reached, so "no problems found" can be judged.
for /f "usebackq delims=" %%L in ("%out%\fuzz.txt") do (
    set "line=%%L"
    if "!line:~0,20!"=="duckforge: coverage:" echo       coverage: !line:~21!
)
goto fuzzdone

:skipfuzz
set "msg=0 runs asked for"
set "detail="
call :report fuzz skipped

:fuzzdone

if !problems!==0 (
    if !warnings! gtr 0 (
        echo VERIFY OK with !warnings! warning^(s^) above: %game% ^(pictures and summaries in %out%^)
    ) else (
        echo VERIFY OK: %game% ^(pictures and summaries in %out%^)
    )
    exit /b 0
)
echo VERIFY FAILED: !problems! of the steps above for %game% ^(everything in %out%^)
exit /b 1

rem Prints a step's result line (the text is in msg), and on failure up to
rem 6 lines of the file named in detail.
rem   call :report NAME ok|skipped|FAILED
:report
set "label=%~1                            "
echo   !label:~0,28! %~2: !msg!
if /i "%~2"=="ok" exit /b 0
if /i "%~2"=="skipped" exit /b 0
set /a problems+=1
if "!detail!"=="" exit /b 0
rem The first 6 lines, exactly as they are (cmd would drop any "!").
powershell -NoProfile -Command "Get-Content -LiteralPath '!detail!' -TotalCount 6 | ForEach-Object { '      ' + $_ }"
exit /b 0

rem Sets skip=1 if the playthrough named %1 (without .json) belongs to
rem another example: mini_craft.json is mini_craft's, not a game called mini's.
rem   call :owner NAME
:owner
set "skip=0"
for /f "tokens=1 delims=-" %%o in ("%~1") do set "owner=%%o"
if /i "!owner!"=="%game%" exit /b 0
if exist "examples\!owner!.rs" set "skip=1"
if exist "examples\!owner!\main.rs" set "skip=1"
exit /b 0

:usage
echo usage: tools\verify.bat ^<example^> [fuzz runs]   ^(examples\^<example^>.rs or examples\^<example^>\main.rs must exist; fuzz runs is a number, 0 to skip fuzzing^)
echo        tools\verify.bat --list-playthroughs ^<example^>   ^(lists the playthroughs it would replay^)
exit /b 2
