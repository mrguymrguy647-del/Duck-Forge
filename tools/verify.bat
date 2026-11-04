@echo off
rem verify: the whole check for one game, in one command.
rem
rem   tools\verify.bat dodge          (50 fuzz runs)
rem   tools\verify.bat dodge 200      (200 fuzz runs)
rem
rem It builds the example headless, runs its play tests, replays every
rem playthroughs\<game>.json and playthroughs\<game>-*.json with a contact
rem sheet and the debug overlay, fuzzes it, and prints one short report.
rem Everything it saves goes in out\verify\<game>\. It stops with exit code 1
rem if anything failed. tools/verify.sh does the same on Linux and macOS.

setlocal EnableDelayedExpansion
set "game=%~1"
set "runs=%~2"
if "%runs%"=="" set "runs=50"
cd /d "%~dp0.."
if "%game%"=="" goto usage
if not exist "examples\%game%.rs" goto usage

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
    findstr /b /c:"error" "%out%\build.txt" > "%out%\build-errors.txt"
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
if "!code!"=="0" (
    set "msg=!result!"
    set "detail="
    call :report "play tests" ok
) else (
    rem Each failing test's place and message (the line after "panicked"),
    rem and compile errors with their file and line.
    set "next=0"
    (for /f "usebackq delims=" %%L in ("%out%\tests.txt") do (
        set "line=%%L"
        if "!next!"=="1" echo %%L
        set "next=0"
        if not "!line:panicked=!"=="!line!" (
            echo %%L
            set "next=1"
        )
        if "!line:~0,5!"=="error" echo %%L
        if not "!line:--> =!"=="!line!" echo %%L
    )) > "%out%\tests-failed.txt"
    set "msg=!result!; all in %out%\tests.txt"
    set "detail=%out%\tests-failed.txt"
    call :report "play tests" FAILED
)

rem 3. Every playthrough, with a contact sheet and the debug overlay.
set "found=0"
for %%S in ("playthroughs\%game%.json" "playthroughs\%game%-*.json") do (
    if exist "%%~S" (
        set "found=1"
        set "name=%%~nS"
        set "folder=%out%\%%~nS"
        cargo run -q --no-default-features --example %game% -- --headless --inputs "%%~S" --sheet --debug --quiet --out "!folder!" > "!folder!.txt" 2>&1
        set "failed=!errorlevel!"
        set "changed=0"
        for %%A in ("!folder!.txt") do if %%~zA GTR 0 (
            set "failed=1"
            rem Only "playthrough changed" lines: a warning, not a failure.
            findstr /v /b /c:"duckforge: playthrough changed: " "!folder!.txt" > nul
            if errorlevel 1 set "changed=1"
        )
        if "!changed!"=="1" (
            set "changes="
            for /f "usebackq delims=" %%L in ("!folder!.txt") do (
                set "line=%%L"
                set "changes=!line:duckforge: playthrough changed: =!"
            )
            set "label=!name!.json                            "
            echo   !label:~0,28! changed: !changes!
            echo       if the new ending is right: cargo run --no-default-features --example %game% -- --headless --inputs %%~S --accept
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
    )
)
if "!found!"=="0" (
    set "msg=none (save one as playthroughs\%game%.json)"
    set "detail="
    call :report playthroughs ok
)

rem 4. A short fuzz: random key presses, looking for crashes, failed checks
rem    and freezes.
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
rem   call :report NAME ok|FAILED
:report
set "label=%~1                            "
echo   !label:~0,28! %~2: !msg!
if /i "%~2"=="ok" exit /b 0
set /a problems+=1
if "!detail!"=="" exit /b 0
set /a shown=0
for /f "usebackq delims=" %%L in ("!detail!") do (
    if !shown! lss 6 echo       %%L
    set /a shown+=1
)
exit /b 0

:usage
echo usage: tools\verify.bat ^<example^> [fuzz runs]   (examples\^<example^>.rs must exist)
exit /b 2
