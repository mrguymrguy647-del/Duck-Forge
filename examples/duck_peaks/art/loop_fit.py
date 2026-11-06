"""Makes a 16-bit mono WAV exactly SECONDS long (cut or padded with
silence), so a music loop keeps its beat when it repeats.

    python3 art/loop_fit.py music-raw.wav assets/music.wav 4.0
"""
import struct
import sys

source, target, seconds = sys.argv[1], sys.argv[2], float(sys.argv[3])
data = open(source, "rb").read()
at = data.index(b"data")
size = struct.unpack("<I", data[at + 4 : at + 8])[0]
samples = data[at + 8 : at + 8 + size]
want = round(seconds * 44100) * 2  # 16-bit samples
samples = samples[:want].ljust(want, b"\0")
header = bytearray(data[: at + 8])
struct.pack_into("<I", header, 4, len(header) - 8 + len(samples))
struct.pack_into("<I", header, at + 4, len(samples))
open(target, "wb").write(bytes(header) + samples)
