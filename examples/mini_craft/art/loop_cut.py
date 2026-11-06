"""Keeps the last 2 seconds of a 16-bit mono WAV: a loop that repeats every
2 seconds, cut where it's steady, so its end meets its start.

    python3 art/loop_cut.py hum-long.wav assets/hum.wav
"""
import struct
import sys

source, target = sys.argv[1], sys.argv[2]
data = open(source, "rb").read()
at = data.index(b"data")
size = struct.unpack("<I", data[at + 4 : at + 8])[0]
samples = data[at + 8 : at + 8 + size]
keep = 2 * 44100 * 2  # 2 seconds of 16-bit samples
tail = samples[-keep:]
header = bytearray(data[:at + 8])
struct.pack_into("<I", header, 4, len(header) - 8 + len(tail))
struct.pack_into("<I", header, at + 4, len(tail))
open(target, "wb").write(bytes(header) + tail)
