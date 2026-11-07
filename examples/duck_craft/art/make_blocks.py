"""Writes blocks.json: Duck Craft's block atlas as a Duck Quill painting script.

Every tile is 16 x 16 pixels, painted with `pixels` rows ('.' is see-through),
from a fixed random seed, so the same script comes out every time.

    python3 art/make_blocks.py            # writes art/blocks.json
    quill export art/blocks.json --sheet --out assets

Run both in examples/duck_craft.
"""

import json
import math
import random
from pathlib import Path

N = 16
rng = random.Random(7)

PALETTE = {
    "grass": "#5BB043", "grass_dark": "#4A9638", "grass_light": "#74C658",
    "dirt": "#8A5A35", "dirt_dark": "#6E4426", "dirt_light": "#A06C42",
    "stone": "#8A8E96", "stone_dark": "#6E727A", "stone_light": "#A2A6AE",
    "mortar_dark": "#55585E",
    "sand": "#E6D69A", "sand_dark": "#D2BE7E", "sand_light": "#F2E6B4",
    "bark_dark": "#4A321C", "bark": "#6E4E2C", "ring": "#A67C4A", "ring_light": "#C29A62",
    "leaf": "#3E8A34", "leaf_dark": "#2E6A28", "leaf_light": "#56A846",
    "plank": "#B88A52", "plank_dark": "#94683A", "plank_light": "#CFA36A",
    "glass": "#CFEFFF", "shine": "#FFFFFF", "glass_dark": "#9CCBE0",
    "brick": "#A84A3A", "brick_dark": "#8A3A2E", "mortar": "#C8BEB0",
    "coal": "#222226", "gold": "#F2C230", "gold_light": "#FFF4B0",
    "water": "#3A70D0", "water_light": "#5A90E8", "water_dark": "#2E5CB0",
    "bedrock": "#333338", "bedrock_light": "#5A5A62",
    "crack": "#1E1E22",
    "cloud": "#FFFFFF", "cloud_shade": "#E6EEF6",
    "lamp": "#FFD45A", "lamp_light": "#FFF6C8", "lamp_frame": "#9A6A1C",
    "stem": "#3C7A2C", "petal_yellow": "#FFD43A", "petal_red": "#E0383A",
    "petal_white": "#F4F0E8", "apple": "#D8322C", "apple_dark": "#A01E1E",
    "seed": "#D8B868", "seed_dark": "#9A7838", "crust": "#B86E2C",
    "crumb": "#F0D490", "gold_dark": "#B88A10",
}

# One letter per palette colour, for the pixel rows.
LETTERS = {}
for name, letter in zip(PALETTE, "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789#$%&*+"):
    LETTERS[name] = letter


def blank(fill="."):
    return [[fill] * N for _ in range(N)]


def noise(base, specks):
    """`base` everywhere, then each (colour, chance) sprinkled over it."""
    grid = blank(base)
    for y in range(N):
        for x in range(N):
            for colour, chance in specks:
                if rng.random() < chance:
                    grid[y][x] = colour
    return grid


def grass_top():
    return noise("grass", [("grass_dark", 0.28), ("grass_light", 0.12)])


def dirt():
    return noise("dirt", [("dirt_dark", 0.25), ("dirt_light", 0.1)])


def grass_side():
    grid = dirt()
    for x in range(N):
        depth = 3 + (rng.random() < 0.45) + (rng.random() < 0.2)
        for y in range(depth):
            grid[y][x] = "grass_dark" if rng.random() < 0.3 else "grass"
    return grid


def stone():
    grid = noise("stone", [("stone_dark", 0.12), ("stone_light", 0.12)])
    for _ in range(4):  # a few short dark streaks
        x, y = rng.randrange(N), rng.randrange(N)
        for i in range(rng.randrange(2, 5)):
            grid[y][(x + i) % N] = "stone_dark"
    return grid


def cobblestone():
    """Rounded stones with dark mortar between them (a wrapping Voronoi)."""
    seeds = [(rng.uniform(0, N), rng.uniform(0, N)) for _ in range(9)]
    shade = [rng.choice(["stone", "stone_light", "stone"]) for _ in seeds]

    def nearest(x, y):
        best = []
        for i, (sx, sy) in enumerate(seeds):
            dx = min(abs(x - sx), N - abs(x - sx))
            dy = min(abs(y - sy), N - abs(y - sy))
            best.append((dx * dx + dy * dy, i))
        best.sort()
        return best[0][1], math.sqrt(best[1][0]) - math.sqrt(best[0][0])

    grid = blank()
    for y in range(N):
        for x in range(N):
            cell, gap = nearest(x + 0.5, y + 0.5)
            if gap < 1.0:
                grid[y][x] = "mortar_dark"
            elif gap < 1.8:
                grid[y][x] = "stone_dark"
            else:
                grid[y][x] = shade[cell] if rng.random() > 0.1 else "stone_light"
    return grid


def sand():
    return noise("sand", [("sand_dark", 0.2), ("sand_light", 0.15)])


def log_side():
    grid = blank()
    stripes = [rng.choice(["bark", "bark", "bark_dark"]) for _ in range(N)]
    for y in range(N):
        for x in range(N):
            grid[y][x] = stripes[x]
            if rng.random() < 0.08:
                grid[y][x] = "bark_dark"
    return grid


def log_top():
    grid = blank()
    for y in range(N):
        for x in range(N):
            d = max(abs(x - 7.5), abs(y - 7.5))
            if d > 6.6:
                grid[y][x] = "bark"
            elif int(d) % 2 == 0:
                grid[y][x] = "ring_light"
            else:
                grid[y][x] = "ring"
    return grid


def leaves():
    grid = noise("leaf", [("leaf_dark", 0.3), ("leaf_light", 0.15)])
    for y in range(N):
        for x in range(N):
            if rng.random() < 0.16:
                grid[y][x] = "."
    return grid


def planks():
    grid = noise("plank", [("plank_light", 0.12)])
    for board in range(4):
        top = board * 4
        seam = (5 if board % 2 == 0 else 12)
        for x in range(N):
            grid[top + 3][x] = "plank_dark"
            if rng.random() < 0.15:
                grid[top + 1][x] = "plank_dark"  # grain
        for y in range(top, top + 3):
            grid[y][seam] = "plank_dark"
    return grid


def glass():
    grid = blank()
    for i in range(N):
        for (x, y) in [(i, 0), (i, N - 1), (0, i), (N - 1, i)]:
            grid[y][x] = "glass_dark" if (x + y) % 5 == 0 else "glass"
    for i in range(3):  # two little shines
        grid[3 + i][4 - i] = "shine"
        grid[9 + i][12 - i] = "shine"
    grid[5][3] = "shine"
    return grid


def brick():
    grid = blank("brick")
    for y in range(N):
        row = y // 4
        for x in range(N):
            if y % 4 == 3 or (x + (4 if row % 2 else 0)) % 8 == 7:
                grid[y][x] = "mortar"
            elif rng.random() < 0.18:
                grid[y][x] = "brick_dark"
    return grid


def ore(colour, light):
    grid = stone()
    for _ in range(5):
        cx, cy = rng.randrange(2, 14), rng.randrange(2, 14)
        for dx, dy in [(0, 0), (1, 0), (0, 1), (1, 1), (-1, 0), (0, -1), (-1, 1)]:
            if rng.random() < 0.85:
                grid[cy + dy][cx + dx] = colour
        grid[cy][cx] = light
    return grid


def water():
    grid = blank("water")
    for y in range(N):
        for x in range(N):
            wave = math.sin((x + y * 0.6) * 0.8) + math.sin(y * 1.3 - x * 0.4)
            if wave > 1.3:
                grid[y][x] = "water_light"
            elif wave < -1.2:
                grid[y][x] = "water_dark"
    return grid


def bedrock():
    return noise("bedrock", [("bedrock_light", 0.35), ("coal", 0.15)])


def lamp():
    """A glowing lantern block: a dark frame round warm, bright glass."""
    grid = blank("lamp")
    for y in range(N):
        for x in range(N):
            edge = x in (0, N - 1) or y in (0, N - 1)
            bar = x in (5, 10) or y in (5, 10)
            if edge or bar:
                grid[y][x] = "lamp_frame"
            elif max(abs(x - 7.5), abs(y - 7.5)) < 2 or rng.random() < 0.15:
                grid[y][x] = "lamp_light"
    return grid


def picture(rows, legend):
    """A tile drawn by hand: one character per pixel, `legend` names each
    character's colour ('.' is see-through)."""
    assert len(rows) == N and all(len(r) == N for r in rows), rows
    return [["." if ch == "." else legend[ch] for ch in row] for row in rows]


def tall_grass():
    """Blades of grass, for a plant drawn as two crossed pictures."""
    grid = blank()
    for blade in range(9):
        x = 1 + blade * 14 / 8
        height = rng.randrange(7, 15)
        lean = rng.uniform(-0.25, 0.25)
        colour = rng.choice(["grass", "grass_dark", "grass_light", "grass"])
        for i in range(height):
            px = int(x + lean * i) % N
            grid[N - 1 - i][px] = colour
    return grid


def flower(petal, middle):
    rows = [
        "................",
        "................",
        "................",
        "......pp........",
        ".....pmmp.......",
        "....pmmmmp......",
        ".....pmmp.......",
        "......pp........",
        ".......s........",
        ".......s..ll....",
        ".......s.lll....",
        "...ll..sll......",
        "...llls.........",
        ".....ls.........",
        ".......s........",
        ".......s........",
    ]
    return picture(rows, {"p": petal, "m": middle, "s": "stem", "l": "leaf_light"})


def apple():
    return picture([
        "................",
        "........s.......",
        ".......s..ll....",
        ".......sll......",
        "....aaasaaa.....",
        "...aaaaaaaaa....",
        "..aawaaaaaaaa...",
        "..awwaaaaaaad...",
        "..aaaaaaaaaad...",
        "..aaaaaaaaaad...",
        "..aaaaaaaaadd...",
        "...aaaaaaaadd...",
        "...daaaaaaddd...",
        "....ddaddddd....",
        ".....dd..dd.....",
        "................",
    ], {"a": "apple", "d": "apple_dark", "w": "shine", "s": "bark", "l": "leaf_light"})


def seeds():
    return picture([
        "................",
        "................",
        "................",
        ".........sd.....",
        "....sd....dd....",
        "....dd..........",
        "........sd......",
        "..sd.....d...sd.",
        "...d.........dd.",
        "......sd........",
        "......dd...sd...",
        "...sd.......d...",
        "....d..sd.......",
        ".........d......",
        "................",
        "................",
    ], {"s": "seed", "d": "seed_dark"})


def bread():
    return picture([
        "................",
        "................",
        "................",
        "................",
        ".....cccccc.....",
        "...ccmcccmccc...",
        "..ccmcccmcccmc..",
        ".cccccccccccccc.",
        ".cccccccccccccc.",
        ".kccccccccccccck",
        ".kkccccccccccckk",
        "..kkkkkkkkkkkk..",
        "................",
        "................",
        "................",
        "................",
    ], {"c": "crust", "m": "crumb", "k": "bark"})


def gold_nugget():
    return picture([
        "................",
        "................",
        "................",
        "................",
        "......gggg......",
        "....ggwwgggg....",
        "...gwwgggggdg...",
        "...ggggggggddg..",
        "..gggggggggddd..",
        "..dggggggggddd..",
        "...ddggggdddd...",
        "....dddddddd....",
        "................",
        "................",
        "................",
        "................",
    ], {"g": "gold", "w": "gold_light", "d": "gold_dark"})


def coal_lump():
    return picture([
        "................",
        "................",
        "................",
        "................",
        ".......kk.......",
        ".....kkkkkk.....",
        "....kkskkkkk....",
        "...kksskkkkkk...",
        "...kkkkkkkkskk..",
        "..kkkkkkkkkkkk..",
        "...kkkkkskkkk...",
        "....kkkkkkkk....",
        "......kkkk......",
        "................",
        "................",
        "................",
    ], {"k": "coal", "s": "stone_dark"})


def stone_blade():
    """A rough blade of knapped stone, bound to a stick with grass."""
    return picture([
        "................",
        "............sl..",
        "...........sls..",
        "..........slsd..",
        ".........slsd...",
        "........slsd....",
        ".......slsd.....",
        "......slsd......",
        ".....slsd.......",
        "..gg.lsd........",
        "...gggd.........",
        "...bgg..........",
        "..b..g..........",
        ".b..............",
        "b...............",
        "................",
    ], {"s": "stone", "l": "stone_light", "d": "stone_dark", "g": "grass_dark", "b": "bark"})


def cloud():
    return noise("cloud", [("cloud_shade", 0.1)])


# Cracks for breaking: each stage keeps the last one's lines and adds more.
crack_paths = []


def grow_cracks(count):
    for _ in range(count):
        x, y = rng.uniform(5, 11), rng.uniform(5, 11)
        angle = rng.uniform(0, math.tau)
        path = []
        for _ in range(rng.randrange(4, 9)):
            path.append((int(x) % N, int(y) % N))
            angle += rng.uniform(-0.7, 0.7)
            x += math.cos(angle)
            y += math.sin(angle)
        crack_paths.append(path)


def crack_stage():
    grid = blank()
    for path in crack_paths:
        for x, y in path:
            grid[y][x] = "crack"
    return grid


def cracks():
    stages = []
    for count in [2, 3, 4, 5]:
        grow_cracks(count)
        stages.append(crack_stage())
    return stages


TILES = [
    ("grass_top", grass_top()),
    ("grass_side", grass_side()),
    ("dirt", dirt()),
    ("stone", stone()),
    ("cobblestone", cobblestone()),
    ("sand", sand()),
    ("log_side", log_side()),
    ("log_top", log_top()),
    ("leaves", leaves()),
    ("planks", planks()),
    ("glass", glass()),
    ("brick", brick()),
    ("coal_ore", ore("coal", "stone_dark")),
    ("gold_ore", ore("gold", "gold_light")),
    ("water", water()),
    ("bedrock", bedrock()),
    ("cloud", cloud()),
    ("lamp", lamp()),
    ("tall_grass", tall_grass()),
    ("flower_yellow", flower("petal_yellow", "crust")),
    ("flower_red", flower("petal_red", "petal_yellow")),
    ("flower_white", flower("petal_white", "petal_yellow")),
    ("apple", apple()),
    ("seeds", seeds()),
    ("bread", bread()),
    ("gold_nugget", gold_nugget()),
    ("coal_lump", coal_lump()),
    ("stone_blade", stone_blade()),
]
for i, stage in enumerate(cracks()):
    TILES.append((f"crack_{i + 1}", stage))


def rows(grid):
    return ["".join("." if c == "." else LETTERS[c] for c in row) for row in grid]


script = {
    "title": "Duck Craft's block atlas: one 16 x 16 tile per animation, for Mesh::block",
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
        for name, grid in TILES
    ],
    # One one-frame animation per tile: `quill export --sheet` makes each a
    # row of the atlas, and the game's Tileset finds it by this name.
    "animations": {
        name: {"frames": 1, "fps": 1, "commands": [{"cmd": "use", "name": name}]}
        for name, _ in TILES
    },
}

out = Path(__file__).with_name("blocks.json")
out.write_text(json.dumps(script, indent=1) + "\n")
print(f"wrote {out} ({len(TILES)} tiles)")
