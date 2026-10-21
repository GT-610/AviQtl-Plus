#!/usr/bin/env python3
"""Generate the AviUtl `.exo` text-decoding tables used by `aviqtl-rust-core`.

AviUtl writes object files in the system code page, not Unicode: Chinese packs
produce GBK, Japanese installs produce Shift-JIS, and a few tools write UTF-8.
Rust has no built-in multi-byte Asian codec and this project does not add a
decoding dependency for one file format, so the two double-byte maps are
generated once and committed as binary tables.

Run from the repository root:

    python tools/gen_exo_encoding_tables.py

Each table stores one big-endian `u16` Unicode scalar per index. `0xFFFF` means
"this byte pair is not mapped" (the decoder then emits U+FFFD). The index
schemes are fixed by the standards and are documented next to the decoder in
`rust/aviqtl-core/src/exo/encoding.rs`.
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "rust" / "aviqtl-core" / "data"

NONE = 0xFFFF


def gbk_index(lead: int, trail: int) -> int:
    """GBK: lead 0x81..0xFE, trail 0x40..0xFE skipping 0x7F."""
    trail_index = trail - 0x40 if trail < 0x7F else trail - 0x41
    return (lead - 0x81) * 190 + trail_index


def sjis_index(lead: int, trail: int) -> int:
    """CP932: lead 0x81..0x9F or 0xE0..0xFC, trail 0x40..0xFC skipping 0x7F.

    The upper lead range runs past 0xEF on purpose: CP932, which is what a
    Japanese Windows writes, adds the NEC-selected IBM extensions at 0xFA..0xFC
    (髙, 﨑 and friends). Strict JIS X 0208 stops at 0xEF and would drop them.
    """
    trail_index = trail - 0x40 if trail < 0x7F else trail - 0x41
    if lead <= 0x9F:
        return (lead - 0x81) * 188 + trail_index
    return (0x9F - 0x81 + 1) * 188 + (lead - 0xE0) * 188 + trail_index


GBK_SIZE = 126 * 190
SJIS_SIZE = (0x9F - 0x81 + 1 + 0xFC - 0xE0 + 1) * 188


def build(name: str, size: int, index) -> bytes:
    table = bytearray([0xFF, 0xFF]) * size
    mapped = 0
    for lead in range(0x81, 0xFF):
        for trail in list(range(0x40, 0x7F)) + list(range(0x80, 0xFF)):
            if lead == 0x80 or (name == "cp932" and 0xA0 <= lead <= 0xDF):
                continue
            try:
                text = bytes([lead, trail]).decode(name)
            except UnicodeDecodeError:
                continue
            if len(text) != 1:
                continue
            codepoint = ord(text)
            if codepoint > 0xFFFF:
                continue
            offset = index(lead, trail)
            if not 0 <= offset < size:
                continue
            table[offset * 2] = codepoint >> 8
            table[offset * 2 + 1] = codepoint & 0xFF
            mapped += 1
    return bytes(table), mapped


def main() -> int:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    # `cp932`, not `shift_jis`: the strict codec is JIS X 0208 and leaves the
    # NEC/IBM extensions unmapped, while a Japanese install writes CP932. The
    # two also disagree on a few code points (0x8160 is U+FF5E, not U+301C).
    for name, codec, size, index, filename in (
        ("gbk", "gbk", GBK_SIZE, gbk_index, "gbk.bin"),
        ("shift_jis", "cp932", SJIS_SIZE, sjis_index, "shift_jis.bin"),
    ):
        table, mapped = build(codec, size, index)
        (OUTPUT / filename).write_bytes(table)
        print(f"{filename}: {len(table)} bytes, {mapped} mapped pairs ({codec})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
