#!/usr/bin/env bash
# new_game: start a new game from a starter, ready to build and verify.
#
#   tools/new_game.sh mygame topdown      (or platformer, shooter, runner3d, tiny)
#
# It copies the starter to examples/mygame, renames
# the game's main type (TopDown -> Mygame), sets the window title, points
# the playthrough bot at playthroughs/mygame.json, adds the [[example]]
# entry to Cargo.toml, records a first playthrough with the starter's bot
# (if it has one), and runs tools/verify.sh mygame, which must pass.
# Set NEW_GAME_NO_VERIFY=1 to skip that last step.
# tools/new_game.bat does the same on Windows.

set -u
name="${1:-}"
kind="${2:-}"
usage() {
    echo "usage: tools/new_game.sh <name> <platformer|topdown|shooter|runner3d|tiny>"
    echo "  the name is lowercase letters, digits and _, starting with a letter, like bee_maze"
    exit 2
}
cd "$(dirname "$0")/.." || exit 2
[ -n "$name" ] && [ -n "$kind" ] || usage
case "$name" in
    [a-z]*) ;;
    *) usage ;;
esac
if [ -n "$(printf '%s' "$name" | tr -d 'a-z0-9_')" ]; then
    usage
fi
case "$kind" in
    platformer) starter=template; type=Template; title="Template - duckforge" ;;
    topdown) starter=starter_topdown; type=TopDown; title="Top-down starter - duckforge" ;;
    shooter) starter=starter_shooter; type=Shooter; title="Shooter starter - duckforge" ;;
    runner3d) starter=starter_runner3d; type=Runner; title="3D runner starter - duckforge" ;;
    tiny) starter=quickstart; type=Catch; title="" ;;
    *) usage ;;
esac
if [ -e "examples/$name.rs" ] || [ -e "examples/$name" ] || grep -q "^name = \"$name\"$" Cargo.toml; then
    echo "new_game: there's already a game called $name (examples/$name or in Cargo.toml); pick another name"
    exit 1
fi

# The files: one file, or a folder with main.rs, tests.rs and bot.rs.
if [ -d "examples/$starter" ]; then
    cp -R "examples/$starter" "examples/$name"
    files=$(ls "examples/$name"/*.rs)
    main="examples/$name/main.rs"
else
    cp "examples/$starter.rs" "examples/$name.rs"
    files="examples/$name.rs"
    main="examples/$name.rs"
fi
# Not the starter's tune file: its values would quietly win over the ones
# you write in the new game's code. Make one when you want to tune, with
# --write-tune. Its feel file (the Studio's labels, ranges and presets for
# those values) does come along.
if [ -f "examples/$starter.feel.json" ]; then
    cp "examples/$starter.feel.json" "examples/$name.feel.json"
fi

# The game's type: bee_maze -> BeeMaze (BeeMazeGame if the starter
# already uses that word for something else).
new_type=$(printf '%s' "$name" | awk -F_ '{ for (i = 1; i <= NF; i++) printf "%s", toupper(substr($i, 1, 1)) substr($i, 2) }')
for file in $files; do
    if grep -q -E "(^|[^A-Za-z0-9_])$new_type([^A-Za-z0-9_]|\$)" "$file"; then
        new_type="${new_type}Game"
    fi
done
# Its name in words, each starting with a capital: bee_maze -> Bee Maze.
pretty=$(printf '%s' "$name" | tr '_' ' ' | awk '{ for (i = 1; i <= NF; i++) $i = toupper(substr($i, 1, 1)) substr($i, 2); print }')

# Whole words only: the type, and the starter's name in commands and paths.
for file in $files; do
    # The title is set aside first, since both the old title and the new
    # one can contain the type's name ("Template - duckforge", "My Shooter").
    if [ -n "$title" ]; then
        sed "s/\"$title\"/\"NEW_GAME_TITLE\"/" "$file" >"$file.new" && mv "$file.new" "$file"
    fi
    for _ in 1 2; do # twice, for names next to each other like "TopDown(TopDown"
        # In sentences ("plays the template") the starter becomes "the game";
        # in commands and paths it becomes the new name.
        sed -E \
            -e "s/(^|[^A-Za-z0-9_])$type([^A-Za-z0-9_]|\$)/\\1$new_type\\2/g" \
            -e "s/(^|[^A-Za-z0-9_])([Tt]he) $starter([^A-Za-z0-9_\/.]|\$)/\\1\\2 game\\3/g" \
            -e "s/(^|[^A-Za-z0-9_])$starter([^A-Za-z0-9_]|\$)/\\1$name\\2/g" \
            "$file" >"$file.new" && mv "$file.new" "$file"
    done
    sed "s/\"NEW_GAME_TITLE\"/\"$pretty - duckforge\"/" "$file" >"$file.new" && mv "$file.new" "$file"
done
# Say where it came from, at the top.
{
    printf '//! %s: started from the %s starter with tools/new_game.\n//!\n' "$pretty" "$kind"
    cat "$main"
} >"$main.new" && mv "$main.new" "$main"

printf '\n[[example]]\nname = "%s"\ntest = true\n' "$name" >>Cargo.toml

echo "new_game: made examples/$name$([ -d "examples/$name" ] && echo /) from the $kind starter ($starter), with the type $new_type"
if [ "${NEW_GAME_NO_VERIFY:-}" = 1 ]; then
    exit 0
fi
# A first playthrough, recorded by the starter's bot, so verify has
# something to replay (and to compare with later).
if [ -f "examples/$name/bot.rs" ]; then
    echo "new_game: recording playthroughs/$name.json with its bot"
    cargo test -q --no-default-features --example "$name" -- --ignored record_playthrough >/dev/null 2>&1 ||
        echo "new_game: the bot couldn't record a playthrough (run: cargo test --no-default-features --example $name -- --ignored)"
fi
echo "new_game: checking it with tools/verify.sh $name"
exec tools/verify.sh "$name"
