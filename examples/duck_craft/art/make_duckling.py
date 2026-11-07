"""Writes duckling.json: the ducklings' skin as a Duck Quill painting script.

The same layout as the duck's skin (see make_skin.py): each part's six
faces unwrapped onto one 64 x 32 picture. `Duckling` parts in
ducklings.rs use the same numbers. 1 pixel = 1/16 of a block, so a
duckling is about 10 pixels (0.6 blocks) tall.

    python3 art/make_duckling.py
    quill render art/duckling.json --out assets      # assets/duckling.png

      u    u+d    u+d+w   u+2d+w
    v      [ top ][bottom]
    v+d [+x][front][ -x ][ back ]
"""

import json
from pathlib import Path

W, H = 64, 32
PALETTE = {
    "fluff": "#FFE04A",
    "fluff_light": "#FFF2A0",
    "fluff_dark": "#F8CC34",
    "wing": "#F4BC28",
    "orange": "#F08020",
    "orange_dark": "#C86010",
    "ink": "#201C18",
    "white": "#FFFFFF",
}
LETTERS = {name: letter for name, letter in zip(PALETTE, "abcdefghijklmnopqrstuvwxyz")}
grid = [["."] * W for _ in range(H)]

# name -> (w, h, d, u, v); ducklings.rs has the same numbers.
PARTS = {
    "body": (6, 5, 8, 0, 0),
    "head": (4, 4, 4, 28, 0),
    "beak": (3, 1, 2, 44, 0),
    "tail": (2, 2, 1, 56, 0),
    "leg": (1, 2, 1, 28, 8),
}


def fill(x, y, w, h, colour):
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            grid[yy][xx] = colour


def faces(part):
    w, h, d, u, v = PARTS[part]
    return {
        "top": (u + d, v, w, d),
        "bottom": (u + d + w, v, w, d),
        "right": (u, v + d, d, h),
        "front": (u + d, v + d, w, h),
        "left": (u + d + w, v + d, d, h),
        "back": (u + 2 * d + w, v + d, w, h),
    }


def paint_all(part, colour):
    for rect in faces(part).values():
        fill(*rect, colour)


def speckle(rect, colour, every):
    """Fluff: every `every`th pixel in a diagonal pattern."""
    x, y, w, h = rect
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            if (xx * 3 + yy * 5) % every == 0:
                grid[yy][xx] = colour


# Body: fluffy yellow, lighter on top, darker underneath, wings on the sides.
paint_all("body", "fluff")
for name, rect in faces("body").items():
    speckle(rect, "fluff_light" if name == "top" else "fluff_dark", 7)
fill(*faces("body")["top"], "fluff_light")
speckle(faces("body")["top"], "fluff", 5)
fill(*faces("body")["bottom"], "fluff_dark")
for side in ("left", "right"):
    x, y, w, h = faces("body")[side]
    # A wing: a darker patch from the middle back, the tip pointing back.
    wing_x = x + (1 if side == "left" else 2)
    fill(wing_x, y + 1, 5, 3, "wing")
    tip = x + (6 if side == "left" else 1)
    grid[y + 2][tip] = "wing"
    fill(wing_x, y + 3, 5, 1, "fluff_dark")

# Head: yellow, black eyes on both sides and a little on the front corners.
paint_all("head", "fluff")
fill(*faces("head")["top"], "fluff_light")
for side in ("left", "right"):
    x, y, w, h = faces("head")[side]
    eye = x + (2 if side == "left" else 1)
    fill(eye, y + 1, 1, 1, "ink")
    grid[y + 1][eye + (1 if side == "left" else -1)] = "white"
fx, fy, fw, fh = faces("head")["front"]
grid[fy + 1][fx] = "ink"
grid[fy + 1][fx + fw - 1] = "ink"
speckle(faces("head")["back"], "fluff_dark", 4)

# Beak and legs: orange.
paint_all("beak", "orange")
fill(*faces("beak")["bottom"], "orange_dark")
paint_all("leg", "orange")
fill(*faces("leg")["bottom"], "orange_dark")

# Tail: a little tuft, darker.
paint_all("tail", "fluff_dark")
fill(*faces("tail")["top"], "fluff")

script = {
    "title": "Duck Craft's duckling skin: each part's six faces unwrapped, like a Minecraft mob skin",
    "palette": PALETTE,
    "commands": [
        {"cmd": "canvas", "width": W, "height": H, "pixel": True},
        {
            "cmd": "pixels",
            "at": [0, 0],
            "colors": {letter: name for name, letter in LETTERS.items()},
            "rows": ["".join("." if c == "." else LETTERS[c] for c in row) for row in grid],
        },
    ],
}
out = Path(__file__).with_name("duckling.json")
out.write_text(json.dumps(script, indent=1) + "\n")
print(f"wrote {out}")
