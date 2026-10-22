//! AviUtl ExEdit object files (`.exo`).
//!
//! An object file is a small INI-like document: one `[exedit]` header section that describes
//! the scene the objects were authored in, one `[n]` section per object, and one `[n.k]`
//! section per filter in that object's stack. Keys are not quoted and values may contain
//! `=`, so a line splits on its first `=` only.
//!
//! The model here is deliberately lossless rather than semantic: known keys are exposed
//! through typed accessors, and every key a file contains is kept in file order, so parsing
//! and writing an untouched file reproduces it. Translating objects and filters into
//! AviQtl-Plus project state is the application's job, not this module's.
//!
//! Facts taken from `exedit.auf` (see `notes/exo-format.md` outside the repository):
//! frames and layers are 1-based on disk, `duration = end - start + 1`, and the filter
//! stack is stored in evaluation order with the drawing/transform filter last.

mod encoding;
mod track;

pub use encoding::TextEncoding;
pub use track::TrackValue;

use std::fmt::{Display, Formatter};

/// Upper bound on object sections. A real project has thousands, not hundreds of thousands;
/// the limit exists so a malformed file cannot make the parser allocate without bound.
pub const MAX_OBJECTS: usize = 100_000;
/// Upper bound on the keys one section may carry.
pub const MAX_ENTRIES_PER_SECTION: usize = 4096;
/// Largest key or value the parser accepts. Text payloads are hex-encoded UTF-16 and grow
/// with the object's text, so this is generous but still bounded.
pub const MAX_VALUE_BYTES: usize = 1 << 20;

/// Why an object file could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExoError {
    /// The document does not start with an `[exedit]` section.
    NotAnObjectFile,
    /// A line has no `=` and is not a section header.
    MalformedLine { line: usize },
    /// A section header is not `[exedit]`, `[n]`, or `[n.k]`.
    MalformedSection { line: usize },
    /// The document exceeds [`MAX_OBJECTS`].
    TooManyObjects,
    /// A section exceeds [`MAX_ENTRIES_PER_SECTION`].
    TooManyEntries { line: usize },
    /// A line exceeds [`MAX_VALUE_BYTES`].
    OversizedLine { line: usize },
    /// A filter section appeared before any object section.
    FilterWithoutObject { line: usize },
}

impl Display for ExoError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotAnObjectFile => formatter.write_str("not an AviUtl object file"),
            Self::MalformedLine { line } => {
                write!(
                    formatter,
                    "line {line} is neither a section nor a key/value pair"
                )
            }
            Self::MalformedSection { line } => {
                write!(formatter, "line {line} has an unknown section")
            }
            Self::TooManyObjects => {
                formatter.write_str("the object file contains too many objects")
            }
            Self::TooManyEntries { line } => {
                write!(
                    formatter,
                    "the section starting at line {line} has too many keys"
                )
            }
            Self::OversizedLine { line } => write!(formatter, "line {line} is too long"),
            Self::FilterWithoutObject { line } => {
                write!(formatter, "line {line} adds a filter before any object")
            }
        }
    }
}

impl std::error::Error for ExoError {}

/// The `[exedit]` header: the scene the objects were authored in.
#[derive(Debug, Clone, PartialEq)]
pub struct ExoHeader {
    pub width: i32,
    pub height: i32,
    pub rate: f64,
    pub scale: i32,
    pub length: i32,
    pub audio_rate: i32,
    pub audio_channels: i32,
    pub alpha: bool,
    pub name: Option<String>,
    /// Keys this module does not model, kept in file order.
    pub extra: Vec<(String, String)>,
}

impl Default for ExoHeader {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            rate: 60.0,
            scale: 100,
            length: 300,
            audio_rate: 48_000,
            audio_channels: 2,
            alpha: false,
            name: None,
            extra: Vec::new(),
        }
    }
}

/// One `[n]` object section: a clip's placement and its filter stack.
#[derive(Debug, Clone, PartialEq)]
pub struct ExoObject {
    /// First frame, 1-based as stored.
    pub start: i32,
    /// Last frame, inclusive and 1-based as stored.
    pub end: i32,
    /// Layer, 1-based as stored.
    pub layer: i32,
    pub group: Option<i32>,
    pub overlay: bool,
    pub clipping: bool,
    pub camera: Option<bool>,
    pub audio: bool,
    pub chain: bool,
    pub filters: Vec<ExoFilter>,
    pub extra: Vec<(String, String)>,
}

impl ExoObject {
    /// Number of frames the object occupies.
    pub fn duration(&self) -> i32 {
        self.end.saturating_sub(self.start).saturating_add(1).max(0)
    }

    /// The object's own filter, that is `[n.0]`, which decides whether it is text, a shape,
    /// an image, a video, or audio.
    pub fn object_filter(&self) -> Option<&ExoFilter> {
        self.filters.first()
    }
}

/// One `[n.k]` filter section, kept as ordered key/value pairs.
///
/// Values stay textual because ExEdit writes them in several shapes (`track0`, `check0`,
/// `color`, `_exdata`, and per-object names such as `X` or `大小`). Typed accessors parse on
/// demand and never fail the whole document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExoFilter {
    pub entries: Vec<(String, String)>,
}

impl ExoFilter {
    /// Builds a filter from ordered entries.
    pub fn new(entries: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// The localized display name (`_name`).
    pub fn display_name(&self) -> &str {
        self.value("_name").unwrap_or_default()
    }

    /// The script identifier of a script-backed object or filter (`name`).
    pub fn script_name(&self) -> Option<&str> {
        self.value("name").filter(|name| !name.is_empty())
    }

    /// Whether the filter is present but switched off (`_disable`).
    pub fn is_disabled(&self) -> bool {
        self.value("_disable").is_some_and(|value| value != "0")
    }

    /// The script parameter block (`param`).
    pub fn script_parameter(&self) -> Option<&str> {
        self.value("param").filter(|param| !param.is_empty())
    }

    /// Any value by key.
    pub fn value(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|(entry_key, _)| entry_key == key)
            .map(|(_, value)| value.as_str())
    }

    /// The positional trackbar with the given index (`track0`, `track1`, …).
    pub fn track(&self, index: usize) -> Option<TrackValue> {
        TrackValue::parse(self.value(&format!("track{index}"))?)
    }

    /// A named trackbar, as the built-in filters store theirs (`X`, `缩放率`, …).
    pub fn named_track(&self, key: &str) -> Option<TrackValue> {
        TrackValue::parse(self.value(key)?)
    }

    /// A positional boolean (`check0`, `check1`, …).
    pub fn check(&self, index: usize) -> bool {
        self.value(&format!("check{index}"))
            .and_then(|value| value.trim().parse::<i32>().ok())
            .is_some_and(|value| value != 0)
    }

    /// A named boolean stored as `0`/`1`, such as the text object's `B` or `I`.
    pub fn named_flag(&self, key: &str) -> Option<bool> {
        self.value(key)
            .and_then(|value| value.trim().parse::<i32>().ok())
            .map(|value| value != 0)
    }

    /// A named integer, such as the text object's `align` or `大小`.
    pub fn named_integer(&self, key: &str) -> Option<i32> {
        self.value(key)?.trim().parse().ok()
    }

    /// A colour parameter, stored as six hex digits (`color`, `color2`, …).
    pub fn color(&self, key: &str) -> Option<u32> {
        u32::from_str_radix(self.value(key)?.trim(), 16).ok()
    }

    /// A hex-encoded binary payload (`_exdata`, or a text object's `text`).
    ///
    /// Anything that is not an even run of hex digits is rejected rather than sliced apart.
    /// The value is read from the file, so a hand-edited one can hold a multi-byte character
    /// where a byte pair was expected; cutting a character in half used to panic the import.
    pub fn hex_payload(&self, key: &str) -> Option<Vec<u8>> {
        let value = self.value(key)?.trim();
        let digits = value.as_bytes();
        if digits.len() % 2 != 0 {
            return None;
        }
        digits.chunks_exact(2).map(hex_byte).collect()
    }

    /// The object's text content, which ExEdit stores as hex-encoded UTF-16LE.
    ///
    /// The buffer is fixed-size and NUL-padded, so trailing NULs are trimmed.
    pub fn utf16_text(&self, key: &str) -> Option<String> {
        let bytes = self.hex_payload(key)?;
        if bytes.len() % 2 != 0 {
            return None;
        }
        let units = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect::<Vec<_>>();
        Some(String::from_utf16_lossy(&units))
    }

    /// Sets a key, keeping the existing position when the key is already present.
    pub fn set(&mut self, key: &str, value: impl Into<String>) {
        let value = value.into();
        match self.entries.iter_mut().find(|(entry, _)| entry == key) {
            Some((_, existing)) => *existing = value,
            None => self.entries.push((key.to_owned(), value)),
        }
    }

    /// Removes a key, returning its value.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        let index = self.entries.iter().position(|(entry, _)| entry == key)?;
        Some(self.entries.remove(index).1)
    }
}

/// A parsed object file.
#[derive(Debug, Clone, PartialEq)]
pub struct ExoFile {
    /// Encoding the document was read with; writing reuses it.
    pub encoding: TextEncoding,
    pub header: ExoHeader,
    pub objects: Vec<ExoObject>,
}

/// One byte written as two hex digits, or `None` when the pair is not hex.
fn hex_byte(pair: &[u8]) -> Option<u8> {
    Some(hex_digit(pair[0])? << 4 | hex_digit(pair[1])?)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

impl ExoFile {
    /// Reads an object file, detecting its text encoding.
    pub fn parse(bytes: &[u8]) -> Result<Self, ExoError> {
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
        let encoding = TextEncoding::detect(bytes);
        let text = encoding.decode(bytes);
        let mut parser = Parser::new(encoding);
        parser.parse(&text)?;
        parser.finish()
    }

    /// Writes the document back out, using the encoding it was read with.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut text = String::new();
        text.push_str("[exedit]\r\n");
        push_entry(&mut text, "width", &self.header.width.to_string());
        push_entry(&mut text, "height", &self.header.height.to_string());
        push_entry(&mut text, "rate", &format_rate(self.header.rate));
        push_entry(&mut text, "scale", &self.header.scale.to_string());
        push_entry(&mut text, "length", &self.header.length.to_string());
        push_entry(&mut text, "audio_rate", &self.header.audio_rate.to_string());
        push_entry(
            &mut text,
            "audio_ch",
            &self.header.audio_channels.to_string(),
        );
        if self.header.alpha {
            push_entry(&mut text, "alpha", "1");
        }
        if let Some(name) = &self.header.name {
            push_entry(&mut text, "name", name);
        }
        for (key, value) in &self.header.extra {
            push_entry(&mut text, key, value);
        }

        for (index, object) in self.objects.iter().enumerate() {
            text.push_str(&format!("[{index}]\r\n"));
            push_entry(&mut text, "start", &object.start.to_string());
            push_entry(&mut text, "end", &object.end.to_string());
            push_entry(&mut text, "layer", &object.layer.to_string());
            if let Some(group) = object.group {
                push_entry(&mut text, "group", &group.to_string());
            }
            if object.overlay {
                push_entry(&mut text, "overlay", "1");
            }
            if object.clipping {
                push_entry(&mut text, "clipping", "1");
            }
            if let Some(camera) = object.camera {
                push_entry(&mut text, "camera", if camera { "1" } else { "0" });
            }
            if object.audio {
                push_entry(&mut text, "audio", "1");
            }
            if object.chain {
                push_entry(&mut text, "chain", "1");
            }
            for (key, value) in &object.extra {
                push_entry(&mut text, key, value);
            }
            for (filter_index, filter) in object.filters.iter().enumerate() {
                text.push_str(&format!("[{index}.{filter_index}]\r\n"));
                for (key, value) in &filter.entries {
                    push_entry(&mut text, key, value);
                }
            }
        }
        self.encoding.encode(&text)
    }
}

fn push_entry(text: &mut String, key: &str, value: &str) {
    text.push_str(key);
    text.push('=');
    text.push_str(value);
    text.push_str("\r\n");
}

fn format_rate(value: f64) -> String {
    let integer = value.trunc();
    if (value - integer).abs() < 1e-9 {
        format!("{integer:.0}")
    } else {
        let mut text = format!("{value:.6}");
        while text.ends_with('0') {
            text.pop();
        }
        text.trim_end_matches('.').to_owned()
    }
}

struct Parser {
    encoding: TextEncoding,
    header: Option<ExoHeader>,
    objects: Vec<ExoObject>,
    current_object: Option<usize>,
}

impl Parser {
    fn new(encoding: TextEncoding) -> Self {
        Self {
            encoding,
            header: None,
            objects: Vec::new(),
            current_object: None,
        }
    }

    fn parse(&mut self, text: &str) -> Result<(), ExoError> {
        for (index, raw) in text.lines().enumerate() {
            let line_number = index + 1;
            if raw.len() > MAX_VALUE_BYTES {
                return Err(ExoError::OversizedLine { line: line_number });
            }
            let line = raw.trim_end_matches('\r').trim();
            if line.is_empty() {
                continue;
            }
            if let Some(section) = line
                .strip_prefix('[')
                .and_then(|rest| rest.strip_suffix(']'))
            {
                self.begin_section(section.trim(), line_number)?;
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(ExoError::MalformedLine { line: line_number });
            };
            self.entry(key.trim(), value, line_number)?;
        }
        Ok(())
    }

    fn begin_section(&mut self, section: &str, line: usize) -> Result<(), ExoError> {
        if section.eq_ignore_ascii_case("exedit") {
            self.header.get_or_insert_with(ExoHeader::default);
            self.current_object = None;
            return Ok(());
        }
        match section.split_once('.') {
            Some((object, filter)) => {
                let object_index: usize = object
                    .parse()
                    .map_err(|_| ExoError::MalformedSection { line })?;
                filter
                    .parse::<usize>()
                    .map_err(|_| ExoError::MalformedSection { line })?;
                let Some(target) = self.objects.get_mut(object_index) else {
                    return Err(ExoError::FilterWithoutObject { line });
                };
                if target.filters.len() >= MAX_ENTRIES_PER_SECTION {
                    return Err(ExoError::TooManyEntries { line });
                }
                target.filters.push(ExoFilter::default());
                self.current_object = Some(object_index);
                Ok(())
            }
            None => {
                let object_index: usize = section
                    .parse()
                    .map_err(|_| ExoError::MalformedSection { line })?;
                if self.objects.len() >= MAX_OBJECTS {
                    return Err(ExoError::TooManyObjects);
                }
                if object_index != self.objects.len() {
                    // ExEdit writes objects in order; refuse gaps and duplicates instead of
                    // silently rewriting indices, which would corrupt a hand-edited file.
                    return Err(ExoError::MalformedSection { line });
                }
                if self.header.is_none() {
                    return Err(ExoError::NotAnObjectFile);
                }
                self.objects.push(ExoObject {
                    start: 1,
                    end: 1,
                    layer: 1,
                    group: None,
                    overlay: false,
                    clipping: false,
                    camera: None,
                    audio: false,
                    chain: false,
                    filters: Vec::new(),
                    extra: Vec::new(),
                });
                self.current_object = Some(self.objects.len() - 1);
                Ok(())
            }
        }
    }

    fn entry(&mut self, key: &str, value: &str, line: usize) -> Result<(), ExoError> {
        let value = value.trim();
        let Some(object_index) = self.current_object else {
            let Some(header) = self.header.as_mut() else {
                return Err(ExoError::NotAnObjectFile);
            };
            if header.extra.len() >= MAX_ENTRIES_PER_SECTION {
                return Err(ExoError::TooManyEntries { line });
            }
            match key {
                "width" => header.width = parse_integer(value, header.width),
                "height" => header.height = parse_integer(value, header.height),
                "rate" => header.rate = parse_rate(value, header.rate),
                "scale" => header.scale = parse_integer(value, header.scale),
                "length" => header.length = parse_integer(value, header.length),
                "audio_rate" => header.audio_rate = parse_integer(value, header.audio_rate),
                "audio_ch" => header.audio_channels = parse_integer(value, header.audio_channels),
                "alpha" => header.alpha = value != "0",
                "name" => header.name = Some(value.to_owned()),
                _ => header.extra.push((key.to_owned(), value.to_owned())),
            }
            return Ok(());
        };
        let object = &mut self.objects[object_index];
        if object
            .filters
            .last()
            .is_some_and(|filter| filter.entries.len() >= MAX_ENTRIES_PER_SECTION)
        {
            return Err(ExoError::TooManyEntries { line });
        }
        if let Some(filter) = object.filters.last_mut() {
            filter.entries.push((key.to_owned(), value.to_owned()));
            return Ok(());
        }
        if object.extra.len() >= MAX_ENTRIES_PER_SECTION {
            return Err(ExoError::TooManyEntries { line });
        }
        match key {
            "start" => object.start = parse_integer(value, object.start),
            "end" => object.end = parse_integer(value, object.end),
            "layer" => object.layer = parse_integer(value, object.layer),
            "group" => object.group = value.parse().ok(),
            "overlay" => object.overlay = value != "0",
            "clipping" => object.clipping = value != "0",
            "camera" => object.camera = Some(value != "0"),
            "audio" => object.audio = value != "0",
            "chain" => object.chain = value != "0",
            _ => object.extra.push((key.to_owned(), value.to_owned())),
        }
        Ok(())
    }

    fn finish(self) -> Result<ExoFile, ExoError> {
        let Some(mut header) = self.header else {
            return Err(ExoError::NotAnObjectFile);
        };
        // A missing or nonsensical header key must still produce a usable scene, so it falls
        // back to the same defaults a new project starts from.
        header.width = bounded(header.width, 32_768, ExoHeader::default().width);
        header.height = bounded(header.height, 32_768, ExoHeader::default().height);
        header.scale = bounded(header.scale, 1000, ExoHeader::default().scale);
        header.audio_rate = bounded(header.audio_rate, 192_000, ExoHeader::default().audio_rate);
        header.audio_channels = bounded(
            header.audio_channels,
            8,
            ExoHeader::default().audio_channels,
        );
        if !header.rate.is_finite() || header.rate <= 0.0 || header.rate > 1000.0 {
            header.rate = ExoHeader::default().rate;
        }
        Ok(ExoFile {
            encoding: self.encoding,
            header,
            objects: self.objects,
        })
    }
}

fn bounded(value: i32, maximum: i32, fallback: i32) -> i32 {
    if (1..=maximum).contains(&value) {
        value
    } else {
        fallback
    }
}

fn parse_integer(value: &str, fallback: i32) -> i32 {
    value.trim().parse().unwrap_or(fallback)
}

fn parse_rate(value: &str, fallback: f64) -> f64 {
    // AviUtl also accepts "30000/1001" for fractional rates.
    let value = value.trim();
    let parsed = match value.split_once('/') {
        Some((numerator, denominator)) => numerator
            .trim()
            .parse::<f64>()
            .ok()
            .zip(denominator.trim().parse::<f64>().ok())
            .and_then(|(numerator, denominator)| {
                (denominator != 0.0 && denominator.is_finite()).then_some(numerator / denominator)
            }),
        None => value.parse::<f64>().ok(),
    };
    parsed.filter(|rate| rate.is_finite()).unwrap_or(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The first two objects of `script/[Undofish]/exo/uf_easing_obj.exo`, byte for byte,
    /// with the Chinese filter name kept as GBK.
    fn sample_gbk() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"[exedit]\r\nwidth=864\r\nheight=486\r\nrate=30\r\n");
        bytes.extend_from_slice(b"scale=1\r\nlength=30\r\naudio_rate=44100\r\naudio_ch=2\r\n");
        bytes
            .extend_from_slice(b"[0]\r\nstart=1\r\nend=30\r\nlayer=1\r\noverlay=1\r\ncamera=0\r\n");
        bytes.extend_from_slice(b"[0.0]\r\n_name=");
        bytes.extend_from_slice(&[0xd7, 0xd4, 0xb6, 0xa8, 0xd2, 0xe5, 0xce, 0xef, 0xbc, 0xfe]);
        bytes.extend_from_slice(
            b"\r\ntrack0=0.00\r\ntrack1=0.00\r\ntrack2=25.00\r\ntrack3=3.00\r\n",
        );
        bytes.extend_from_slice(b"check0=100\r\ntype=0\r\nfilter=2\r\nname=uf_easing\r\n");
        bytes.extend_from_slice(b"param=local P_VIEW=1;\r\n");
        bytes
    }

    #[test]
    fn a_non_hex_payload_is_refused_instead_of_sliced() {
        // `_exdata=ああ` is six bytes, so the old length check passed and the pair slicing
        // cut a three-byte character in half. A hand-edited file could crash the import.
        let filter = ExoFilter::new([
            ("_name".to_owned(), "テキスト".to_owned()),
            ("_exdata".to_owned(), "ああ".to_owned()),
            ("text".to_owned(), "4100zz4100".to_owned()),
        ]);
        assert_eq!(filter.hex_payload("_exdata"), None);
        assert_eq!(filter.hex_payload("text"), None);
        assert_eq!(filter.utf16_text("text"), None);

        let good = ExoFilter::new([("text".to_owned(), "41004200".to_owned())]);
        assert_eq!(good.hex_payload("text"), Some(vec![0x41, 0x00, 0x42, 0x00]));
        assert_eq!(good.utf16_text("text").as_deref(), Some("AB"));
    }

    #[test]
    fn parses_the_header_object_and_filter() {
        let file = ExoFile::parse(&sample_gbk()).unwrap();
        assert_eq!(file.encoding, TextEncoding::Gbk);
        assert_eq!(file.header.width, 864);
        assert_eq!(file.header.height, 486);
        assert_eq!(file.header.rate, 30.0);
        assert_eq!(file.header.length, 30);
        assert_eq!(file.header.audio_rate, 44_100);
        assert_eq!(file.objects.len(), 1);

        let object = &file.objects[0];
        assert_eq!(object.start, 1);
        assert_eq!(object.end, 30);
        assert_eq!(object.duration(), 30);
        assert_eq!(object.layer, 1);
        assert!(object.overlay);
        assert_eq!(object.camera, Some(false));

        let filter = object.object_filter().unwrap();
        assert_eq!(filter.display_name(), "自定义物件");
        assert_eq!(filter.script_name(), Some("uf_easing"));
        assert_eq!(filter.track(2).unwrap().start, 25.0);
        assert!(filter.check(0));
        assert_eq!(filter.script_parameter(), Some("local P_VIEW=1;"));
    }

    #[test]
    fn writing_reproduces_a_parsed_file() {
        let original = sample_gbk();
        let file = ExoFile::parse(&original).unwrap();
        assert_eq!(file.to_bytes(), original);
    }

    #[test]
    fn keeps_unknown_keys_in_order() {
        let input = b"[exedit]\r\nwidth=640\r\nheight=480\r\nrate=30\r\nscale=100\r\nlength=5\r\naudio_rate=44100\r\naudio_ch=2\r\nfuture_key=42\r\n[0]\r\nstart=1\r\nend=5\r\nlayer=2\r\n[0.0]\r\n_name=x\r\nodd=keeps=equals\r\n";
        let file = ExoFile::parse(input).unwrap();
        assert_eq!(
            file.header.extra,
            vec![("future_key".to_owned(), "42".to_owned())]
        );
        assert_eq!(file.to_bytes(), input);
    }

    #[test]
    fn rejects_documents_that_are_not_object_files() {
        assert_eq!(
            ExoFile::parse(b"width=100").unwrap_err(),
            ExoError::NotAnObjectFile
        );
        assert_eq!(
            ExoFile::parse(b"[0]\r\nstart=1\r\n").unwrap_err(),
            ExoError::NotAnObjectFile
        );
        assert!(matches!(
            ExoFile::parse(b"[exedit]\r\nnonsense\r\n").unwrap_err(),
            ExoError::MalformedLine { line: 2 }
        ));
        assert!(matches!(
            ExoFile::parse(b"[exedit]\r\n[x]\r\n").unwrap_err(),
            ExoError::MalformedSection { line: 2 }
        ));
        assert!(matches!(
            ExoFile::parse(b"[exedit]\r\n[0.0]\r\n_name=x\r\n").unwrap_err(),
            ExoError::FilterWithoutObject { line: 2 }
        ));
    }

    #[test]
    fn decodes_and_encodes_text_objects() {
        let mut filter = ExoFilter::default();
        filter.set("_name", "文本");
        filter.set("font", "黑体");
        filter.set("text", "137fa852");
        assert_eq!(filter.utf16_text("text").as_deref(), Some("缓动"));
        assert_eq!(filter.value("font"), Some("黑体"));
        assert_eq!(filter.color("color"), None);
        filter.set("color", "ffffff");
        assert_eq!(filter.color("color"), Some(0xff_ff_ff));
    }

    #[test]
    fn trims_nul_padded_text_buffers() {
        let mut filter = ExoFilter::default();
        // "A" followed by two NUL units, padded as a fixed-size buffer is.
        filter.set("text", "4100000000000000".to_owned());
        assert_eq!(filter.utf16_text("text").as_deref(), Some("A"));
    }

    #[test]
    fn accepts_fractional_rates_and_bounds_the_header() {
        let file =
            ExoFile::parse(b"[exedit]\r\nwidth=0\r\nheight=99999\r\nrate=30000/1001\r\n").unwrap();
        assert_eq!(file.header.width, 1920);
        assert_eq!(file.header.height, 1080);
        assert!((file.header.rate - 29.970_029).abs() < 1e-4);
    }

    #[test]
    fn object_and_filter_accessors_report_missing_keys() {
        let object = ExoObject {
            start: 10,
            end: 19,
            layer: 3,
            group: None,
            overlay: false,
            clipping: false,
            camera: None,
            audio: false,
            chain: false,
            filters: vec![ExoFilter::default()],
            extra: Vec::new(),
        };
        assert_eq!(object.duration(), 10);
        let filter = &object.filters[0];
        assert_eq!(filter.display_name(), "");
        assert!(filter.track(0).is_none());
        assert!(!filter.check(0));
        assert_eq!(filter.utf16_text("text"), None);
    }
}
