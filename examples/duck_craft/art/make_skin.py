"""Writes skin.json: the duck's skin as a Duck Quill painting script.

Like a Minecraft skin: one 64 x 32 picture, each body part's six faces
unwrapped onto it. The game (`body_parts` in main.rs) builds the same boxes
and reads each face from the same place, so this layout and `main.rs` must
agree. 1 pixel = 1/16 of a block.

    python3 art/make_skin.py                    # writes art/skin.json
    quill render art/skin.json --out assets      # makes assets/skin.png

A part is a box w x h x d (wide, tall, deep) unwrapped at (u, v):

      u    u+d    u+d+w   u+2d+w
    v      [ top ][bottom]
    v+d [+x][front][ -x ][ back ]

The duck: a rubber-duck head (yellow, black eyes, orange beak) on a
muscular body in a navy office suit, white shirt and red tie, yellow
hands, orange feet.
"""

import json
from pathlib import Path

W, H = 64, 32
PALETTE = {
    "yellow": "#F2C230",
    "yellow_dark": "#D9A21E",
    "yellow_light": "#FFE27A",
    "orange": "#F08020",
    "orange_dark": "#C86010",
    "ink": "#201C18",
    "white": "#FFFFFF",
    "navy": "#2A3450",
    "navy_dark": "#1C2438",
    "navy_light": "#3A4868",
    "red": "#C83030",
    "red_dark": "#982020",
    "shirt": "#F4F4F8",
    "shirt_shade": "#D8D8E0",
}
LETTERS = {name: letter for name, letter in zip(PALETTE, "abcdefghijklmnopqrstuvwxyz")}

grid = [["."] * W for _ in range(H)]


def fill(x, y, w, h, colour):
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            grid[yy][xx] = colour


# The parts: name -> (w, h, d, u, v). main.rs has the same numbers.
PARTS = {
    "head": (8, 8, 8, 0, 0),
    "beak": (4, 2, 3, 32, 0),
    "body": (8, 10, 4, 0, 16),
    "arm": (4, 10, 4, 24, 16),
    "leg": (4, 10, 4, 40, 16),
}


def faces(part):
    """Each face's rectangle (x, y, w, h) on the picture."""
    w, h, d, u, v = PARTS[part]
    return {
        "top": (u + d, v, w, d),
        "bottom": (u + d + w, v, w, d),
        "right": (u, v + d, d, h),  # +x
        "front": (u + d, v + d, w, h),  # +z, the way the duck faces
        "left": (u + d + w, v + d, d, h),  # -x
        "back": (u + 2 * d + w, v + d, w, h),  # -z
    }


def paint_all(part, colour):
    for rect in faces(part).values():
        fill(*rect, colour)


# Head: smooth yellow, a lighter top, a darker chin.
paint_all("head", "yellow")
fill(*faces("head")["top"], "yellow_light")
f = faces("head")["front"]
fill(f[0], f[1] + 7, 8, 1, "yellow_dark")  # chin
for side in ("left", "right", "back"):
    r = faces("head")[side]
    fill(r[0], r[1] + 7, r[2], 1, "yellow_dark")
# Eyes: black with a white glint, 2 wide, on the front.
for ex in (1, 5):
    fill(f[0] + ex, f[1] + 2, 2, 2, "ink")
    grid[f[1] + 2][f[0] + ex] = "white"
# A hint of eyes on the sides (so the head reads from the side too).
for side, ex in (("left", 1), ("right", 6)):
    r = faces("head")[side]
    fill(r[0] + ex, r[1] + 2, 1, 2, "ink")

# Beak: orange, a mouth line on the front, darker underneath.
paint_all("beak", "orange")
b = faces("beak")
fill(*b["bottom"], "orange_dark")
fill(b["front"][0], b["front"][1] + 1, 4, 1, "orange_dark")
grid[b["top"][1] + 1][b["top"][0] + 1] = "orange_dark"  # nostrils
grid[b["top"][1] + 1][b["top"][0] + 2] = "orange_dark"

# Body: a navy suit, white shirt in a V, red tie, buttons.
paint_all("body", "navy")
f = faces("body")["front"]
x0, y0 = f[0], f[1]
# The shirt: a V from the shoulders, 2 wide at the top, down to row 5.
for row in range(5):
    width = max(1, 4 - row)  # 4, 3, 2, 1, 1 each side of the middle
    half = (width + 1) // 2
    fill(x0 + 4 - half, y0 + row, half * 2, 1, "shirt")
fill(x0 + 3, y0 + 1, 2, 1, "shirt_shade")  # collar shadow
# The tie: red down the middle.
fill(x0 + 3, y0 + 1, 2, 7, "red")
fill(x0 + 3, y0 + 1, 1, 7, "red_dark")
grid[y0][x0 + 3] = "red_dark"
grid[y0][x0 + 4] = "red_dark"
# Lapels: lighter navy edges of the V.
for row in range(5):
    width = max(1, 4 - row)
    half = (width + 1) // 2
    grid[y0 + row][x0 + 4 - half - 1] = "navy_light"
    grid[y0 + row][x0 + 4 + half] = "navy_light"
# Buttons under the tie.
grid[y0 + 8][x0 + 4] = "yellow_dark"
grid[y0 + 9][x0 + 4] = "yellow_dark"
# A seam down the back, shoulders lighter on top.
bk = faces("body")["back"]
fill(bk[0] + 4, bk[1], 1, 10, "navy_dark")
fill(*faces("body")["top"], "navy_light")
fill(*faces("body")["bottom"], "navy_dark")

# Arms: navy sleeves, a white cuff, yellow hands (the bottom 2 px).
paint_all("arm", "navy")
for name, r in faces("arm").items():
    if name in ("top", "bottom"):
        continue
    fill(r[0], r[1] + 7, r[2], 1, "shirt")  # cuff
    fill(r[0], r[1] + 8, r[2], 2, "yellow")  # hand
fill(*faces("arm")["bottom"], "yellow")
fill(*faces("arm")["top"], "navy_light")
# A muscle fold on the sleeve.
r = faces("arm")["front"]
fill(r[0] + 1, r[1] + 2, 2, 1, "navy_light")

# Legs: navy trousers with a crease, orange feet (the bottom 2 px).
paint_all("leg", "navy")
for name, r in faces("leg").items():
    if name in ("top", "bottom"):
        continue
    fill(r[0], r[1] + 8, r[2], 2, "orange")
fill(*faces("leg")["bottom"], "orange_dark")
fill(*faces("leg")["top"], "navy_dark")
r = faces("leg")["front"]
fill(r[0] + 1, r[1], 1, 8, "navy_dark")  # crease
fill(r[0] + 1, r[1] + 8, 2, 1, "orange_dark")  # the foot's edge

script = {
    "title": "Duck Craft's duck skin: each body part's six faces unwrapped, like a Minecraft skin",
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
out = Path(__file__).with_name("skin.json")
out.write_text(json.dumps(script, indent=1) + "\n")
print(f"wrote {out}")
