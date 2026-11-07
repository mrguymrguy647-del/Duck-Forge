"""Writes hud.json: the little 9 x 9 icons over the hotbar, as a Duck Quill
script: hearts for health, slices of bread for hunger (ducks love bread),
and bubbles for air under water. Full, half and empty of each.

    python3 art/make_hud.py
    quill export art/hud.json --sheet --out assets     # assets/hud.png, hud.json
"""

import json
from pathlib import Path

N = 9
PALETTE = {
    "outline": "#1A1418",
    "red": "#E02A2A",
    "red_dark": "#A01818",
    "shine": "#FFFFFF",
    "empty": "#4A3E44",
    "crust": "#B86E2C",
    "crust_dark": "#7A4418",
    "crumb": "#F0D490",
    "crumb_dark": "#C8A860",
    "bubble": "#8ED8FF",
    "bubble_dark": "#3A8AC8",
}
LETTERS = {name: letter for name, letter in zip(PALETTE, "abcdefghijklmnopqrstuvwxyz")}

HEART = [
    ".oo...oo.",
    "orro.orro",
    "orwrrrrro",
    "orrrrrrro",
    "orrrrrrdo",
    ".orrrrdo.",
    "..orrdo..",
    "...odo...",
    "....o....",
]
BREAD = [
    "..ooooo..",
    ".occccco.",
    "occmmmcco",
    "ocmmmmmco",
    "ocmmmmmco",
    "ocmmmmnco",
    "ocmmnnnco",
    "occcccccd",
    ".odddddo.",
]
BUBBLE = [
    "..ooooo..",
    ".obbbbbo.",
    "obwwbbbbo",
    "obwbbbbbo",
    "obbbbbbbo",
    "obbbbbbdo",
    "obbbbbddo",
    ".obbddbo.",
    "..ooooo..",
]


def draw(shape, legend, part):
    """`part`: 'full', 'half' (the left half filled) or 'empty' (outline
    only, the inside dark)."""
    grid = []
    for y, row in enumerate(shape):
        line = []
        for x, ch in enumerate(row):
            if ch == ".":
                line.append(".")
            elif ch == "o":
                line.append("outline")
            elif part == "empty" or (part == "half" and x > 4):
                line.append("empty")
            else:
                line.append(legend[ch])
        grid.append(line)
    return grid


ICONS = []
heart = {"r": "red", "w": "shine", "d": "red_dark"}
bread = {"c": "crust", "m": "crumb", "n": "crumb_dark", "d": "crust_dark"}
bubble = {"b": "bubble", "w": "shine", "d": "bubble_dark"}
for part in ("full", "half", "empty"):
    ICONS.append((f"heart_{part}", draw(HEART, heart, part)))
    ICONS.append((f"bread_{part}", draw(BREAD, bread, part)))
ICONS.append(("bubble", draw(BUBBLE, bubble, "full")))


def rows(grid):
    return ["".join("." if c == "." else LETTERS[c] for c in row) for row in grid]


script = {
    "title": "Duck Craft's HUD icons: hearts, bread and bubbles, 9 x 9",
    "palette": PALETTE,
    "commands": [{"cmd": "canvas", "width": N, "height": N, "pixel": True}]
    + [
        {
            "cmd": "define",
            "name": name,
            "commands": [
                {
                    "cmd": "pixels",
                    "at": [0, 0],
                    "colors": {letter: name for name, letter in LETTERS.items()},
                    "rows": rows(grid),
                }
            ],
        }
        for name, grid in ICONS
    ],
    "animations": {
        name: {"frames": 1, "fps": 1, "commands": [{"cmd": "use", "name": name}]}
        for name, _ in ICONS
    },
}
out = Path(__file__).with_name("hud.json")
out.write_text(json.dumps(script, indent=1) + "\n")
print(f"wrote {out} ({len(ICONS)} icons)")
