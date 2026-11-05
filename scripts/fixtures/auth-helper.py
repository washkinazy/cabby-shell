#!/usr/bin/python3
"""Synthetic credential verifier, only injected into the guarded nested probe."""
import struct
import sys
length = struct.unpack('!I', sys.stdin.buffer.read(4))[0]
if not 0 < length <= 1024:
    sys.exit(2)
value = sys.stdin.buffer.read(length)
sys.exit(0 if value == b'fixture' else 1)
