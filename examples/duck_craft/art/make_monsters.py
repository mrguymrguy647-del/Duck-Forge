"""Writes hollow.json and watcher.json: the night's two shapes, as Duck
Quill painting scripts. Both use the duck's skin layout (see
make_skin.py), so they wear the same blocky body as the duck: they are
what's left of ducks.

    python3 art/make_monsters.py
    quill render art/hollow.json --out assets       # assets/hollow.png
    quill render art/watcher.json --out assets      # assets/watcher.png

A Hollow: ash-grey, eyes gone to black holes, the beak broken off short,
the suit rotted through in places, the tie gone.
The Watcher: no colour at all, two white points where eyes should be.
"""

import json
import random
from pathlib import Path

W, H = 64, 32
PARTS = {
    "head": (8, 8, 8, 0, 0),
    "beak": (4, 2, 3, 32, 0),
    "body": (8, 10, 4, 0, 16),
    "arm": (4, 10, 4, 24, 16),
    "leg": (4, 10, 4, 40, 16),
}


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


class Skin:
    def __init__(self, palette):
        self.palette = palette
        self.grid = [["."] * W for _ in range(H)]

    def fill(self, x, y, w, h, colour):
        for yy in range(y, y + h):
            for xx in range(x, x + w):
                self.grid[yy][xx] = colour

    def paint_all(self, part, colour):
        for rect in faces(part).values():
            self.fill(*rect, colour)

    def mottle(self, part, colours, chance, rng):
        for x0, y0, w, h in faces(part).values():
            for y in range(y0, y0 + h):
                for x in range(x0, x0 + w):
                    if rng.random() < chance:
                        self.grid[y][x] = rng.choice(colours)

    def script(self, title):
        letters = {name: l for name, l in zip(self.palette, "abcdefghijklmnopqrstuvwxyz")}
        return {
            "title": title,
            "palette": self.palette,
            "commands": [
                {"cmd": "canvas", "width": W, "height": H, "pixel": True},
                {
                    "cmd": "pixels",
                    "at": [0, 0],
                    "colors": {l: name for name, l in letters.items()},
                    "rows": ["".join("." if c == "." else letters[c] for c in row) for row in self.grid],
                },
            ],
        }


def hollow():
    rng = random.Random(13)
    s = Skin({
        "ash": "#8E8B86", "ash_dark": "#6C6964", "ash_light": "#A8A5A0",
        "void": "#050506", "rot": "#2E2F35", "rot_dark": "#1C1D22",
        "bone": "#C9C3B6", "stain": "#4A3F3A",
    })
    # Head: ash, mottled; black sockets running down like tears.
    s.paint_all("head", "ash")
    s.mottle("head", ["ash_dark", "ash_light"], 0.2, rng)
    x, y, _, _ = faces("head")["front"]
    for ex in (1, 5):
        s.fill(x + ex, y + 2, 2, 2, "void")
        s.grid[y + 4][x + ex] = "void"  # running down
        s.grid[y + 5][x + ex + rng.randrange(2)] = "ash_dark"
    s.fill(x + 3, y + 6, 2, 1, "ash_dark")  # a mouth that isn't one
    for side in ("left", "right"):
        sx, sy, _, _ = faces("head")[side]
        s.fill(sx + (1 if side == "left" else 6), sy + 2, 1, 2, "void")
    # Beak: snapped off, the stump grey and cracked.
    s.paint_all("beak", "ash_dark")
    bx, by, bw, bh = faces("beak")["front"]
    s.fill(bx, by, bw, bh, "bone")
    s.grid[by][bx + 1] = "void"
    s.grid[by + 1][bx + 2] = "void"
    # Body: the suit gone dark and rotten, holes showing grey under it.
    s.paint_all("body", "rot")
    s.mottle("body", ["rot_dark", "ash_dark", "stain"], 0.18, rng)
    fx, fy, fw, fh = faces("body")["front"]
    for row in range(5):  # where the shirt was: grey, no tie
        width = max(1, 4 - row)
        half = (width + 1) // 2
        s.fill(fx + 4 - half, fy + row, half * 2, 1, "ash_dark")
    for _ in range(5):  # holes
        s.grid[fy + rng.randrange(fh)][fx + rng.randrange(fw)] = "ash"
    # Arms: rotted sleeves, bare grey hands, a little longer looking.
    s.paint_all("arm", "rot")
    s.mottle("arm", ["rot_dark", "stain"], 0.15, rng)
    for name, (ax, ay, aw, ah) in faces("arm").items():
        if name not in ("top", "bottom"):
            s.fill(ax, ay + 6, aw, 4, "ash")
            s.grid[ay + 9][ax + rng.randrange(aw)] = "bone"  # a knuckle
    s.fill(*faces("arm")["bottom"], "ash_dark")
    # Legs: dark, the feet grey and splayed.
    s.paint_all("leg", "rot_dark")
    s.mottle("leg", ["rot", "stain"], 0.15, rng)
    for name, (lx, ly, lw, lh) in faces("leg").items():
        if name not in ("top", "bottom"):
            s.fill(lx, ly + 8, lw, 2, "ash_dark")
    s.fill(*faces("leg")["bottom"], "ash_dark")
    return s.script("A Hollow: what's left of a duck. The duck's skin layout.")


def watcher():
    s = Skin({"black": "#060608", "black_edge": "#121218", "eye": "#F4F4FF", "eye_glow": "#8890B0"})
    for part in PARTS:
        s.paint_all(part, "black")
    # Edges a hair lighter, so the shape reads against the night sky.
    for part in PARTS:
        for x0, y0, w, h in faces(part).values():
            for x in range(x0, x0 + w):
                s.grid[y0][x] = "black_edge"
    x, y, _, _ = faces("head")["front"]
    for ex in (2, 5):
        s.grid[y + 3][x + ex] = "eye"
        s.grid[y + 4][x + ex] = "eye_glow"
    return s.script("The Watcher: no colour, two points of light. The duck's skin layout.")


for name, make in [("hollow", hollow), ("watcher", watcher)]:
    out = Path(__file__).with_name(f"{name}.json")
    out.write_text(json.dumps(make(), indent=1) + "\n")
    print(f"wrote {out}")
