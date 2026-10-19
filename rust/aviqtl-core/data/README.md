# Generated `.exo` text-decoding tables

AviUtl writes object files in the operating system's ANSI code page: Chinese
translation packs produce GBK, a stock Japanese install produces Shift-JIS, and
some tools write UTF-8. Rust ships no multi-byte Asian codec, and this project
does not take a decoding dependency for one file format, so the two
double-byte maps are generated once and committed here.

Regenerate with Python 3 from the repository root:

```sh
python tools/gen_exo_encoding_tables.py
```

## Format

Each file is a flat array of big-endian `u16` code points, one per index.
`0xFFFF` marks a byte pair the code page does not map.

| File | Index | Size |
| --- | --- | --- |
| `gbk.bin` | `(lead - 0x81) * 190 + trail_index` for lead `0x81..=0xFE` | 23 940 entries |
| `shift_jis.bin` | `(lead - 0x81) * 188 + trail_index` for lead `0x81..=0x9F`, then `31 * 188 + (lead - 0xE0) * 188 + trail_index` for lead `0xE0..=0xEF` | 8 836 entries |

`trail_index` is `trail - 0x40` for trails below `0x7F` and `trail - 0x41`
above it, because `0x7F` is never a valid trail byte. Single bytes are handled
in code: ASCII below `0x80`, and the half-width katakana block `0xA1..=0xDF`
for Shift-JIS.

Both tables were produced by encoding every valid lead/trail pair and recording
which ones decode to exactly one BMP code point, so they contain no hand-edited
exceptions.
