#!/usr/bin/env bash
# verify: the whole check for one game, in one command.
#
#   tools/verify.sh dodge          (50 fuzz runs)
#   tools/verify.sh dodge 200      (200 fuzz runs)
#   tools/verify.sh dodge 0        (no fuzzing: the step says "skipped")
#   tools/verify.sh --list-playthroughs dodge   (only list what it would replay)
#
# It builds the example headless, runs its play tests, replays every
# playthrough of the game (playthroughs/<game>.json, <game>-*.json and
# <game>_*.json, like duck_craft_night.json) with a contact sheet and the
# debug overlay, fuzzes it, and prints one short report. A file named after
# another example is that example's: mini_craft.json isn't replayed for a
# game called mini.
# Everything it saves goes in out/verify/<game>/. It stops with exit code 1
# if anything failed. tools/verify.bat does the same on Windows.

set -u
list_only=0
if [ "${1:-}" = "--list-playthroughs" ]; then
    list_only=1
    shift
fi
game="${1:-}"
runs="${2:-50}"
cd "$(dirname "$0")/.." || exit 2
case "$runs" in
    '' | *[!0-9]*) runs=bad ;;
esac
if [ -z "$game" ] || [ "$runs" = bad ] || { [ ! -f "examples/$game.rs" ] && [ ! -f "examples/$game/main.rs" ]; }; then
    echo "usage: tools/verify.sh <example> [fuzz runs]   (examples/<example>.rs or examples/<example>/main.rs must exist; fuzz runs is a number, 0 to skip fuzzing)"
    echo "       tools/verify.sh --list-playthroughs <example>   (lists the playthroughs it would replay)"
    exit 2
fi

# The game's playthroughs: <game>.json, <game>-*.json and <game>_*.json in
# playthroughs/, except files named after another example (mini_craft.json
# belongs to mini_craft, not to a game called mini).
playthroughs() {
    for script in "playthroughs/$game.json" playthroughs/"$game"-*.json playthroughs/"$game"_*.json; do
        [ -f "$script" ] || continue
        owner=$(basename "$script" .json)
        owner=${owner%%-*}
        if [ "$owner" != "$game" ] && { [ -f "examples/$owner.rs" ] || [ -f "examples/$owner/main.rs" ]; }; then
            continue
        fi
        echo "$script"
    done
}
if [ $list_only -eq 1 ]; then
    playthroughs
    exit 0
fi
out="out/verify/$game"
rm -rf "$out"
mkdir -p "$out"
# Short failure messages: the one-line panic, not a backtrace.
export RUST_BACKTRACE=0
problems=0
warnings=0
started=$SECONDS

# Prints a step's result line, and on failure up to 6 lines of detail.
report() { # report NAME ok|skipped|FAILED TEXT [DETAIL_FILE]
    printf '  %-28s %s: %s\n' "$1" "$2" "$3"
    if [ "$2" = FAILED ]; then
        problems=$((problems + 1))
        if [ -n "${4:-}" ]; then
            head -n 6 "$4" | sed 's/^/      /'
        fi
    fi
}

echo "verify $game"

# 1. Build. Nothing else can run if this fails.
if cargo build -q --no-default-features --example "$game" >"$out/build.txt" 2>&1; then
    report build ok "built without a window"
else
    grep -E -A4 '^error' "$out/build.txt" >"$out/build-errors.txt"
    report build FAILED "see $out/build.txt" "$out/build-errors.txt"
    echo "VERIFY FAILED: $game does not build"
    exit 1
fi

# 2. Play tests.
cargo test -q --no-default-features --example "$game" >"$out/tests.txt" 2>&1
code=$?
result=$(grep -E '^test result' "$out/tests.txt" | tail -n 1 | sed -E 's/^test result: [A-Za-z]+\. //; s/; [0-9]+ measured.*//')
if [ $code -eq 0 ]; then
    report "play tests" ok "${result:-no tests}"
else
    # Each failing test's name, where it panicked and the message (the line
    # after "panicked"), and compile errors with their file and line.
    grep -E -A1 'panicked' "$out/tests.txt" | grep -v -E '^--$|^note: ' >"$out/tests-failed.txt"
    grep -E '^ *(left|right): |^error|^ *--> ' "$out/tests.txt" >>"$out/tests-failed.txt"
    report "play tests" FAILED "${result:-did not run}; all in $out/tests.txt" "$out/tests-failed.txt"
fi

# 3. Every playthrough, with a contact sheet of its events and the debug overlay.
found=0
while IFS= read -r script; do
    found=1
    name=$(basename "$script" .json)
    folder="$out/$name"
    cargo run -q --no-default-features --example "$game" -- \
        --headless --inputs "$script" --sheet-events --debug --quiet --out "$folder" \
        </dev/null >"$folder.txt" 2>&1
    code=$?
    if [ $code -eq 0 ] && [ ! -s "$folder.txt" ]; then
        final=$(grep -E '^final: ' "$folder/summary.txt" | cut -c 8-)
        frames=$(head -n 1 "$folder/summary.txt" | sed -E 's/.* for ([0-9]+) frames.*/\1/')
        report "$name.json" ok "$frames frames; ${final:-nothing reported}"
    elif [ $code -eq 0 ] && ! grep -q -v -E '^duckforge: playthrough (changed|tuned): ' "$folder.txt"; then
        # Nothing broke, but it no longer ends the way it was recorded, or
        # it was recorded with other tune values (and still ends as
        # expected): a warning, with the command to accept it.
        while IFS= read -r line; do
            kind=$(printf '%s' "$line" | sed -E 's/^duckforge: playthrough (changed|tuned): .*/\1/')
            text=$(printf '%s' "$line" | sed -E 's/^duckforge: playthrough (changed|tuned): //; s/; (if that.s right|--accept stores).*//')
            printf '  %-28s %s: %s\n' "$name.json" "$kind" "$text"
        done <"$folder.txt"
        echo "      if that's right: cargo run --no-default-features --example $game -- --headless --inputs $script --accept"
        echo "      (or record it again: F10 in the game, or its record_playthrough test)"
        warnings=$((warnings + 1))
    else
        report "$name.json" FAILED "see $folder/summary.txt" "$folder.txt"
    fi
    # Tune values that aren't the ones in the code: never hidden.
    if [ -f "$folder/summary.txt" ]; then
        grep '^TUNED' "$folder/summary.txt" | sed 's/^/      /'
    fi
done < <(playthroughs)
if [ $found -eq 0 ]; then
    report playthroughs ok "none (save one as playthroughs/$game.json)"
fi

# 4. A short fuzz: random key presses, looking for crashes, failed checks
#    and freezes. 0 runs skips it.
if [ "$runs" -eq 0 ]; then
    report fuzz skipped "0 runs asked for"
else
    cargo run -q --no-default-features --example "$game" -- \
        --fuzz "$runs" --out "$out/fuzz" >"$out/fuzz.txt" 2>&1
    code=$?
    if [ $code -eq 0 ] && grep -q 'no problems found' "$out/fuzz.txt"; then
        report "fuzz $runs runs" ok "no problems found"
    else
        grep -v -E 'fuzzed [0-9]+ runs|^duckforge: coverage: ' "$out/fuzz.txt" | sed 's/^duckforge: //' >"$out/fuzz-problems.txt"
        count=$(grep -c '^seed' "$out/fuzz-problems.txt")
        report "fuzz $runs runs" FAILED "$count different problem(s); replay scripts in $out/fuzz" "$out/fuzz-problems.txt"
    fi
    # What the random runs reached, so "no problems found" can be judged.
    grep '^duckforge: coverage: ' "$out/fuzz.txt" | sed 's/^duckforge: coverage: /      coverage: /'
fi

seconds=$((SECONDS - started))
if [ $problems -eq 0 ] && [ $warnings -gt 0 ]; then
    echo "VERIFY OK with $warnings warning(s) above: $game in ${seconds} s (pictures and summaries in $out)"
elif [ $problems -eq 0 ]; then
    echo "VERIFY OK: $game in ${seconds} s (pictures and summaries in $out)"
else
    echo "VERIFY FAILED: $problems of the steps above for $game (everything in $out)"
    exit 1
fi
