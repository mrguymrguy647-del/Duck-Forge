"""Makes the soundtrack for the door_short video from its cues.json.

Every sound is synthesized from simple waves and noise, so there are no
audio files to download. Usage:

    python3 tools/door_sounds.py target/door_frames/cues.json out.wav
"""

import json
import sys

import numpy as np
from scipy.io import wavfile
from scipy.signal import butter, sosfilt

SR = 44100
rng = np.random.default_rng(7)


def t_axis(seconds):
    return np.arange(int(seconds * SR)) / SR


def note(name):
    """'C5' -> frequency in Hz."""
    steps = {"C": -9, "D": -7, "E": -5, "F": -4, "G": -2, "A": 0, "B": 2}
    n = steps[name[0]]
    rest = name[1:]
    if rest.startswith("#"):
        n += 1
        rest = rest[1:]
    if rest.startswith("b"):
        n -= 1
        rest = rest[1:]
    return 440.0 * 2 ** ((n + 12 * (int(rest) - 4)) / 12)


def decay(n, seconds):
    return np.exp(-np.arange(n) / (seconds * SR))


def attack(sig, seconds=0.005):
    n = min(len(sig), int(seconds * SR))
    sig[:n] *= np.linspace(0, 1, n)
    return sig


def fade_out(sig, seconds):
    n = min(len(sig), int(seconds * SR))
    sig[-n:] *= np.linspace(1, 0, n)
    return sig


def sine(freq, seconds):
    return np.sin(2 * np.pi * freq * t_axis(seconds))


def sweep(f0, f1, seconds, shape=np.sin):
    t = t_axis(seconds)
    freq = np.geomspace(f0, f1, len(t))
    phase = 2 * np.pi * np.cumsum(freq) / SR
    return shape(phase)


def square(phase):
    return np.sign(np.sin(phase)) * 0.6


def triangle(phase):
    return 2 / np.pi * np.arcsin(np.sin(phase))


def noise(seconds):
    return rng.uniform(-1, 1, int(seconds * SR))


def band(sig, low, high, order=2):
    sos = butter(order, [low, high], btype="band", fs=SR, output="sos")
    return sosfilt(sos, sig)


def lowpass(sig, cutoff, order=2):
    return sosfilt(butter(order, cutoff, btype="low", fs=SR, output="sos"), sig)


def highpass(sig, cutoff, order=2):
    return sosfilt(butter(order, cutoff, btype="high", fs=SR, output="sos"), sig)


def bell(freq, seconds, length=0.5):
    t = t_axis(seconds)
    s = sum(a * np.sin(2 * np.pi * freq * m * t) for m, a in [(1, 1), (2, 0.4), (3, 0.2), (4.2, 0.1)])
    return attack(s * np.exp(-t / length), 0.002)


def mix_at(out, sig, at):
    i = int(at * SR)
    j = min(len(out), i + len(sig))
    if j > i:
        out[i:j] += sig[: j - i]


# ---- The sound effects ----


def s_pop():
    return attack(sweep(380, 950, 0.09) * decay(int(0.09 * SR), 0.03)) * 0.6


def s_title():
    out = np.zeros(int(1.2 * SR))
    for k, n in enumerate(["C5", "E5", "G5", "C6"]):
        tone = sweep(note(n), note(n), 0.5, square) * decay(int(0.5 * SR), 0.18)
        mix_at(out, lowpass(attack(tone), 4000) * 0.35, k * 0.09)
    mix_at(out, bell(note("C7"), 1.0, 0.4) * 0.25, 0.36)
    return out


def s_whoosh():
    n = noise(0.5)
    t = t_axis(0.5)
    # A band of noise that sweeps up and down, swelling in the middle.
    pieces = [band(n, 300 + 1500 * k, 900 + 3000 * k) for k in (0.0, 0.5, 1.0)]
    shape = np.sin(np.pi * t / 0.5) ** 2
    mixed = pieces[0] * (1 - t / 0.5) + pieces[2] * (t / 0.5) + pieces[1] * 0.5
    return mixed * shape * 0.8


def s_tick(last=False):
    freq = 1250 if last else 1800
    length = 0.12 if last else 0.05
    tone = sine(freq, length) + (0.5 * sine(freq * 2, length) if last else 0)
    click = highpass(noise(length), 3000) * decay(int(length * SR), 0.004)
    return attack(tone * decay(len(tone), length / 3) * 0.45 + click * 0.3, 0.001) * (1.3 if last else 1.0)


def s_drumroll():
    out = np.zeros(int(0.85 * SR))
    at, gap = 0.0, 0.07
    while at < 0.75:
        hit = band(noise(0.06), 1500, 7000) * decay(int(0.06 * SR), 0.015)
        body = sine(190, 0.06) * decay(int(0.06 * SR), 0.02) * 0.5
        mix_at(out, (hit + body) * (0.15 + 0.35 * at / 0.75), at)
        at += gap
        gap = max(0.028, gap * 0.92)
    return out


def s_creak():
    t = t_axis(0.42)
    # A slow, wobbly saw wave chopped into little grains, like an old hinge.
    freq = 210 + 50 * np.sin(2 * np.pi * 3.0 * t) + 40 * t
    phase = 2 * np.pi * np.cumsum(freq) / SR
    saw = (phase / np.pi) % 2 - 1
    grains = (np.sin(2 * np.pi * (34 + 20 * t) * t) > 0.2).astype(float)
    sig = band(saw * grains, 400, 2500) * np.sin(np.pi * t / 0.42) ** 0.5
    return lowpass(sig, 3000) * 0.55


def s_win():
    out = np.zeros(int(1.2 * SR))
    for k, n in enumerate(["C6", "E6", "G6", "C7"]):
        mix_at(out, bell(note(n), 0.9, 0.35) * 0.28, k * 0.06)
    sparkle = highpass(noise(0.6), 6000) * decay(int(0.6 * SR), 0.15) * 0.12
    mix_at(out, sparkle, 0.05)
    return out


def s_ok():
    out = np.zeros(int(0.6 * SR))
    for k, n in enumerate(["G4", "C5"]):
        tone = sweep(note(n), note(n), 0.3, triangle) * decay(int(0.3 * SR), 0.1)
        mix_at(out, attack(tone) * 0.45, k * 0.12)
    return out


def s_bad():
    out = np.zeros(int(0.9 * SR))
    # "Wah-wah": two falling, buzzy notes, then a thump.
    for k, (f0, f1) in enumerate([(233, 220), (220, 165)]):
        length = 0.22 if k == 0 else 0.45
        buzz = sweep(f0, f1, length, square) + sweep(f0 * 1.01, f1 * 1.01, length, square)
        buzz = lowpass(buzz, 1400) * np.sin(np.pi * t_axis(length) / length) ** 0.4
        mix_at(out, buzz * 0.28, k * 0.24)
    thump = sweep(120, 45, 0.3) * decay(int(0.3 * SR), 0.08)
    mix_at(out, thump * 0.6, 0.0)
    return out


def brass(freq, seconds):
    t = t_axis(seconds)
    saw = sum(np.sin(2 * np.pi * freq * m * t) / m for m in range(1, 8))
    env = np.minimum(t / 0.03, 1) * np.exp(-t / (seconds * 0.8))
    return lowpass(saw * env, 3500) * 0.3


def s_sting():
    out = np.zeros(int(1.2 * SR))
    for k, chord in enumerate([["C5", "E5", "G5"], ["E5", "G5", "C6"]]):
        length = 0.16 if k == 0 else 0.7
        for n in chord:
            mix_at(out, brass(note(n), length) * 0.5, k * 0.17)
    mix_at(out, bell(note("G6"), 0.8, 0.3) * 0.15, 0.17)
    return out


def s_fanfare():
    out = np.zeros(int(2.4 * SR))
    melody = [("G4", 0.0, 0.14), ("C5", 0.15, 0.14), ("E5", 0.3, 0.14), ("G5", 0.45, 0.3), ("E5", 0.78, 0.14), ("G5", 0.93, 1.2)]
    for n, at, length in melody:
        mix_at(out, brass(note(n), length) * 0.7, at)
        mix_at(out, brass(note(n) / 2, length) * 0.35, at)
    for n in ["C6", "E6", "G6"]:
        mix_at(out, bell(note(n), 1.4, 0.6) * 0.12, 0.93)
    return out


# ---- Background music: a cheerful chiptune loop at 120 beats per minute ----


def music(seconds):
    beat = 0.5
    out = np.zeros(int(seconds * SR))
    chords = [["C3", "E4", "G4"], ["A2", "C4", "E4"], ["F2", "A3", "C4"], ["G2", "B3", "D4"]]
    bar = 0
    at = 0.0
    while at < seconds:
        root, *upper = chords[bar % 4]
        for b in range(4):
            start = at + b * beat
            # Bass: root on the beat, octave on the off-beat.
            for half, mult in [(0, 1), (0.5, 2)]:
                tone = sweep(note(root) * mult, note(root) * mult, beat / 2, triangle)
                mix_at(out, attack(tone * decay(len(tone), 0.12)) * 0.5, start + half * beat)
            # Hi-hat on the off-beat.
            hat = highpass(noise(0.04), 7000) * decay(int(0.04 * SR), 0.01)
            mix_at(out, hat * 0.25, start + beat / 2)
            # Kick on beats 1 and 3, snare-ish on 2 and 4.
            if b % 2 == 0:
                kick = sweep(150, 50, 0.15) * decay(int(0.15 * SR), 0.05)
                mix_at(out, kick * 0.7, start)
            else:
                snare = band(noise(0.1), 1000, 6000) * decay(int(0.1 * SR), 0.03)
                mix_at(out, snare * 0.25, start)
        # A little arpeggio over the chord, every eighth note.
        for k in range(8):
            n = upper[k % len(upper)] if k % 4 != 3 else upper[0]
            freq = note(n) * 2
            tone = sweep(freq, freq, 0.2, square) * decay(int(0.2 * SR), 0.06)
            mix_at(out, lowpass(attack(tone), 3000) * 0.12, at + k * beat / 2)
        at += 4 * beat
        bar += 1
    return fade_out(out, 1.5)


def s_coins():
    """A shower of coins: lots of short metallic clinks, thinning out as they settle."""
    out = np.zeros(int(1.8 * SR))
    at = 0.0
    while at < 1.6:
        freq = rng.uniform(2600, 5200)
        length = 0.12
        t = t_axis(length)
        clink = sum(np.sin(2 * np.pi * freq * m * t) * a for m, a in [(1, 1), (1.47, 0.6), (2.09, 0.35)])
        clink = attack(clink * np.exp(-t / 0.03), 0.001)
        mix_at(out, clink * rng.uniform(0.08, 0.2) * (1 - at / 1.8), at)
        at += rng.uniform(0.015, 0.05) + at * 0.06
    return out


SOUNDS = {
    "coins": s_coins,
    "pop": s_pop,
    "title": s_title,
    "whoosh": s_whoosh,
    "tick": s_tick,
    "tick_last": lambda: s_tick(last=True),
    "drumroll": s_drumroll,
    "creak": s_creak,
    "win": s_win,
    "ok": s_ok,
    "bad": s_bad,
    "sting": s_sting,
    "fanfare": s_fanfare,
}


def main():
    cues = json.load(open(sys.argv[1]))
    out_path = sys.argv[2]
    total = max(c["t"] for c in cues)
    effects = np.zeros(int(total * SR) + 1)
    bed = np.zeros_like(effects)
    music_start = next(c["t"] for c in cues if c["sound"] == "music")
    music_end = next(c["t"] for c in cues if c["sound"] == "fanfare")
    for cue in cues:
        name = cue["sound"]
        if name in SOUNDS:
            mix_at(effects, SOUNDS[name](), cue["t"])
    mix_at(bed, music(music_end - music_start), music_start)

    mixed = effects + bed * 0.22
    # A soft limiter, then normalize to just under full scale.
    mixed = np.tanh(mixed * 1.4) / np.tanh(1.4)
    mixed = mixed / np.max(np.abs(mixed)) * 0.9
    stereo = np.stack([mixed, mixed], axis=1)
    wavfile.write(out_path, SR, (stereo * 32767).astype(np.int16))
    print(f"wrote {out_path}: {len(mixed) / SR:.1f} s, {len(cues)} cues")


if __name__ == "__main__":
    main()
