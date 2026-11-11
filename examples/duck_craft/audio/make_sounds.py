"""Writes Duck Craft's music and sound effects as .wav files, from nothing
but sine waves, noise and arithmetic (Duck Echo isn't ready yet).

    python3 audio/make_sounds.py          # run in examples/duck_craft

writes into assets/:

  music_day.wav    "Still Water": slow felt piano over a soft pad, D minor,
                   64 s, loops with no seam
  music_night.wav  "Nothing Waits": a low drone, a minor second that comes
                   and goes, wind, a far bell; 64 s, loops
  music_title.wav  "The Island Doesn't Remember": a falling piano figure
                   over the drone; 32 s, loops
  groan.wav        a Hollow, somewhere near
  groan_far.wav    the same, further off: quieter and muffled (the engine
                   plays a .wav at one loudness, so distance is a file)
  hollow_hurt.wav  a Hollow, hit
  hollow_die.wav   a Hollow, coming apart
  sting.wav        the Watcher, seen
  heartbeat.wav    one beat, for looping when the duck is hurt
  bell_dusk.wav    nightfall
  bell_dawn.wav    morning

Everything comes from fixed random seeds, so the files come out the same
every time. Music is 16000 samples a second (it has little above 6 kHz,
and it keeps the files small); effects are 22050.
"""

from pathlib import Path

import numpy as np
from scipy.signal import butter, lfilter, sosfilt

OUT = Path(__file__).resolve().parent.parent / "assets"
MUSIC_RATE = 16000
SFX_RATE = 22050


# ---- Building blocks ----


def note_hz(name):
    """'D4' -> Hz. Sharps as 'C#4', flats as 'Bb3'."""
    names = {"C": 0, "C#": 1, "Db": 1, "D": 2, "D#": 3, "Eb": 3, "E": 4, "F": 5,
             "F#": 6, "Gb": 6, "G": 7, "G#": 8, "Ab": 8, "A": 9, "A#": 10, "Bb": 10, "B": 11}
    pitch, octave = name[:-1], int(name[-1])
    midi = 12 * (octave + 1) + names[pitch]
    return 440.0 * 2 ** ((midi - 69) / 12)


def snap(hz, length):
    """The nearest frequency that fits a whole number of times in `length`
    seconds, so a held tone joins up where the loop repeats."""
    return round(hz * length) / length


def time_axis(seconds, rate):
    return np.arange(int(seconds * rate)) / rate


def envelope(n, rate, attack, decay):
    """Rises over `attack` seconds, then dies away (exponentially) with
    time constant `decay`."""
    t = np.arange(n) / rate
    rise = np.clip(t / max(attack, 1e-4), 0, 1)
    return rise * np.exp(-np.maximum(t - attack, 0) / decay)


def felt_piano(hz, seconds, rate, velocity=1.0):
    """A soft piano: a few harmonics (slightly stretched, as real strings
    are), the high ones dying first, a gentle felt-hammer start."""
    t = time_axis(seconds, rate)
    out = np.zeros_like(t)
    for k, amp in enumerate([1.0, 0.45, 0.22, 0.12, 0.06], start=1):
        f = hz * k * (1 + 0.0004 * k * k)
        if f > rate / 2.2:
            break
        decay = (3.2 if hz < 200 else 2.4) / k ** 0.7
        out += amp * np.sin(2 * np.pi * f * t) * envelope(len(t), rate, 0.012, decay)
    thump = np.random.default_rng(int(hz)).normal(0, 1, len(t)) * envelope(len(t), rate, 0.001, 0.01) * 0.04
    return (out + thump) * velocity


def bell(hz, seconds, rate, seed=0):
    """A deep bell: partials at the odd, inharmonic ratios of real bells,
    each ringing for its own time."""
    t = time_axis(seconds, rate)
    rng = np.random.default_rng(seed)
    out = np.zeros_like(t)
    for ratio, amp, decay in [(0.5, 0.6, 6.0), (1.0, 1.0, 4.0), (1.19, 0.5, 3.0), (1.5, 0.35, 2.5),
                              (2.0, 0.4, 2.0), (2.74, 0.25, 1.4), (3.76, 0.15, 0.9), (5.4, 0.08, 0.5)]:
        f = hz * ratio * (1 + rng.normal(0, 0.002))
        if f > rate / 2.2:
            continue
        beat = 1 + 0.15 * np.sin(2 * np.pi * rng.uniform(0.3, 1.2) * t)  # the slow wobble of a bell
        out += amp * np.sin(2 * np.pi * f * t + rng.uniform(0, 6.28)) * envelope(len(t), rate, 0.004, decay) * beat
    return out


def looping_noise(n, rate, cutoff, rng):
    """Noise that joins up with itself when it repeats: made in the
    frequency domain (random phases, nothing above `cutoff`), so it is
    exactly periodic over `n` samples."""
    spectrum = np.zeros(n // 2 + 1, dtype=complex)
    freqs = np.fft.rfftfreq(n, 1 / rate)
    keep = freqs < cutoff
    spectrum[keep] = np.exp(1j * rng.uniform(0, 2 * np.pi, keep.sum())) / (1 + freqs[keep] / (cutoff / 3))
    spectrum[0] = 0
    noise = np.fft.irfft(spectrum, n)
    return noise / np.std(noise)


def lowpass(x, hz, rate, order=2):
    sos = butter(order, hz / (rate / 2), btype="low", output="sos")
    return sosfilt(sos, x)


def bandpass(x, low, high, rate, order=2):
    sos = butter(order, [low / (rate / 2), high / (rate / 2)], btype="band", output="sos")
    return sosfilt(sos, x)


def reverb(x, rate, size=1.0, wet=0.35):
    """A plain Schroeder reverb: four combs in parallel, two all-passes in
    a row. `size` stretches the room."""
    combs = [0.0297, 0.0371, 0.0411, 0.0437]
    out = np.zeros_like(x)
    for delay in combs:
        d = int(delay * size * rate)
        g = 0.84
        # y[n] = x[n] + g * y[n - d]
        a = np.zeros(d + 1)
        a[0], a[d] = 1.0, -g
        out += lfilter([1.0], a, x)
    out /= len(combs)
    for delay, g in [(0.005, 0.7), (0.0017, 0.7)]:
        d = int(delay * rate)
        b = np.zeros(d + 1)
        a = np.zeros(d + 1)
        b[0], b[d] = -g, 1.0
        a[0], a[d] = 1.0, -g
        out = lfilter(b, a, out)
    return (1 - wet) * x + wet * out


def place(track, sound, at_seconds, rate):
    """Adds `sound` into `track` from `at_seconds` (cut at the end)."""
    start = int(at_seconds * rate)
    if start < 0:  # starts before the track: keep the part inside it
        sound, start = sound[-start:], 0
    end = min(len(track), start + len(sound))
    if start < len(track) and end > start:
        track[start:end] += sound[: end - start]


def seamless(render, length, rate):
    """Renders a loop that repeats with no seam: `render(track, offset)`
    places one pass of the music starting at `offset` seconds. Three
    passes go through the effects, and the middle one is kept: its start
    already holds the tails of the pass before it, just as the loop's
    start will when it repeats."""
    track = np.zeros(int(length * rate) * 3)
    for k in range(3):
        render(track, k * length)
    return track


def normalize(x, peak=0.8):
    return x / (np.max(np.abs(x)) + 1e-9) * peak


def fade(x, rate, fade_in=0.0, fade_out=0.0):
    x = x.copy()
    a, b = int(fade_in * rate), int(fade_out * rate)
    if a:
        x[:a] *= np.linspace(0, 1, a)
    if b:
        x[-b:] *= np.linspace(1, 0, b)
    return x


def write(name, samples, rate):
    """A 16-bit mono .wav."""
    import wave
    pcm = (np.clip(samples, -1, 1) * 32767).astype("<i2")
    path = OUT / name
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(rate)
        w.writeframes(pcm.tobytes())
    seam = abs(float(samples[0]) - float(samples[-1]))
    print(f"  {name:18} {len(samples) / rate:5.1f} s  peak {np.max(np.abs(samples)):.2f}"
          f"  rms {np.sqrt(np.mean(samples ** 2)):.3f}  end-to-start step {seam:.4f}")


# ---- Music ----


def pad(track, chord, start, seconds, rate, level):
    """A soft held chord: two slightly detuned sines a note, slow in and
    out, a little brightness from a quiet octave above."""
    t = time_axis(seconds, rate)
    swell = np.sin(np.pi * np.clip(t / seconds, 0, 1)) ** 1.5
    out = np.zeros_like(t)
    for name in chord:
        hz = note_hz(name)
        for detune in (-0.12, 0.12):
            out += np.sin(2 * np.pi * (hz + detune) * t)
        out += 0.15 * np.sin(2 * np.pi * hz * 2 * t)
    place(track, out * swell * level / len(chord), start, rate)


def day_music():
    """'Still Water': i - VI - III - VII in D minor, four bars a chord at
    60 beats a minute. A felt piano wanders over a pad; bass notes on the
    first beat. Sparse on purpose: lots of room for the wind."""
    rate, length = MUSIC_RATE, 64.0
    rng = np.random.default_rng(11)
    chords = [
        (["D3", "F3", "A3"], "D2", ["D4", "F4", "A4", "C5", "E5", "D5"]),
        (["Bb2", "D3", "F3"], "Bb1", ["D4", "F4", "Bb4", "A4", "C5", "D5"]),
        (["F3", "A3", "C4"], "F2", ["C4", "F4", "A4", "C5", "G4", "E4"]),
        (["C3", "E3", "G3"], "C2", ["C4", "E4", "G4", "D5", "E5", "G4"]),
    ]
    # The melody, chosen once (the same each pass): per chord, 16 beats,
    # a note on some of them.
    melody = []
    for c, (_, _, notes) in enumerate(chords):
        for beat in range(16):
            # Fewer notes as each chord goes on; never on the very last beat.
            if beat < 15 and rng.random() < (0.55 if beat % 4 == 0 else 0.22):
                melody.append((c * 16 + beat + rng.choice([0, 0, 0.5]), rng.choice(notes), rng.uniform(0.5, 0.85)))

    def render(track, offset):
        for c, (chord, bass, _) in enumerate(chords):
            start = offset + c * 16.0
            pad(track, chord, start - 1.0, 18.0, rate, 0.10)
            for bar in range(4):
                velocity = 0.55 if bar % 2 == 0 else 0.35
                place(track, felt_piano(note_hz(bass), 6.0, rate, velocity), start + bar * 4.0, rate)
        for beat, name, velocity in melody:
            place(track, felt_piano(note_hz(name), 5.0, rate, velocity * 0.6), offset + beat, rate)

    track = seamless(render, length, rate)
    track = reverb(track, rate, size=1.6, wet=0.45)
    n = int(length * rate)
    return normalize(track[n:2 * n], 0.6)


def night_music():
    """'Nothing Waits': D and A held low, an E-flat that creeps in a minor
    second above (and leaves), wind, and a bell now and then, far away."""
    rate, length = MUSIC_RATE, 64.0
    t = time_axis(length, rate)
    rng = np.random.default_rng(23)
    # Drones, tuned to join up at the seam; slow swells that repeat
    # exactly once or twice a loop.
    swell = lambda times, phase=0.0: 0.5 + 0.5 * np.sin(2 * np.pi * times * t / length + phase)
    drone = np.zeros_like(t)
    for name, level, times in [("D1", 0.9, 1), ("D2", 0.55, 2), ("A2", 0.30, 3)]:
        hz = note_hz(name)
        for detune in (-0.07, 0.0, 0.07):
            f = snap(hz + detune, length)
            drone += level * np.sin(2 * np.pi * f * t) * (0.6 + 0.4 * swell(times, detune * 10))
    # The minor second: comes in over the first half of the loop, leaves
    # over the second.
    eb = sum(np.sin(2 * np.pi * snap(note_hz("Eb3") + d, length) * t) for d in (-0.1, 0.1))
    drone += 0.18 * eb * np.sin(np.pi * t / length) ** 3
    # A faint high whine, a tritone above, beating slowly.
    drone += 0.03 * np.sin(2 * np.pi * snap(note_hz("G#5"), length) * t) * swell(2, 1.0)
    # Wind: noise, low-passed, gusting.
    wind = looping_noise(len(t), rate, 500, rng) * 0.12
    wind *= 0.4 + 0.6 * swell(3, 2.0) * swell(5, 0.5)

    def render(track, offset):
        n0 = int(offset * rate)
        track[n0:n0 + len(t)] += drone + wind
        for at, name in [(6.0, "D2"), (29.0, "Ab2"), (47.5, "D2")]:
            place(track, bell(note_hz(name), 9.0, rate, seed=int(at)) * 0.6, offset + at, rate)

    track = seamless(render, length, rate)
    track = reverb(track, rate, size=2.2, wet=0.4)
    n = int(length * rate)
    return normalize(track[n:2 * n], 0.55)


def title_music():
    """'The Island Doesn't Remember': a four-note figure falling through
    D minor, over the low drone, slowing, then the bell."""
    rate, length = MUSIC_RATE, 32.0
    t = time_axis(length, rate)
    drone = np.zeros_like(t)
    for name, level in [("D2", 0.5), ("A2", 0.25)]:
        for detune in (-0.06, 0.06):
            drone += level * np.sin(2 * np.pi * snap(note_hz(name) + detune, length) * t)
    drone *= 0.7 + 0.3 * np.sin(2 * np.pi * t / length)
    figure = [("A4", 0.0), ("F4", 1.0), ("E4", 2.0), ("D4", 3.0),
              ("G4", 4.0), ("E4", 5.0), ("D4", 6.0), ("C#4", 7.0),
              ("F4", 8.0), ("D4", 9.0), ("C4", 10.0), ("Bb3", 11.0),
              ("A3", 12.5), ("D4", 14.5)]

    def render(track, offset):
        n0 = int(offset * rate)
        track[n0:n0 + len(t)] += drone
        for name, beat in figure:
            place(track, felt_piano(note_hz(name), 6.0, rate, 0.55), offset + 1.0 + beat * 1.1, rate)
        place(track, bell(note_hz("D2"), 10.0, rate, seed=3) * 0.3, offset + 19.0, rate)

    track = seamless(render, length, rate)
    track = reverb(track, rate, size=2.0, wet=0.5)
    n = int(length * rate)
    return normalize(track[n:2 * n], 0.6)


# ---- Effects ----


def saw(f, t):
    """A sawtooth with frequency `f` (an array: it can glide)."""
    phase = np.cumsum(f) / SFX_RATE
    return 2 * (phase % 1.0) - 1


def voice(f, t, formants, rng, breath=0.25):
    """A throat: a buzzing saw through mouth-shaped band-passes, and air."""
    buzz = saw(f, t)
    out = np.zeros_like(t)
    for (low, high), level in formants:
        out += bandpass(buzz, low, high, SFX_RATE) * level
    out += bandpass(rng.normal(0, 1, len(t)), 300, 2400, SFX_RATE) * breath
    return out


def groan():
    """Low, slow, wavering: something that used to be a duck."""
    rng = np.random.default_rng(5)
    t = time_axis(1.8, SFX_RATE)
    f = 62 + 8 * np.sin(2 * np.pi * 0.7 * t) + 3 * np.sin(2 * np.pi * 6.5 * t) - 10 * t
    jitter = lowpass(rng.normal(0, 1, len(t)), 30, SFX_RATE) * 30
    out = voice(f + jitter, t, [((90, 400), 1.2), ((450, 800), 1.0), ((900, 1400), 0.4)], rng, breath=0.06)
    out = np.tanh(out * 2.5)  # a rasp
    out *= envelope(len(t), SFX_RATE, 0.35, 0.9)
    return fade(normalize(reverb(out, SFX_RATE, 1.2, 0.3), 0.7), SFX_RATE, 0, 0.1)


def groan_far():
    return lowpass(groan(), 700, SFX_RATE) * 0.35


def hollow_hurt():
    rng = np.random.default_rng(6)
    t = time_axis(0.45, SFX_RATE)
    f = 140 - 120 * t
    out = voice(f, t, [((350, 900), 1.0), ((1200, 2000), 0.4)], rng, breath=0.5)
    out = np.tanh(out * 3)  # rough
    out *= envelope(len(t), SFX_RATE, 0.01, 0.15)
    return fade(normalize(out, 0.7), SFX_RATE, 0, 0.05)


def hollow_die():
    """A falling moan that breaks into dry crackle, like ash."""
    rng = np.random.default_rng(7)
    t = time_axis(1.6, SFX_RATE)
    f = 110 * np.exp(-t * 1.4) + 30
    moan = voice(f, t, [((90, 400), 1.2), ((250, 650), 1.0)], rng, breath=0.08) * envelope(len(t), SFX_RATE, 0.02, 0.5)
    moan = np.tanh(moan * 2)
    crackle = (rng.random(len(t)) < 0.004 * np.clip(t * 2 - 0.3, 0, 1)) * rng.normal(0, 1, len(t))
    crackle = bandpass(crackle, 1500, 6000, SFX_RATE) * 2 * np.exp(-np.maximum(t - 0.6, 0) * 2)
    out = moan + crackle
    return fade(normalize(reverb(out, SFX_RATE, 1.5, 0.35), 0.75), SFX_RATE, 0, 0.2)


def sting():
    """The scare: a boom, and strings that rise all wrong together."""
    rng = np.random.default_rng(8)
    t = time_axis(2.6, SFX_RATE)
    boom = np.sin(2 * np.pi * (55 * np.exp(-t * 3) + 30) * t) * envelope(len(t), SFX_RATE, 0.005, 0.6)
    boom += lowpass(rng.normal(0, 1, len(t)), 200, SFX_RATE) * envelope(len(t), SFX_RATE, 0.002, 0.3) * 2
    cluster = np.zeros_like(t)
    for name in ["D6", "Eb6", "E6", "Ab6", "A5", "Bb5"]:
        hz = note_hz(name)
        vibrato = 1 + 0.006 * np.sin(2 * np.pi * rng.uniform(5, 7) * t + rng.uniform(0, 6))
        glide = 1 + 0.03 * t  # bending up, wrong
        cluster += saw(np.full_like(t, hz) * vibrato * glide, t)
    cluster = bandpass(cluster, 600, 5000, SFX_RATE) / 6
    rise = np.clip(t / 0.25, 0, 1) * np.exp(-np.maximum(t - 0.9, 0) * 3)
    out = boom * 0.9 + cluster * rise * 1.2
    return fade(normalize(reverb(out, SFX_RATE, 1.8, 0.4), 0.85), SFX_RATE, 0, 0.3)


def heartbeat():
    """Lub-dub, one second: loops at 60 beats a minute."""
    t = time_axis(1.0, SFX_RATE)
    out = np.zeros_like(t)
    for at, hz, level in [(0.0, 55, 1.0), (0.26, 48, 0.75)]:
        local = np.clip(t - at, 0, None)
        hit = np.sin(2 * np.pi * hz * local) * np.exp(-local / 0.06) * (t >= at)
        out += hit * level
    out = lowpass(out, 180, SFX_RATE)
    return normalize(out, 0.8)


def bell_dusk():
    return fade(normalize(reverb(bell(note_hz("D2"), 7.0, SFX_RATE, seed=9), SFX_RATE, 2.0, 0.4), 0.7), SFX_RATE, 0, 0.5)


def bell_dawn():
    """Two small notes, high and plain: morning, for what it's worth."""
    out = np.zeros(int(4.0 * SFX_RATE))
    place(out, bell(note_hz("A5"), 3.5, SFX_RATE, seed=10) * 0.6, 0.0, SFX_RATE)
    place(out, bell(note_hz("D5"), 3.0, SFX_RATE, seed=11) * 0.5, 0.45, SFX_RATE)
    return fade(normalize(reverb(out, SFX_RATE, 1.4, 0.35), 0.6), SFX_RATE, 0, 0.5)


def main():
    OUT.mkdir(exist_ok=True)
    print("music:")
    write("music_day.wav", day_music(), MUSIC_RATE)
    write("music_night.wav", night_music(), MUSIC_RATE)
    write("music_title.wav", title_music(), MUSIC_RATE)
    print("effects:")
    for name, make in [("groan", groan), ("groan_far", groan_far), ("hollow_hurt", hollow_hurt), ("hollow_die", hollow_die),
                       ("sting", sting), ("heartbeat", heartbeat), ("bell_dusk", bell_dusk),
                       ("bell_dawn", bell_dawn)]:
        write(f"{name}.wav", make(), SFX_RATE)


if __name__ == "__main__":
    main()
