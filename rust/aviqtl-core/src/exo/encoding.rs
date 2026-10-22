//! Text decoding for AviUtl object files.
//!
//! AviUtl stores `.exo` strings in the operating system's ANSI code page. The
//! Chinese translation packs write GBK, Japanese installs write Shift-JIS, and
//! a few tools write UTF-8. Rust ships no multi-byte Asian codec, so the two
//! double-byte maps are committed as generated tables; see
//! `tools/gen_exo_encoding_tables.py` for how they are produced and
//! `data/README.md` for what each index means.

use std::collections::HashMap;
use std::sync::OnceLock;

/// GBK double-byte map: one big-endian `u16` per index, `0xFFFF` for unmapped.
const GBK_TABLE: &[u8] = include_bytes!("../../data/gbk.bin");
/// Shift-JIS double-byte map, same representation.
const SHIFT_JIS_TABLE: &[u8] = include_bytes!("../../data/shift_jis.bin");

const UNMAPPED: u16 = 0xFFFF;
const GBK_TRAILS: usize = 190;
const SHIFT_JIS_TRAILS: usize = 188;

/// The text encoding of an object file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextEncoding {
    /// Plain UTF-8, used by hand-written files and by our own exports.
    Utf8,
    /// GBK / CP936, the code page of the AviUtl Chinese translation packs.
    #[default]
    Gbk,
    /// Shift-JIS / CP932, the code page of an unmodified Japanese install.
    ShiftJis,
}

impl TextEncoding {
    /// Picks the encoding that decodes `bytes` with the fewest unmapped sequences.
    ///
    /// Valid UTF-8 wins outright. Otherwise GBK and Shift-JIS are both attempted and the
    /// cleaner decode is kept; when both are clean, Japanese kana in the Shift-JIS decode
    /// wins over GBK, because a Shift-JIS file read as GBK produces plausible-looking but
    /// wrong Han characters and no kana at all.
    pub fn detect(bytes: &[u8]) -> Self {
        if std::str::from_utf8(bytes).is_ok() {
            return Self::Utf8;
        }
        let gbk = Self::Gbk.decode_scored(bytes);
        let shift_jis = Self::ShiftJis.decode_scored(bytes);
        if gbk.invalid != shift_jis.invalid {
            return if gbk.invalid < shift_jis.invalid {
                Self::Gbk
            } else {
                Self::ShiftJis
            };
        }
        // An object file is mostly ASCII keys and numbers, so kana has to be measured
        // against the non-ASCII characters rather than the whole document: four katakana in
        // a header of several hundred characters would otherwise never reach any threshold.
        //
        // The share only has to clear an eighth. A drawing filter is written with kanji-only
        // parameter names, so a minimal export — one 画像ファイル object and its 標準描画,
        // nothing else — is four kana against eighteen non-ASCII characters, and a quarter
        // would call that Chinese and read every name as mojibake. Chinese text read as
        // Shift-JIS carries almost no full-width kana at all, because GB2312 never uses the
        // `0x82`/`0x83` lead bytes kana are stored behind, so the margin stays wide.
        let non_ascii = shift_jis.non_ascii.max(1);
        if shift_jis.kana >= 2 && shift_jis.kana * 8 >= non_ascii && shift_jis.kana > gbk.kana {
            Self::ShiftJis
        } else {
            Self::Gbk
        }
    }

    /// Decodes `bytes`, replacing unmappable sequences with U+FFFD.
    pub fn decode(self, bytes: &[u8]) -> String {
        match self {
            Self::Utf8 => String::from_utf8_lossy(bytes).into_owned(),
            Self::Gbk => decode_multibyte(bytes, GBK_TABLE, GBK_TRAILS, Self::Gbk),
            Self::ShiftJis => {
                decode_multibyte(bytes, SHIFT_JIS_TABLE, SHIFT_JIS_TRAILS, Self::ShiftJis)
            }
        }
    }

    /// Encodes `text`, writing `?` for characters the code page cannot represent.
    ///
    /// UTF-8 is lossless. GBK and Shift-JIS fall back to `?` only for characters outside
    /// the code page, which keeps the bytes valid for AviUtl's reader instead of emitting
    /// a file it cannot parse.
    pub fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Gbk => encode_with(&REVERSE_GBK, GBK_TABLE, GBK_TRAILS, Self::Gbk, text),
            Self::ShiftJis => encode_with(
                &REVERSE_SHIFT_JIS,
                SHIFT_JIS_TABLE,
                SHIFT_JIS_TRAILS,
                Self::ShiftJis,
                text,
            ),
        }
    }

    fn decode_scored(self, bytes: &[u8]) -> DecodeScore {
        let text = self.decode(bytes);
        DecodeScore {
            invalid: text
                .chars()
                .filter(|character| *character == '\u{fffd}')
                .count(),
            kana: text.chars().filter(|character| is_kana(*character)).count(),
            non_ascii: text
                .chars()
                .filter(|character| !character.is_ascii())
                .count(),
        }
    }
}

struct DecodeScore {
    invalid: usize,
    kana: usize,
    /// Characters outside ASCII. A code-page decision only concerns those.
    non_ascii: usize,
}

/// Full-width hiragana and katakana, the characters that identify Japanese prose.
///
/// Half-width katakana (`U+FF61..=U+FF9F`) are deliberately excluded: a GBK byte pair such as
/// `0xCEC4` (文本) decodes to `ﾎﾄ` under Shift-JIS, so counting half-width forms made short
/// Chinese values look Japanese and could pick the wrong code page for a whole file.
fn is_kana(character: char) -> bool {
    matches!(character, '\u{3040}'..='\u{30ff}')
}

fn table_lookup(table: &[u8], index: usize) -> Option<char> {
    let offset = index.checked_mul(2)?;
    let pair = table.get(offset..offset + 2)?;
    let codepoint = u16::from_be_bytes([pair[0], pair[1]]);
    if codepoint == UNMAPPED {
        return None;
    }
    char::from_u32(u32::from(codepoint))
}

fn gbk_pair_is_valid(lead: u8, trail: u8) -> bool {
    (0x81..=0xfe).contains(&lead) && (0x40..=0xfe).contains(&trail) && trail != 0x7f
}

fn shift_jis_pair_is_valid(lead: u8, trail: u8) -> bool {
    // CP932, not strict JIS X 0208: a Japanese install writes the NEC/IBM extensions at
    // 0xFA..0xFC, and the table carries them, so the lead range has to reach 0xFC or those
    // pairs are refused before the table is consulted.
    ((0x81..=0x9f).contains(&lead) || (0xe0..=0xfc).contains(&lead))
        && (0x40..=0xfc).contains(&trail)
        && trail != 0x7f
}

fn decode_multibyte(bytes: &[u8], table: &[u8], trails: usize, encoding: TextEncoding) -> String {
    let mut output = String::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte < 0x80 {
            output.push(char::from(byte));
            index += 1;
            continue;
        }
        if encoding == TextEncoding::ShiftJis && (0xa1..=0xdf).contains(&byte) {
            output.push(char::from_u32(u32::from(byte) - 0xa1 + 0xff61).unwrap_or('\u{fffd}'));
            index += 1;
            continue;
        }
        let Some(&trail) = bytes.get(index + 1) else {
            output.push('\u{fffd}');
            break;
        };
        let valid = match encoding {
            TextEncoding::Gbk => gbk_pair_is_valid(byte, trail),
            _ => shift_jis_pair_is_valid(byte, trail),
        };
        if !valid {
            output.push('\u{fffd}');
            index += 1;
            continue;
        }
        let trail_index = if trail < 0x7f {
            trail - 0x40
        } else {
            trail - 0x41
        } as usize;
        let lead_index = match encoding {
            TextEncoding::Gbk => byte as usize - 0x81,
            _ if byte <= 0x9f => byte as usize - 0x81,
            _ => 31 + byte as usize - 0xe0,
        };
        let mapped = table_lookup(table, lead_index * trails + trail_index);
        match mapped {
            Some(character) => {
                output.push(character);
                index += 2;
            }
            None => {
                output.push('\u{fffd}');
                index += 2;
            }
        }
    }
    output
}

fn encode_with(
    reverse: &OnceLock<HashMap<char, u16>>,
    table: &[u8],
    trails: usize,
    encoding: TextEncoding,
    text: &str,
) -> Vec<u8> {
    let reverse = reverse.get_or_init(|| build_reverse_for(table, trails, encoding));
    let mut output = Vec::with_capacity(text.len());
    for character in text.chars() {
        let codepoint = u32::from(character);
        if codepoint < 0x80 {
            output.push(codepoint as u8);
            continue;
        }
        match reverse.get(&character) {
            Some(&pair) => {
                output.push((pair >> 8) as u8);
                output.push(pair as u8);
            }
            None => output.push(b'?'),
        }
    }
    output
}

/// Maps every decodable character back to its two-byte code.
///
/// Each code page gets its own map, built from its own table: the GBK and Shift-JIS spaces
/// overlap, so a shared map would encode characters as bytes the other table never produced.
fn build_reverse_for(table: &[u8], trails: usize, encoding: TextEncoding) -> HashMap<char, u16> {
    let mut reverse = HashMap::with_capacity(table.len() / 2);
    for lead_index in 0..(table.len() / 2 / trails) {
        for trail_index in 0..trails {
            let Some(character) = table_lookup(table, lead_index * trails + trail_index) else {
                continue;
            };
            let lead = match encoding {
                TextEncoding::Gbk => 0x81 + lead_index,
                _ if lead_index < 31 => 0x81 + lead_index,
                _ => 0xe0 + (lead_index - 31),
            } as u8;
            let trail_offset = if trail_index < 0x3f { 0x40 } else { 0x41 };
            let trail = (trail_index + trail_offset) as u8;
            reverse
                .entry(character)
                .or_insert(u16::from_be_bytes([lead, trail]));
        }
    }
    reverse
}

static REVERSE_GBK: OnceLock<HashMap<char, u16>> = OnceLock::new();
static REVERSE_SHIFT_JIS: OnceLock<HashMap<char, u16>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_and_decodes_gbk_from_the_chinese_pack() {
        // "标准属性" and "缩放率", as stored by the AviUtl CN packs.
        let bytes = [0xb1, 0xea, 0xd7, 0xbc, 0xca, 0xf4, 0xd0, 0xd4];
        assert_eq!(TextEncoding::detect(&bytes), TextEncoding::Gbk);
        assert_eq!(TextEncoding::Gbk.decode(&bytes), "标准属性");
        let scale = [0xcb, 0xf5, 0xb7, 0xc5, 0xc2, 0xca];
        assert_eq!(TextEncoding::Gbk.decode(&scale), "缩放率");
    }

    #[test]
    fn prefers_gbk_when_the_shift_jis_reading_is_only_half_width_kana() {
        // `文本` followed by `标准属性`, both GBK. Under Shift-JIS the first pair is `ﾎﾄ`,
        // half-width katakana, which used to make the whole file look Japanese.
        let bytes = [
            0xce, 0xc4, 0xb1, 0xbe, 0xb1, 0xea, 0xd7, 0xbc, 0xca, 0xf4, 0xd0, 0xd4,
        ];
        assert_eq!(TextEncoding::detect(&bytes), TextEncoding::Gbk);
        assert_eq!(TextEncoding::Gbk.decode(&bytes), "文本标准属性");
    }

    #[test]
    fn detects_shift_jis_by_its_kana() {
        // "テキスト" in Shift-JIS.
        let bytes = [0x83, 0x65, 0x83, 0x4c, 0x83, 0x58, 0x83, 0x67];
        assert_eq!(TextEncoding::detect(&bytes), TextEncoding::ShiftJis);
        assert_eq!(TextEncoding::ShiftJis.decode(&bytes), "テキスト");
    }

    #[test]
    fn prefers_utf8_and_round_trips_every_encoding() {
        assert_eq!(TextEncoding::detect("動画".as_bytes()), TextEncoding::Utf8);
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Gbk,
            TextEncoding::ShiftJis,
        ] {
            let text = match encoding {
                TextEncoding::Utf8 => "AviUtl 動画 テキスト",
                TextEncoding::Gbk => "标准属性 缩放率",
                TextEncoding::ShiftJis => "テキスト 図形",
            };
            assert_eq!(
                encoding.decode(&encoding.encode(text)),
                text,
                "{encoding:?}"
            );
        }
    }

    #[test]
    fn a_minimal_japanese_export_is_not_read_as_chinese() {
        // Dragging one image object out of AviUtl writes just its own sections, and every
        // parameter of the drawing filter is a kanji-only name: four katakana (ファイル)
        // among eighteen non-ASCII characters, which is 0.22 rather than the 0.25 a quarter
        // threshold asked for. Read as GBK the same bytes are 夋憸僼傽僀儖昗弨昤夋奼戝, so the
        // object and its filter both became unresolvable placeholders.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(
            b"[exedit]\r\nwidth=1280\r\nheight=720\r\nrate=30\r\nscale=100\r\nlength=60\r\n\
              audio_rate=44100\r\naudio_ch=2\r\n[0]\r\nstart=1\r\nend=60\r\nlayer=1\r\n[0.0]\r\n_name=",
        );
        bytes.extend_from_slice(&[
            0x89, 0xe6, 0x91, 0x9c, 0x83, 0x74, 0x83, 0x40, 0x83, 0x43, 0x83, 0x8b,
        ]); // 画像ファイル
        bytes.extend_from_slice(b"\r\n[0.1]\r\n_name=");
        bytes.extend_from_slice(&[0x95, 0x57, 0x8f, 0x80, 0x95, 0x60, 0x89, 0xe6]); // 標準描画
        bytes.extend_from_slice(b"\r\nX=0.0\r\nY=0.0\r\nZ=0.0\r\n");
        bytes.extend_from_slice(&[0x8a, 0x67, 0x91, 0xe5, 0x97, 0x66]); // 拡大率
        bytes.extend_from_slice(b"=100.0\r\n");
        bytes.extend_from_slice(&[0x93, 0x78, 0x96, 0xbe, 0x93, 0x78]); // 透明度
        bytes.extend_from_slice(b"=0.0\r\n");
        bytes.extend_from_slice(&[0x89, 0xf1, 0x93, 0x5c]); // 回転
        bytes.extend_from_slice(b"=0.0\r\nblend=0\r\n");

        assert_eq!(TextEncoding::detect(&bytes), TextEncoding::ShiftJis);
        assert!(
            TextEncoding::ShiftJis
                .decode(&bytes)
                .contains("画像ファイル\r\n[0.1]\r\n_name=標準描画")
        );
    }

    #[test]
    fn keeps_the_cp932_extensions_a_japanese_install_writes() {
        // Strict JIS X 0208 stops at 0xEF and spells 0x8160 as U+301C. A Japanese install
        // writes CP932, which adds the NEC-selected characters at lead 0x87 and the IBM
        // extensions at 0xFA..0xFC, and spells 0x8160 as U+FF5E.
        for (bytes, expected) in [
            ([0x87, 0x40], '①'),
            ([0x87, 0x82], '№'),
            ([0x87, 0x8a], '㈱'),
            ([0xee, 0xe0], '髙'),
            ([0xed, 0x95], '﨑'),
            ([0x81, 0x60], '～'),
            ([0x81, 0x7c], '－'),
        ] {
            assert_eq!(
                TextEncoding::ShiftJis.decode(&bytes),
                expected.to_string(),
                "{bytes:02x?} should decode to {expected}"
            );
        }
    }

    #[test]
    fn unmappable_sequences_become_replacement_characters() {
        let decoded = TextEncoding::Gbk.decode(&[0xff, 0xff]);
        assert!(decoded.contains('\u{fffd}'));
        assert_eq!(TextEncoding::Gbk.encode("\u{1f600}"), b"?");
    }

    #[test]
    fn ascii_is_shared_by_every_code_page() {
        for encoding in [
            TextEncoding::Utf8,
            TextEncoding::Gbk,
            TextEncoding::ShiftJis,
        ] {
            assert_eq!(encoding.decode(b"track0=1.00"), "track0=1.00");
            assert_eq!(encoding.encode("track0"), b"track0");
        }
    }
}
