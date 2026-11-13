//! # pixcore — the Pixel Studio core engine
//!
//! Platform-independent, deterministic and dependency-free (Rust `std` only).
//! Android and Windows hosts load it through JNI (`ffi`); every pixel change from a human
//! or an AI agent runs through the same [`engine::Engine::execute`].

pub mod animation {
    //! Frames, per-frame timing, animation tags and playback order.
    //!
    //! There is no fixed frame count: frames live in a growable list limited only by memory.

    pub const DEFAULT_FRAME_MS: u32 = 100;
    pub const MIN_FRAME_MS: u32 = 1;
    pub const MAX_FRAME_MS: u32 = 60_000;

    #[derive(Debug, Clone, PartialEq)]
    pub struct Frame {
        /// Stable identifier; survives reordering.
        pub id: u32,
        pub duration_ms: u32,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Direction {
        Forward,
        Reverse,
        PingPong,
    }

    impl Direction {
        pub fn parse(s: &str) -> Option<Direction> {
            match s.to_ascii_lowercase().replace(['-', '_'], "").as_str() {
                "forward" | "loop" => Some(Direction::Forward),
                "reverse" | "backward" => Some(Direction::Reverse),
                "pingpong" => Some(Direction::PingPong),
                _ => None,
            }
        }

        pub fn as_str(self) -> &'static str {
            match self {
                Direction::Forward => "forward",
                Direction::Reverse => "reverse",
                Direction::PingPong => "pingpong",
            }
        }

        pub fn code(self) -> u8 {
            match self {
                Direction::Forward => 0,
                Direction::Reverse => 1,
                Direction::PingPong => 2,
            }
        }

        pub fn from_code(c: u8) -> Option<Direction> {
            match c {
                0 => Some(Direction::Forward),
                1 => Some(Direction::Reverse),
                2 => Some(Direction::PingPong),
                _ => None,
            }
        }
    }

    /// A named frame range such as "Idle", "Walk" or "Attack".
    /// `from`/`to` are frame ids, so tags follow frames when they are reordered.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Tag {
        pub name: String,
        pub from_id: u32,
        pub to_id: u32,
        pub direction: Direction,
        /// 0 = loop forever.
        pub repeat: u32,
    }

    /// Frame order of one playback cycle for a frame index range.
    pub fn playback_order(from: usize, to: usize, direction: Direction) -> Vec<usize> {
        let (lo, hi) = (from.min(to), from.max(to));
        let forward: Vec<usize> = (lo..=hi).collect();
        match direction {
            Direction::Forward => forward,
            Direction::Reverse => forward.into_iter().rev().collect(),
            Direction::PingPong => {
                let mut seq = forward.clone();
                if hi > lo + 1 {
                    seq.extend((lo + 1..hi).rev());
                } else if hi == lo + 1 {
                    // two frames: a b (no duplicated end points)
                }
                seq
            }
        }
    }

    /// Timing summary for a playback sequence.
    pub struct Timing {
        pub total_ms: u64,
        pub average_fps: f64,
    }

    pub fn timing(durations: &[u32]) -> Timing {
        let total: u64 = durations.iter().map(|&d| d as u64).sum();
        let average_fps = if total == 0 { 0.0 } else { durations.len() as f64 * 1000.0 / total as f64 };
        Timing { total_ms: total, average_fps }
    }

    pub fn validate_duration(ms: i64) -> Result<u32, String> {
        if ms < MIN_FRAME_MS as i64 || ms > MAX_FRAME_MS as i64 {
            return Err(format!("frame duration must be between {MIN_FRAME_MS} and {MAX_FRAME_MS} ms"));
        }
        Ok(ms as u32)
    }
}
pub mod commands {
    //! The command protocol: the single entry point used by the UI, MCP, REST and tests.
    //!
    //! Request (UTF-8 JSON):
    //! ```json
    //! {"op":"set_pixel","actor":"claude-agent","operationId":"op_1042","expectedRevision":125,
    //!  "args":{"x":12,"y":18,"color":"#D94B42","frame":4}}
    //! ```
    //! Arguments may also be given flat next to `op`. Response:
    //! `{"ok":true,"op":"set_pixel","revision":126,"result":{...}}` or
    //! `{"ok":false,"op":"set_pixel","revision":125,"error":{"code":"conflict","message":"..."}}`.

    pub mod json {
        //! Minimal, dependency-free JSON implementation used by the command protocol.
        //!
        //! Objects keep insertion order so responses are deterministic and readable.

        use std::fmt::Write as _;

        const MAX_DEPTH: usize = 128;

        #[derive(Debug, Clone, PartialEq)]
        pub enum Json {
            Null,
            Bool(bool),
            Num(f64),
            Str(String),
            Arr(Vec<Json>),
            Obj(Vec<(String, Json)>),
        }

        impl Json {
            pub fn obj() -> Json {
                Json::Obj(Vec::new())
            }

            /// Builder-style insert (replaces an existing key).
            pub fn with(mut self, key: &str, value: impl Into<Json>) -> Json {
                self.set(key, value);
                self
            }

            pub fn set(&mut self, key: &str, value: impl Into<Json>) {
                if let Json::Obj(entries) = self {
                    let value = value.into();
                    if let Some(slot) = entries.iter_mut().find(|(k, _)| k == key) {
                        slot.1 = value;
                    } else {
                        entries.push((key.to_string(), value));
                    }
                }
            }

            pub fn get(&self, key: &str) -> Option<&Json> {
                match self {
                    Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
                    _ => None,
                }
            }

            pub fn as_str(&self) -> Option<&str> {
                match self {
                    Json::Str(s) => Some(s),
                    _ => None,
                }
            }

            pub fn as_f64(&self) -> Option<f64> {
                match self {
                    Json::Num(n) => Some(*n),
                    _ => None,
                }
            }

            /// Integer view: only accepts numbers that are exactly integral.
            pub fn as_i64(&self) -> Option<i64> {
                match self {
                    Json::Num(n) if n.fract() == 0.0 && n.abs() <= 9.007_199_254_740_992e15 => Some(*n as i64),
                    _ => None,
                }
            }

            pub fn as_bool(&self) -> Option<bool> {
                match self {
                    Json::Bool(b) => Some(*b),
                    _ => None,
                }
            }

            pub fn as_array(&self) -> Option<&[Json]> {
                match self {
                    Json::Arr(items) => Some(items),
                    _ => None,
                }
            }

            pub fn is_null(&self) -> bool {
                matches!(self, Json::Null)
            }

            pub fn to_json_string(&self) -> String {
                let mut out = String::with_capacity(128);
                write_value(self, &mut out);
                out
            }

            pub fn parse(text: &str) -> Result<Json, String> {
                let mut p = Parser { bytes: text.as_bytes(), pos: 0 };
                p.skip_ws();
                let value = p.value(0)?;
                p.skip_ws();
                if p.pos != p.bytes.len() {
                    return Err(format!("unexpected trailing data at byte {}", p.pos));
                }
                Ok(value)
            }
        }

        impl From<bool> for Json {
            fn from(v: bool) -> Self {
                Json::Bool(v)
            }
        }
        impl From<&str> for Json {
            fn from(v: &str) -> Self {
                Json::Str(v.to_string())
            }
        }
        impl From<String> for Json {
            fn from(v: String) -> Self {
                Json::Str(v)
            }
        }
        impl From<f64> for Json {
            fn from(v: f64) -> Self {
                Json::Num(v)
            }
        }
        macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Json {
            fn from(v: $t) -> Self { Json::Num(v as f64) }
        }
    )*};
}
        from_int!(i32, i64, u8, u16, u32, u64, usize);
        impl From<Vec<Json>> for Json {
            fn from(v: Vec<Json>) -> Self {
                Json::Arr(v)
            }
        }
        impl<T: Into<Json>> From<Option<T>> for Json {
            fn from(v: Option<T>) -> Self {
                match v {
                    Some(v) => v.into(),
                    None => Json::Null,
                }
            }
        }

        fn write_value(v: &Json, out: &mut String) {
            match v {
                Json::Null => out.push_str("null"),
                Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                Json::Num(n) => write_number(*n, out),
                Json::Str(s) => write_string(s, out),
                Json::Arr(items) => {
                    out.push('[');
                    for (i, item) in items.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        write_value(item, out);
                    }
                    out.push(']');
                }
                Json::Obj(entries) => {
                    out.push('{');
                    for (i, (k, item)) in entries.iter().enumerate() {
                        if i > 0 {
                            out.push(',');
                        }
                        write_string(k, out);
                        out.push(':');
                        write_value(item, out);
                    }
                    out.push('}');
                }
            }
        }

        fn write_number(n: f64, out: &mut String) {
            if !n.is_finite() {
                out.push_str("null");
            } else if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
                let _ = write!(out, "{}", n as i64);
            } else {
                let _ = write!(out, "{}", n);
            }
        }

        fn write_string(s: &str, out: &mut String) {
            out.push('"');
            for c in s.chars() {
                match c {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    '\t' => out.push_str("\\t"),
                    c if (c as u32) < 0x20 => {
                        let _ = write!(out, "\\u{:04x}", c as u32);
                    }
                    c => out.push(c),
                }
            }
            out.push('"');
        }

        struct Parser<'a> {
            bytes: &'a [u8],
            pos: usize,
        }

        impl Parser<'_> {
            fn skip_ws(&mut self) {
                while let Some(b) = self.bytes.get(self.pos) {
                    if matches!(b, b' ' | b'\t' | b'\n' | b'\r') {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
            }

            fn err<T>(&self, msg: &str) -> Result<T, String> {
                Err(format!("{} at byte {}", msg, self.pos))
            }

            fn expect_literal(&mut self, lit: &str, value: Json) -> Result<Json, String> {
                if self.bytes[self.pos..].starts_with(lit.as_bytes()) {
                    self.pos += lit.len();
                    Ok(value)
                } else {
                    self.err("invalid literal")
                }
            }

            fn value(&mut self, depth: usize) -> Result<Json, String> {
                if depth > MAX_DEPTH {
                    return self.err("nesting too deep");
                }
                match self.bytes.get(self.pos) {
                    None => self.err("unexpected end of input"),
                    Some(b'n') => self.expect_literal("null", Json::Null),
                    Some(b't') => self.expect_literal("true", Json::Bool(true)),
                    Some(b'f') => self.expect_literal("false", Json::Bool(false)),
                    Some(b'"') => Ok(Json::Str(self.string()?)),
                    Some(b'[') => {
                        self.pos += 1;
                        let mut items = Vec::new();
                        self.skip_ws();
                        if self.bytes.get(self.pos) == Some(&b']') {
                            self.pos += 1;
                            return Ok(Json::Arr(items));
                        }
                        loop {
                            self.skip_ws();
                            items.push(self.value(depth + 1)?);
                            self.skip_ws();
                            match self.bytes.get(self.pos) {
                                Some(b',') => self.pos += 1,
                                Some(b']') => {
                                    self.pos += 1;
                                    return Ok(Json::Arr(items));
                                }
                                _ => return self.err("expected ',' or ']'"),
                            }
                        }
                    }
                    Some(b'{') => {
                        self.pos += 1;
                        let mut entries: Vec<(String, Json)> = Vec::new();
                        self.skip_ws();
                        if self.bytes.get(self.pos) == Some(&b'}') {
                            self.pos += 1;
                            return Ok(Json::Obj(entries));
                        }
                        loop {
                            self.skip_ws();
                            if self.bytes.get(self.pos) != Some(&b'"') {
                                return self.err("expected object key");
                            }
                            let key = self.string()?;
                            self.skip_ws();
                            if self.bytes.get(self.pos) != Some(&b':') {
                                return self.err("expected ':'");
                            }
                            self.pos += 1;
                            self.skip_ws();
                            let v = self.value(depth + 1)?;
                            if let Some(slot) = entries.iter_mut().find(|(k, _)| *k == key) {
                                slot.1 = v;
                            } else {
                                entries.push((key, v));
                            }
                            self.skip_ws();
                            match self.bytes.get(self.pos) {
                                Some(b',') => self.pos += 1,
                                Some(b'}') => {
                                    self.pos += 1;
                                    return Ok(Json::Obj(entries));
                                }
                                _ => return self.err("expected ',' or '}'"),
                            }
                        }
                    }
                    Some(b'-' | b'0'..=b'9') => self.number(),
                    Some(_) => self.err("unexpected character"),
                }
            }

            fn number(&mut self) -> Result<Json, String> {
                let start = self.pos;
                if self.bytes.get(self.pos) == Some(&b'-') {
                    self.pos += 1;
                }
                let digits_start = self.pos;
                while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
                if self.pos == digits_start {
                    return self.err("invalid number");
                }
                if self.bytes.get(self.pos) == Some(&b'.') {
                    self.pos += 1;
                    let frac_start = self.pos;
                    while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                        self.pos += 1;
                    }
                    if self.pos == frac_start {
                        return self.err("invalid number fraction");
                    }
                }
                if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
                    self.pos += 1;
                    if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                        self.pos += 1;
                    }
                    let exp_start = self.pos;
                    while matches!(self.bytes.get(self.pos), Some(b'0'..=b'9')) {
                        self.pos += 1;
                    }
                    if self.pos == exp_start {
                        return self.err("invalid number exponent");
                    }
                }
                let text = std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|e| e.to_string())?;
                text.parse::<f64>().map(Json::Num).map_err(|e| e.to_string())
            }

            fn hex4(&mut self) -> Result<u32, String> {
                let slice = self.bytes.get(self.pos..self.pos + 4).ok_or("truncated \\u escape")?;
                let text = std::str::from_utf8(slice).map_err(|e| e.to_string())?;
                let v = u32::from_str_radix(text, 16).map_err(|_| "invalid \\u escape".to_string())?;
                self.pos += 4;
                Ok(v)
            }

            fn string(&mut self) -> Result<String, String> {
                self.pos += 1; // opening quote
                let mut out = String::new();
                loop {
                    let start = self.pos;
                    while let Some(&b) = self.bytes.get(self.pos) {
                        if b == b'"' || b == b'\\' || b < 0x20 {
                            break;
                        }
                        self.pos += 1;
                    }
                    out.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| "invalid UTF-8 in string".to_string())?);
                    match self.bytes.get(self.pos) {
                        None => return self.err("unterminated string"),
                        Some(b'"') => {
                            self.pos += 1;
                            return Ok(out);
                        }
                        Some(b'\\') => {
                            self.pos += 1;
                            let esc = *self.bytes.get(self.pos).ok_or("truncated escape")?;
                            self.pos += 1;
                            match esc {
                                b'"' => out.push('"'),
                                b'\\' => out.push('\\'),
                                b'/' => out.push('/'),
                                b'b' => out.push('\u{08}'),
                                b'f' => out.push('\u{0c}'),
                                b'n' => out.push('\n'),
                                b'r' => out.push('\r'),
                                b't' => out.push('\t'),
                                b'u' => {
                                    let hi = self.hex4()?;
                                    let code = if (0xD800..0xDC00).contains(&hi) {
                                        if self.bytes.get(self.pos..self.pos + 2) != Some(b"\\u") {
                                            return self.err("unpaired surrogate");
                                        }
                                        self.pos += 2;
                                        let lo = self.hex4()?;
                                        if !(0xDC00..0xE000).contains(&lo) {
                                            return self.err("invalid low surrogate");
                                        }
                                        0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                                    } else {
                                        hi
                                    };
                                    out.push(char::from_u32(code).ok_or("invalid code point")?);
                                }
                                _ => return self.err("invalid escape"),
                            }
                        }
                        Some(_) => return self.err("control character in string"),
                    }
                }
            }
        }
    }

    mod args {
        //! Argument decoding shared by all commands. Every accessor validates and produces a
        //! precise error message an AI agent can act on.

        use super::json::Json;
        use super::{CmdError, CmdResult};
        use crate::pixel::{Rgba, TRANSPARENT};
        use crate::project::Document;
        use crate::raster::Region;

        pub fn opt_i64(a: &Json, key: &str) -> CmdResult<Option<i64>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(v) => v.as_i64().map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be an integer"))),
            }
        }

        pub fn req_i64(a: &Json, key: &str) -> CmdResult<i64> {
            opt_i64(a, key)?.ok_or_else(|| CmdError::invalid(format!("missing required integer '{key}'")))
        }

        /// Integer coordinates are bounded so arithmetic can never overflow.
        pub fn coord(a: &Json, key: &str) -> CmdResult<i64> {
            let v = req_i64(a, key)?;
            if v.abs() > 1_000_000 {
                return Err(CmdError::invalid(format!("'{key}' is out of range")));
            }
            Ok(v)
        }

        pub fn opt_bool(a: &Json, key: &str) -> CmdResult<Option<bool>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(v) => v.as_bool().map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be a boolean"))),
            }
        }

        pub fn opt_str<'a>(a: &'a Json, key: &str) -> CmdResult<Option<&'a str>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(v) => v.as_str().map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be a string"))),
            }
        }

        pub fn req_str<'a>(a: &'a Json, key: &str) -> CmdResult<&'a str> {
            opt_str(a, key)?.ok_or_else(|| CmdError::invalid(format!("missing required string '{key}'")))
        }

        /// Layer by index (0 = bottom) or by name; defaults to the active layer.
        pub fn layer_index(doc: &Document, a: &Json, key: &str) -> CmdResult<usize> {
            match a.get(key) {
                None | Some(Json::Null) => doc.layer_index(doc.active_layer).ok_or_else(|| CmdError::not_found("no active layer")),
                Some(Json::Str(name)) => doc
                    .layers
                    .iter()
                    .position(|l| l.name == *name)
                    .or_else(|| doc.layers.iter().position(|l| l.name.eq_ignore_ascii_case(name)))
                    // Function-calling adapters that only support string parameters send indices as text.
                    .or_else(|| name.trim().parse::<usize>().ok().filter(|i| *i < doc.layers.len()))
                    .ok_or_else(|| CmdError::not_found(format!("no layer named '{name}'"))),
                Some(v) => {
                    let i = v.as_i64().ok_or_else(|| CmdError::invalid(format!("'{key}' must be a layer index or name")))?;
                    if i < 0 || i as usize >= doc.layers.len() {
                        return Err(CmdError::not_found(format!("layer index {i} out of range (0..{})", doc.layers.len())));
                    }
                    Ok(i as usize)
                }
            }
        }

        /// Frame by 0-based index; defaults to the active frame.
        pub fn frame_index(doc: &Document, a: &Json, key: &str) -> CmdResult<usize> {
            match opt_i64(a, key)? {
                None => doc.frame_index(doc.active_frame).ok_or_else(|| CmdError::not_found("no active frame")),
                Some(i) if i < 0 || i as usize >= doc.frames.len() => {
                    Err(CmdError::not_found(format!("frame index {i} out of range (0..{})", doc.frames.len())))
                }
                Some(i) => Ok(i as usize),
            }
        }

        /// Color as `#RRGGBB[AA]` / `transparent` / null (any of 2^32 RGBA values) or a swatch index.
        /// Returns the canvas pixel value.
        pub fn color_value(doc: &Document, v: &Json, key: &str) -> CmdResult<u32> {
            match v {
                Json::Str(s) => {
                    let c = Rgba::parse_hex(s)
                        .ok_or_else(|| CmdError::invalid(format!("'{key}': '{s}' is not a #RRGGBB or #RRGGBBAA color")))?;
                    Ok(c.to_pixel())
                }
                Json::Null => Ok(TRANSPARENT),
                v => {
                    let i = v.as_i64().ok_or_else(|| CmdError::invalid(format!("'{key}' must be a hex color string or swatch index")))?;
                    if i < 0 || i as usize >= doc.palette.len() {
                        return Err(CmdError::not_found(format!("swatch {i} does not exist (palette has {} swatches)", doc.palette.len())));
                    }
                    Ok(doc.palette.get(i as usize).map(Rgba::to_pixel).unwrap_or(TRANSPARENT))
                }
            }
        }

        pub fn color(doc: &Document, a: &Json, key: &str) -> CmdResult<u32> {
            let v = a.get(key).ok_or_else(|| CmdError::invalid(format!("missing required color '{key}'")))?;
            color_value(doc, v, key)
        }

        /// Optional 3-component vector `[x, y, z]`.
        pub fn opt_vec3(a: &Json, key: &str) -> CmdResult<Option<[f64; 3]>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(Json::Arr(v)) if v.len() == 3 => {
                    let mut out = [0.0; 3];
                    for (i, x) in v.iter().enumerate() {
                        let f = x
                            .as_f64()
                            .filter(|f| f.is_finite() && f.abs() <= 1.0e6)
                            .ok_or_else(|| CmdError::invalid(format!("'{key}' must be [x, y, z] numbers")))?;
                        out[i] = f;
                    }
                    Ok(Some(out))
                }
                Some(_) => Err(CmdError::invalid(format!("'{key}' must be [x, y, z]"))),
            }
        }

        pub fn req_vec3(a: &Json, key: &str) -> CmdResult<[f64; 3]> {
            opt_vec3(a, key)?.ok_or_else(|| CmdError::invalid(format!("missing required [x, y, z] '{key}'")))
        }

        pub fn opt_f64(a: &Json, key: &str) -> CmdResult<Option<f64>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(v) => {
                    v.as_f64().filter(|f| f.is_finite()).map(Some).ok_or_else(|| CmdError::invalid(format!("'{key}' must be a number")))
                }
            }
        }

        /// Optional x/y/width/height; defaults to the whole canvas.
        pub fn region(doc: &Document, a: &Json) -> CmdResult<Region> {
            let x = opt_i64(a, "x")?.unwrap_or(0);
            let y = opt_i64(a, "y")?.unwrap_or(0);
            let w = opt_i64(a, "width")?.unwrap_or(doc.width as i64 - x);
            let h = opt_i64(a, "height")?.unwrap_or(doc.height as i64 - y);
            if w <= 0 || h <= 0 {
                return Err(CmdError::invalid("region width and height must be positive"));
            }
            for v in [x, y, w, h] {
                if v.abs() > 1_000_000 {
                    return Err(CmdError::invalid("region is out of range"));
                }
            }
            Ok(Region { x, y, w, h })
        }

        pub fn scale(a: &Json, default: u32) -> CmdResult<u32> {
            match opt_i64(a, "scale")? {
                None => Ok(default),
                Some(s) if (1..=64).contains(&s) => Ok(s as u32),
                Some(_) => Err(CmdError::invalid("'scale' must be between 1 and 64")),
            }
        }

        /// Resolves the target cel for drawing and refuses locked layers.
        pub fn target(doc: &Document, a: &Json) -> CmdResult<(u32, u32)> {
            let li = layer_index(doc, a, "layer")?;
            let fi = frame_index(doc, a, "frame")?;
            let layer = &doc.layers[li];
            if layer.locked {
                return Err(CmdError::new("locked", format!("layer '{}' is locked", layer.name)));
            }
            Ok((layer.id, doc.frames[fi].id))
        }
    }
    mod artist {
        //! Hand-pixeling commands for detailed (64-256 px) characters: material ramps and a
        //! ramp-stepping shade brush, pixel-perfect freehand strokes, per-layer pixel density and a
        //! reference importer that recovers the true pixel grid of upscaled art.

        use std::collections::HashMap;
        use std::sync::Arc;

        use super::args::{color, color_value, layer_index, opt_bool, opt_i64, opt_str, req_str, target};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome};
        use crate::pixel::{PixelGrid, Rgba, TRANSPARENT};
        use crate::project::{Document, Ramp};
        use crate::renderer::Image;

        fn colors_arg(doc: &Document, a: &Json, key: &str) -> CmdResult<Vec<u32>> {
            match a.get(key) {
                Some(Json::Arr(list)) => list.iter().map(|c| color_value(doc, c, key)).collect(),
                _ => Err(CmdError::invalid(format!("'{key}' must be an array of colors"))),
            }
        }

        /// Creates or replaces a material ramp (dark → light), e.g. skin, hair, cloth, metal.
        pub fn ramp_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?.trim().to_string();
            if name.is_empty() || name.len() > 64 {
                return Err(CmdError::invalid("ramp 'name' must be 1-64 characters"));
            }
            let colors = colors_arg(doc, a, "colors")?;
            if !(2..=16).contains(&colors.len()) || colors.contains(&TRANSPARENT) {
                return Err(CmdError::invalid("a ramp holds 2-16 opaque colors, dark to light"));
            }
            for c in &colors {
                let _ = doc.palette.intern(Rgba::from_u32(*c));
            }
            match doc.ramps.iter_mut().find(|r| r.name == name) {
                Some(r) => r.colors = colors,
                None => doc.ramps.push(Ramp { name: name.clone(), colors }),
            }
            Ok(Outcome::new(Json::obj().with("ramp", name).with("ramps", doc.ramps.len()), 0))
        }

        pub fn ramp_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?;
            let before = doc.ramps.len();
            doc.ramps.retain(|r| r.name != name);
            if doc.ramps.len() == before {
                return Err(CmdError::not_found(format!("no ramp '{name}'")));
            }
            Ok(Outcome::new(Json::obj().with("deleted", name), 0))
        }

        /// Where a color sits in the ramps: (ramp index, position).
        pub fn ramp_index(doc: &Document) -> HashMap<u32, (usize, usize)> {
            let mut m = HashMap::new();
            for (ri, r) in doc.ramps.iter().enumerate() {
                for (i, &c) in r.colors.iter().enumerate() {
                    m.entry(c).or_insert((ri, i));
                }
            }
            m
        }

        /// The shade brush: moves each targeted pixel `steps` along its own material ramp
        /// (+1 lighter, -1 darker). Never invents a color — it is how pixel artists shade by hand.
        pub fn shade(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let steps = opt_i64(a, "steps")?.unwrap_or(-1).clamp(-15, 15);
            if doc.ramps.is_empty() {
                return Err(CmdError::invalid("define material ramps first (ramp_set, color_ramp with name, or reference_import)"));
            }
            let mut targets: Vec<(i64, i64)> = Vec::new();
            match a.get("pixels") {
                Some(Json::Arr(list)) => {
                    for p in list {
                        match p.as_array() {
                            Some([x, y, ..]) => targets.push((x.as_i64().unwrap_or(-1), y.as_i64().unwrap_or(-1))),
                            _ => return Err(CmdError::invalid("pixels must be [[x, y], …]")),
                        }
                    }
                }
                _ => {
                    let (x, y) = (super::args::req_i64(a, "x")?, super::args::req_i64(a, "y")?);
                    let (w, h) = (opt_i64(a, "width")?.unwrap_or(1).clamp(1, 4096), opt_i64(a, "height")?.unwrap_or(1).clamp(1, 4096));
                    for yy in y..y + h {
                        for xx in x..x + w {
                            targets.push((xx, yy));
                        }
                    }
                }
            }
            let only = match a.get("only_color") {
                Some(v) if !v.is_null() => Some(color_value(doc, v, "only_color")?),
                _ => None,
            };
            let index = ramp_index(doc);
            let ramps: Vec<Vec<u32>> = doc.ramps.iter().map(|r| r.colors.clone()).collect();
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let (mut changed, mut skipped) = (0u64, 0u64);
            for (x, y) in targets {
                let v = grid.get(x, y);
                if v == TRANSPARENT || only.is_some_and(|o| o != v) {
                    continue;
                }
                let Some(&(ri, i)) = index.get(&(v | 0xFF)) else {
                    skipped += 1;
                    continue;
                };
                let r = &ramps[ri];
                let ni = (i as i64 + steps).clamp(0, r.len() as i64 - 1) as usize;
                let nv = (r[ni] & !0xFF) | (v & 0xFF);
                changed += grid.set(x, y, nv) as u64;
            }
            Ok(Outcome::new(Json::obj().with("changed", changed).with("notInARamp", skipped), changed))
        }

        /// Freehand polyline with "pixel perfect" corners (no L-shaped doubles), 1-4 px wide.
        pub fn draw_stroke(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let pts: Vec<(i64, i64)> = match a.get("points").and_then(Json::as_array) {
                Some(list) if !list.is_empty() && list.len() <= 8192 => list
                    .iter()
                    .map(|p| match p.as_array() {
                        Some([x, y]) => Ok((x.as_f64().unwrap_or(0.0).round() as i64, y.as_f64().unwrap_or(0.0).round() as i64)),
                        _ => Err(CmdError::invalid("points must be [[x, y], …]")),
                    })
                    .collect::<CmdResult<_>>()?,
                _ => return Err(CmdError::invalid("'points' must list 1-8192 [x, y] points")),
            };
            let size = opt_i64(a, "size")?.unwrap_or(1).clamp(1, 4);
            let perfect = opt_bool(a, "pixel_perfect")?.unwrap_or(true);
            let v = color(doc, a, "color")?;
            // Rasterize the polyline into an ordered pixel path.
            let mut path: Vec<(i64, i64)> = vec![pts[0]];
            for w in pts.windows(2) {
                let ((mut x0, mut y0), (x1, y1)) = (w[0], w[1]);
                let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
                let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
                let mut err = dx + dy;
                while (x0, y0) != (x1, y1) {
                    let e2 = 2 * err;
                    if e2 >= dy {
                        err += dy;
                        x0 += sx;
                    }
                    if e2 <= dx {
                        err += dx;
                        y0 += sy;
                    }
                    if path.last() != Some(&(x0, y0)) {
                        path.push((x0, y0));
                    }
                }
            }
            if perfect && size == 1 {
                // Drop the middle pixel of every L: (a, b, c) where a and c touch diagonally.
                let mut out: Vec<(i64, i64)> = Vec::with_capacity(path.len());
                for &p in &path {
                    if out.len() >= 2 {
                        let (a0, b0) = (out[out.len() - 2], out[out.len() - 1]);
                        if (a0.0 - p.0).abs() == 1 && (a0.1 - p.1).abs() == 1 && (b0.0 == a0.0 || b0.1 == a0.1) {
                            out.pop();
                        }
                    }
                    out.push(p);
                }
                path = out;
            }
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            for (x, y) in path {
                for oy in 0..size {
                    for ox in 0..size {
                        changed += grid.set(x + ox - (size - 1) / 2, y + oy - (size - 1) / 2, v) as u64;
                    }
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        /// Changes a layer's density (1, 2 or 4 pixels per canvas pixel), resampling its cels.
        fn set_density(doc: &mut Document, li: usize, d: u8) {
            let old = doc.layers[li].density.max(1) as u32;
            if old == d as u32 {
                return;
            }
            let id = doc.layers[li].id;
            doc.layers[li].density = d;
            let (w, h) = doc.layer_dims(id);
            let keys: Vec<(u32, u32)> = doc.cels.keys().filter(|(l, _)| *l == id).copied().collect();
            for key in keys {
                let src = doc.cels[&key].clone();
                let mut out = PixelGrid::new(w, h);
                let (new, block) = (d as u32, (old / d as u32).max(1));
                for y in 0..h {
                    for x in 0..w {
                        let v = if new > old {
                            src.get((x * old / new) as i64, (y * old / new) as i64)
                        } else {
                            // Downsampling: the block's most common color, if it covers half the block.
                            let mut votes: HashMap<u32, u32> = HashMap::new();
                            for by in 0..block {
                                for bx in 0..block {
                                    let p = src.get((x * block + bx) as i64, (y * block + by) as i64);
                                    if p != TRANSPARENT {
                                        *votes.entry(p).or_default() += 1;
                                    }
                                }
                            }
                            let total: u32 = votes.values().sum();
                            match votes.into_iter().max_by_key(|(c, n)| (*n, *c)) {
                                Some((c, _)) if total * 2 >= block * block => c,
                                _ => TRANSPARENT,
                            }
                        };
                        if v != TRANSPARENT {
                            out.set(x as i64, y as i64, v);
                        }
                    }
                }
                doc.cels.insert(key, Arc::new(out));
            }
        }

        /// One tool for every layer property: name, visible, locked, opacity, blend, density.
        pub fn layer_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let li = layer_index(doc, a, "layer")?;
            if let Some(n) = opt_str(a, "name")? {
                let n = n.trim();
                if n.is_empty() || n.chars().count() > 128 {
                    return Err(CmdError::invalid("layer name must be 1-128 characters"));
                }
                doc.layers[li].name = n.to_string();
            }
            if let Some(v) = opt_bool(a, "visible")? {
                doc.layers[li].visible = v;
            }
            if let Some(v) = opt_bool(a, "locked")? {
                doc.layers[li].locked = v;
            }
            if let Some(o) = opt_i64(a, "opacity")? {
                doc.layers[li].opacity = o.clamp(0, 255) as u8;
            }
            if let Some(b) = opt_str(a, "blend")? {
                doc.layers[li].blend =
                    crate::layers::Blend::parse(b).ok_or_else(|| CmdError::invalid(format!("unknown blend mode '{b}'")))?;
            }
            if let Some(d) = opt_i64(a, "density")? {
                if !matches!(d, 1 | 2 | 4) {
                    return Err(CmdError::invalid("'density' must be 1, 2 or 4"));
                }
                set_density(doc, li, d as u8);
            }
            let l = &doc.layers[li];
            Ok(Outcome::new(
                Json::obj()
                    .with("layer", li)
                    .with("name", l.name.as_str())
                    .with("visible", l.visible)
                    .with("locked", l.locked)
                    .with("opacity", l.opacity)
                    .with("blend", l.blend.as_str())
                    .with("density", l.density),
                0,
            ))
        }

        /// Finds the smallest pixel grid that reproduces an upscaled image (nearest-neighbour,
        /// integer or non-integer scale such as 3.35×). Returns the native (w, h).
        pub fn detect_grid(img: &Image) -> (u32, u32) {
            let (w, h) = (img.width, img.height);
            let step = ((w as u64 * h as u64 / 40_000) as f64).sqrt().ceil().max(1.0) as u32;
            let samples: Vec<(u32, u32)> =
                (0..h).step_by(step as usize).flat_map(|y| (0..w).step_by(step as usize).map(move |x| (x, y))).collect();
            let err = |nw: u32, nh: u32| -> usize {
                samples
                    .iter()
                    .filter(|&&(x, y)| {
                        let (i, j) = ((x as u64 * nw as u64 / w as u64) as u32, (y as u64 * nh as u64 / h as u64) as u32);
                        let (sx, sy) = (
                            (((i as f64 + 0.5) * w as f64 / nw as f64) as u32).min(w - 1),
                            (((j as f64 + 0.5) * h as f64 / nh as f64) as u32).min(h - 1),
                        );
                        img.get(x, y) != img.get(sx, sy)
                    })
                    .count()
            };
            let mut results: Vec<(u32, u32, usize)> = Vec::new();
            for nw in (w / 16).max(4)..=w {
                let base = (nw as f64 * h as f64 / w as f64).round() as u32;
                let best = [base.saturating_sub(1).max(1), base, (base + 1).min(h)]
                    .into_iter()
                    .map(|nh| (nh, err(nw, nh)))
                    .min_by_key(|p| p.1)
                    .unwrap();
                results.push((nw, best.0, best.1));
            }
            // Tolerance scales with the amount of detail: a sparse image (a few dots on an empty
            // canvas) must reproduce exactly, a busy one may differ on a few anti-aliased edges.
            let edges = samples.iter().filter(|&&(x, y)| x + 1 < w && img.get(x, y) != img.get(x + 1, y)).count();
            let tolerance = results.iter().map(|r| r.2).min().unwrap_or(0) + edges / 50;
            results.iter().find(|r| r.2 <= tolerance).map(|r| (r.0, r.1)).unwrap_or((w, h))
        }

        /// Imports a PNG as pixel art: recovers its true grid (`grid: auto`), optionally makes the
        /// background transparent, limits colors, and turns the palette into material ramps
        /// (grouped by hue, sorted dark → light). As a reference it lands on a locked, faded layer
        /// to trace over; otherwise it becomes editable pixels.
        pub fn reference_import(doc: &mut Document, a: &Json, payload: &[u8]) -> CmdResult<Outcome> {
            let png = if !payload.is_empty() {
                payload.to_vec()
            } else {
                super::voxel::base64_decode(
                    req_str(a, "data_base64")
                        .map_err(|_| CmdError::invalid("provide the PNG as 'data_base64' (or as the binary payload)"))?,
                )?
            };
            let src = crate::export::png::decode(&png).map_err(|e| CmdError::new("invalid_file", e))?;
            let (nw, nh) = match a.get("grid") {
                None | Some(Json::Null) => detect_grid(&src),
                Some(Json::Str(s)) if s == "auto" => detect_grid(&src),
                Some(Json::Str(s)) if s == "none" => (src.width, src.height),
                Some(v) => {
                    let k =
                        v.as_f64().filter(|k| *k >= 1.0).ok_or_else(|| CmdError::invalid("'grid' must be auto, none or a scale ≥ 1"))?;
                    (((src.width as f64 / k).round() as u32).max(1), ((src.height as f64 / k).round() as u32).max(1))
                }
            };
            let mut img = Image::new(nw, nh);
            for j in 0..nh {
                for i in 0..nw {
                    let c = src.get(
                        (((i as f64 + 0.5) * src.width as f64 / nw as f64) as u32).min(src.width - 1),
                        (((j as f64 + 0.5) * src.height as f64 / nh as f64) as u32).min(src.height - 1),
                    );
                    let o = ((j * nw + i) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
                }
            }
            if opt_str(a, "transparent")?.unwrap_or("auto") == "auto" {
                // A solid frame color (all four corners equal) is background.
                let k = img.get(0, 0);
                if k.a == 255 && [img.get(nw - 1, 0), img.get(0, nh - 1), img.get(nw - 1, nh - 1)].iter().all(|c| *c == k) {
                    for p in img.pixels.as_chunks_mut::<4>().0 {
                        if *p == [k.r, k.g, k.b, 255] {
                            *p = [0, 0, 0, 0];
                        }
                    }
                }
            }
            if let Some(n) = opt_i64(a, "max_colors")? {
                let samples = crate::pixelart::image_samples(&img);
                let pal = crate::pixelart::median_cut(&samples, n.clamp(2, 256) as usize);
                crate::pixelart::quantize_image(&mut img, &pal, crate::pixelart::Dither::None, 0.0);
            }
            // Palette → material ramps. Near-blacks are the outline ramp, greys get their own,
            // chromatic colors are grouped by hue (split at gaps > 18°, groups capped at 8 tones)
            // and every group is sorted dark → light.
            let mut counts: HashMap<u32, u32> = HashMap::new();
            for p in img.pixels.as_chunks::<4>().0 {
                if p[3] > 0 {
                    *counts.entry(Rgba::new(p[0], p[1], p[2], 255).to_pixel()).or_default() += 1;
                }
            }
            let mut made = Vec::new();
            if opt_bool(a, "ramps")?.unwrap_or(false) {
                let (mut dark, mut grey, mut hued) = (Vec::new(), Vec::new(), Vec::new());
                for (&c, &n) in &counts {
                    if n < 2 {
                        continue;
                    }
                    let [l, ch, hue] = crate::pixelart::lch(Rgba::from_u32(c));
                    if l < 0.22 {
                        dark.push((l, c, hue));
                    } else if ch < 0.03 {
                        grey.push((l, c, hue));
                    } else {
                        hued.push((l, c, hue));
                    }
                }
                hued.sort_by(|a, b| a.2.total_cmp(&b.2));
                let mut groups: Vec<Vec<(f32, u32, f32)>> = Vec::new();
                if !hued.is_empty() {
                    // Start after the widest circular gap so a red group is not cut at 0°/360°.
                    let n = hued.len();
                    let gap = |i: usize| (hued[(i + 1) % n].2 - hued[i].2).rem_euclid(360.0);
                    let widest = (0..n).max_by(|&i, &j| gap(i).total_cmp(&gap(j))).unwrap_or(0);
                    let order: Vec<usize> = (1..=n).map(|k| (widest + k) % n).collect();
                    let mut cur = vec![hued[order[0]]];
                    for w in order.windows(2) {
                        if (hued[w[1]].2 - hued[w[0]].2).rem_euclid(360.0) > 18.0 {
                            groups.push(std::mem::take(&mut cur));
                        }
                        cur.push(hued[w[1]]);
                    }
                    groups.push(cur);
                }
                let mut k = 0;
                while k < groups.len() {
                    if groups[k].len() > 8 {
                        let g = &groups[k];
                        let cut = (1..g.len()).max_by(|&i, &j| (g[i].2 - g[i - 1].2).total_cmp(&(g[j].2 - g[j - 1].2))).unwrap();
                        let tail = groups[k].split_off(cut);
                        groups.insert(k + 1, tail);
                    } else {
                        k += 1;
                    }
                }
                type Tone = (f32, u32, f32);
                let mut named: Vec<(String, Vec<Tone>)> = vec![("outline".into(), dark), ("grey".into(), grey)];
                for g in groups {
                    let mean = g.iter().map(|t| t.2).sum::<f32>() / g.len() as f32;
                    named.push((format!("hue{:03}", mean.round() as u32), g));
                }
                for (base, mut tones) in named {
                    if tones.len() < 2 {
                        continue;
                    }
                    tones.sort_by(|x, y| x.0.total_cmp(&y.0));
                    tones.truncate(16);
                    let mut name = format!("ref_{base}");
                    if made.iter().any(|m: &Json| m.as_str() == Some(name.as_str())) {
                        name = format!("{name}_{}", made.len());
                    }
                    let colors: Vec<u32> = tones.iter().map(|t| t.1).collect();
                    match doc.ramps.iter_mut().find(|r| r.name == name) {
                        Some(r) => r.colors = colors,
                        None => doc.ramps.push(Ramp { name: name.clone(), colors }),
                    }
                    made.push(Json::from(name));
                }
            }
            let reference = opt_bool(a, "reference")?.unwrap_or(false);
            // Target: an existing layer (index or name), a new named layer, or the active layer.
            let lid = match a.get("layer") {
                Some(Json::Str(n)) if !doc.layers.iter().any(|l| &l.name == n) => {
                    doc.insert_layer(n, if reference { 0 } else { doc.layers.len() })
                }
                Some(v) if !v.is_null() => doc.layers[layer_index(doc, a, "layer")?].id,
                _ if reference => match doc.layers.iter().find(|l| l.name == "Reference") {
                    Some(l) => l.id,
                    None => doc.insert_layer("Reference", 0),
                },
                _ => doc.active_layer,
            };
            if !reference && doc.layers.iter().any(|l| l.id == lid && l.locked) {
                return Err(CmdError::new("locked", "layer is locked"));
            }
            let layer_name = doc.layers.iter().find(|l| l.id == lid).map(|l| l.name.clone()).unwrap_or_default();
            let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
            let fi = super::args::frame_index(doc, a, "frame")?;
            let fid = doc.frames[fi].id;
            let grid = doc.cel_mut(lid, fid);
            let mut changed = 0u64;
            for y in 0..nh {
                for x in 0..nw {
                    let c = img.get(x, y);
                    if c.a > 0 {
                        changed += grid.set(ox + x as i64, oy + y as i64, c.to_pixel()) as u64;
                    }
                }
            }
            if reference && let Some(l) = doc.layers.iter_mut().find(|l| l.id == lid) {
                l.locked = true;
                l.opacity = 96;
            }
            if !reference {
                doc.active_layer = lid;
            }
            let colors: std::collections::HashSet<[u8; 4]> = img.pixels.as_chunks::<4>().0.iter().filter(|p| p[3] > 0).copied().collect();
            Ok(Outcome::new(
                Json::obj()
                    .with("source", Json::obj().with("width", src.width).with("height", src.height))
                    .with("native", Json::obj().with("width", nw).with("height", nh))
                    .with("scale", (src.width as f64 / nw as f64 * 100.0).round() / 100.0)
                    .with("colors", colors.len())
                    .with("ramps", Json::Arr(made))
                    .with("layer", layer_name)
                    .with("changed", changed),
                changed,
            ))
        }
    }

    mod charart {
        //! Character-art commands: auto_shade, gradient_map, draw_sparkle, pixel_cleanup (and the
        //! sel-out options of `outline`, which live in pixel2d).

        use std::collections::HashMap;
        use std::sync::Arc;

        use super::args::{color_value, frame_index, layer_index, opt_bool, opt_f64, opt_i64, opt_str, target};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome};
        use crate::pixel::{PixelGrid, Rgba};
        use crate::pixelart::charart::{self, ShadeSettings};
        use crate::pixelart::{grid_to_image, image_to_grid};
        use crate::project::Document;

        pub fn light_arg(a: &Json, key: &str, default: (f32, f32)) -> CmdResult<(f32, f32)> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(default),
                Some(Json::Str(s)) => Ok(match s.as_str() {
                    "top-left" | "top_left" => (-1.0, -1.2),
                    "top-right" | "top_right" => (1.0, -1.2),
                    "top" => (0.0, -1.0),
                    "left" => (-1.0, 0.0),
                    "right" => (1.0, 0.0),
                    "bottom" => (0.0, 1.0),
                    "front" => (-0.3, -0.5),
                    other => {
                        return Err(CmdError::invalid(format!(
                            "unknown light '{other}' (top-left, top-right, top, left, right, bottom, front or [x, y])"
                        )));
                    }
                }),
                Some(Json::Arr(v)) if v.len() == 2 => {
                    let x = v[0].as_f64().ok_or_else(|| CmdError::invalid("light must be [x, y]"))? as f32;
                    let y = v[1].as_f64().ok_or_else(|| CmdError::invalid("light must be [x, y]"))? as f32;
                    if x == 0.0 && y == 0.0 {
                        return Err(CmdError::invalid("light direction cannot be [0, 0]"));
                    }
                    Ok((x, y))
                }
                Some(_) => Err(CmdError::invalid(format!("'{key}' must be a direction name or [x, y]"))),
            }
        }

        /// Cels addressed by scope (cel / layer / frame / all), skipping locked layers.
        fn cels(doc: &Document, a: &Json) -> CmdResult<Vec<(usize, usize)>> {
            let li = layer_index(doc, a, "layer")?;
            let fi = frame_index(doc, a, "frame")?;
            let v: Vec<(usize, usize)> = match opt_str(a, "scope")?.unwrap_or("cel") {
                "cel" => vec![(li, fi)],
                "layer" => (0..doc.frames.len()).map(|f| (li, f)).collect(),
                "frame" => (0..doc.layers.len()).map(|l| (l, fi)).collect(),
                "all" => (0..doc.layers.len()).flat_map(|l| (0..doc.frames.len()).map(move |f| (l, f))).collect(),
                other => return Err(CmdError::invalid(format!("unknown scope '{other}' (cel, layer, frame, all)"))),
            };
            if v.len() == 1 && doc.layers[v[0].0].locked {
                return Err(CmdError::new("locked", format!("layer '{}' is locked", doc.layers[v[0].0].name)));
            }
            Ok(v.into_iter().filter(|(l, _)| !doc.layers[*l].locked).collect())
        }

        fn color_list(doc: &Document, v: Option<&Json>, key: &str) -> CmdResult<Vec<Rgba>> {
            match v {
                None | Some(Json::Null) => Ok(Vec::new()),
                Some(Json::Arr(list)) => list.iter().map(|c| color_value(doc, c, key).map(Rgba::from_u32)).collect(),
                Some(_) => Err(CmdError::invalid(format!("'{key}' must be an array of colors"))),
            }
        }

        fn edit_cel(doc: &mut Document, l: usize, f: usize, edit: impl FnOnce(&mut crate::renderer::Image) -> usize) -> u64 {
            let key = (doc.layers[l].id, doc.frames[f].id);
            let Some(cel) = doc.cels.get_mut(&key) else { return 0 };
            let grid: &mut PixelGrid = Arc::make_mut(cel);
            let mut img = grid_to_image(grid);
            if edit(&mut img) == 0 {
                return 0;
            }
            image_to_grid(&img, grid) as u64
        }

        /// Automatic cel shading of flat-colored shapes (see pixelart::charart::auto_shade).
        pub fn auto_shade(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let mut s = ShadeSettings {
                light: light_arg(a, "light", (-1.0, -1.2))?,
                tones: opt_i64(a, "tones")?.unwrap_or(3).clamp(2, 4) as u32,
                bevel: opt_f64(a, "bevel")?.unwrap_or(3.0).clamp(0.5, 32.0) as f32,
                form: opt_f64(a, "form")?.unwrap_or(0.45).clamp(0.0, 2.0) as f32,
                hue_shift: opt_f64(a, "hue_shift")?.unwrap_or(22.0).clamp(0.0, 90.0) as f32,
                contrast: opt_f64(a, "contrast")?.unwrap_or(0.5).clamp(0.05, 1.0) as f32,
                shadow_amount: opt_f64(a, "shadow")?.unwrap_or(0.0).clamp(-1.0, 1.0) as f32,
                min_region: opt_i64(a, "min_region")?.unwrap_or(6).clamp(1, 1_000_000) as usize,
                ..Default::default()
            };
            s.preserve = color_list(doc, a.get("preserve"), "preserve")?.iter().map(|c| c.to_u32() | 0xFF).collect();
            if let Some(Json::Obj(entries)) = a.get("ramps") {
                let mut ramps = HashMap::new();
                for (k, v) in entries {
                    let base = Rgba::parse_hex(k).ok_or_else(|| CmdError::invalid(format!("ramps key '{k}' is not a hex color")))?;
                    let tones = color_list(doc, Some(v), "ramps")?;
                    let t: [Rgba; 4] = match tones.len() {
                        2 => [tones[0], tones[0], base, tones[1]],
                        3 => [tones[0], tones[0], tones[1], tones[2]],
                        4 => [tones[0], tones[1], tones[2], tones[3]],
                        _ => {
                            return Err(CmdError::invalid(
                                "each ramp needs [shadow, highlight], [shadow, base, highlight] or [deep, shadow, base, highlight]",
                            ));
                        }
                    };
                    ramps.insert(base.to_u32() | 0xFF, t);
                }
                s.ramps = ramps;
            }
            // Palette lock: every color of a document ramp shades with its ramp neighbours, and
            // colors outside the ramps stay untouched — the result uses only the artist's palette.
            s.palette_lock = opt_bool(a, "palette_lock")?.unwrap_or(!doc.ramps.is_empty());
            if s.palette_lock {
                for r in &doc.ramps {
                    let c: Vec<Rgba> = r.colors.iter().map(|v| Rgba::from_u32(*v)).collect();
                    let at = |i: isize| c[i.clamp(0, c.len() as isize - 1) as usize];
                    for i in 0..c.len() as isize {
                        s.ramps.entry(c[i as usize].to_u32() | 0xFF).or_insert([at(i - 2), at(i - 1), at(i), at(i + 1)]);
                    }
                }
            }
            if let Some(v) = a.get("rim").filter(|v| !v.is_null()) {
                let c = Rgba::from_u32(color_value(doc, v, "rim")?);
                s.rim = Some((c, opt_f64(a, "rim_strength")?.unwrap_or(0.55).clamp(0.0, 1.0) as f32));
            }
            let cast = opt_i64(a, "cast_shadow")?.unwrap_or(2).clamp(0, 64) as u32;
            let mut changed = 0u64;
            for (l, f) in cels(doc, a)? {
                // Pixels of visible layers above this one cast shadows onto it (mapped between
                // layer densities, so a 2× detail layer and a 1× body layer still interact).
                let (w, h) = doc.layer_dims(doc.layers[l].id);
                let (w, h) = (w as usize, h as usize);
                let dl = doc.layers[l].density.max(1) as usize;
                let mut occ = vec![false; if cast > 0 { w * h } else { 0 }];
                if cast > 0 {
                    for upper in &doc.layers[l + 1..] {
                        if !upper.visible {
                            continue;
                        }
                        let du = upper.density.max(1) as usize;
                        if let Some(g) = doc.cel(upper.id, doc.frames[f].id) {
                            g.for_each_opaque(|x, y, v| {
                                if v & 0xFF < 128 {
                                    return;
                                }
                                let (x0, y0) = (x as usize * dl / du, y as usize * dl / du);
                                let span = (dl / du).max(1);
                                for yy in y0..(y0 + span).min(h) {
                                    for xx in x0..(x0 + span).min(w) {
                                        occ[yy * w + xx] = true;
                                    }
                                }
                            });
                        }
                    }
                }
                changed += edit_cel(doc, l, f, |img| charart::auto_shade(img, &s, &occ, cast));
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        /// Color grading: luminance mapped onto a gradient (mood, time of day, flashbacks).
        pub fn gradient_map(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let stops = color_list(doc, a.get("colors"), "colors")?;
            if stops.len() < 2 || stops.len() > 32 {
                return Err(CmdError::invalid("'colors' must list 2-32 colors from dark to light"));
            }
            let strength = opt_f64(a, "strength")?.unwrap_or(1.0).clamp(0.0, 1.0) as f32;
            let mut changed = 0u64;
            for (l, f) in cels(doc, a)? {
                changed += edit_cel(doc, l, f, |img| charart::gradient_map(img, &stops, strength));
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        /// Stamps 4-point sparkles / stars (one or many).
        pub fn draw_sparkle(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let color = Rgba::from_u32(color_value(doc, a.get("color").unwrap_or(&Json::Str("#FFE9B0".into())), "color")?);
            let core = Rgba::from_u32(color_value(doc, a.get("core").unwrap_or(&Json::Str("#FFFFFF".into())), "core")?);
            let style = opt_str(a, "style")?.unwrap_or("star4");
            if !["star4", "cross", "diamond", "dot"].contains(&style) {
                return Err(CmdError::invalid("'style' must be star4, cross, diamond or dot"));
            }
            let default_size = opt_i64(a, "size")?.unwrap_or(3);
            let mut points: Vec<(i64, i64, i64)> = Vec::new();
            match a.get("points") {
                Some(Json::Arr(list)) => {
                    for p in list {
                        match p.as_array() {
                            Some([x, y]) => points.push((x.as_i64().unwrap_or(0), y.as_i64().unwrap_or(0), default_size)),
                            Some([x, y, s]) => {
                                points.push((x.as_i64().unwrap_or(0), y.as_i64().unwrap_or(0), s.as_i64().unwrap_or(default_size)))
                            }
                            _ => return Err(CmdError::invalid("points must be [[x, y] or [x, y, size], …]")),
                        }
                    }
                }
                _ => points.push((opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0), default_size)),
            }
            if points.len() > 4096 {
                return Err(CmdError::new("limit", "at most 4096 sparkles per call"));
            }
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            for (x, y, size) in points {
                for (dx, dy, is_core) in charart::sparkle_shape(size.clamp(1, 8) as u32, style) {
                    let c = if is_core { core } else { color };
                    changed += grid.set(x + dx as i64, y + dy as i64, c.to_pixel()) as u64;
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        pub fn pixel_cleanup(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let only = match a.get("color") {
                None | Some(Json::Null) => None,
                Some(v) => Some(Rgba::from_u32(color_value(doc, v, "color")?)),
            };
            let passes = opt_i64(a, "passes")?.unwrap_or(1).clamp(1, 4);
            let mut changed = 0u64;
            for (l, f) in cels(doc, a)? {
                for _ in 0..passes {
                    changed += edit_cel(doc, l, f, |img| charart::pixel_cleanup(img, only));
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }
    }
    mod draw {
        //! Pixel-level drawing commands.

        use super::args::{color, color_value, coord, frame_index, layer_index, opt_bool, opt_i64, opt_str, region, req_i64, target};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome};
        use crate::pixel::{Rgba, TRANSPARENT};
        use crate::project::Document;
        use crate::raster::{self, PasteMode, Region};

        pub const MAX_BATCH_PIXELS: usize = 262_144;

        fn hex_of(pixel: u32) -> Json {
            if pixel == TRANSPARENT { Json::Null } else { Json::Str(Rgba::from_u32(pixel).to_hex()) }
        }

        pub fn set_pixel(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let (x, y) = (coord(a, "x")?, coord(a, "y")?);
            let (l, f) = target(doc, a)?;
            let v = color(doc, a, "color")?;
            let (w, h) = doc.layer_dims(l);
            if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                return Err(CmdError::invalid(format!("pixel ({x},{y}) is outside the {w}x{h} layer grid")));
            }
            let changed = doc.cel_mut(l, f).set(x, y, v) as u64;
            Ok(Outcome::new(Json::obj().with("x", x).with("y", y).with("color", hex_of(v)).with("changed", changed), changed))
        }

        /// Batch of explicit pixels: `[{"x":1,"y":2,"color":"#fff"}, [3,4,"#000"], ...]`.
        /// Every pixel is still an individual coordinate+color, so the operation stays auditable.
        pub fn set_pixels(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let items = a.get("pixels").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'pixels' must be an array"))?;
            if items.len() > MAX_BATCH_PIXELS {
                return Err(CmdError::new("limit", format!("at most {MAX_BATCH_PIXELS} pixels per call")));
            }
            let (l, f) = target(doc, a)?;
            let default = match a.get("color") {
                Some(v) => Some(color_value(doc, v, "color")?),
                None => None,
            };
            let mut resolved = Vec::with_capacity(items.len());
            for (i, item) in items.iter().enumerate() {
                let (x, y, c) = match item {
                    Json::Arr(p) if p.len() == 2 || p.len() == 3 => (p[0].as_i64(), p[1].as_i64(), p.get(2).cloned()),
                    Json::Obj(_) => {
                        (item.get("x").and_then(Json::as_i64), item.get("y").and_then(Json::as_i64), item.get("color").cloned())
                    }
                    _ => (None, None, None),
                };
                let (Some(x), Some(y)) = (x, y) else {
                    return Err(CmdError::invalid(format!("pixels[{i}] needs integer x and y")));
                };
                let v = match c {
                    Some(c) => color_value(doc, &c, &format!("pixels[{i}].color"))?,
                    None => {
                        default.ok_or_else(|| CmdError::invalid(format!("pixels[{i}] has no color and no default 'color' was given")))?
                    }
                };
                resolved.push((x, y, v));
            }
            let grid = doc.cel_mut(l, f);
            let (mut changed, mut clipped) = (0u64, 0u64);
            for (x, y, v) in resolved {
                if !grid.in_bounds(x, y) {
                    clipped += 1;
                    continue;
                }
                changed += grid.set(x, y, v) as u64;
            }
            Ok(Outcome::new(Json::obj().with("requested", items.len()).with("changed", changed).with("outOfBounds", clipped), changed))
        }

        pub fn draw_line(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let (x0, y0, x1, y1) = (coord(a, "x0")?, coord(a, "y0")?, coord(a, "x1")?, coord(a, "y1")?);
            let (l, f) = target(doc, a)?;
            let v = color(doc, a, "color")?;
            let changed = raster::line(doc.cel_mut(l, f), x0, y0, x1, y1, v) as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        fn shape(doc: &mut Document, a: &Json, ellipse: bool) -> CmdResult<Outcome> {
            let r = Region { x: coord(a, "x")?, y: coord(a, "y")?, w: coord(a, "width")?, h: coord(a, "height")? };
            if r.w <= 0 || r.h <= 0 {
                return Err(CmdError::invalid("width and height must be positive"));
            }
            let filled = opt_bool(a, "filled")?.unwrap_or(false);
            let (l, f) = target(doc, a)?;
            let v = color(doc, a, "color")?;
            let grid = doc.cel_mut(l, f);
            let changed = if ellipse { raster::ellipse(grid, r, v, filled) } else { raster::rectangle(grid, r, v, filled) } as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        pub fn draw_rectangle(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            shape(doc, a, false)
        }

        pub fn draw_ellipse(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            shape(doc, a, true)
        }

        fn points(a: &Json) -> CmdResult<Vec<(f64, f64)>> {
            let list = a.get("points").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'points' must be [[x, y], …]"))?;
            if list.len() < 3 || list.len() > 4096 {
                return Err(CmdError::invalid("a polygon needs 3-4096 points"));
            }
            list.iter()
                .map(|p| match p.as_array() {
                    Some([x, y]) => match (x.as_f64(), y.as_f64()) {
                        (Some(x), Some(y)) if x.abs() <= 1e6 && y.abs() <= 1e6 => Ok((x, y)),
                        _ => Err(CmdError::invalid("polygon points must be numbers")),
                    },
                    _ => Err(CmdError::invalid("each point must be [x, y]")),
                })
                .collect()
        }

        /// Polygon (roofs, gables, crystals, banners): filled by pixel centers + crisp edges.
        pub fn draw_polygon(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let pts = points(a)?;
            let filled = opt_bool(a, "filled")?.unwrap_or(true);
            let (l, f) = target(doc, a)?;
            let v = color(doc, a, "color")?;
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            if filled {
                let (ymin, ymax) = pts.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.1), hi.max(p.1)));
                for y in (ymin.floor() as i64).max(0)..=(ymax.ceil() as i64).min(grid.height() as i64 - 1) {
                    let cy = y as f64 + 0.5;
                    let mut xs: Vec<f64> = Vec::new();
                    for i in 0..pts.len() {
                        let (a0, b0) = (pts[i], pts[(i + 1) % pts.len()]);
                        if (a0.1 > cy) != (b0.1 > cy) {
                            xs.push(a0.0 + (cy - a0.1) * (b0.0 - a0.0) / (b0.1 - a0.1));
                        }
                    }
                    xs.sort_by(|p, q| p.total_cmp(q));
                    for pair in xs.chunks(2) {
                        if let [x0, x1] = pair {
                            let (from, to) = ((x0 - 0.5).ceil() as i64, (x1 - 0.5).floor() as i64);
                            for x in from.max(0)..=to.min(grid.width() as i64 - 1) {
                                changed += grid.set(x, y, v) as u64;
                            }
                        }
                    }
                }
            }
            for i in 0..pts.len() {
                let (p, q) = (pts[i], pts[(i + 1) % pts.len()]);
                let r = |v: f64| v.floor() as i64;
                changed += raster::line(grid, r(p.0), r(p.1), r(q.0), r(q.1), v) as u64;
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        fn phash(x: i64, y: i64, seed: i64) -> u32 {
            let mut h =
                (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ (seed as u32).wrapping_mul(0xCB1A_B31F);
            h ^= h >> 13;
            h = h.wrapping_mul(0x5BD1_E995);
            h ^ (h >> 15)
        }

        /// Pixel-art surface textures for buildings and terrain, applied inside a region
        /// (by default only over existing pixels, so you draw the shape flat first):
        /// brick, shingle, plank, stone, tile, grass, wave, checker. `color` draws the seams
        /// (mortar, grout, plank joints); `color2` fills the cells (optional); `accent` varies
        /// some cells; `size` scales the pattern.
        pub fn pattern_fill(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let pattern = opt_str(a, "pattern")?.ok_or_else(|| CmdError::invalid("missing 'pattern'"))?;
            const KINDS: [&str; 8] = ["brick", "shingle", "plank", "stone", "tile", "grass", "wave", "checker"];
            if !KINDS.contains(&pattern) {
                return Err(CmdError::invalid(format!("unknown pattern '{pattern}' ({})", KINDS.join(", "))));
            }
            let r = region(doc, a)?;
            let line = color(doc, a, "color")?;
            let fill = match a.get("color2") {
                None | Some(Json::Null) => None,
                Some(v) => Some(color_value(doc, v, "color2")?),
            };
            let accent = match a.get("accent") {
                None | Some(Json::Null) => None,
                Some(v) => Some(color_value(doc, v, "accent")?),
            };
            let size = opt_i64(a, "size")?.unwrap_or(4).clamp(2, 64);
            let seed = opt_i64(a, "seed")?.unwrap_or(1);
            let only_opaque = opt_bool(a, "only_opaque")?.unwrap_or(true);
            let only = match a.get("only_color") {
                None | Some(Json::Null) => None,
                Some(v) => Some(color_value(doc, v, "only_color")?),
            };
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    if !grid.in_bounds(x, y) {
                        continue;
                    }
                    let cur = grid.get(x, y);
                    if (only_opaque && cur == TRANSPARENT) || only.is_some_and(|o| o != cur) {
                        continue;
                    }
                    let (lx, ly) = (x - r.x, y - r.y);
                    let s = size;
                    // Each pattern answers: seam? and which cell (for accents).
                    let (seam, cell): (bool, (i64, i64)) = match pattern {
                        "brick" => {
                            let row = ly.div_euclid(s);
                            let w = s * 2;
                            let ox = lx + if row % 2 == 1 { s } else { 0 };
                            (ly.rem_euclid(s) == s - 1 || ox.rem_euclid(w) == w - 1, (ox.div_euclid(w), row))
                        }
                        "shingle" => {
                            let row = ly.div_euclid(s);
                            let w = s + s / 2;
                            let ox = lx + if row % 2 == 1 { w / 2 } else { 0 };
                            let u = ox.rem_euclid(w);
                            let v = ly.rem_euclid(s);
                            // Rounded shingle tips: the seam curves up at both tile edges.
                            let edge = u == w - 1 || (v == s - 1) || (v == s - 2 && (u == 0 || u == w - 2));
                            (edge, (ox.div_euclid(w), row))
                        }
                        "plank" => {
                            let row = ly.div_euclid(s);
                            let len = s * 4 + (phash(row, 0, seed) % (s as u32 * 3)) as i64;
                            let off = (phash(row, 1, seed) % len as u32) as i64;
                            (ly.rem_euclid(s) == s - 1 || (lx + off).rem_euclid(len) == len - 1, ((lx + off).div_euclid(len), row))
                        }
                        "stone" => {
                            // Voronoi cobbles: seam where the two nearest seeds are almost equidistant.
                            let cs = s * 2;
                            let (cx, cy) = (lx.div_euclid(cs), ly.div_euclid(cs));
                            let mut best = [(i64::MAX, (0, 0)); 2];
                            for dy in -1..=1 {
                                for dx in -1..=1 {
                                    let (gx, gy) = (cx + dx, cy + dy);
                                    let h = phash(gx, gy, seed);
                                    let sx = gx * cs + (h % cs as u32) as i64;
                                    let sy = gy * cs + ((h >> 8) % cs as u32) as i64;
                                    let d = (sx - lx).pow(2) + (sy - ly).pow(2);
                                    if d < best[0].0 {
                                        best[1] = best[0];
                                        best[0] = (d, (gx, gy));
                                    } else if d < best[1].0 {
                                        best[1] = (d, (gx, gy));
                                    }
                                }
                            }
                            let gap = ((best[1].0 as f64).sqrt() - (best[0].0 as f64).sqrt()).abs();
                            (gap < 1.0, best[0].1)
                        }
                        "tile" => (lx.rem_euclid(s) == s - 1 || ly.rem_euclid(s) == s - 1, (lx.div_euclid(s), ly.div_euclid(s))),
                        "grass" => {
                            let h = phash(lx, ly, seed);
                            let blade = h % 100 < 9 || (phash(lx, ly + 1, seed) % 100 < 9 && h.is_multiple_of(3));
                            (blade, (lx, ly))
                        }
                        "wave" => {
                            let row = ly.div_euclid(s);
                            let phase = ((lx as f64 / s as f64 * 1.3 + row as f64 * 1.7).sin() * 1.2).round() as i64;
                            let on = ly.rem_euclid(s) == (s / 2 + phase).rem_euclid(s) && (lx + row * 3).rem_euclid(s * 3) < s * 2;
                            (on, (lx.div_euclid(s * 3), row))
                        }
                        _ => {
                            let c = (lx.div_euclid(s) + ly.div_euclid(s)).rem_euclid(2) == 1;
                            (c, (lx.div_euclid(s), ly.div_euclid(s)))
                        }
                    };
                    let v = if seam {
                        line
                    } else if let Some(acc) = accent.filter(|_| phash(cell.0, cell.1, seed + 7).is_multiple_of(5)) {
                        acc
                    } else if let Some(fl) = fill {
                        fl
                    } else {
                        continue;
                    };
                    changed += grid.set(x, y, v) as u64;
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed).with("pattern", pattern), changed))
        }

        pub fn fill_region(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let (x, y) = (coord(a, "x")?, coord(a, "y")?);
            let contiguous = opt_bool(a, "contiguous")?.unwrap_or(true);
            let (l, f) = target(doc, a)?;
            let v = color(doc, a, "color")?;
            let changed = raster::fill(doc.cel_mut(l, f), x, y, v, contiguous) as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        pub fn flip(doc: &mut Document, a: &Json, horizontal: bool) -> CmdResult<Outcome> {
            let r = region(doc, a)?;
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let changed = if horizontal { raster::flip_horizontal(grid, r) } else { raster::flip_vertical(grid, r) } as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        pub fn rotate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let degrees = req_i64(a, "degrees")?;
            let r = region(doc, a)?;
            let (l, f) = target(doc, a)?;
            let changed = raster::rotate(doc.cel_mut(l, f), r, degrees)? as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        fn paste_mode(a: &Json) -> CmdResult<PasteMode> {
            match opt_str(a, "mode")?.unwrap_or("over") {
                "over" => Ok(PasteMode::Over),
                "replace" => Ok(PasteMode::Replace),
                other => Err(CmdError::invalid(format!("unknown mode '{other}' (over, replace)"))),
            }
        }

        pub fn move_region(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let r = region(doc, a)?;
            let (dx, dy) = (coord(a, "dx")?, coord(a, "dy")?);
            let mode = paste_mode(a)?;
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let data = raster::extract(grid, r);
            let mut changed = raster::clear_region(grid, r) as u64;
            changed += raster::paste(grid, r.x + dx, r.y + dy, r.w, r.h, &data, mode) as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        /// Copies a region, optionally between layers and frames — the workhorse for building
        /// animation frames from a previous pose.
        pub fn copy_region(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let r = region(doc, a)?;
            let src_l = doc.layers[layer_index(doc, a, "layer")?].id;
            let src_f = doc.frames[frame_index(doc, a, "frame")?].id;
            let dst_li = match a.get("toLayer") {
                Some(_) => layer_index(doc, a, "toLayer")?,
                None => doc.layer_index(src_l).unwrap(),
            };
            let dst_fi = match a.get("toFrame") {
                Some(_) => frame_index(doc, a, "toFrame")?,
                None => doc.frame_index(src_f).unwrap(),
            };
            let dst_layer = &doc.layers[dst_li];
            if dst_layer.locked {
                return Err(CmdError::new("locked", format!("layer '{}' is locked", dst_layer.name)));
            }
            let (dst_l, dst_f) = (dst_layer.id, doc.frames[dst_fi].id);
            let to_x = opt_i64(a, "toX")?.unwrap_or(r.x);
            let to_y = opt_i64(a, "toY")?.unwrap_or(r.y);
            let mode = paste_mode(a)?;
            let data = match doc.cel(src_l, src_f) {
                Some(g) => raster::extract(g, r),
                None => vec![TRANSPARENT; (r.w * r.h) as usize],
            };
            let changed = raster::paste(doc.cel_mut(dst_l, dst_f), to_x, to_y, r.w, r.h, &data, mode) as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        pub fn clear(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let r = region(doc, a)?;
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let changed = raster::clear_region(grid, r) as u64;
            grid.compact();
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }
    }
    mod fx {
        //! One-call 2D effect generator: fire, campfire, slash, hit, magic, smoke, lightning,
        //! heal and explosion — banded pixel-art palettes, seamless loops where the effect loops,
        //! any size, frame count and palette. Effects are designed on a 64×64 grid and sampled
        //! to the requested size.

        use std::collections::HashMap;
        use std::f32::consts::TAU;

        use super::args::{color_value, layer_index, opt_i64, opt_str, req_str};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome};
        use crate::animation::{Direction, Tag};
        use crate::pixel::{Rgba, TRANSPARENT};
        use crate::project::Document;

        const S: i32 = 64;
        type Img = HashMap<(i32, i32), u8>;

        /// (name, default frames, ms per frame, loops)
        pub const KINDS: [(&str, u32, u32, bool); 23] = [
            ("fire", 16, 70, true),
            ("campfire", 16, 70, true),
            ("slash", 8, 45, false),
            ("hit", 8, 50, false),
            ("magic", 12, 60, false),
            ("smoke", 8, 80, false),
            ("lightning", 8, 50, false),
            ("heal", 12, 60, true),
            ("explosion", 12, 60, false),
            ("shockwave", 10, 50, false),
            ("beam", 12, 50, false),
            ("portal", 12, 70, true),
            ("ice", 10, 55, false),
            ("poison", 12, 70, true),
            ("splash", 10, 55, false),
            ("dash", 8, 40, false),
            ("shield", 12, 80, true),
            ("levelup", 14, 60, false),
            ("pickup", 8, 90, true),
            ("dust", 8, 60, false),
            ("holy", 12, 60, false),
            ("shadow", 16, 70, true),
            ("teleport", 12, 50, false),
        ];

        fn palette(kind: &str) -> &'static [&'static str] {
            match kind {
                "fire" => &["#FFF9D6", "#FFE066", "#FFA431", "#F2581F", "#B72727", "#5E1A2A"],
                "campfire" => &["#FFF9D6", "#FFE066", "#FFA431", "#F2581F", "#B72727", "#5E1A2A", "#6E4128", "#4A2A1C", "#3A1F16"],
                "slash" => &["#FFFFFF", "#D9FBFF", "#7FE3FF", "#3A8FE0", "#26407A"],
                "hit" => &["#FFFFFF", "#FFF2A6", "#FFC53D", "#F2742B", "#9C2F2F"],
                "magic" => &["#FFFFFF", "#E7C9FF", "#B57BFF", "#7A3FE0", "#3D2690", "#7FF3FF"],
                "smoke" => &["#E9E4E6", "#C5BDC2", "#9A9098", "#6E6470", "#4A4250"],
                "lightning" => &["#FFFFFF", "#E3F6FF", "#8FD8FF", "#3F8CFF", "#2A3FA0"],
                "heal" => &["#FFFFFF", "#D9FFC9", "#8BF07A", "#3FBF5A", "#1F7A4A"],
                "shockwave" => &["#FFFFFF", "#FFF3C4", "#FFC861", "#E8743B", "#8A3A3A"],
                "beam" => &["#FFFFFF", "#D6FBFF", "#6FE3FF", "#2E8FE8", "#1D3F8A"],
                "portal" => &["#FFFFFF", "#F2C9FF", "#C67BFF", "#7A3FE0", "#3A1F7A", "#160C33"],
                "ice" => &["#FFFFFF", "#DFF6FF", "#9ED8FF", "#5AA8F0", "#2F5FB0"],
                "poison" => &["#F4FFD0", "#C6F26B", "#7CCB3A", "#3F8F3A", "#245A3A", "#B05AD8"],
                "splash" => &["#FFFFFF", "#C9EEFF", "#6FC3F2", "#2F7FC8", "#1F4A8A"],
                "dash" => &["#FFFFFF", "#E9F2FF", "#A9C4F0", "#6E86C8"],
                "shield" => &["#FFFFFF", "#C9FFF4", "#6FF0D8", "#2FB8B0", "#1F6E7A"],
                "levelup" => &["#FFFFFF", "#FFF6B0", "#FFD84A", "#F2A23A", "#B8662E"],
                "pickup" => &["#FFFFFF", "#FFF3B0", "#FFD24A", "#D99A2B", "#6E5A4E"],
                "dust" => &["#F2E8DA", "#D4C4AE", "#A8957E", "#7A6A5A"],
                "holy" => &["#FFFFFF", "#FFF8DA", "#FFE89A", "#F2C55A", "#C88A3A"],
                "shadow" => &["#B57BFF", "#6E3FB0", "#3A2260", "#1E1236", "#0C0818"],
                "teleport" => &["#FFFFFF", "#E0F4FF", "#8FD0FF", "#4A7FE8", "#2A3A9A"],
                _ => &["#FFFFFF", "#FFF0A6", "#FFB53D", "#F25F22", "#A52A2A", "#5A4A4E", "#3A3036"],
            }
        }

        fn hnoise(x: i32, y: i32, z: i32, seed: i32) -> f32 {
            let mut n = (x as u32).wrapping_mul(374_761_393)
                ^ (y as u32).wrapping_mul(668_265_263)
                ^ (z as u32).wrapping_mul(2_147_483_647)
                ^ (seed as u32).wrapping_mul(1_013_904_223);
            n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
            ((n ^ (n >> 16)) & 0xFFFF) as f32 / 65535.0
        }

        fn vnoise(x: f32, y: f32, z: f32, seed: i32) -> f32 {
            let (xi, yi, zi) = (x.floor(), y.floor(), z.floor());
            let f = |t: f32| t * t * (3.0 - 2.0 * t);
            let (sx, sy, sz) = (f(x - xi), f(y - yi), f(z - zi));
            let mut acc = 0.0;
            for dz in 0..2 {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let w = (if dx == 1 { sx } else { 1.0 - sx })
                            * (if dy == 1 { sy } else { 1.0 - sy })
                            * (if dz == 1 { sz } else { 1.0 - sz });
                        acc += w * hnoise(xi as i32 + dx, yi as i32 + dy, zi as i32 + dz, seed);
                    }
                }
            }
            acc
        }

        /// Noise that loops over `period` frames (time on a circle).
        fn loop_noise(x: f32, y: f32, t: f32, period: f32, seed: i32, r: f32) -> f32 {
            let a = TAU * t / period;
            vnoise(x + r * a.cos(), y + r * a.sin(), 0.0, seed)
        }

        fn band(v: f32, thresholds: &[f32]) -> Option<u8> {
            thresholds.iter().position(|&th| v >= th).map(|i| i as u8)
        }

        struct Rng(u64);
        impl Rng {
            fn new(seed: u64) -> Rng {
                Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
            }
            fn next(&mut self) -> f32 {
                self.0 ^= self.0 << 13;
                self.0 ^= self.0 >> 7;
                self.0 ^= self.0 << 17;
                (self.0 >> 40) as f32 / (1u64 << 24) as f32
            }
            fn uniform(&mut self, a: f32, b: f32) -> f32 {
                a + (b - a) * self.next()
            }
        }

        fn put(img: &mut Img, x: f32, y: f32, c: u8) {
            img.insert((x.round() as i32, y.round() as i32), c);
        }

        /// Maps frame t of n onto a table designed for `len` steps.
        fn step(t: u32, n: u32, len: usize) -> usize {
            if n <= 1 { 0 } else { ((t as f32 / (n - 1) as f32) * (len - 1) as f32).round() as usize }
        }

        fn fire(t: u32, n: u32, logs: bool) -> Img {
            let (t, nf) = (t as f32, n as f32);
            let mut img = Img::new();
            for y in 0..S {
                for x in 0..S {
                    let dx = (x as f32 - 32.0) / 16.0;
                    let h = (58.0 - y as f32) / 44.0;
                    if !(0.0..=1.15).contains(&h) {
                        continue;
                    }
                    let width = (1.0 - h).max(0.0).powf(0.8) * (1.0 - 0.15 * (h * 6.0 + TAU * t / nf).sin());
                    let sway = 0.25 * (TAU * t / nf + h * 3.5).sin() * h;
                    let body = 1.0 - (dx - sway).abs() / width.max(0.01);
                    let turb = loop_noise(x as f32 * 0.18, y as f32 * 0.16 + t * 0.9, t, nf, 3, 1.2) * 0.55
                        + loop_noise(x as f32 * 0.45, y as f32 * 0.4 + t * 1.6, t, nf, 9, 1.2) * 0.3;
                    let v = body * 0.95 + turb * 0.55 - h * 0.55;
                    if let Some(c) = band(v, &[0.95, 0.78, 0.6, 0.42, 0.28, 0.18]) {
                        img.insert((x, y), c);
                    }
                }
            }
            for i in 0..10 {
                let p = (t / nf + i as f32 * 0.137).fract();
                let ex = 32.0 + (TAU * (p + i as f32 * 0.31)).sin() * (4 + i % 5) as f32;
                put(&mut img, ex, 46.0 - p * 44.0, if p < 0.5 { 1 } else { 3 });
            }
            if logs {
                for x in 18..47 {
                    for y in 57..61 {
                        if (x - 32i32).abs() + (y - 57) * 3 < 16 {
                            img.insert((x, y), if (x + y) % 5 != 0 { 6 } else { 7 });
                        }
                    }
                }
                for x in 20..45 {
                    img.insert((x, 61), 8);
                }
            }
            img
        }

        fn slash(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = t as f32 / (n.max(2) - 1) as f32;
            let (start, span) = (200f32.to_radians(), 220f32.to_radians());
            let head = start + span * (0.25 + p * 1.5).min(1.0);
            let tail = start + span * ((p - 0.2) * 1.3).clamp(0.0, 0.9);
            let fade = ((p - 0.55) / 0.45).max(0.0);
            for y in 0..S {
                for x in 0..S {
                    let (dx, dy) = (x as f32 - 31.5, y as f32 - 33.5);
                    let r = dx.hypot(dy);
                    let mut a = (-dy).atan2(dx).rem_euclid(TAU);
                    if a < TAU / 4.0 {
                        a += TAU;
                    }
                    if !(tail..=head).contains(&a) {
                        continue;
                    }
                    let u = (a - tail) / (head - tail).max(1e-3);
                    let thick = 1.5 + 7.0 * u * (1.0 - 0.7 * fade);
                    let d = (r - 22.0).abs();
                    if d > thick {
                        continue;
                    }
                    let k = d / thick;
                    let mut c = if k < 0.25 && u > 0.4 {
                        0
                    } else if k < 0.5 {
                        1
                    } else if k < 0.75 {
                        2
                    } else {
                        3
                    };
                    if fade > 0.5 && k > 0.4 {
                        c = 4;
                    }
                    img.insert((x, y), c);
                }
            }
            img
        }

        fn hit(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let mut rng = Rng::new(4);
            let rays: Vec<(f32, f32)> = (0..8).map(|_| (rng.uniform(0.0, TAU), rng.uniform(14.0, 26.0))).collect();
            let i = step(t, n, 6);
            let grow = [0.45, 0.9, 1.0, 0.85, 0.6, 0.3][i];
            for (ang, ln) in rays {
                let l = ln * grow;
                for s in 0..l as i32 {
                    let w = (2.6 * (1.0 - s as f32 / l.max(1.0)) * (1.2 - i as f32 * 0.18)).max(0.0);
                    for o in -(w as i32)..=(w as i32) {
                        let x = 32.0 + ang.cos() * s as f32 - ang.sin() * o as f32 * 0.6;
                        let y = 32.0 + ang.sin() * s as f32 + ang.cos() * o as f32 * 0.6;
                        let k = s as f32 / l.max(1.0);
                        put(
                            &mut img,
                            x,
                            y,
                            if k < 0.3 && i < 3 {
                                0
                            } else if k < 0.55 {
                                1
                            } else if k < 0.8 {
                                2
                            } else {
                                3
                            },
                        );
                    }
                }
            }
            let core = [6, 9, 7, 4, 2, 0][i];
            for y in 32 - core..33 + core {
                for x in 32 - core..33 + core {
                    let d = (x - 32i32).abs() + (y - 32i32).abs();
                    if d <= core {
                        img.insert((x, y), if (d as f32) < core as f32 * 0.6 { 0 } else { 1 });
                    }
                }
            }
            if i >= 3 {
                for k in 0..10 {
                    let a = TAU * k as f32 / 10.0 + 0.3;
                    let r = 12.0 + i as f32 * 4.0;
                    put(&mut img, 32.0 + a.cos() * r, 32.0 + a.sin() * r, if i > 4 { 4 } else { 3 });
                }
            }
            img
        }

        fn magic(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = t as f32 / (n.max(2) - 1) as f32;
            let big_r = 6.0 + 22.0 * (1.0 - (1.0 - p).powi(2));
            let thick = 4.0 * (1.0 - p) + 1.0;
            for y in 0..S {
                for x in 0..S {
                    let r = (x as f32 - 31.5).hypot(y as f32 - 31.5);
                    let d = (r - big_r).abs();
                    if d <= thick {
                        let k = d / thick;
                        img.insert(
                            (x, y),
                            if k < 0.35 {
                                1
                            } else if k < 0.7 {
                                2
                            } else {
                                3
                            },
                        );
                    } else if r < big_r * 0.55 && p < 0.5 {
                        img.insert((x, y), if r < big_r * 0.25 { 0 } else { 1 });
                    }
                }
            }
            for i in 0..6 {
                let a = TAU * i as f32 / 6.0 + p * 2.2;
                for s in (big_r * 0.6) as i32..(big_r * 0.85) as i32 {
                    put(&mut img, 32.0 + a.cos() * s as f32, 32.0 + a.sin() * s as f32, 5);
                }
            }
            let mut rng = Rng::new(8);
            for i in 0..18 {
                let a = rng.uniform(0.0, TAU);
                let r = big_r * (0.8 + rng.uniform(0.0, 0.6)) + p * 6.0;
                let (x, y) = ((32.0 + a.cos() * r).round(), (32.0 + a.sin() * r).round());
                if !(t + i).is_multiple_of(3) {
                    put(&mut img, x, y, 0);
                    if p < 0.7 {
                        for (ox, oy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                            put(&mut img, x + ox, y + oy, 2);
                        }
                    }
                }
            }
            img
        }

        fn smoke(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = t as f32 / (n.max(2) - 1) as f32;
            let mut rng = Rng::new(12);
            let puffs: Vec<(f32, f32, f32)> =
                (0..7).map(|_| (rng.uniform(-10.0, 10.0), rng.uniform(-6.0, 6.0), rng.uniform(5.0, 9.0))).collect();
            for (ox, oy, r0) in puffs {
                let (cx, cy) = (32.0 + ox * (1.0 + p), 40.0 + oy - p * 14.0);
                let r = r0 * (0.6 + p * 0.9);
                for y in (cy - r - 1.0) as i32..(cy + r + 2.0) as i32 {
                    for x in (cx - r - 1.0) as i32..(cx + r + 2.0) as i32 {
                        let d = (x as f32 - cx).hypot(y as f32 - cy) / r;
                        if d > 1.0 {
                            continue;
                        }
                        let shade = d * 0.6 + (y as f32 - cy + r) / (2.0 * r) * 0.5 + p * 0.5;
                        if p > 0.55 && (((x + y * 2).rem_euclid(4)) as f32) < (p - 0.55) * 9.0 {
                            continue;
                        }
                        if let Some(c) = band(1.0 - shade, &[0.75, 0.55, 0.35, 0.15, -0.2]) {
                            img.insert((x, y), c);
                        }
                    }
                }
            }
            img
        }

        fn lightning(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let i = step(t, n, 8);
            let mut rng = Rng::new(if i >= 6 { 77 } else { 30 + i as u64 / 2 });
            let mut pts = vec![(32.0f32, 0.0f32)];
            let mut x = 32.0f32;
            for y in (4..62).step_by(4) {
                x = (x + rng.uniform(-5.0, 5.0)).clamp(10.0, 54.0);
                pts.push((x, y as f32));
            }
            let mut branches = Vec::new();
            for _ in 0..3 {
                let k = 2 + (rng.next() * (pts.len() - 5) as f32) as usize;
                let (mut bx, mut by) = pts[k];
                let mut b = vec![(bx, by)];
                for _ in 0..4 {
                    bx += if rng.next() < 0.5 { -1.0 } else { 1.0 } * rng.uniform(2.0, 5.0);
                    by += 4.0;
                    b.push((bx, by));
                }
                branches.push(b);
            }
            let intensity = [1.0, 0.6, 1.0, 0.8, 0.5, 0.9, 0.3, 0.15][i];
            let stroke = |path: &[(f32, f32)], width: i32, img: &mut Img| {
                for w in path.windows(2) {
                    let ((x0, y0), (x1, y1)) = (w[0], w[1]);
                    let steps = (x1 - x0).abs().max((y1 - y0).abs()) as i32 + 1;
                    for s in 0..=steps {
                        let u = s as f32 / steps as f32;
                        let (x, y) = (x0 + (x1 - x0) * u, y0 + (y1 - y0) * u);
                        for o in -width..=width {
                            let c = if o == 0 {
                                0
                            } else if o.abs() == 1 {
                                2
                            } else {
                                3
                            };
                            let key = (x.round() as i32 + o, y.round() as i32);
                            if !img.contains_key(&key) || c == 0 {
                                img.insert(key, c);
                            }
                        }
                    }
                }
            };
            stroke(&pts, if intensity > 0.7 { 2 } else { 1 }, &mut img);
            for b in &branches {
                stroke(b, if intensity > 0.5 { 1 } else { 0 }, &mut img);
            }
            if intensity < 0.4 {
                for ((kx, ky), c) in img.iter_mut() {
                    if *c != 0 && (kx + ky) % 2 != 0 {
                        *c = 4;
                    }
                }
            }
            for y in 56..64 {
                for x in 20..45 {
                    let d = ((x as f32 - 32.0) / 1.8).hypot(y as f32 - 62.0);
                    if d < 7.0 * intensity {
                        img.entry((x, y)).or_insert(if d < 3.0 { 2 } else { 3 });
                    }
                }
            }
            img
        }

        fn heal(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let (tf, nf) = (t as f32, n as f32);
            for i in 0..9 {
                let p = (tf / nf + i as f32 / 9.0).fract();
                let x = 32.0 + (TAU * (p * 0.8 + i as f32 * 0.37)).sin() * (6 + (i % 4) * 4) as f32;
                let y = 58.0 - p * 50.0;
                let size = if p < 0.75 { 2 } else { 1 };
                let col = if p < 0.4 {
                    0
                } else if p < 0.75 {
                    2
                } else {
                    3
                };
                for o in -size..=size {
                    put(&mut img, x + o as f32, y, col);
                    put(&mut img, x, y + o as f32, col);
                }
            }
            for y in 14..58 {
                for x in 26..39 {
                    let k = (x as f32 - 32.0).abs() / 6.5 + (58 - y) as f32 / 70.0;
                    let on = if k > 0.55 { (x + y + t as i32) % 3 == 0 } else { (x + y) % 2 == 0 };
                    if k < 1.0 && on {
                        img.entry((x, y)).or_insert(if k > 0.55 { 3 } else { 2 });
                    }
                }
            }
            for k in 0..80 {
                let a = TAU * k as f32 / 80.0 + TAU * tf / nf;
                if (3.0 * a - TAU * tf / nf).sin() > 0.0 {
                    put(&mut img, 32.0 + a.cos() * 18.0, 56.0 + a.sin() * 5.0, if a.sin() > 0.0 { 2 } else { 4 });
                }
            }
            img
        }

        fn explosion(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = t as f32 / (n.max(2) - 1) as f32;
            let first = (n as f32 * 0.17).round().max(1.0) as u32;
            if t < first {
                let r = 6.0 + t as f32 * 8.0 * 2.0 / first as f32;
                for y in 0..S {
                    for x in 0..S {
                        let d = (x as f32 - 32.0).hypot(y as f32 - 34.0);
                        if d < r {
                            img.insert((x, y), if d < r * 0.6 { 0 } else { 1 });
                        }
                    }
                }
                return img;
            }
            let big_r = 10.0 + 18.0 * (p * 1.6).min(1.0);
            for y in 0..S {
                for x in 0..S {
                    let d = (x as f32 - 32.0).hypot((y as f32 - 34.0) * 1.1);
                    let nz = loop_noise(x as f32 * 0.22, y as f32 * 0.22, t as f32, n as f32, 5, 0.3);
                    let edge = big_r * (0.82 + 0.3 * nz);
                    if d > edge {
                        continue;
                    }
                    let heat = (1.0 - d / edge) * (1.0 - p) * 1.4 + nz * 0.3 - p * 0.35;
                    let c = if p > 0.45 && d > edge * 0.55 - (p - 0.45) * 20.0 {
                        if p > 0.8 && (x + y) % 2 != 0 {
                            continue;
                        }
                        Some(if nz > 0.45 { 5 } else { 6 })
                    } else {
                        band(heat, &[0.95, 0.7, 0.45, 0.22, -1.0])
                    };
                    if let Some(c) = c {
                        img.insert((x, y), c);
                    }
                }
            }
            let mut rng = Rng::new(3);
            for _ in 0..12 {
                let a = rng.uniform(0.0, TAU);
                let r = big_r + 4.0 + p * 14.0;
                put(&mut img, 32.0 + a.cos() * r, 34.0 + a.sin() * r * 0.9, if p < 0.66 { 2 } else { 4 });
            }
            img
        }

        fn ease_out(p: f32) -> f32 {
            1.0 - (1.0 - p) * (1.0 - p)
        }

        fn bayer4(x: i32, y: i32) -> f32 {
            [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5][((y & 3) * 4 + (x & 3)) as usize] as f32 / 16.0
        }

        fn disc(img: &mut Img, cx: f32, cy: f32, r: f32, mut color: impl FnMut(f32, f32, f32) -> Option<u8>) {
            for y in (cy - r - 1.0) as i32..=(cy + r + 1.0) as i32 {
                for x in (cx - r - 1.0) as i32..=(cx + r + 1.0) as i32 {
                    let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    let d = dx.hypot(dy) / r.max(0.01);
                    if d <= 1.0
                        && let Some(c) = color(dx, dy, d)
                    {
                        img.insert((x, y), c);
                    }
                }
            }
        }

        fn phase(t: u32, n: u32) -> f32 {
            t as f32 / (n.max(2) - 1) as f32
        }

        fn shockwave(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            if p < 0.12 {
                disc(&mut img, 32.0, 40.0, 3.0 + p / 0.12 * 4.0, |_, _, d| Some(if d < 0.5 { 0 } else { 1 }));
            }
            let r0 = 4.0 + 26.0 * ease_out(p);
            let thick = 4.0 * (1.0 - p) + 1.2;
            let shift = (p * 2.0) as u8;
            for y in 0..S {
                for x in 0..S {
                    let (dx, dy) = (x as f32 - 31.5, (y as f32 - 39.5) * 2.0);
                    let d = (dx.hypot(dy) - r0).abs();
                    if d > thick || (p > 0.7 && bayer4(x, y) < (p - 0.7) / 0.3) {
                        continue;
                    }
                    let k = d / thick;
                    img.insert(
                        (x, y),
                        ((if k < 0.35 {
                            0
                        } else if k < 0.7 {
                            1
                        } else {
                            2
                        }) + shift)
                            .min(4),
                    );
                }
            }
            let mut rng = Rng::new(21);
            for _ in 0..10 {
                let a = rng.uniform(0.0, TAU);
                let s = rng.uniform(0.8, 1.2);
                if p < 0.9 {
                    let dist = (r0 + 2.0) * s;
                    let h = 16.0 * s * 4.0 * p * (1.0 - p);
                    put(&mut img, 32.0 + a.cos() * dist, 40.0 + a.sin() * dist * 0.5 - h, if p < 0.6 { 2 } else { 3 });
                }
            }
            img
        }

        fn beam(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            let tf = t as f32;
            if p < 0.3 {
                let q = p / 0.3;
                disc(&mut img, 8.0, 32.0, 2.0 + 7.0 * q, |_, _, d| {
                    Some(if d < 0.35 {
                        0
                    } else if d < 0.7 {
                        1
                    } else {
                        2
                    })
                });
                for i in 0..8 {
                    let a = i as f32 * TAU / 8.0 + q * 2.0;
                    let r = 18.0 * (1.0 - q) + 3.0;
                    put(&mut img, 8.0 + a.cos() * r, 32.0 + a.sin() * r, 1);
                }
            } else if p < 0.8 {
                let q = (p - 0.3) / 0.5;
                let w = (6.0 * (std::f32::consts::PI * q).sin()).max(1.5) + 1.0;
                for x in 6..62 {
                    let xf = x as f32;
                    let wobble = (xf * 0.5 + tf * 1.7).sin() * 0.8;
                    let half = w * (0.85 + 0.15 * (xf * 0.9 - tf * 2.3).sin());
                    for y in 0..S {
                        let d = (y as f32 - 32.0 - wobble * 0.3).abs();
                        if d <= half {
                            let k = d / half;
                            img.insert(
                                (x, y),
                                if k < 0.3 {
                                    0
                                } else if k < 0.6 {
                                    1
                                } else if k < 0.85 {
                                    2
                                } else {
                                    3
                                },
                            );
                        }
                    }
                }
                disc(&mut img, 8.0, 32.0, 8.0, |_, _, d| Some(if d < 0.45 { 0 } else { 1 }));
                for i in 0..6 {
                    let a = std::f32::consts::PI * (0.6 + 0.8 * i as f32 / 5.0);
                    let len = 4.0 + ((i * 3 + t as i32) % 5) as f32;
                    for s in 0..len as i32 {
                        put(&mut img, 58.0 + a.cos() * s as f32, 32.0 + a.sin() * s as f32, if s < 2 { 1 } else { 2 });
                    }
                }
            } else {
                let q = (p - 0.8) / 0.2;
                for x in 6..62 {
                    if ((x + t as i32 * 3).rem_euclid(4) as f32) < 3.0 - q * 3.0 {
                        img.insert((x, 32), if q < 0.5 { 2 } else { 3 });
                    }
                }
                if q < 0.9 {
                    disc(&mut img, 8.0, 32.0, 8.0 * (1.0 - q) + 0.6, |_, _, d| Some(if d < 0.5 { 1 } else { 2 }));
                }
            }
            img
        }

        fn portal(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let ph = TAU * t as f32 / n as f32;
            for y in 0..S {
                for x in 0..S {
                    let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
                    let r = dx.hypot(dy);
                    if r > 27.0 {
                        continue;
                    }
                    if r < 7.0 {
                        img.insert((x, y), if hnoise(x, y, t as i32, 4) > 0.96 { 1 } else { 5 });
                        continue;
                    }
                    let arm = (3.0 * dy.atan2(dx) - r * 0.38 + ph).sin();
                    let c = if r > 24.0 {
                        if arm > -0.2 { 2 } else { 3 }
                    } else if arm > 0.35 {
                        1 + ((r - 7.0) / 17.0 * 2.0) as u8
                    } else if arm > -0.1 {
                        4
                    } else {
                        5
                    };
                    img.insert((x, y), c);
                }
            }
            for i in 0..6 {
                let a = ph + i as f32 * TAU / 6.0;
                let (x, y) = (31.5 + a.cos() * 29.5, 31.5 + a.sin() * 29.5);
                put(&mut img, x, y, 0);
                for (ox, oy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    put(&mut img, x + ox, y + oy, 1);
                }
            }
            img
        }

        fn ice(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            let mut rng = Rng::new(5);
            let spikes: Vec<(f32, f32)> =
                (0..7).map(|i| (i as f32 * TAU / 7.0 + rng.uniform(-0.3, 0.3), rng.uniform(14.0, 26.0))).collect();
            if p < 0.65 {
                let grow = ease_out((p / 0.35).min(1.0));
                for &(ang, len) in &spikes {
                    let (cx, cy) = (ang.cos(), ang.sin());
                    let l = len * grow;
                    for y in 0..S {
                        for x in 0..S {
                            let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
                            let u = dx * cx + dy * cy;
                            let v = (-dx * cy + dy * cx).abs();
                            let half = 1.8 * (1.0 - u / l.max(0.1)) + 0.4;
                            if u > 0.0 && u < l && v < half {
                                let c = if v < 0.6 {
                                    0
                                } else if v > half - 0.7 {
                                    3
                                } else if u < l * 0.5 {
                                    2
                                } else {
                                    1
                                };
                                img.insert((x, y), c);
                            }
                        }
                    }
                }
                let core = 5.0 * grow;
                for y in 0..S {
                    for x in 0..S {
                        let d = (x as f32 - 31.5).abs() + (y as f32 - 31.5).abs();
                        if d <= core {
                            img.insert((x, y), if d < core * 0.5 { 0 } else { 1 });
                        }
                    }
                }
            } else {
                let q = (p - 0.65) / 0.35;
                for &(ang, len) in &spikes {
                    for (j, f) in [0.4f32, 0.7, 1.0].iter().enumerate() {
                        let dist = len * f * (1.0 + 0.6 * q);
                        let (x, y) = (31.5 + ang.cos() * dist, 31.5 + ang.sin() * dist + 12.0 * q * q);
                        let s = 2.0 * (1.0 - q) + 1.0;
                        for oy in -(s as i32)..=(s as i32) {
                            for ox in -(s as i32)..=(s as i32) {
                                let (px, py) = (x as i32 + ox, y as i32 + oy);
                                if (ox.abs() + oy.abs()) as f32 <= s && !(q > 0.5 && bayer4(px, py) < (q - 0.5) * 2.0) {
                                    img.insert((px, py), if ox.abs() + oy.abs() == 0 { 1 } else { 2 + (j % 2) as u8 });
                                }
                            }
                        }
                    }
                }
            }
            let mist_r = 10.0 + 16.0 * p;
            for y in 0..S {
                for x in 0..S {
                    let r = (x as f32 - 31.5).hypot(y as f32 - 31.5);
                    if (r - mist_r).abs() < 2.0 && bayer4(x, y) < 0.35 * (1.0 - p) {
                        img.entry((x, y)).or_insert(4);
                    }
                }
            }
            img
        }

        fn poison(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let ph = t as f32 / n as f32;
            for y in 0..S {
                for x in 0..S {
                    let (dx, dy) = ((x as f32 - 31.5) / 22.0, (y as f32 - 53.5) / 6.0);
                    let d = dx.hypot(dy);
                    if d <= 1.0 {
                        let ripple = ((d * 9.0 - ph * TAU).sin() > 0.8) as u8;
                        img.insert((x, y), if y < 51 { 2 - ripple } else { 3 + (d > 0.85) as u8 });
                    }
                }
            }
            for i in 0..8 {
                let p = (ph + i as f32 / 8.0).fract();
                let x = 20.0 + ((i * 7) % 24) as f32 + (TAU * (p + i as f32 * 0.3)).sin() * 2.0;
                let y = 52.0 - p * 44.0;
                let r = 1.5 + p * 2.5;
                if p > 0.88 {
                    for k in 0..8 {
                        let a = k as f32 * TAU / 8.0;
                        put(&mut img, x + a.cos() * (r + 1.5), y + a.sin() * (r + 1.5), 1);
                    }
                } else {
                    disc(&mut img, x, y, r, |dx, dy, d| {
                        Some(if dx < -r * 0.3 && dy < -r * 0.3 {
                            0
                        } else if d > 0.75 {
                            2
                        } else {
                            1
                        })
                    });
                }
                if i % 3 == 0 && p < 0.5 {
                    put(&mut img, x + 3.0, y - 2.0, 5);
                }
            }
            img
        }

        fn splash(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            if p < 0.15 {
                let y = 10.0 + p / 0.15 * 36.0;
                disc(&mut img, 32.0, y, 2.2, |dx, dy, _| Some(if dx < 0.0 && dy < 0.0 { 0 } else { 1 }));
                return img;
            }
            let q = (p - 0.15) / 0.85;
            let rx = 6.0 + 18.0 * ease_out(q);
            let ry = rx * 0.3;
            for y in 0..S {
                for x in 0..S {
                    let d = ((x as f32 - 31.5) / rx).hypot((y as f32 - 50.5) / ry);
                    if (0.75..=1.0).contains(&d) && !(q > 0.6 && bayer4(x, y) < (q - 0.6) * 2.5) {
                        img.insert((x, y), if y < 50 { 2 } else { 3 });
                    }
                }
            }
            let h = 10.0 * (std::f32::consts::PI * (q * 1.5).min(1.0)).sin();
            for k in 0..8 {
                let a = k as f32 * TAU / 8.0;
                let (bx, by) = (31.5 + a.cos() * rx, 50.5 + a.sin() * ry);
                if a.sin() > 0.2 {
                    continue;
                }
                for s in 0..h as i32 {
                    put(&mut img, bx, by - s as f32, if s as f32 > h - 2.0 { 0 } else { 1 });
                }
            }
            let mut rng = Rng::new(9);
            for _ in 0..12 {
                let vx = rng.uniform(-1.2, 1.2);
                let vy = rng.uniform(-2.6, -1.6);
                let tt = q * 14.0;
                let (x, y) = (32.0 + vx * tt * 1.6, 48.0 + vy * tt + 0.18 * tt * tt);
                if y < 56.0 {
                    disc(&mut img, x, y, 1.1, |_, _, _| Some(1));
                }
            }
            img
        }

        fn dash(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            let mut rng = Rng::new(13);
            for i in 0..9 {
                let y = rng.uniform(14.0, 50.0) as i32;
                let len = rng.uniform(18.0, 44.0);
                let head = 60.0 - 70.0 * ease_out(p) + rng.uniform(-6.0, 6.0);
                let tail = head + len * (std::f32::consts::PI * p).sin();
                for x in head.max(0.0) as i32..tail.min(63.0) as i32 {
                    let k = (x as f32 - head) / (tail - head).max(1.0);
                    let c = if k < 0.15 {
                        0
                    } else if k < 0.4 {
                        1
                    } else if k < 0.75 {
                        2
                    } else {
                        3
                    };
                    img.insert((x, y), c);
                    if i % 3 == 0 && k < 0.6 {
                        img.insert((x, y + 1), (c + 1).min(3));
                    }
                }
            }
            if p > 0.3 {
                for k in 0..4 {
                    let x = 50.0 - k as f32 * 7.0 + p * 6.0;
                    let r = 2.0 + k as f32 * 0.6 + p * 1.5;
                    disc(&mut img, x, 54.0 - p * 3.0, r, |_, dy, _| {
                        if p > 0.7 && bayer4(x as i32, k) > 1.6 - p { None } else { Some(if dy < 0.0 { 2 } else { 3 }) }
                    });
                }
            }
            img
        }

        fn shield(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let ph = t as f32 / n as f32;
            let big_r = 25.0;
            for y in 0..S {
                for x in 0..S {
                    let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
                    let r = dx.hypot(dy);
                    if r > big_r + 0.5 {
                        continue;
                    }
                    let c = if r > big_r - 1.5 {
                        let a = dy.atan2(dx).to_degrees().rem_euclid(360.0);
                        if (200.0..290.0).contains(&a) { 0 } else { 1 }
                    } else if ((x + y) as f32 - ph * 128.0).rem_euclid(64.0) < 5.0 {
                        2
                    } else if (x.rem_euclid(8) == 0 || (x + 2 * y).rem_euclid(16) == 0 || (x - 2 * y).rem_euclid(16) == 0)
                        && (x + y) % 2 == 0
                    {
                        3
                    } else if bayer4(x, y) < 0.12 {
                        4
                    } else {
                        continue;
                    };
                    img.insert((x, y), c);
                }
            }
            img
        }

        fn levelup(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            for y in 0..S {
                for x in 0..S {
                    let d = ((x as f32 - 31.5) / 18.0).hypot((y as f32 - 56.5) / 4.0);
                    if d <= 1.0 && bayer4(x, y) < 0.6 * (1.0 - d) {
                        img.insert((x, y), if p > 0.7 { 4 } else { 3 });
                    }
                }
            }
            if p < 0.75 {
                let w = 10.0 * (std::f32::consts::PI * (p / 0.75)).sin();
                for y in 4..58 {
                    let width = w * (1.0 - (58 - y) as f32 / 120.0);
                    for x in 0..S {
                        let k = (x as f32 - 31.5).abs() / width.max(0.1);
                        if k <= 1.0 && !(y < 14 && bayer4(x, y) < (14 - y) as f32 / 10.0) {
                            img.insert(
                                (x, y),
                                if k < 0.3 {
                                    0
                                } else if k < 0.6 {
                                    1
                                } else {
                                    2
                                },
                            );
                        }
                    }
                }
            }
            for i in 0..3 {
                let q = p * 1.3 - i as f32 * 0.25;
                if !(0.0..=1.0).contains(&q) {
                    continue;
                }
                let y = 56.0 - q * 48.0;
                let rx = 16.0 * (1.0 - q * 0.4);
                for k in 0..64 {
                    let a = k as f32 * TAU / 64.0;
                    put(&mut img, 32.0 + a.cos() * rx, y + a.sin() * 4.0, if a.sin() > 0.0 { 2 } else { 3 });
                }
            }
            for i in 0..6 {
                let q = (p * 1.5 + i as f32 / 6.0).fract();
                let (x, y) = (32.0 + (i as f32 * 1.7).sin() * 14.0, 56.0 - q * 50.0);
                put(&mut img, x, y, 0);
                if q < 0.6 {
                    for (ox, oy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        put(&mut img, x + ox, y + oy, 1);
                    }
                }
            }
            img
        }

        fn pickup(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let ph = t as f32 / n as f32;
            let ang = TAU * ph;
            let bob = (TAU * ph).sin() * 1.5;
            let (cx, cy) = (31.5, 29.5 + bob);
            let rx = (9.0 * ang.cos().abs()).max(1.3);
            for y in 0..S {
                for x in 0..S {
                    let d = ((x as f32 + 0.5 - cx - 0.5) / rx).hypot((y as f32 + 0.5 - cy - 0.5) / 10.0);
                    if d > 1.0 {
                        continue;
                    }
                    let front = ang.cos() > 0.0;
                    let c = if d > 0.8 {
                        3
                    } else if (x as f32) < cx - rx * 0.35 {
                        if front { 0 } else { 2 }
                    } else if d < 0.5 && front {
                        1
                    } else if front {
                        2
                    } else {
                        3
                    };
                    img.insert((x, y), c);
                }
            }
            for y in 0..S {
                for x in 0..S {
                    let d = ((x as f32 - 31.5) / (7.0 - bob)).hypot((y as f32 - 46.5) / 1.6);
                    if d <= 1.0 {
                        img.entry((x, y)).or_insert(4);
                    }
                }
            }
            for (i, (sx, sy)) in [(18.0, 16.0), (46.0, 22.0), (40.0, 8.0)].iter().enumerate() {
                let size = [0, 1, 2, 1][(t as usize + i * 2) % 4];
                if size == 0 {
                    continue;
                }
                put(&mut img, *sx, *sy, 0);
                for s in 1..=size {
                    for (ox, oy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        put(&mut img, sx + ox * s as f32, sy + oy * s as f32, 1);
                    }
                }
            }
            img
        }

        fn dust(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            for side in [-1.0f32, 1.0] {
                for k in 0..4 {
                    let kf = k as f32;
                    let x = 32.0 + side * (4.0 + p * 22.0 * (0.6 + 0.12 * kf) + kf * 2.0);
                    let y = 50.0 - p * 2.0 * kf;
                    let r = (1.5 + kf * 0.7) * (0.6 + p * 0.9);
                    disc(&mut img, x, y, r, |dx, dy, d| {
                        let px = (x + dx) as i32;
                        let py = (y + dy) as i32;
                        if p > 0.45 && bayer4(px, py) < (p - 0.45) * 1.8 {
                            return None;
                        }
                        band(1.0 - d * 0.6 - (dy > 0.0) as u8 as f32 * 0.3 - p * 0.3, &[0.6, 0.4, 0.2, -1.0])
                    });
                }
            }
            img
        }

        fn holy(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            let env = if p < 0.2 { p / 0.2 } else { 1.0 - (p - 0.2) / 0.8 };
            for y in 0..S {
                for x in 0..S {
                    let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
                    let r = dx.hypot(dy);
                    let ray = (12.0 * (dy.atan2(dx) - p * 0.4)).cos();
                    if r < 30.0 * env.max(0.25) && ray > 0.86 && bayer4(x, y) < 1.2 - r / 30.0 {
                        img.insert(
                            (x, y),
                            if r < 8.0 {
                                0
                            } else if ray > 0.96 {
                                1
                            } else {
                                2
                            },
                        );
                    }
                }
            }
            let big_r = 10.0 + 12.0 * ease_out(p);
            for k in 0..96 {
                let a = k as f32 * TAU / 96.0;
                if p < 0.85 {
                    put(&mut img, 31.5 + a.cos() * big_r, 31.5 + a.sin() * big_r, if p < 0.5 { 1 } else { 3 });
                }
            }
            let len = (6.0 + 10.0 * env) as i32;
            for s in -len..=len {
                put(&mut img, 31.5 + s as f32, 31.5, 0);
                put(&mut img, 31.5, 31.5 + s as f32, 0);
            }
            disc(&mut img, 32.0, 32.0, 4.0 * env + 0.6, |_, _, _| Some(0));
            img
        }

        fn shadow(t: u32, n: u32) -> Img {
            let mut img = fire(t, n, false);
            for c in img.values_mut() {
                *c = [4, 4, 3, 2, 1, 0][*c as usize];
            }
            img
        }

        fn teleport(t: u32, n: u32) -> Img {
            let mut img = Img::new();
            let p = phase(t, n);
            if p < 0.35 {
                let q = p / 0.35;
                let w = 12.0 * (1.0 - q) + 1.0;
                for y in 6..58 {
                    for x in 0..S {
                        let k = (x as f32 - 31.5).abs() / w;
                        if k <= 1.0 && !((y + t as i32) % 3 == 0 && k > 0.5) {
                            img.insert(
                                (x, y),
                                if k < 0.25 {
                                    0
                                } else if k < 0.6 {
                                    1
                                } else {
                                    2
                                },
                            );
                        }
                    }
                }
            } else if p < 0.5 {
                let q = (p - 0.35) / 0.15;
                let half = (26.0 * (1.0 - q)) as i32 + 2;
                for y in 32 - half..32 + half {
                    img.insert((32, y), 0);
                    img.insert((31, y), 1);
                }
            } else {
                let q = (p - 0.5) / 0.5;
                let mut rng = Rng::new(17);
                for _ in 0..14 {
                    let vx = rng.uniform(-1.0, 1.0);
                    let v = rng.uniform(0.5, 1.0);
                    if hnoise(t as i32, (vx * 100.0) as i32, 0, 2) < q * 0.8 {
                        continue;
                    }
                    put(
                        &mut img,
                        32.0 + vx * q * 20.0,
                        32.0 - q * 28.0 * v,
                        if q < 0.3 {
                            1
                        } else if q < 0.7 {
                            2
                        } else {
                            3
                        },
                    );
                }
            }
            for y in 0..S {
                for x in 0..S {
                    let d = ((x as f32 - 31.5) / 12.0).hypot((y as f32 - 58.5) / 3.0);
                    if (0.7..=1.0).contains(&d) && bayer4(x, y) < 1.0 - p {
                        img.insert((x, y), 3);
                    }
                }
            }
            img
        }

        fn frame_image(kind: &str, t: u32, n: u32) -> Img {
            match kind {
                "fire" => fire(t, n, false),
                "campfire" => fire(t, n, true),
                "slash" => slash(t, n),
                "hit" => hit(t, n),
                "magic" => magic(t, n),
                "smoke" => smoke(t, n),
                "lightning" => lightning(t, n),
                "heal" => heal(t, n),
                "shockwave" => shockwave(t, n),
                "beam" => beam(t, n),
                "portal" => portal(t, n),
                "ice" => ice(t, n),
                "poison" => poison(t, n),
                "splash" => splash(t, n),
                "dash" => dash(t, n),
                "shield" => shield(t, n),
                "levelup" => levelup(t, n),
                "pickup" => pickup(t, n),
                "dust" => dust(t, n),
                "holy" => holy(t, n),
                "shadow" => shadow(t, n),
                "teleport" => teleport(t, n),
                _ => explosion(t, n),
            }
        }

        /// Adds frames until `start + n` exist (new frames get `ms`). New frames hold the other
        /// layers of the previous frame (shared cels), so a still scene stays visible while the
        /// effect layer animates.
        fn ensure_frames(doc: &mut Document, start: usize, n: usize, ms: u32, except: u32) -> CmdResult<()> {
            if start > doc.frames.len() {
                return Err(CmdError::invalid(format!("'frame' {start} is past the last frame ({})", doc.frames.len())));
            }
            while doc.frames.len() < start + n {
                if doc.frames.len() >= crate::project::MAX_FRAMES {
                    return Err(CmdError::new("limit", "frame safety limit reached"));
                }
                let at = doc.frames.len();
                let id = doc.insert_frame(at, ms);
                if at > 0 {
                    let prev = doc.frames[at - 1].id;
                    let held: Vec<(u32, std::sync::Arc<crate::pixel::PixelGrid>)> =
                        doc.cels.iter().filter(|((l, f), _)| *f == prev && *l != except).map(|((l, _), g)| (*l, g.clone())).collect();
                    for (l, g) in held {
                        doc.cels.insert((l, id), g);
                    }
                }
            }
            Ok(())
        }

        fn add_tag(doc: &mut Document, a: &Json, start: usize, n: usize, looped: bool) -> CmdResult<()> {
            if let Some(tag) = opt_str(a, "tag")? {
                let name = tag.trim().to_string();
                if !name.is_empty() && !doc.tags.iter().any(|t| t.name == name) {
                    doc.tags.push(Tag {
                        name,
                        from_id: doc.frames[start].id,
                        to_id: doc.frames[start + n - 1].id,
                        direction: Direction::Forward,
                        repeat: if looped { 0 } else { 1 },
                    });
                }
            }
            Ok(())
        }

        fn writable_layer(doc: &Document, a: &Json) -> CmdResult<usize> {
            let li = layer_index(doc, a, "layer")?;
            if doc.layers[li].locked {
                return Err(CmdError::new("locked", format!("layer '{}' is locked", doc.layers[li].name)));
            }
            Ok(li)
        }

        /// Layer for the additive "lit" glow pass (created above the effect layer when missing).
        fn glow_layer(doc: &mut Document, a: &Json, li: usize) -> CmdResult<u32> {
            let name = opt_str(a, "glow_layer")?.map(str::to_string).unwrap_or_else(|| format!("{} Glow", doc.layers[li].name));
            if let Some(l) = doc.layers.iter().find(|l| l.name == name) {
                return Ok(l.id);
            }
            if doc.layers.len() >= crate::project::MAX_LAYERS {
                return Err(CmdError::new("limit", "layer limit reached"));
            }
            let id = doc.insert_layer(&name, li + 1);
            if let Some(l) = doc.layers.iter_mut().find(|l| l.id == id) {
                l.blend = crate::layers::Blend::Add;
            }
            Ok(id)
        }

        /// Dithered halo around the opaque pixels of `mask` (w×h), at most `radius` px away.
        fn halo(mask: &[bool], w: i64, h: i64, radius: i64) -> Vec<(i64, i64)> {
            let mut out = Vec::new();
            for y in 0..h {
                for x in 0..w {
                    if mask[(y * w + x) as usize] {
                        continue;
                    }
                    let mut best = f32::MAX;
                    for dy in -radius..=radius {
                        for dx in -radius..=radius {
                            let (nx, ny) = (x + dx, y + dy);
                            if nx >= 0 && ny >= 0 && nx < w && ny < h && mask[(ny * w + nx) as usize] {
                                best = best.min(((dx * dx + dy * dy) as f32).sqrt());
                            }
                        }
                    }
                    let level = 1.0 - best / (radius as f32 + 0.5);
                    if level > 0.0 && level * 0.9 > bayer4(x as i32, y as i32) {
                        out.push((x, y));
                    }
                }
            }
            out
        }

        /// Generates a complete animated effect into consecutive frames of a layer.
        pub fn fx_generate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let kind = req_str(a, "kind")?;
            let Some(&(_, default_frames, default_ms, looped)) = KINDS.iter().find(|(k, _, _, _)| *k == kind) else {
                let names: Vec<&str> = KINDS.iter().map(|k| k.0).collect();
                return Err(CmdError::invalid(format!("unknown effect '{kind}' ({})", names.join(", "))));
            };
            let n = opt_i64(a, "frames")?.unwrap_or(default_frames as i64).clamp(1, 64) as u32;
            let ms = opt_i64(a, "frame_ms")?.unwrap_or(default_ms as i64).clamp(1, 60_000) as u32;
            let size = opt_i64(a, "size")?.unwrap_or(doc.width.min(doc.height) as i64).clamp(4, 1024);
            let cx = opt_i64(a, "x")?.unwrap_or(doc.width as i64 / 2);
            let cy = opt_i64(a, "y")?.unwrap_or(doc.height as i64 / 2);
            let start = opt_i64(a, "frame")?.unwrap_or(0).max(0) as usize;
            let timing = opt_str(a, "timing")?.unwrap_or("auto");
            if !matches!(timing, "auto" | "uniform") {
                return Err(CmdError::invalid("'timing' must be auto or uniform"));
            }
            let mut colors: Vec<u32> = palette(kind).iter().map(|h| Rgba::parse_hex(h).expect("palette").to_pixel()).collect();
            if let Some(Json::Arr(list)) = a.get("colors") {
                for (i, c) in list.iter().enumerate().take(colors.len()) {
                    colors[i] = color_value(doc, c, "colors")?;
                }
            }
            let glow = match a.get("glow") {
                None | Some(Json::Null) | Some(Json::Bool(false)) => None,
                Some(Json::Bool(true)) => Some(colors[colors.len().min(3) - 1]),
                Some(v) => Some(color_value(doc, v, "glow")?),
            };
            let li = writable_layer(doc, a)?;
            let layer_id = doc.layers[li].id;
            let glow_id = match glow {
                Some(_) => Some(glow_layer(doc, a, li)?),
                None => None,
            };
            ensure_frames(doc, start, n as usize, ms, layer_id)?;
            let images: Vec<Img> = (0..n).map(|t| frame_image(kind, t, n)).collect();
            // Timing (slow-in/out, held peak): one-shots rush the anticipation, hold the peak
            // frame and ease out of the last one; loops stay even so they never stutter.
            let peak = images.iter().enumerate().max_by_key(|(_, img)| img.len()).map(|(i, _)| i).unwrap_or(0);
            let duration = |t: usize| -> u32 {
                if timing == "uniform" || looped || n < 3 {
                    return ms;
                }
                let f = if t == peak {
                    1.7
                } else if t < peak {
                    0.8
                } else if t + 1 == n as usize {
                    1.3
                } else {
                    1.0
                };
                ((ms as f32 * f).round() as u32).max(1)
            };
            let (x0, y0) = (cx - size / 2, cy - size / 2);
            let mut changed = 0u64;
            for (t, img) in images.iter().enumerate() {
                let fi = start + t;
                doc.frames[fi].duration_ms = duration(t);
                let frame_id = doc.frames[fi].id;
                let mut mask = vec![false; (size * size) as usize];
                let grid = doc.cel_mut(layer_id, frame_id);
                for y in 0..size {
                    for x in 0..size {
                        let (sx, sy) = ((x * S as i64 / size) as i32, (y * S as i64 / size) as i32);
                        let v = img.get(&(sx, sy)).map(|&c| colors[c as usize]).unwrap_or(TRANSPARENT);
                        mask[(y * size + x) as usize] = v != TRANSPARENT;
                        changed += grid.set(x0 + x, y0 + y, v) as u64;
                    }
                }
                if let (Some(gid), Some(gc)) = (glow_id, glow) {
                    let radius = (size / 16).clamp(2, 8);
                    let grid = doc.cel_mut(gid, frame_id);
                    grid.clear();
                    for (x, y) in halo(&mask, size, size, radius) {
                        changed += grid.set(x0 + x, y0 + y, gc) as u64;
                    }
                }
            }
            add_tag(doc, a, start, n as usize, looped)?;
            let durations: Vec<Json> = (0..n as usize).map(|t| Json::from(duration(t))).collect();
            Ok(Outcome::new(
                Json::obj()
                    .with("kind", kind)
                    .with("frames", n)
                    .with("firstFrame", start)
                    .with("loop", looped)
                    .with("durations", Json::Arr(durations))
                    .with("glow", glow_id.is_some())
                    .with("changed", changed),
                changed,
            ))
        }

        fn pair(a: &Json, key: &str, default: (f32, f32)) -> CmdResult<(f32, f32)> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(default),
                Some(Json::Arr(v)) if v.len() == 2 => match (v[0].as_f64(), v[1].as_f64()) {
                    (Some(x), Some(y)) if x.is_finite() && y.is_finite() => Ok((x as f32, y as f32)),
                    _ => Err(CmdError::invalid(format!("'{key}' must be [a, b] numbers"))),
                },
                Some(v) => match v.as_f64() {
                    Some(x) if x.is_finite() => Ok((x as f32, x as f32)),
                    _ => Err(CmdError::invalid(format!("'{key}' must be a number or [a, b]"))),
                },
            }
        }

        /// General particle emitter for custom effects: sparks, debris, dust, embers, rain,
        /// snow, confetti, bubbles. Physics per frame (speed, gravity, drag), a color ramp over
        /// each particle's life, size over life, five shapes, burst or looping stream.
        pub fn fx_particles(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let n = opt_i64(a, "frames")?.unwrap_or(8).clamp(1, 64) as usize;
            let ms = opt_i64(a, "frame_ms")?.unwrap_or(60).clamp(1, 60_000) as u32;
            let count = opt_i64(a, "count")?.unwrap_or(24).clamp(1, 2000) as usize;
            let origin = pair(a, "origin", (doc.width as f32 / 2.0, doc.height as f32 / 2.0))?;
            let area = pair(a, "area", (0.0, 0.0))?;
            let angle = super::args::opt_f64(a, "angle")?.unwrap_or(-90.0) as f32;
            let spread = super::args::opt_f64(a, "spread")?.unwrap_or(360.0).clamp(0.0, 360.0) as f32;
            let speed = pair(a, "speed", (1.0, 3.0))?;
            let gravity = super::args::opt_f64(a, "gravity")?.unwrap_or(0.15) as f32;
            let drag = super::args::opt_f64(a, "drag")?.unwrap_or(0.02).clamp(0.0, 0.95) as f32;
            let life = pair(a, "life", (n as f32 * 0.6, n as f32))?;
            let size = pair(a, "size", (2.0, 1.0))?;
            let shape = opt_str(a, "shape")?.unwrap_or("dot");
            if !matches!(shape, "dot" | "square" | "plus" | "star" | "streak") {
                return Err(CmdError::invalid("'shape' must be dot, square, plus, star or streak"));
            }
            let emit = opt_str(a, "emit")?.unwrap_or("burst");
            if !matches!(emit, "burst" | "stream") {
                return Err(CmdError::invalid("'emit' must be burst or stream"));
            }
            let looped = super::args::opt_bool(a, "loop")?.unwrap_or(emit == "stream");
            let seed = opt_i64(a, "seed")?.unwrap_or(1) as u64;
            let colors: Vec<u32> = match a.get("colors") {
                Some(Json::Arr(list)) if !list.is_empty() => {
                    list.iter().map(|c| color_value(doc, c, "colors")).collect::<CmdResult<_>>()?
                }
                _ => ["#FFFFFF", "#FFE9A0", "#FFB040", "#C04020"].iter().map(|h| Rgba::parse_hex(h).expect("ramp").to_pixel()).collect(),
            };
            let clear = super::args::opt_bool(a, "clear")?.unwrap_or(true);
            let start = opt_i64(a, "frame")?.unwrap_or(0).max(0) as usize;
            let li = writable_layer(doc, a)?;
            let layer_id = doc.layers[li].id;
            ensure_frames(doc, start, n, ms, layer_id)?;
            let mut rng = Rng::new(seed);
            struct P {
                spawn: f32,
                x: f32,
                y: f32,
                vx: f32,
                vy: f32,
                life: f32,
            }
            let parts: Vec<P> = (0..count)
                .map(|i| {
                    let a0 = (angle + rng.uniform(-spread / 2.0, spread / 2.0)).to_radians();
                    let v = rng.uniform(speed.0, speed.1);
                    P {
                        spawn: if emit == "burst" { 0.0 } else { i as f32 * n as f32 / count as f32 },
                        x: origin.0 + rng.uniform(-area.0, area.0),
                        y: origin.1 + rng.uniform(-area.1, area.1),
                        vx: a0.cos() * v,
                        vy: a0.sin() * v,
                        life: rng.uniform(life.0, life.1).max(1.0),
                    }
                })
                .collect();
            let mut changed = 0u64;
            for t in 0..n {
                let frame_id = doc.frames[start + t].id;
                doc.frames[start + t].duration_ms = ms;
                let grid = doc.cel_mut(layer_id, frame_id);
                if clear {
                    grid.clear();
                }
                for p in &parts {
                    let mut age = t as f32 - p.spawn;
                    if looped {
                        age = age.rem_euclid(n as f32);
                    }
                    if age < 0.0 || age > p.life {
                        continue;
                    }
                    let (mut x, mut y, mut vx, mut vy) = (p.x, p.y, p.vx, p.vy);
                    for _ in 0..age as u32 {
                        x += vx;
                        y += vy;
                        vx *= 1.0 - drag;
                        vy = vy * (1.0 - drag) + gravity;
                    }
                    let q = (age / p.life).clamp(0.0, 0.999);
                    let c = colors[(q * colors.len() as f32) as usize];
                    let s = (size.0 + (size.1 - size.0) * q).round().max(1.0) as i64;
                    let (ix, iy) = (x.round() as i64, y.round() as i64);
                    let mut plot = |px: i64, py: i64| changed += grid.set(px, py, c) as u64;
                    match shape {
                        "square" => {
                            for oy in 0..s {
                                for ox in 0..s {
                                    plot(ix + ox - s / 2, iy + oy - s / 2);
                                }
                            }
                        }
                        "plus" => {
                            plot(ix, iy);
                            for k in 1..s {
                                plot(ix + k, iy);
                                plot(ix - k, iy);
                                plot(ix, iy + k);
                                plot(ix, iy - k);
                            }
                        }
                        "star" => {
                            for (dx, dy, _) in crate::pixelart::charart::sparkle_shape(s as u32, "star4") {
                                plot(ix + dx as i64, iy + dy as i64);
                            }
                        }
                        "streak" => {
                            let len = (vx.hypot(vy) * s as f32).max(1.0);
                            let steps = len.ceil() as i64;
                            for k in 0..=steps {
                                let f = k as f32 / steps as f32;
                                plot(
                                    (x - vx / vx.hypot(vy).max(0.01) * len * f).round() as i64,
                                    (y - vy / vx.hypot(vy).max(0.01) * len * f).round() as i64,
                                );
                            }
                        }
                        _ => {
                            let r = s as f32 / 2.0;
                            for oy in -(s / 2)..=(s / 2) {
                                for ox in -(s / 2)..=(s / 2) {
                                    if ((ox * ox + oy * oy) as f32) <= r * r + 0.5 {
                                        plot(ix + ox, iy + oy);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            add_tag(doc, a, start, n, looped)?;
            Ok(Outcome::new(Json::obj().with("frames", n).with("particles", count).with("loop", looped).with("changed", changed), changed))
        }

        /// Turns a still sprite into an animation (secondary motion): breathe, bob, float, sway
        /// (follow-through: the top lags), squash (area-preserving), shake, pulse (glow) or hurt
        /// (white hit-flash, then a decaying shake). Frame 0 stays the original pose; extremes
        /// are held longer (slow-in/out). Every layer with content moves together unless
        /// `layer` restricts it, so multi-layer sprites stay aligned.
        pub fn anim_generate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            const KINDS: [(&str, u32, f32); 8] = [
                ("breathe", 140, 1.0),
                ("bob", 120, 1.0),
                ("float", 110, 2.0),
                ("sway", 120, 2.0),
                ("squash", 90, 0.12),
                ("shake", 50, 1.0),
                ("pulse", 100, 0.35),
                ("hurt", 60, 1.0),
            ];
            let kind = req_str(a, "kind")?;
            let Some(&(_, default_ms, default_amount)) = KINDS.iter().find(|k| k.0 == kind) else {
                let names: Vec<&str> = KINDS.iter().map(|k| k.0).collect();
                return Err(CmdError::invalid(format!("unknown motion '{kind}' ({})", names.join(", "))));
            };
            let n = opt_i64(a, "frames")?.unwrap_or(8).clamp(2, 64) as usize;
            let ms = opt_i64(a, "frame_ms")?.unwrap_or(default_ms as i64).clamp(1, 60_000) as u32;
            let amount = super::args::opt_f64(a, "amount")?.map(|v| v as f32).unwrap_or(default_amount).clamp(-64.0, 64.0);
            let start = opt_i64(a, "frame")?.unwrap_or(0).max(0) as usize;
            if start >= doc.frames.len() {
                return Err(CmdError::not_found("'frame' (the source pose) does not exist"));
            }
            let source = doc.frames[start].id;
            let only = match a.get("layer") {
                Some(v) if !v.is_null() => Some(doc.layers[layer_index(doc, a, "layer")?].id),
                _ => None,
            };
            let layers: Vec<(u32, bool)> = doc
                .layers
                .iter()
                .filter(|l| doc.cel(l.id, source).is_some_and(|g| !g.is_empty()))
                .map(|l| (l.id, only.is_none_or(|o| o == l.id) && !l.locked))
                .collect();
            if layers.iter().all(|(_, m)| !m) {
                return Err(CmdError::invalid("nothing to animate: the source frame has no unlocked pixels"));
            }
            // Pivot: bottom-center of the moving content.
            let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
            for (id, moving) in &layers {
                if let (true, Some(g)) = (*moving, doc.cel(*id, source))
                    && let Some((bx, by, bw, bh)) = g.bounds()
                {
                    (x0, y0) = (x0.min(bx as i64), y0.min(by as i64));
                    (x1, y1) = (x1.max((bx + bw) as i64 - 1), y1.max((by + bh) as i64 - 1));
                }
            }
            let (pivot, cx, height) = (y1 as f32 + 0.5, (x0 + x1) as f32 / 2.0 + 0.5, (y1 - y0 + 1).max(1) as f32);
            ensure_frames(doc, start, n, ms, u32::MAX)?;
            let mut changed = 0u64;
            for k in 0..n {
                let phi = TAU * k as f32 / n as f32;
                let hold = match kind {
                    "breathe" => phi.cos().abs(),
                    "shake" | "hurt" => 0.0,
                    _ => phi.sin().abs(),
                };
                doc.frames[start + k].duration_ms = ((ms as f32 * (1.0 + 0.4 * hold)).round() as u32).max(1);
                if k == 0 {
                    continue;
                }
                let target = doc.frames[start + k].id;
                let decay = 1.0 - k as f32 / n as f32;
                let jitter = |s: u32| -> f32 { ((hnoise(k as i32, s as i32, 0, 11) * 3.0).floor() - 1.0) * amount * decay };
                for &(id, moving) in &layers {
                    let src = doc.cels[&(id, source)].clone();
                    if !moving {
                        doc.cels.insert((id, target), src);
                        continue;
                    }
                    let (lw, lh) = doc.layer_dims(id);
                    let ld = (lw / doc.width.max(1)) as f32;
                    let mut out = crate::pixel::PixelGrid::new(lw, lh);
                    for y in 0..lh as i64 {
                        for x in 0..lw as i64 {
                            // Work in canvas units so every layer moves by the same amount.
                            let (xf, yf) = ((x as f32 + 0.5) / ld, (y as f32 + 0.5) / ld);
                            let (sx, sy) = match kind {
                                "bob" => (xf, yf + (amount * phi.sin().abs()).round()),
                                "float" => (xf, yf + (amount * phi.sin()).round()),
                                "breathe" => {
                                    let s = 1.0 + amount / height * (1.0 - phi.cos()) / 2.0;
                                    (xf, if yf <= pivot { pivot - (pivot - yf) / s } else { yf })
                                }
                                "squash" => {
                                    let q = (phi.sin() * amount).clamp(-0.6, 0.6);
                                    (cx + (xf - cx) * (1.0 - q), if yf <= pivot { pivot - (pivot - yf) / (1.0 - q) } else { yf })
                                }
                                "sway" => {
                                    let hh = ((pivot - yf) / height).clamp(0.0, 1.0);
                                    (xf - (amount * hh.powf(1.5) * (phi - 1.2 * hh).sin()).round(), yf)
                                }
                                "shake" => (xf - jitter(1).round(), yf - jitter(2).round()),
                                "hurt" if k >= 3 => (xf - if k % 2 == 0 { amount } else { -amount } * decay * 2.0, yf),
                                _ => (xf, yf),
                            };
                            let v = src.get((sx * ld).floor() as i64, (sy * ld).floor() as i64);
                            if v == TRANSPARENT {
                                continue;
                            }
                            let v = match kind {
                                "pulse" => {
                                    let f = amount * (1.0 - phi.cos()) / 2.0;
                                    let c = Rgba::from_u32(v);
                                    let up = |ch: u8| (ch as f32 + (255.0 - ch as f32) * f).round() as u8;
                                    Rgba::new(up(c.r), up(c.g), up(c.b), c.a).to_pixel()
                                }
                                "hurt" if k <= 2 => Rgba::new(255, 255, 255, Rgba::from_u32(v).a).to_pixel(),
                                _ => v,
                            };
                            out.set(x, y, v);
                        }
                    }
                    changed += 1;
                    doc.cels.insert((id, target), std::sync::Arc::new(out));
                }
            }
            add_tag(doc, a, start, n, !matches!(kind, "hurt" | "shake"))?;
            Ok(Outcome::new(
                Json::obj()
                    .with("kind", kind)
                    .with("frames", n)
                    .with("layers", layers.iter().filter(|l| l.1).count())
                    .with("changed", changed),
                changed,
            ))
        }
    }

    pub mod gamedev {
        //! Game-production commands: blend modes, slices (hitbox/hurtbox/pivot), palette swaps,
        //! RotSprite, tilesets/tilemaps with blob autotiling, normal-mapped 2D light previews and
        //! engine exports (Aseprite-compatible atlas JSON, Godot SpriteFrames, Tiled TMJ).

        use std::sync::Arc;

        use super::args::{color_value, frame_index, layer_index, opt_bool, opt_f64, opt_i64, opt_str, req_i64, req_str, scale, target};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome, Query};
        use crate::export;
        use crate::gamedev::atlas::{self, Placement};
        use crate::gamedev::light2d::{self, Light2D, LightSettings};
        use crate::gamedev::tiles::{self, BlobStyle, Tilemap, Tileset, TilesetKind};
        use crate::gamedev::{Slice, SliceKey, rotsprite};
        use crate::layers::Blend;
        use crate::pixel::{PixelGrid, Rgba, TRANSPARENT};
        use crate::pixelart;
        use crate::project::Document;
        use crate::renderer::{Image, RenderOptions, render};

        const MAX_TILE_SIDE: i64 = 256;
        const MAX_TILES: usize = 4096;
        const MAX_MAP_SIDE: i64 = 4096;

        fn png_query(img: &Image, result: Json) -> Query {
            let png = export::png::encode(img);
            Query {
                result: result.with("width", img.width).with("height", img.height).with("mimeType", "image/png").with("bytes", png.len()),
                binary: png,
            }
        }

        fn hex_or(doc: &Document, a: &Json, key: &str, default: Rgba) -> CmdResult<Rgba> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(default),
                Some(v) => Ok(Rgba::from_u32(color_value(doc, v, key)?)),
            }
        }

        // ---- layers & slices ---------------------------------------------------------------------------

        /// Creates or updates a slice key. Without `frame` the key starts at the first frame, so it
        /// applies to the whole animation until a later key overrides it.
        pub fn slice_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?.trim().to_string();
            if name.is_empty() || name.len() > 128 {
                return Err(CmdError::invalid("slice 'name' must be 1-128 characters"));
            }
            let fi = if a.get("frame").is_some_and(|v| !v.is_null()) { frame_index(doc, a, "frame")? } else { 0 };
            let frame_id = doc.frames[fi].id;
            let (x, y) = (req_i64(a, "x")?, req_i64(a, "y")?);
            let (w, h) = (req_i64(a, "width")?, req_i64(a, "height")?);
            if !(1..=65_536).contains(&w) || !(1..=65_536).contains(&h) || x.abs() > 65_536 || y.abs() > 65_536 {
                return Err(CmdError::invalid("slice rectangle out of range (width/height ≥ 1)"));
            }
            let pivot = match a.get("pivot") {
                None | Some(Json::Null) => None,
                Some(Json::Arr(p)) if p.len() == 2 => {
                    let px = p[0].as_i64().ok_or_else(|| CmdError::invalid("'pivot' must be [x, y]"))?;
                    let py = p[1].as_i64().ok_or_else(|| CmdError::invalid("'pivot' must be [x, y]"))?;
                    Some((px.clamp(-65_536, 65_536) as i32, py.clamp(-65_536, 65_536) as i32))
                }
                Some(_) => return Err(CmdError::invalid("'pivot' must be [x, y] relative to the slice")),
            };
            let default_color = match name.to_ascii_lowercase() {
                n if n.contains("hurt") => Rgba::new(64, 160, 255, 255),
                n if n.contains("hit") || n.contains("attack") => Rgba::new(255, 64, 64, 255),
                n if n.contains("pivot") || n.contains("origin") => Rgba::new(255, 220, 64, 255),
                _ => Rgba::new(0, 0, 255, 255),
            };
            let color = hex_or(doc, a, "color", default_color)?.to_u32();
            let key = SliceKey { frame_id, x: x as i32, y: y as i32, w: w as u32, h: h as u32, pivot };
            let created = match doc.slices.iter_mut().find(|s| s.name == name) {
                Some(s) => {
                    if a.get("color").is_some() {
                        s.color = color;
                    }
                    match s.keys.iter_mut().find(|k| k.frame_id == frame_id) {
                        Some(k) => *k = key,
                        None => s.keys.push(key),
                    }
                    false
                }
                None => {
                    doc.slices.push(Slice { name: name.clone(), color, keys: vec![key] });
                    true
                }
            };
            Ok(Outcome::new(Json::obj().with("slice", name).with("frame", fi).with("created", created), 0))
        }

        pub fn slice_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?;
            let before = doc.slices.len();
            match a.get("frame").filter(|v| !v.is_null()) {
                Some(_) => {
                    let id = doc.frames[frame_index(doc, a, "frame")?].id;
                    let s =
                        doc.slices.iter_mut().find(|s| s.name == name).ok_or_else(|| CmdError::not_found(format!("no slice '{name}'")))?;
                    s.keys.retain(|k| k.frame_id != id);
                    doc.slices.retain(|s| !s.keys.is_empty());
                }
                None => {
                    doc.slices.retain(|s| s.name != name);
                    if doc.slices.len() == before {
                        return Err(CmdError::not_found(format!("no slice '{name}'")));
                    }
                }
            }
            Ok(Outcome::new(Json::obj().with("deleted", name), 0))
        }

        fn slice_json(doc: &Document, s: &Slice, scale: u32) -> Json {
            let keys = s
                .keys
                .iter()
                .filter_map(|k| {
                    let fi = doc.frame_index(k.frame_id)?;
                    let sc = scale as i32;
                    let mut j = Json::obj()
                        .with("frame", fi)
                        .with("bounds", Json::obj().with("x", k.x * sc).with("y", k.y * sc).with("w", k.w * scale).with("h", k.h * scale));
                    if let Some((px, py)) = k.pivot {
                        j.set("pivot", Json::obj().with("x", px * sc).with("y", py * sc));
                    }
                    Some(j)
                })
                .collect();
            Json::obj().with("name", s.name.as_str()).with("color", Rgba::from_u32(s.color).to_hex()).with("keys", Json::Arr(keys))
        }

        // ---- recoloring & rotation -----------------------------------------------------------------------

        fn scope_cels(doc: &Document, a: &Json) -> CmdResult<Vec<(u32, u32)>> {
            let scope = opt_str(a, "scope")?.unwrap_or("cel");
            let li = layer_index(doc, a, "layer")?;
            let fi = frame_index(doc, a, "frame")?;
            Ok(match scope {
                "cel" => vec![(doc.layers[li].id, doc.frames[fi].id)],
                "layer" => doc.frames.iter().map(|f| (doc.layers[li].id, f.id)).collect(),
                "frame" => doc.layers.iter().map(|l| (l.id, doc.frames[fi].id)).collect(),
                "all" => doc.layers.iter().flat_map(|l| doc.frames.iter().map(move |f| (l.id, f.id))).collect(),
                other => return Err(CmdError::invalid(format!("unknown scope '{other}' (cel, layer, frame, all)"))),
            })
        }

        /// Remaps colors (RGB match, alpha kept): team colors, palette variants, day/night swaps.
        pub fn palette_swap(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let mut pairs: Vec<(u32, Rgba)> = Vec::new();
            match (a.get("map"), a.get("from"), a.get("to")) {
                (Some(Json::Obj(entries)), _, _) => {
                    for (k, v) in entries {
                        let from = Rgba::parse_hex(k).ok_or_else(|| CmdError::invalid(format!("map key '{k}' is not a hex color")))?;
                        pairs.push((from.to_u32() | 0xFF, Rgba::from_u32(color_value(doc, v, "map")?)));
                    }
                }
                (_, Some(from @ Json::Str(_)), Some(to)) if !to.is_null() => {
                    let f = Rgba::from_u32(color_value(doc, from, "from")?);
                    pairs.push((f.to_u32() | 0xFF, Rgba::from_u32(color_value(doc, to, "to")?)));
                }
                (_, Some(Json::Arr(from)), Some(Json::Arr(to))) if from.len() == to.len() => {
                    for (f, t) in from.iter().zip(to) {
                        let f = Rgba::from_u32(color_value(doc, f, "from")?);
                        pairs.push((f.to_u32() | 0xFF, Rgba::from_u32(color_value(doc, t, "to")?)));
                    }
                }
                _ => return Err(CmdError::invalid("give 'map' {\"#from\": \"#to\"} or equally long 'from' and 'to' arrays")),
            }
            let cels = scope_cels(doc, a)?;
            let mut changed = 0u64;
            for key in cels {
                if doc.layers.iter().any(|l| l.id == key.0 && l.locked) {
                    continue;
                }
                let Some(cel) = doc.cels.get_mut(&key) else { continue };
                let mut writes = Vec::new();
                cel.for_each_opaque(|x, y, v| {
                    if let Some((_, to)) = pairs.iter().find(|(f, _)| *f == v | 0xFF) {
                        let alpha = ((v & 0xFF) * to.a as u32 / 255) as u8;
                        writes.push((x, y, Rgba::new(to.r, to.g, to.b, alpha).to_pixel()));
                    }
                });
                let grid: &mut PixelGrid = Arc::make_mut(cel);
                for (x, y, v) in writes {
                    changed += grid.set(x as i64, y as i64, v) as u64;
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed).with("mappings", pairs.len()), changed))
        }

        /// Pixel-art rotation (Scale2x×3 + nearest) that keeps lines unbroken and adds no colors.
        pub fn rotsprite_cmd(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let degrees = opt_f64(a, "degrees")?.ok_or_else(|| CmdError::invalid("missing required number 'degrees'"))? as f32;
            let (l, f) = target(doc, a)?;
            let (w, h) = (doc.width, doc.height);
            if (w as u64) * (h as u64) > 1024 * 1024 {
                return Err(CmdError::new("limit", "rotsprite works on canvases up to 1024×1024"));
            }
            let grid = doc.cel_mut(l, f);
            let (cx, cy) = match a.get("center") {
                Some(Json::Arr(c)) if c.len() == 2 => (c[0].as_f64().unwrap_or(0.0) as f32, c[1].as_f64().unwrap_or(0.0) as f32),
                _ => match grid.bounds() {
                    Some((x, y, bw, bh)) => (x as f32 + bw as f32 / 2.0, y as f32 + bh as f32 / 2.0),
                    None => return Ok(Outcome::new(Json::obj().with("changed", 0), 0)),
                },
            };
            let mut src = vec![0u32; (w * h) as usize];
            grid.for_each_opaque(|x, y, v| src[(y * w + x) as usize] = v);
            let out = rotsprite::rotate(&src, w as usize, h as usize, degrees, cx, cy);
            let mut changed = 0u64;
            for y in 0..h {
                for x in 0..w {
                    changed += grid.set(x as i64, y as i64, out[(y * w + x) as usize]) as u64;
                }
            }
            Ok(Outcome::new(
                Json::obj().with("changed", changed).with("center", Json::Arr(vec![(cx as f64).into(), (cy as f64).into()])),
                changed,
            ))
        }

        // ---- tilesets & tilemaps -------------------------------------------------------------------------

        fn by_name_or_index<T>(items: &[Arc<T>], name: impl Fn(&T) -> &str, v: Option<&Json>, what: &str) -> CmdResult<usize> {
            match v {
                Some(Json::Str(s)) => items
                    .iter()
                    .position(|t| name(t) == s)
                    .or_else(|| s.parse::<usize>().ok().filter(|i| *i < items.len()))
                    .ok_or_else(|| CmdError::not_found(format!("no {what} named '{s}'"))),
                Some(v) => {
                    let i = v.as_i64().ok_or_else(|| CmdError::invalid(format!("'{what}' must be a name or index")))?;
                    if i < 0 || i as usize >= items.len() {
                        return Err(CmdError::not_found(format!("{what} index {i} out of range (0..{})", items.len())));
                    }
                    Ok(i as usize)
                }
                None if items.len() == 1 => Ok(0),
                None => Err(CmdError::invalid(format!("missing '{what}' (name or index)"))),
            }
        }

        fn tileset_idx(doc: &Document, a: &Json) -> CmdResult<usize> {
            by_name_or_index(&doc.tilesets, |t| &t.name, a.get("tileset"), "tileset")
        }

        fn tilemap_idx(doc: &Document, a: &Json) -> CmdResult<usize> {
            by_name_or_index(&doc.tilemaps, |t| &t.name, a.get("tilemap"), "tilemap")
        }

        fn unique_name(name: &str, taken: impl Fn(&str) -> bool) -> CmdResult<String> {
            let name = name.trim();
            if name.is_empty() || name.len() > 128 {
                return Err(CmdError::invalid("'name' must be 1-128 characters"));
            }
            if taken(name) {
                return Err(CmdError::new("conflict", format!("'{name}' already exists")));
            }
            Ok(name.to_string())
        }

        fn tile_size(a: &Json) -> CmdResult<(u32, u32)> {
            let s = opt_i64(a, "tile_size")?.unwrap_or(16);
            let w = opt_i64(a, "tile_width")?.unwrap_or(s);
            let h = opt_i64(a, "tile_height")?.unwrap_or(s);
            if !(1..=MAX_TILE_SIDE).contains(&w) || !(1..=MAX_TILE_SIDE).contains(&h) {
                return Err(CmdError::invalid(format!("tile size must be 1-{MAX_TILE_SIDE}")));
            }
            Ok((w as u32, h as u32))
        }

        pub fn tileset_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = unique_name(req_str(a, "name")?, |n| doc.tilesets.iter().any(|t| t.name == n))?;
            let (tile_w, tile_h) = tile_size(a)?;
            doc.tilesets.push(Arc::new(Tileset { name: name.clone(), tile_w, tile_h, kind: TilesetKind::Plain, tiles: Vec::new() }));
            Ok(Outcome::new(Json::obj().with("tileset", doc.tilesets.len() - 1).with("name", name), 0))
        }

        fn render_source(doc: &Document, a: &Json) -> CmdResult<Image> {
            let fi = frame_index(doc, a, "frame")?;
            let mut opts = RenderOptions::default();
            if a.get("layer").is_some_and(|v| !v.is_null()) {
                opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
            }
            Ok(render(doc, fi, &opts))
        }

        fn cut(img: &Image, x: i64, y: i64, w: u32, h: u32) -> Vec<u32> {
            let mut t = vec![0u32; (w * h) as usize];
            for ty in 0..h as i64 {
                for tx in 0..w as i64 {
                    let (sx, sy) = (x + tx, y + ty);
                    if sx < 0 || sy < 0 || sx >= img.width as i64 || sy >= img.height as i64 {
                        continue;
                    }
                    let c = img.get(sx as u32, sy as u32);
                    t[(ty * w as i64 + tx) as usize] = if c.a == 0 { 0 } else { c.to_pixel() };
                }
            }
            t
        }

        /// Cuts a grid of tiles from the canvas into a tileset (duplicates are skipped by default).
        pub fn tileset_add_tiles(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let ti = tileset_idx(doc, a)?;
            let img = render_source(doc, a)?;
            let (x0, y0) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
            let ts = &doc.tilesets[ti];
            let (tw, th) = (ts.tile_w, ts.tile_h);
            let cols = opt_i64(a, "columns")?.unwrap_or((img.width / tw) as i64).clamp(1, 256);
            let rows = opt_i64(a, "rows")?.unwrap_or((img.height / th) as i64).clamp(1, 256);
            let dedupe = opt_bool(a, "dedupe")?.unwrap_or(true);
            let skip_empty = opt_bool(a, "skip_empty")?.unwrap_or(true);
            let ts = Arc::make_mut(&mut doc.tilesets[ti]);
            let mut indices = Vec::new();
            for r in 0..rows {
                for c in 0..cols {
                    let t = cut(&img, x0 + c * tw as i64, y0 + r * th as i64, tw, th);
                    if skip_empty && t.iter().all(|&v| v == 0) {
                        continue;
                    }
                    if let Some(i) = dedupe.then(|| ts.tiles.iter().position(|e| *e == t)).flatten() {
                        indices.push(Json::from(i));
                        continue;
                    }
                    if ts.tiles.len() >= MAX_TILES {
                        return Err(CmdError::new("limit", format!("a tileset holds at most {MAX_TILES} tiles")));
                    }
                    ts.tiles.push(t);
                    indices.push(Json::from(ts.tiles.len() - 1));
                }
            }
            Ok(Outcome::new(Json::obj().with("tiles", Json::Arr(indices)).with("tileCount", ts.tiles.len()), 0))
        }

        fn hash(x: u32, y: u32, seed: u32) -> u32 {
            let mut h = x.wrapping_mul(0x8DA6_B343) ^ y.wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
            h ^= h >> 13;
            h = h.wrapping_mul(0x5BD1_E995);
            h ^ (h >> 15)
        }

        /// Seamless procedural fill: base color with small wrapped clusters of the accent colors
        /// (grass blades, pebbles, cracks) — never a single noisy pixel soup.
        fn procedural_fill(w: u32, h: u32, base: Rgba, accents: &[Rgba], density: f32, seed: u32) -> Vec<u32> {
            let mut t = vec![base.to_pixel(); (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    let r = hash(x, y, seed);
                    if (r % 1000) as f32 >= density * 1000.0 || accents.is_empty() {
                        continue;
                    }
                    let c = accents[(r / 1000) as usize % accents.len()].to_pixel();
                    t[(y * w + x) as usize] = c;
                    // Two-pixel vertical cluster (wraps → seamless).
                    if r & 0x10000 != 0 {
                        t[(((y + h - 1) % h) * w + x) as usize] = c;
                    }
                }
            }
            t
        }

        /// Generates the complete 47-tile blob terrain set from a fill (canvas region or procedural).
        pub fn tileset_generate_blob(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = unique_name(req_str(a, "name")?, |n| doc.tilesets.iter().any(|t| t.name == n))?;
            let (tw, th) = tile_size(a)?;
            let fill: Vec<u32> = if a.get("source_x").is_some() {
                let img = render_source(doc, a)?;
                cut(&img, req_i64(a, "source_x")?, opt_i64(a, "source_y")?.unwrap_or(0), tw, th)
            } else {
                let base = hex_or(doc, a, "fill", Rgba::new(72, 140, 64, 255))?;
                let accents = match a.get("accents") {
                    Some(Json::Arr(list)) => {
                        list.iter().map(|c| color_value(doc, c, "accents").map(Rgba::from_u32)).collect::<CmdResult<Vec<_>>>()?
                    }
                    _ => {
                        let r = pixelart::ramp(base, 5, 20.0, 0.6);
                        vec![r[3], r[1]]
                    }
                };
                let density = opt_f64(a, "density")?.unwrap_or(0.12).clamp(0.0, 1.0) as f32;
                procedural_fill(tw, th, base, &accents, density, opt_i64(a, "seed")?.unwrap_or(7) as u32)
            };
            let opaque: Vec<Rgba> = fill.iter().filter(|&&v| v != 0).map(|&v| Rgba::from_u32(v)).collect();
            if opaque.is_empty() {
                return Err(CmdError::invalid("the fill tile is empty"));
            }
            let n = opaque.len() as u32;
            let sum = opaque.iter().fold([0u32; 3], |s, c| [s[0] + c.r as u32, s[1] + c.g as u32, s[2] + c.b as u32]);
            let avg = Rgba::new((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, 255);
            let ramp = pixelart::ramp(avg, 7, 24.0, 0.8);
            let style = BlobStyle {
                inset: opt_i64(a, "inset")?.unwrap_or(1).clamp(0, (tw.min(th) / 4) as i64) as u32,
                edge: opt_i64(a, "edge")?.unwrap_or(1).clamp(0, (tw.min(th) / 3) as i64) as u32,
                light: hex_or(doc, a, "light", ramp[5])?,
                dark: hex_or(doc, a, "dark", ramp[2])?,
                outline: hex_or(doc, a, "outline", pixelart::outline_color(avg))?,
                lip: opt_i64(a, "lip")?.unwrap_or(0).clamp(0, (th / 2) as i64) as u32,
                lip_color: hex_or(doc, a, "lip_color", Rgba::new(110, 72, 52, 255))?,
                rounded: opt_bool(a, "rounded")?.unwrap_or(true),
            };
            let tiles = tiles::synthesize_blob(&fill, tw, th, &style);
            doc.tilesets.push(Arc::new(Tileset { name: name.clone(), tile_w: tw, tile_h: th, kind: TilesetKind::Blob, tiles }));
            Ok(Outcome::new(Json::obj().with("tileset", doc.tilesets.len() - 1).with("name", name).with("tileCount", 47), 0))
        }

        pub fn tileset_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let ti = tileset_idx(doc, a)?;
            if let Some(m) = doc.tilemaps.iter().find(|m| m.tileset == ti) {
                return Err(CmdError::new("conflict", format!("tilemap '{}' still uses this tileset", m.name)));
            }
            let ts = doc.tilesets.remove(ti);
            for m in &mut doc.tilemaps {
                if m.tileset > ti {
                    Arc::make_mut(m).tileset -= 1;
                }
            }
            Ok(Outcome::new(Json::obj().with("deleted", ts.name.as_str()), 0))
        }

        pub fn tilemap_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = unique_name(req_str(a, "name")?, |n| doc.tilemaps.iter().any(|t| t.name == n))?;
            let ti = tileset_idx(doc, a)?;
            let (cols, rows) = (req_i64(a, "columns")?, req_i64(a, "rows")?);
            if !(1..=MAX_MAP_SIDE).contains(&cols) || !(1..=MAX_MAP_SIDE).contains(&rows) || cols * rows > 1 << 22 {
                return Err(CmdError::invalid(format!("tilemap size must be 1-{MAX_MAP_SIDE} per side")));
            }
            doc.tilemaps.push(Arc::new(Tilemap::new(&name, ti, cols as u32, rows as u32)));
            Ok(Outcome::new(Json::obj().with("tilemap", doc.tilemaps.len() - 1).with("name", name), 0))
        }

        pub fn tilemap_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let mi = tilemap_idx(doc, a)?;
            let m = doc.tilemaps.remove(mi);
            Ok(Outcome::new(Json::obj().with("deleted", m.name.as_str()), 0))
        }

        /// Places explicit tiles: `cells` = [[x, y, tile], …] (tile -1/null = empty) or x/y/tile, or
        /// `rows` of space-separated tile numbers.
        pub fn tilemap_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let mi = tilemap_idx(doc, a)?;
            let count = doc.tilesets[doc.tilemaps[mi].tileset].tiles.len() as i64;
            let mut writes: Vec<(i64, i64, i64)> = Vec::new();
            let tile_of = |v: &Json| -> CmdResult<i64> {
                match v {
                    Json::Null => Ok(-1),
                    v => v.as_i64().ok_or_else(|| CmdError::invalid("tile must be an index or null")),
                }
            };
            if let Some(Json::Arr(cells)) = a.get("cells") {
                for c in cells {
                    match c.as_array() {
                        Some([x, y, t]) => writes.push((
                            x.as_i64().ok_or_else(|| CmdError::invalid("cells: x must be an integer"))?,
                            y.as_i64().ok_or_else(|| CmdError::invalid("cells: y must be an integer"))?,
                            tile_of(t)?,
                        )),
                        _ => return Err(CmdError::invalid("each cell must be [x, y, tile]")),
                    }
                }
            } else if let Some(Json::Arr(rows)) = a.get("rows") {
                let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
                for (y, row) in rows.iter().enumerate() {
                    let s = row.as_str().ok_or_else(|| CmdError::invalid("rows must be strings of tile numbers"))?;
                    for (x, tok) in s.split_whitespace().enumerate() {
                        let t = if tok == "." || tok == "-" {
                            -1
                        } else {
                            tok.parse::<i64>().map_err(|_| CmdError::invalid(format!("'{tok}' is not a tile")))?
                        };
                        writes.push((ox + x as i64, oy + y as i64, t));
                    }
                }
            } else {
                writes.push((req_i64(a, "x")?, req_i64(a, "y")?, tile_of(a.get("tile").unwrap_or(&Json::Null))?));
            }
            let m = Arc::make_mut(&mut doc.tilemaps[mi]);
            let mut changed = 0u64;
            for (x, y, t) in writes {
                if !m.in_bounds(x, y) {
                    return Err(CmdError::invalid(format!("cell ({x}, {y}) is outside the {}×{} map", m.cols, m.rows)));
                }
                if t >= count {
                    return Err(CmdError::not_found(format!("tile {t} does not exist (tileset has {count})")));
                }
                let i = (y as u32 * m.cols + x as u32) as usize;
                let v = (t + 1).max(0) as u32;
                if m.cells[i] != v {
                    m.cells[i] = v;
                    changed += 1;
                }
            }
            Ok(Outcome::new(Json::obj().with("changedCells", changed), 0))
        }

        /// Paints terrain and re-autotiles: `rows` as ASCII ('#' = terrain), a rectangle, or cells.
        pub fn tilemap_terrain(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let mi = tilemap_idx(doc, a)?;
            if doc.tilesets[doc.tilemaps[mi].tileset].kind != TilesetKind::Blob {
                return Err(CmdError::invalid("terrain painting needs a blob tileset (tileset_generate_blob)"));
            }
            let value = opt_str(a, "mode")?.unwrap_or("add") != "remove";
            let mut cells: Vec<(i64, i64, bool)> = Vec::new();
            if let Some(Json::Arr(rows)) = a.get("rows") {
                let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
                let replace = opt_bool(a, "replace")?.unwrap_or(true);
                for (y, row) in rows.iter().enumerate() {
                    let s = row.as_str().ok_or_else(|| CmdError::invalid("rows must be strings"))?;
                    for (x, ch) in s.chars().enumerate() {
                        let on = matches!(ch, '#' | 'X' | 'x' | '1' | '█');
                        if on || replace {
                            cells.push((ox + x as i64, oy + y as i64, on));
                        }
                    }
                }
            } else if let Some(Json::Arr(list)) = a.get("cells") {
                for c in list {
                    match c.as_array() {
                        Some([x, y]) => cells.push((x.as_i64().unwrap_or(-1), y.as_i64().unwrap_or(-1), value)),
                        _ => return Err(CmdError::invalid("each cell must be [x, y]")),
                    }
                }
            } else {
                let (x, y) = (req_i64(a, "x")?, req_i64(a, "y")?);
                let (w, h) = (opt_i64(a, "width")?.unwrap_or(1), opt_i64(a, "height")?.unwrap_or(1));
                if w < 1 || h < 1 || w > MAX_MAP_SIDE || h > MAX_MAP_SIDE {
                    return Err(CmdError::invalid("width/height must be 1-4096"));
                }
                for yy in y..y + h {
                    for xx in x..x + w {
                        cells.push((xx, yy, value));
                    }
                }
            }
            let m = Arc::make_mut(&mut doc.tilemaps[mi]);
            let mut changed = 0u64;
            for (x, y, on) in cells {
                if !m.in_bounds(x, y) {
                    continue;
                }
                let i = (y as u32 * m.cols + x as u32) as usize;
                changed += (m.terrain[i] != on) as u64;
                m.terrain[i] = on;
            }
            m.autotile();
            Ok(Outcome::new(Json::obj().with("changedCells", changed).with("terrainCells", m.terrain.iter().filter(|&&t| t).count()), 0))
        }

        /// Draws a tilemap into a layer cel (so it composites, lights and exports with the scene).
        pub fn tilemap_bake(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let mi = tilemap_idx(doc, a)?;
            let map = Arc::clone(&doc.tilemaps[mi]);
            let ts = Arc::clone(&doc.tilesets[map.tileset]);
            let img = map.render(&ts);
            let (ox, oy) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
            let replace = opt_bool(a, "replace")?.unwrap_or(false);
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            for y in 0..img.height {
                for x in 0..img.width {
                    let c = img.get(x, y);
                    if c.a == 0 && !replace {
                        continue;
                    }
                    let v = if c.a == 0 { TRANSPARENT } else { c.to_pixel() };
                    changed += grid.set(ox + x as i64, oy + y as i64, v) as u64;
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed).with("width", img.width).with("height", img.height), changed))
        }

        // ---- queries --------------------------------------------------------------------------------------

        pub fn gamedev_inspect(doc: &Document) -> CmdResult<Query> {
            let tilesets = doc
                .tilesets
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    Json::obj()
                        .with("index", i)
                        .with("name", t.name.as_str())
                        .with("tileWidth", t.tile_w)
                        .with("tileHeight", t.tile_h)
                        .with("kind", if t.kind == TilesetKind::Blob { "blob47" } else { "plain" })
                        .with("tileCount", t.tiles.len())
                })
                .collect();
            let tilemaps = doc
                .tilemaps
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let mut j = Json::obj()
                        .with("index", i)
                        .with("name", m.name.as_str())
                        .with("tileset", doc.tilesets[m.tileset].name.as_str())
                        .with("columns", m.cols)
                        .with("rows", m.rows)
                        .with("filledCells", m.cells.iter().filter(|&&c| c != 0).count());
                    if m.cols <= 96 && m.rows <= 64 {
                        let rows = (0..m.rows)
                            .map(|y| {
                                Json::from(
                                    (0..m.cols).map(|x| if m.terrain[(y * m.cols + x) as usize] { '#' } else { '.' }).collect::<String>(),
                                )
                            })
                            .collect();
                        j.set("terrain", Json::Arr(rows));
                    }
                    j
                })
                .collect();
            let layers = doc.layers.iter().map(|l| Json::obj().with("name", l.name.as_str()).with("blend", l.blend.as_str())).collect();
            Ok(Query {
                result: Json::obj()
                    .with("slices", Json::Arr(doc.slices.iter().map(|s| slice_json(doc, s, 1)).collect()))
                    .with("tilesets", Json::Arr(tilesets))
                    .with("tilemaps", Json::Arr(tilemaps))
                    .with("layerBlendModes", Json::Arr(layers))
                    .with("blendModes", Json::Arr(Blend::ALL.iter().map(|(n, _)| Json::from(*n)).collect())),
                binary: Vec::new(),
            })
        }

        fn rgb_arg(doc: &Document, v: Option<&Json>, key: &str, default: [f32; 3]) -> CmdResult<[f32; 3]> {
            match v {
                None | Some(Json::Null) => Ok(default),
                Some(Json::Arr(c)) if c.len() == 3 => {
                    let mut out = [0.0; 3];
                    for (i, x) in c.iter().enumerate() {
                        out[i] = x.as_f64().ok_or_else(|| CmdError::invalid(format!("'{key}' must be [r, g, b] or a hex color")))? as f32;
                    }
                    Ok(out)
                }
                Some(v) => Ok(light2d::rgb_lin(Rgba::from_u32(color_value(doc, v, key)?))),
            }
        }

        /// Renders a frame with normal-mapped 2D point lights (torches, campfires, spells) — the
        /// in-editor preview of what Godot/Unity 2D lights will do with the exported normal map.
        pub fn render_lit(doc: &Document, a: &Json) -> CmdResult<Query> {
            let fi = frame_index(doc, a, "frame")?;
            let unlit: Vec<u32> = match a.get("unlit_layers") {
                Some(Json::Arr(list)) => list
                    .iter()
                    .map(|v| layer_index(doc, &Json::obj().with("l", v.clone()), "l").map(|i| doc.layers[i].id))
                    .collect::<CmdResult<_>>()?,
                _ => Vec::new(),
            };
            let normal_layer = match a.get("normal_layer") {
                Some(v) if !v.is_null() => Some(doc.layers[layer_index(doc, &Json::obj().with("l", v.clone()), "l")?].id),
                _ => None,
            };
            let mut lit_doc = doc.clone();
            for l in &mut lit_doc.layers {
                if unlit.contains(&l.id) || Some(l.id) == normal_layer {
                    l.visible = false;
                }
            }
            let albedo = render(&lit_doc, fi, &RenderOptions::default());
            let normals = match normal_layer {
                Some(id) => render(doc, fi, &RenderOptions { only_layer: Some(id), ..Default::default() }),
                None => {
                    let strength = opt_f64(a, "strength")?.unwrap_or(3.0).clamp(0.1, 20.0) as f32;
                    let bevel = opt_f64(a, "bevel")?.unwrap_or(3.0).clamp(0.5, 32.0) as f32;
                    pixelart::normal_map(&albedo, strength, bevel)
                }
            };
            let mut lights = Vec::new();
            if let Some(Json::Arr(list)) = a.get("lights") {
                for l in list.iter().take(64) {
                    let x = opt_f64(l, "x")?.ok_or_else(|| CmdError::invalid("each light needs x and y"))? as f32;
                    let y = opt_f64(l, "y")?.ok_or_else(|| CmdError::invalid("each light needs x and y"))? as f32;
                    lights.push(Light2D {
                        x,
                        y,
                        z: opt_f64(l, "z")?.unwrap_or(24.0).clamp(0.5, 4096.0) as f32,
                        color: rgb_arg(doc, l.get("color"), "color", [1.0, 0.75, 0.45])?,
                        intensity: opt_f64(l, "intensity")?.unwrap_or(1.5).clamp(0.0, 64.0) as f32,
                        radius: opt_f64(l, "radius")?.unwrap_or(64.0).clamp(1.0, 8192.0) as f32,
                    });
                }
            }
            let settings = LightSettings {
                ambient: rgb_arg(doc, a.get("ambient"), "ambient", [0.18, 0.18, 0.26])?,
                lights,
                bands: opt_i64(a, "bands")?.unwrap_or(0).clamp(0, 32) as u32,
                normal_strength: opt_f64(a, "normal_strength")?.unwrap_or(1.0).clamp(0.0, 8.0) as f32,
            };
            let mut out = light2d::light(&albedo, &normals, &settings);
            if !unlit.is_empty() {
                let mut glow = doc.clone();
                for l in &mut glow.layers {
                    l.visible = unlit.contains(&l.id);
                }
                let g = render(&glow, fi, &RenderOptions::default());
                for y in 0..g.height {
                    for x in 0..g.width {
                        let c = g.get(x, y);
                        if c.a > 0 {
                            let o = ((y * out.width + x) * 4) as usize;
                            crate::renderer::blend_over(&mut out.pixels[o..o + 4], c, 255);
                        }
                    }
                }
            }
            if let Some(bg) = opt_str(a, "background")? {
                let bg = Rgba::parse_hex(bg).ok_or_else(|| CmdError::invalid("'background' must be a hex color"))?;
                let mut base = Image::new(out.width, out.height);
                for p in base.pixels.as_chunks_mut::<4>().0 {
                    *p = [bg.r, bg.g, bg.b, 255];
                }
                for y in 0..out.height {
                    for x in 0..out.width {
                        let o = ((y * out.width + x) * 4) as usize;
                        crate::renderer::blend_over(&mut base.pixels[o..o + 4], out.get(x, y), 255);
                    }
                }
                out = base;
            }
            let s = scale(a, 1)?;
            if out.width * s > 8192 || out.height * s > 8192 {
                return Err(CmdError::new("limit", "scaled output would exceed 8192px"));
            }
            Ok(png_query(&out.scaled(s), Json::obj().with("frame", fi).with("lights", settings.lights.len())))
        }

        /// Frames to export (timeline order): a tag's range or every frame.
        fn export_frames(doc: &Document, a: &Json) -> CmdResult<Vec<usize>> {
            match opt_str(a, "tag")? {
                Some(name) => {
                    let t = doc.tags.iter().find(|t| t.name == name).ok_or_else(|| CmdError::not_found(format!("no tag '{name}'")))?;
                    let (from, to) = (doc.frame_index(t.from_id).unwrap_or(0), doc.frame_index(t.to_id).unwrap_or(0));
                    Ok((from.min(to)..=from.max(to)).collect())
                }
                None => Ok((0..doc.frames.len()).collect()),
            }
        }

        struct Packed {
            frames: Vec<usize>,
            atlas: atlas::Atlas,
            scale: u32,
        }

        fn pack_frames(doc: &Document, a: &Json) -> CmdResult<Packed> {
            let frames = export_frames(doc, a)?;
            let s = scale(a, 1)?;
            if doc.width * s > 4096 || doc.height * s > 4096 {
                return Err(CmdError::new("limit", "frames larger than 4096px; lower 'scale'"));
            }
            let mut opts = RenderOptions::default();
            if a.get("layer").is_some_and(|v| !v.is_null()) {
                opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
            }
            let images: Vec<Image> = frames.iter().map(|&i| render(doc, i, &opts).scaled(s)).collect();
            let atlas = atlas::pack(
                &images,
                opt_bool(a, "trim")?.unwrap_or(true),
                opt_i64(a, "padding")?.unwrap_or(1).clamp(0, 64) as u32,
                opt_i64(a, "extrude")?.unwrap_or(0).clamp(0, 16) as u32,
                opt_bool(a, "power_of_two")?.unwrap_or(false),
            )
            .map_err(|e| CmdError::new("limit", e))?;
            Ok(Packed { frames, atlas, scale: s })
        }

        fn rect_json(x: u32, y: u32, w: u32, h: u32) -> Json {
            Json::obj().with("x", x).with("y", y).with("w", w).with("h", h)
        }

        /// Texture atlas + Aseprite-compatible JSON (array format: frames, frameTags, layers, slices),
        /// readable by Phaser, PixiJS, Unity/Godot Aseprite importers and LibGDX tools.
        pub fn export_atlas(doc: &Document, a: &Json) -> CmdResult<Query> {
            let p = pack_frames(doc, a)?;
            let image_name = opt_str(a, "image")?.unwrap_or("atlas.png");
            let base = doc.name.replace(['/', '\\'], "_");
            let frames: Vec<Json> = p
                .frames
                .iter()
                .zip(&p.atlas.placements)
                .enumerate()
                .map(|(k, (&fi, pl))| frame_entry(format!("{base} {k}.png"), pl, doc.frames[fi].duration_ms))
                .collect();
            let first = p.frames.first().copied().unwrap_or(0);
            let last = p.frames.last().copied().unwrap_or(0);
            let tags: Vec<Json> = doc
                .tags
                .iter()
                .filter_map(|t| {
                    let (from, to) = (doc.frame_index(t.from_id)?, doc.frame_index(t.to_id)?);
                    if from < first || to > last {
                        return None;
                    }
                    let mut j = Json::obj()
                        .with("name", t.name.as_str())
                        .with("from", from - first)
                        .with("to", to - first)
                        .with("direction", t.direction.as_str())
                        .with("color", "#000000ff");
                    if t.repeat > 0 {
                        j.set("repeat", t.repeat.to_string());
                    }
                    Some(j)
                })
                .collect();
            let layers = doc
                .layers
                .iter()
                .map(|l| Json::obj().with("name", l.name.as_str()).with("opacity", l.opacity).with("blendMode", l.blend.as_str()))
                .collect();
            let meta = Json::obj()
                .with("app", "Pixel Studio")
                .with("version", env!("CARGO_PKG_VERSION"))
                .with("image", image_name)
                .with("format", "RGBA8888")
                .with("size", Json::obj().with("w", p.atlas.image.width).with("h", p.atlas.image.height))
                .with("scale", p.scale.to_string())
                .with("frameTags", Json::Arr(tags))
                .with("layers", Json::Arr(layers))
                .with("slices", Json::Arr(doc.slices.iter().map(|s| slice_json(doc, s, p.scale)).collect()));
            let unique = p.atlas.placements.iter().filter(|pl| pl.same_as.is_none()).count();
            let data = Json::obj().with("frames", Json::Arr(frames)).with("meta", meta);
            Ok(png_query(&p.atlas.image, Json::obj().with("data", data).with("uniqueFrames", unique).with("frameCount", p.frames.len())))
        }

        fn frame_entry(filename: String, pl: &Placement, duration: u32) -> Json {
            let f = pl.frame;
            Json::obj()
                .with("filename", filename)
                .with("frame", rect_json(f.x, f.y, f.w, f.h))
                .with("rotated", false)
                .with("trimmed", pl.trimmed && (f.w != pl.source_w || f.h != pl.source_h))
                .with("spriteSourceSize", rect_json(pl.offset_x, pl.offset_y, f.w, f.h))
                .with("sourceSize", Json::obj().with("w", pl.source_w).with("h", pl.source_h))
                .with("duration", duration)
        }

        fn gcd(a: u32, b: u32) -> u32 {
            if b == 0 { a } else { gcd(b, a % b) }
        }

        /// Godot 4 `SpriteFrames` resource (.tres text) over the packed atlas: one animation per tag
        /// (or "default"), trimmed frames restored with AtlasTexture margins, per-frame durations.
        pub fn export_godot(doc: &Document, a: &Json) -> CmdResult<Query> {
            let p = pack_frames(doc, a)?;
            let texture = opt_str(a, "texture_path")?.unwrap_or("res://atlas.png");
            let fps_base = p.frames.iter().map(|&i| doc.frames[i].duration_ms.max(1)).fold(0, gcd).max(1);
            let speed = 1000.0 / fps_base as f64;
            let mut out = String::from("[gd_resource type=\"SpriteFrames\" format=3]\n\n");
            out += &format!("[ext_resource type=\"Texture2D\" path=\"{}\" id=\"1_atlas\"]\n\n", texture.replace('"', ""));
            for (k, pl) in p.atlas.placements.iter().enumerate() {
                let f = pl.frame;
                out += &format!("[sub_resource type=\"AtlasTexture\" id=\"AtlasTexture_{k}\"]\natlas = ExtResource(\"1_atlas\")\n");
                out += &format!("region = Rect2({}, {}, {}, {})\n", f.x, f.y, f.w, f.h);
                if pl.trimmed && (f.w != pl.source_w || f.h != pl.source_h) {
                    out += &format!("margin = Rect2({}, {}, {}, {})\n", pl.offset_x, pl.offset_y, pl.source_w - f.w, pl.source_h - f.h);
                }
                out += "\n";
            }
            let first = p.frames.first().copied().unwrap_or(0);
            let mut anims: Vec<(String, Vec<usize>, bool)> = Vec::new();
            for t in &doc.tags {
                let (Some(from), Some(to)) = (doc.frame_index(t.from_id), doc.frame_index(t.to_id)) else { continue };
                if from < first || to >= first + p.frames.len() {
                    continue;
                }
                let mut seq: Vec<usize> = (from..=to).map(|i| i - first).collect();
                match t.direction {
                    crate::animation::Direction::Reverse => seq.reverse(),
                    crate::animation::Direction::PingPong if seq.len() > 2 => {
                        let back: Vec<usize> = seq[1..seq.len() - 1].iter().rev().copied().collect();
                        seq.extend(back);
                    }
                    _ => {}
                }
                anims.push((t.name.clone(), seq, t.repeat == 0));
            }
            if anims.is_empty() {
                anims.push(("default".into(), (0..p.frames.len()).collect(), true));
            }
            out += "[resource]\nanimations = [";
            for (ai, (name, seq, looping)) in anims.iter().enumerate() {
                if ai > 0 {
                    out += ", ";
                }
                out += "{\n\"frames\": [";
                for (j, &k) in seq.iter().enumerate() {
                    if j > 0 {
                        out += ", ";
                    }
                    let dur = doc.frames[p.frames[k]].duration_ms as f64 / fps_base as f64;
                    out += &format!("{{\n\"duration\": {dur:.1},\n\"texture\": SubResource(\"AtlasTexture_{k}\")\n}}");
                }
                let name = name.replace(['"', '\\'], "");
                out += &format!("],\n\"loop\": {looping},\n\"name\": &\"{name}\",\n\"speed\": {speed:.1}\n}}");
            }
            out += "]\n";
            Ok(png_query(&p.atlas.image, Json::obj().with("tres", out).with("animations", anims.len()).with("texturePath", texture)))
        }

        pub fn export_tileset(doc: &Document, a: &Json) -> CmdResult<Query> {
            let ts = &doc.tilesets[tileset_idx(doc, a)?];
            let cols = opt_i64(a, "columns")?.unwrap_or(if ts.kind == TilesetKind::Blob { 8 } else { 16 }).clamp(1, 256) as u32;
            let img = ts.atlas(cols);
            let s = scale(a, 1)?;
            if img.width * s > 8192 || img.height * s > 8192 {
                return Err(CmdError::new("limit", "scaled output would exceed 8192px"));
            }
            let masks = if ts.kind == TilesetKind::Blob {
                Json::Arr(tiles::blob_masks().iter().map(|&m| Json::from(m as u32)).collect())
            } else {
                Json::Null
            };
            Ok(png_query(&img.scaled(s), Json::obj().with("columns", cols).with("tileCount", ts.tiles.len()).with("blobMasks", masks)))
        }

        pub fn tilemap_render(doc: &Document, a: &Json) -> CmdResult<Query> {
            let m = &doc.tilemaps[tilemap_idx(doc, a)?];
            let img = m.render(&doc.tilesets[m.tileset]);
            let s = scale(a, 1)?;
            if img.width * s > 8192 || img.height * s > 8192 {
                return Err(CmdError::new("limit", "scaled output would exceed 8192px"));
            }
            Ok(png_query(&img.scaled(s), Json::obj().with("tilemap", m.name.as_str())))
        }

        /// Tiled map (TMJ JSON, embedded tileset) + the tileset PNG it references. Blob tilesets also
        /// get a Wang set ("terrain") so the map stays editable with Tiled's terrain brush.
        pub fn export_tiled(doc: &Document, a: &Json) -> CmdResult<Query> {
            let m = &doc.tilemaps[tilemap_idx(doc, a)?];
            let ts = &doc.tilesets[m.tileset];
            let cols = if ts.kind == TilesetKind::Blob { 8 } else { 16u32.min(ts.tiles.len().max(1) as u32) };
            let img = ts.atlas(cols);
            let image_name = opt_str(a, "image")?.unwrap_or("tileset.png");
            let mut tileset = Json::obj()
                .with("firstgid", 1)
                .with("name", ts.name.as_str())
                .with("image", image_name)
                .with("imagewidth", img.width)
                .with("imageheight", img.height)
                .with("tilewidth", ts.tile_w)
                .with("tileheight", ts.tile_h)
                .with("tilecount", ts.tiles.len())
                .with("columns", cols)
                .with("margin", 0)
                .with("spacing", 0);
            if ts.kind == TilesetKind::Blob {
                // Tiled corner+edge ("mixed") Wang IDs: [top, top-right, right, bottom-right, bottom, bottom-left, left, top-left].
                let bits = [tiles::N, tiles::NE, tiles::E, tiles::SE, tiles::S, tiles::SW, tiles::W, tiles::NW];
                let wangtiles = tiles::blob_masks()
                    .iter()
                    .enumerate()
                    .map(|(i, &mask)| {
                        let id = bits.iter().map(|&b| Json::from(if mask & b != 0 { 1 } else { 0 })).collect();
                        Json::obj().with("tileid", i).with("wangid", Json::Arr(id))
                    })
                    .collect();
                tileset.set(
                    "wangsets",
                    Json::Arr(vec![
                        Json::obj()
                            .with("name", ts.name.as_str())
                            .with("type", "mixed")
                            .with("tile", 46)
                            .with(
                                "colors",
                                Json::Arr(vec![
                                    Json::obj()
                                        .with("name", ts.name.as_str())
                                        .with("color", "#4caf50")
                                        .with("tile", 46)
                                        .with("probability", 1),
                                ]),
                            )
                            .with("wangtiles", Json::Arr(wangtiles)),
                    ]),
                );
            }
            let layer = Json::obj()
                .with("id", 1)
                .with("name", m.name.as_str())
                .with("type", "tilelayer")
                .with("x", 0)
                .with("y", 0)
                .with("width", m.cols)
                .with("height", m.rows)
                .with("opacity", 1)
                .with("visible", true)
                .with("data", Json::Arr(m.cells.iter().map(|&c| Json::from(c)).collect()));
            let tmj = Json::obj()
                .with("type", "map")
                .with("version", "1.10")
                .with("tiledversion", "1.11.2")
                .with("orientation", "orthogonal")
                .with("renderorder", "right-down")
                .with("infinite", false)
                .with("width", m.cols)
                .with("height", m.rows)
                .with("tilewidth", ts.tile_w)
                .with("tileheight", ts.tile_h)
                .with("compressionlevel", -1)
                .with("nextlayerid", 2)
                .with("nextobjectid", 1)
                .with("layers", Json::Arr(vec![layer]))
                .with("tilesets", Json::Arr(vec![tileset]));
            Ok(png_query(&img, Json::obj().with("tmj", tmj).with("image", image_name)))
        }
    }
    pub mod pixel2d {
        //! Pixel-art craft commands: ASCII drawing, selective outlines, hue-shifted ramps, palette
        //! library, quantization with dithering, dither fills and sprite normal maps.

        use std::sync::Arc;

        use super::args::{color, color_value, frame_index, layer_index, opt_bool, opt_f64, opt_i64, opt_str, region, scale, target};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome, Query};
        use crate::export;
        use crate::pixel::{Palette, PixelGrid, Rgba, TRANSPARENT};
        use crate::pixelart::{self, Dither, palettes::PALETTES};
        use crate::project::Document;
        use crate::renderer::{RenderOptions, render};

        /// Draws ASCII art: one string per row, a legend maps characters to colors. The format
        /// matches `read_region` (ascii), so agents can read, edit and write sprites as text.
        pub fn draw_ascii(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let rows = a.get("rows").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'rows' must be an array of strings"))?;
            let legend = match a.get("legend") {
                Some(Json::Obj(entries)) => entries,
                _ => return Err(CmdError::invalid("'legend' must map single characters to colors, e.g. {\"o\": \"#1B1B2F\"}")),
            };
            let mut map: Vec<(char, u32)> = Vec::new();
            for (k, v) in legend {
                let mut chars = k.chars();
                let (Some(ch), None) = (chars.next(), chars.next()) else {
                    return Err(CmdError::invalid(format!("legend key '{k}' must be one character")));
                };
                map.push((ch, color_value(doc, v, &format!("legend.{k}"))?));
            }
            let skip = opt_str(a, "transparent")?.unwrap_or(".").chars().next().unwrap_or('.');
            let erase = opt_bool(a, "erase_transparent")?.unwrap_or(false);
            let (x0, y0) = (opt_i64(a, "x")?.unwrap_or(0), opt_i64(a, "y")?.unwrap_or(0));
            let flip = opt_bool(a, "flip_x")?.unwrap_or(false);
            let (l, f) = target(doc, a)?;
            let mut writes = Vec::new();
            for (y, row) in rows.iter().enumerate() {
                let s = row.as_str().ok_or_else(|| CmdError::invalid(format!("rows[{y}] must be a string")))?;
                let chars: Vec<char> = s.chars().collect();
                let w = chars.len();
                for (x, ch) in chars.into_iter().enumerate() {
                    let xx = if flip { w - 1 - x } else { x };
                    if ch == skip || ch == ' ' {
                        if erase {
                            writes.push((x0 + xx as i64, y0 + y as i64, TRANSPARENT));
                        }
                        continue;
                    }
                    let v = map
                        .iter()
                        .find(|(c, _)| *c == ch)
                        .map(|(_, v)| *v)
                        .ok_or_else(|| CmdError::invalid(format!("character '{ch}' in rows[{y}] is not in the legend")))?;
                    writes.push((x0 + xx as i64, y0 + y as i64, v));
                }
            }
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            for (x, y, v) in writes {
                changed += grid.set(x, y, v) as u64;
            }
            Ok(Outcome::new(Json::obj().with("changed", changed).with("rows", rows.len()), changed))
        }

        fn with_cel_image(doc: &mut Document, a: &Json, f: impl FnOnce(&mut crate::renderer::Image)) -> CmdResult<u64> {
            let (l, fr) = target(doc, a)?;
            let grid = doc.cel_mut(l, fr);
            let mut img = pixelart::grid_to_image(grid);
            f(&mut img);
            Ok(pixelart::image_to_grid(&img, grid) as u64)
        }

        /// 1-px outline around a layer's pixels: `color`, selective (darker, cooler bordering color) or
        /// `mode: "selout"` (selective + lighter on the lit side). `inside` draws on the edge pixels
        /// instead of growing the shape; `darkness` 0-1 sets how dark generated outlines are.
        pub fn outline(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let col = match a.get("color") {
                None | Some(Json::Null) => None,
                Some(Json::Str(s)) if s == "selective" || s == "selout" => None,
                Some(v) => Some(Rgba::from_u32(color_value(doc, v, "color")?)),
            };
            let corners = opt_bool(a, "corners")?.unwrap_or(false);
            let mode =
                opt_str(a, "mode")?.unwrap_or(if a.get("color").and_then(Json::as_str) == Some("selout") { "selout" } else { "selective" });
            if !["selective", "selout", "solid"].contains(&mode) {
                return Err(CmdError::invalid("'mode' must be selective, selout or solid"));
            }
            let darkness = opt_f64(a, "darkness")?.unwrap_or(0.58).clamp(0.05, 0.95) as f32;
            let inside = opt_bool(a, "inside")?.unwrap_or(false);
            let light = super::charart::light_arg(a, "light", (-1.0, -1.2))?;
            let changed = with_cel_image(doc, a, |img| {
                crate::pixelart::charart::outline_ex(img, col, corners, mode == "selout", darkness, light, inside);
            })?;
            Ok(Outcome::new(Json::obj().with("changed", changed).with("mode", mode), changed))
        }

        /// Builds a hue-shifted ramp and (by default) adds it to the swatches.
        pub fn color_ramp(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let base = Rgba::from_u32(color(doc, a, "base")?);
            let steps = opt_i64(a, "steps")?.unwrap_or(5).clamp(2, 16) as usize;
            let hue = opt_f64(a, "hue_shift")?.unwrap_or(18.0).clamp(0.0, 90.0) as f32;
            let contrast = opt_f64(a, "contrast")?.unwrap_or(0.75).clamp(0.05, 1.0) as f32;
            let colors = pixelart::ramp(base, steps, hue, contrast);
            if let Some(name) = opt_str(a, "name")? {
                let px: Vec<u32> = colors.iter().map(|c| c.to_pixel()).collect();
                match doc.ramps.iter_mut().find(|r| r.name == name) {
                    Some(r) => r.colors = px,
                    None => doc.ramps.push(crate::project::Ramp { name: name.to_string(), colors: px }),
                }
            }
            if opt_bool(a, "add_to_palette")?.unwrap_or(true) {
                for c in &colors {
                    doc.palette.intern(*c).map_err(|e| CmdError::new("limit", e))?;
                }
            }
            Ok(Outcome::new(Json::obj().with("ramp", Json::Arr(colors.iter().map(|c| Json::from(c.to_hex())).collect())), 0))
        }

        pub fn palette_list() -> Query {
            let list = PALETTES
                .iter()
                .map(|(n, credit, c)| Json::obj().with("name", *n).with("credit", *credit).with("colors", c.len()))
                .collect();
            Query { result: Json::obj().with("palettes", Json::Arr(list)), binary: Vec::new() }
        }

        /// Loads a library palette (or explicit colors) into the swatches.
        pub fn palette_load(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let preset = match opt_str(a, "preset")? {
                Some(p) => Some(p),
                None => opt_str(a, "name")?,
            };
            let colors: Vec<Rgba> = match (preset, a.get("colors")) {
                (Some(n), _) => pixelart::palette_by_name(n)
                    .ok_or_else(|| CmdError::not_found(format!("unknown palette '{n}' (see canvas_inspect detail palette)")))?,
                (None, Some(Json::Arr(list))) => {
                    list.iter().map(|c| color_value(doc, c, "colors").map(Rgba::from_u32)).collect::<CmdResult<_>>()?
                }
                _ => return Err(CmdError::invalid("give 'index'+'color' (edit a swatch), 'preset' (library palette) or 'colors'")),
            };
            let replace = match opt_str(a, "mode")? {
                Some("add") => false,
                Some("replace") | None => opt_bool(a, "replace")?.unwrap_or(true),
                Some(other) => return Err(CmdError::invalid(format!("unknown mode '{other}' (replace, add)"))),
            };
            if replace {
                let mut all = vec![Rgba::TRANSPARENT];
                all.extend(colors.iter().copied());
                doc.palette = Palette::from_colors(all).map_err(CmdError::invalid)?;
            } else {
                for c in &colors {
                    doc.palette.intern(*c).map_err(|e| CmdError::new("limit", e))?;
                }
            }
            Ok(Outcome::new(Json::obj().with("loaded", colors.len()).with("paletteSize", doc.palette.len()), 0))
        }

        /// Resolves the target palette for quantization: a library name, "swatches", or N colors.
        pub fn quantize_palette(doc: &Document, a: &Json, sample: &[(Rgba, u32)]) -> CmdResult<Vec<Rgba>> {
            match a.get("palette") {
                Some(Json::Str(s)) if s == "swatches" => Ok(doc.palette.colors().iter().skip(1).copied().collect()),
                Some(Json::Str(s)) => pixelart::palette_by_name(s).ok_or_else(|| CmdError::not_found(format!("unknown palette '{s}'"))),
                Some(Json::Arr(list)) => list.iter().map(|c| color_value(doc, c, "palette").map(Rgba::from_u32)).collect(),
                _ => {
                    let n = opt_i64(a, "colors")?.unwrap_or(16).clamp(2, 256) as usize;
                    Ok(pixelart::median_cut(sample, n))
                }
            }
        }

        pub fn dither_arg(a: &Json) -> CmdResult<Dither> {
            match opt_str(a, "dither")? {
                None => Ok(Dither::None),
                Some(d) => Dither::parse(d).ok_or_else(|| CmdError::invalid("'dither' must be none, bayer2, bayer4 or bayer8")),
            }
        }

        /// Reduces colors of a cel, a frame (all layers), a layer (all frames) or everything to a
        /// limited palette — the step that turns renders and photos into readable pixel art.
        pub fn quantize(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let scope = opt_str(a, "scope")?.unwrap_or("cel");
            let li = layer_index(doc, a, "layer")?;
            let fi = frame_index(doc, a, "frame")?;
            let cels: Vec<(u32, u32)> = match scope {
                "cel" => vec![(doc.layers[li].id, doc.frames[fi].id)],
                "layer" => doc.frames.iter().map(|f| (doc.layers[li].id, f.id)).collect(),
                "frame" => doc.layers.iter().map(|l| (l.id, doc.frames[fi].id)).collect(),
                "all" => doc.layers.iter().flat_map(|l| doc.frames.iter().map(move |f| (l.id, f.id))).collect(),
                other => return Err(CmdError::invalid(format!("unknown scope '{other}'"))),
            };
            let dither = dither_arg(a)?;
            let strength = opt_f64(a, "dither_strength")?.unwrap_or(28.0).clamp(0.0, 255.0) as f32;
            // One shared palette across the whole scope keeps animation frames consistent.
            let mut samples: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
            for key in &cels {
                if let Some(g) = doc.cels.get(key) {
                    g.for_each_opaque(|_, _, v| *samples.entry(v | 0xFF).or_default() += 1);
                }
            }
            let mut sample: Vec<(Rgba, u32)> = samples.into_iter().map(|(c, n)| (Rgba::from_u32(c), n)).collect();
            sample.sort_by_key(|(c, _)| c.to_u32());
            let pal = quantize_palette(doc, a, &sample)?;
            if pal.is_empty() {
                return Ok(Outcome::new(Json::obj().with("changed", 0), 0));
            }
            let mut changed = 0u64;
            for key in cels {
                if doc.layers.iter().any(|l| l.id == key.0 && l.locked) {
                    continue;
                }
                let Some(cel) = doc.cels.get_mut(&key) else { continue };
                let grid: &mut PixelGrid = Arc::make_mut(cel);
                let mut img = pixelart::grid_to_image(grid);
                pixelart::quantize_image(&mut img, &pal, dither, strength);
                changed += pixelart::image_to_grid(&img, grid) as u64;
            }
            if opt_bool(a, "add_to_palette")?.unwrap_or(false) {
                for c in &pal {
                    let _ = doc.palette.intern(*c);
                }
            }
            Ok(Outcome::new(
                Json::obj().with("changed", changed).with("palette", Json::Arr(pal.iter().map(|c| Json::from(c.to_hex())).collect())),
                changed,
            ))
        }

        /// Fills a region with an ordered-dither blend: level 0 = all `color`, 16 = all `color2`.
        pub fn dither_fill(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let r = region(doc, a)?;
            let c1 = color(doc, a, "color")?;
            let c2 = color(doc, a, "color2")?;
            let level = opt_i64(a, "level")?.unwrap_or(8).clamp(0, 16) as u32;
            let only_opaque = opt_bool(a, "only_opaque")?.unwrap_or(false);
            let (l, f) = target(doc, a)?;
            let grid = doc.cel_mut(l, f);
            let mut changed = 0u64;
            for y in r.y..r.y + r.h {
                for x in r.x..r.x + r.w {
                    if only_opaque && grid.get(x, y) == TRANSPARENT {
                        continue;
                    }
                    let v = if pixelart::dither_pick(x, y, level) { c2 } else { c1 };
                    changed += grid.set(x, y, v) as u64;
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        /// Normal map of a frame (or one layer) for 2D dynamic lighting (Godot/Unity sprite normals).
        pub fn export_normal_map(doc: &Document, a: &Json) -> CmdResult<Query> {
            let fi = frame_index(doc, a, "frame")?;
            let mut opts = RenderOptions::default();
            if a.get("layer").is_some_and(|v| !v.is_null()) {
                opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
            }
            let img = render(doc, fi, &opts);
            let strength = opt_f64(a, "strength")?.unwrap_or(3.0).clamp(0.1, 20.0) as f32;
            let bevel = opt_f64(a, "bevel")?.unwrap_or(3.0).clamp(0.5, 32.0) as f32;
            let n = pixelart::normal_map(&img, strength, bevel).scaled(scale(a, 1)?);
            let png = export::png::encode(&n);
            Ok(Query { result: Json::obj().with("width", n.width).with("height", n.height).with("mimeType", "image/png"), binary: png })
        }
    }
    mod query {
        //! Read-only commands: inspection, pixel reads, rendering and export.

        use super::args::{coord, frame_index, layer_index, opt_bool, opt_i64, opt_str, region, scale};
        use super::json::Json;
        use super::{CmdError, CmdResult, Query};
        use crate::animation::{Direction, playback_order, timing};
        use crate::export::{self, SheetFrame, gif::GifFrame};
        use crate::history::History;
        use crate::pixel::{Rgba, TRANSPARENT};
        use crate::project::Document;
        use crate::renderer::{Image, OnionSkin, RenderOptions, render};
        use crate::storage;

        pub const MAX_READ_AREA: i64 = 128 * 128;
        const MAX_OUTPUT_SIDE: u32 = 8192;
        /// Symbols for the compact text view; '.' is transparent.
        const ASCII_KEYS: &str = "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz#$%&*+=?@^~<>/|";

        fn q(result: Json) -> CmdResult<Query> {
            Ok(Query { result, binary: Vec::new() })
        }

        pub fn run_query(
            doc: &Document,
            history: &History,
            revision: u64,
            op: &str,
            a: &Json,
            cache: &mut super::voxel::RenderCache,
        ) -> CmdResult<Query> {
            match op {
                "voxel_inspect" => super::voxel::voxel_inspect(doc, a),
                "voxel_render" => super::voxel::voxel_render(doc, cache, a),
                "voxel_render_sheet" => super::voxel::voxel_render_sheet(doc, cache, a),
                "voxel_pick" => super::voxel::voxel_pick(doc, cache, a),
                "render_lit" => super::gamedev::render_lit(doc, a),
                "tilemap_render" => super::gamedev::tilemap_render(doc, a),
                "status" => q(Json::obj()
                    .with("engine", "pixcore")
                    .with("version", env!("CARGO_PKG_VERSION"))
                    .with("revision", revision)
                    .with("project", doc.name.as_str())
                    .with("undo", history.can_undo())
                    .with("redo", history.can_redo())
                    .with("abiVersion", crate::ffi::ABI_VERSION)
                    .with("operations", Json::Arr(super::OPERATIONS.iter().map(|(name, _)| Json::from(*name)).collect()))),
                // One inspector: summary (default), full, frame, animation, palette, gamedev.
                "canvas_inspect" => match opt_str(a, "detail")?.unwrap_or("summary") {
                    "summary" => q(inspect(doc, history, revision, false)),
                    "full" => q(inspect(doc, history, revision, true)),
                    "frame" => frame_get(doc, a),
                    "animation" => animation_info(doc, a),
                    "gamedev" => super::gamedev::gamedev_inspect(doc),
                    "palette" => palette_info(doc),
                    other => {
                        Err(CmdError::invalid(format!("unknown detail '{other}' (summary, full, frame, animation, palette, gamedev)")))
                    }
                },
                "get_pixel" => get_pixel(doc, a),
                "read_region" => read_region(doc, a),
                "history_list" => history_list(history, a),
                "render_frame" => render_frame(doc, a),
                "canvas_snapshot" => snapshot(doc, revision, a),
                // One exporter for every game-ready format.
                "export" => match super::args::req_str(a, "format")? {
                    "png" => export_png(doc, a),
                    "gif" => export_gif(doc, a),
                    "spritesheet" => export_sheet(doc, a),
                    "normal_map" => super::pixel2d::export_normal_map(doc, a),
                    "atlas" => super::gamedev::export_atlas(doc, a),
                    "godot" => super::gamedev::export_godot(doc, a),
                    "tileset" => super::gamedev::export_tileset(doc, a),
                    "tiled" => super::gamedev::export_tiled(doc, a),
                    other => Err(CmdError::invalid(format!(
                        "unknown format '{other}' (png, gif, spritesheet, normal_map, atlas, godot, tileset, tiled)"
                    ))),
                },
                "project_save" => {
                    let log: Vec<_> = history.log_entries().cloned().collect();
                    let bytes = storage::save(doc, &log);
                    Ok(Query { result: Json::obj().with("format", "pxs").with("bytes", bytes.len()), binary: bytes })
                }
                _ => Err(CmdError::new("unknown_op", format!("unknown operation '{op}'"))),
            }
        }

        fn palette_info(doc: &Document) -> CmdResult<Query> {
            let presets = super::pixel2d::palette_list().result;
            q(Json::obj()
                .with("presets", presets.get("palettes").cloned().unwrap_or(presets.clone()))
                .with(
                    "ramps",
                    Json::Arr(
                        doc.ramps
                            .iter()
                            .map(|r| {
                                Json::obj()
                                    .with("name", r.name.as_str())
                                    .with("colors", Json::Arr(r.colors.iter().map(|c| Json::from(Rgba::from_u32(*c).to_hex())).collect()))
                            })
                            .collect(),
                    ),
                )
                .with("size", doc.palette.len())
                .with(
                    "colors",
                    Json::Arr(
                        doc.palette
                            .colors()
                            .iter()
                            .enumerate()
                            .skip(1)
                            .map(|(i, c)| Json::obj().with("index", i).with("color", c.to_hex()))
                            .collect(),
                    ),
                ))
        }

        fn inspect(doc: &Document, history: &History, revision: u64, detailed: bool) -> Json {
            let active_frame = doc.frame_index(doc.active_frame).unwrap_or(0);
            let active_layer = doc.layer_index(doc.active_layer).unwrap_or(0);
            let layers = doc
                .layers
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    let mut j = Json::obj()
                        .with("index", i)
                        .with("id", l.id)
                        .with("name", l.name.as_str())
                        .with("visible", l.visible)
                        .with("locked", l.locked)
                        .with("opacity", l.opacity)
                        .with("blend", l.blend.as_str());
                    if detailed {
                        let (n, _) = doc.cel_stats(l.id, doc.active_frame);
                        j.set("pixelsInActiveFrame", n);
                    }
                    j
                })
                .collect();
            let frames = doc
                .frames
                .iter()
                .enumerate()
                .map(|(i, f)| Json::obj().with("index", i).with("id", f.id).with("duration", f.duration_ms))
                .collect();
            let durations: Vec<u32> = doc.frames.iter().map(|f| f.duration_ms).collect();
            let t = timing(&durations);
            let mut j = Json::obj()
                .with("name", doc.name.as_str())
                .with("width", doc.width)
                .with("height", doc.height)
                .with("revision", revision)
                .with("activeLayer", active_layer)
                .with("activeFrame", active_frame)
                .with("layers", Json::Arr(layers))
                .with("frameCount", doc.frames.len())
                .with("frames", Json::Arr(frames))
                .with("tags", Json::Arr(doc.tags.iter().map(|t| tag_json(doc, t)).collect()))
                .with("paletteSize", doc.palette.len())
                .with("totalDurationMs", t.total_ms)
                .with("averageFps", (t.average_fps * 100.0).round() / 100.0)
                .with("undoAvailable", history.can_undo())
                .with("redoAvailable", history.can_redo())
                .with("slices", Json::Arr(doc.slices.iter().map(|s| Json::from(s.name.as_str())).collect()))
                .with("tilesets", doc.tilesets.len())
                .with("tilemaps", doc.tilemaps.len());
            if detailed {
                let tiles: usize = doc.cels.values().map(|c| c.allocated_tiles()).sum();
                j.set("celCount", doc.cels.len());
                j.set("allocatedTiles", tiles);
            }
            j
        }

        fn tag_json(doc: &Document, t: &crate::animation::Tag) -> Json {
            Json::obj()
                .with("name", t.name.as_str())
                .with("from", doc.frame_index(t.from_id))
                .with("to", doc.frame_index(t.to_id))
                .with("direction", t.direction.as_str())
                .with("repeat", t.repeat)
        }

        /// Colors of a rectangle either from one layer's cel or from the composited frame.
        fn read_colors(doc: &Document, a: &Json, x: i64, y: i64, w: i64, h: i64) -> CmdResult<Vec<Option<Rgba>>> {
            let fi = frame_index(doc, a, "frame")?;
            if opt_bool(a, "composite")?.unwrap_or(false) {
                let img = render(doc, fi, &RenderOptions::default());
                let mut out = Vec::with_capacity((w * h) as usize);
                for yy in y..y + h {
                    for xx in x..x + w {
                        let inside = xx >= 0 && yy >= 0 && xx < doc.width as i64 && yy < doc.height as i64;
                        let c = if inside { img.get(xx as u32, yy as u32) } else { Rgba::TRANSPARENT };
                        out.push((c.a > 0).then_some(c));
                    }
                }
                return Ok(out);
            }
            let li = layer_index(doc, a, "layer")?;
            let grid = doc.cel(doc.layers[li].id, doc.frames[fi].id);
            let mut idx = Vec::with_capacity((w * h) as usize);
            for yy in y..y + h {
                for xx in x..x + w {
                    idx.push(grid.map(|g| g.get(xx, yy)).unwrap_or(TRANSPARENT));
                }
            }
            Ok(idx.iter().map(|&p| if p == TRANSPARENT { None } else { Some(Rgba::from_u32(p)) }).collect())
        }

        fn get_pixel(doc: &Document, a: &Json) -> CmdResult<Query> {
            let (x, y) = (coord(a, "x")?, coord(a, "y")?);
            if x < 0 || y < 0 || x >= doc.width as i64 || y >= doc.height as i64 {
                return Err(CmdError::invalid(format!("pixel ({x},{y}) is outside the {}x{} canvas", doc.width, doc.height)));
            }
            let colors = read_colors(doc, a, x, y, 1, 1)?;
            q(Json::obj().with("x", x).with("y", y).with("color", colors[0].map(|c| c.to_hex())).with("transparent", colors[0].is_none()))
        }

        /// Returns pixels as hex rows or a compact character map with a color legend.
        fn read_region(doc: &Document, a: &Json) -> CmdResult<Query> {
            let r = region(doc, a)?;
            if r.w * r.h > MAX_READ_AREA {
                return Err(CmdError::new(
                    "limit",
                    format!("read at most {MAX_READ_AREA} pixels per call (e.g. 128x128); split the region"),
                ));
            }
            let format = opt_str(a, "format")?.unwrap_or("ascii");
            let colors = read_colors(doc, a, r.x, r.y, r.w, r.h)?;
            let mut j = Json::obj().with("x", r.x).with("y", r.y).with("width", r.w).with("height", r.h).with("format", format);
            match format {
                "hex" => {
                    let rows = colors
                        .chunks(r.w as usize)
                        .map(|row| Json::Arr(row.iter().map(|c| Json::from(c.map(|c| c.to_hex()))).collect()))
                        .collect();
                    j.set("rows", Json::Arr(rows));
                }
                "ascii" => {
                    let keys: Vec<char> = ASCII_KEYS.chars().collect();
                    let mut legend: Vec<(Rgba, char)> = Vec::new();
                    let mut rows = Vec::new();
                    for row in colors.chunks(r.w as usize) {
                        let mut s = String::with_capacity(row.len());
                        for c in row {
                            match c {
                                None => s.push('.'),
                                Some(c) => {
                                    let k = match legend.iter().find(|(lc, _)| lc == c) {
                                        Some((_, k)) => *k,
                                        None => {
                                            let k = *keys.get(legend.len()).ok_or_else(|| {
                                                CmdError::new("limit", "too many distinct colors for ascii format; use format 'hex'")
                                            })?;
                                            legend.push((*c, k));
                                            k
                                        }
                                    };
                                    s.push(k);
                                }
                            }
                        }
                        rows.push(Json::Str(s));
                    }
                    let mut lg = Json::obj().with(".", "transparent");
                    for (c, k) in legend {
                        lg.set(&k.to_string(), c.to_hex());
                    }
                    j.set("legend", lg);
                    j.set("rows", Json::Arr(rows));
                }
                other => return Err(CmdError::invalid(format!("unknown format '{other}' (ascii, hex)"))),
            }
            q(j)
        }

        fn frame_get(doc: &Document, a: &Json) -> CmdResult<Query> {
            let fi = frame_index(doc, a, "frame")?;
            let f = &doc.frames[fi];
            let cels = doc
                .layers
                .iter()
                .enumerate()
                .map(|(li, l)| {
                    let (n, b) = doc.cel_stats(l.id, f.id);
                    let bounds = b.map(|(x, y, w, h)| Json::obj().with("x", x).with("y", y).with("width", w).with("height", h));
                    Json::obj().with("layer", li).with("name", l.name.as_str()).with("pixels", n).with("bounds", bounds)
                })
                .collect();
            let tags: Vec<Json> = doc
                .tags
                .iter()
                .filter(|t| {
                    let (a, b) = (doc.frame_index(t.from_id).unwrap_or(0), doc.frame_index(t.to_id).unwrap_or(0));
                    fi >= a && fi <= b
                })
                .map(|t| Json::from(t.name.as_str()))
                .collect();
            q(Json::obj()
                .with("index", fi)
                .with("id", f.id)
                .with("duration", f.duration_ms)
                .with("tags", Json::Arr(tags))
                .with("cels", Json::Arr(cels)))
        }

        /// Frame range (and direction) from `tag` or `from`/`to`, defaulting to all frames.
        fn sequence(doc: &Document, a: &Json) -> CmdResult<(Vec<usize>, Option<u32>)> {
            if let Some(name) = opt_str(a, "tag")? {
                let t = doc.tags.iter().find(|t| t.name == name).ok_or_else(|| CmdError::not_found(format!("no tag named '{name}'")))?;
                let (from, to) = (doc.frame_index(t.from_id).unwrap_or(0), doc.frame_index(t.to_id).unwrap_or(0));
                return Ok((playback_order(from, to, t.direction), Some(t.repeat)));
            }
            let from = opt_i64(a, "from")?.unwrap_or(0);
            let to = opt_i64(a, "to")?.unwrap_or(doc.frames.len() as i64 - 1);
            if from < 0 || to >= doc.frames.len() as i64 || from > to {
                return Err(CmdError::invalid("invalid frame range"));
            }
            let dir = match opt_str(a, "direction")? {
                Some(d) => Direction::parse(d).ok_or_else(|| CmdError::invalid("'direction' must be forward, reverse or pingpong"))?,
                None => Direction::Forward,
            };
            Ok((playback_order(from as usize, to as usize, dir), None))
        }

        fn animation_info(doc: &Document, a: &Json) -> CmdResult<Query> {
            let (order, repeat) = sequence(doc, a)?;
            let durations: Vec<u32> = order.iter().map(|&i| doc.frames[i].duration_ms).collect();
            let t = timing(&durations);
            let seq = order.iter().zip(&durations).map(|(&i, &d)| Json::obj().with("frame", i).with("duration", d)).collect();
            q(Json::obj()
                .with("sequence", Json::Arr(seq))
                .with("totalMs", t.total_ms)
                .with("averageFps", (t.average_fps * 100.0).round() / 100.0)
                .with("repeat", repeat.unwrap_or(0))
                .with("tags", Json::Arr(doc.tags.iter().map(|t| tag_json(doc, t)).collect())))
        }

        fn history_list(history: &History, a: &Json) -> CmdResult<Query> {
            let limit = opt_i64(a, "limit")?.unwrap_or(50).clamp(1, 1000) as usize;
            let entries = history
                .log_entries()
                .rev()
                .take(limit)
                .map(|e| {
                    Json::obj()
                        .with("revision", e.revision)
                        .with("actor", e.actor.as_str())
                        .with("op", e.op.as_str())
                        .with("operationId", e.operation_id.clone())
                        .with("changedPixels", e.changed_pixels)
                        .with("timestamp", e.timestamp_ms)
                })
                .collect();
            q(Json::obj()
                .with("entries", Json::Arr(entries))
                .with("undoAvailable", history.can_undo())
                .with("redoAvailable", history.can_redo()))
        }

        fn options(doc: &Document, a: &Json) -> CmdResult<RenderOptions> {
            let mut opts = RenderOptions::default();
            if a.get("layer").is_some_and(|v| !v.is_null()) {
                opts.only_layer = Some(doc.layers[layer_index(doc, a, "layer")?].id);
            }
            if let Some(bg) = opt_str(a, "background")? {
                opts.background = Some(Rgba::parse_hex(bg).ok_or_else(|| CmdError::invalid("'background' must be a hex color"))?);
            }
            if let Some(d) = opt_i64(a, "density")? {
                if !matches!(d, 1 | 2 | 4) {
                    return Err(CmdError::invalid("'density' must be 1, 2 or 4"));
                }
                opts.density = Some(d as u32);
            }
            if let Some(o) = a.get("onion").filter(|o| !o.is_null()) {
                let n = |k: &str, d: i64| opt_i64(o, k).map(|v| v.unwrap_or(d));
                let (before, after, opacity) = (n("before", 1)?, n("after", 0)?, n("opacity", 96)?);
                if !(0..=8).contains(&before) || !(0..=8).contains(&after) || !(0..=255).contains(&opacity) {
                    return Err(CmdError::invalid("onion: before/after 0-8, opacity 0-255"));
                }
                opts.onion = Some(OnionSkin {
                    before: before as usize,
                    after: after as usize,
                    opacity: opacity as u8,
                    tint: opt_bool(o, "tint")?.unwrap_or(true),
                });
            }
            Ok(opts)
        }

        fn check_output(doc: &Document, s: u32) -> CmdResult<()> {
            let s = s * doc.max_density();
            if doc.width * s > MAX_OUTPUT_SIDE || doc.height * s > MAX_OUTPUT_SIDE {
                return Err(CmdError::new("limit", format!("scaled output would exceed {MAX_OUTPUT_SIDE}px; lower 'scale'")));
            }
            Ok(())
        }

        fn render_frame(doc: &Document, a: &Json) -> CmdResult<Query> {
            let fi = frame_index(doc, a, "frame")?;
            let s = scale(a, 1)?;
            check_output(doc, s)?;
            let img = render(doc, fi, &options(doc, a)?).scaled(s);
            Ok(Query {
                result: Json::obj().with("width", img.width).with("height", img.height).with("format", "rgba8").with("frame", fi),
                binary: img.pixels,
            })
        }

        fn auto_scale(doc: &Document) -> u32 {
            (512 / doc.width.max(doc.height)).clamp(1, 16)
        }

        /// The agent's "eyes": a PNG plus a structured description of what is on the canvas.
        fn snapshot(doc: &Document, revision: u64, a: &Json) -> CmdResult<Query> {
            let fi = frame_index(doc, a, "frame")?;
            let s = scale(a, auto_scale(doc))?;
            check_output(doc, s)?;
            let img = render(doc, fi, &options(doc, a)?);
            let mut used: Vec<(Rgba, u32)> = Vec::new();
            let (mut x0, mut y0, mut x1, mut y1, mut any) = (u32::MAX, u32::MAX, 0, 0, false);
            for y in 0..img.height {
                for x in 0..img.width {
                    let c = img.get(x, y);
                    if c.a == 0 {
                        continue;
                    }
                    any = true;
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
                    match used.iter_mut().find(|(u, _)| *u == c) {
                        Some((_, n)) => *n += 1,
                        None => used.push((c, 1)),
                    }
                }
            }
            used.sort_by_key(|u| std::cmp::Reverse(u.1));
            let colors = used.iter().take(64).map(|(c, n)| Json::obj().with("color", c.to_hex()).with("pixels", *n)).collect();
            let bounds = any.then(|| Json::obj().with("x", x0).with("y", y0).with("width", x1 - x0 + 1).with("height", y1 - y0 + 1));
            let png = export::png::encode(&img.scaled(s));
            let result = Json::obj()
                .with(
                    "canvas",
                    Json::obj()
                        .with("width", doc.width)
                        .with("height", doc.height)
                        .with("activeFrame", fi)
                        .with("frameCount", doc.frames.len())
                        .with("revision", revision),
                )
                .with("contentBounds", bounds)
                .with("usedColors", Json::Arr(colors))
                .with("distinctColors", used.len())
                .with("imageScale", s)
                .with("mimeType", "image/png");
            Ok(Query { result, binary: png })
        }

        fn export_png(doc: &Document, a: &Json) -> CmdResult<Query> {
            let fi = frame_index(doc, a, "frame")?;
            let s = scale(a, 1)?;
            check_output(doc, s)?;
            let img = render(doc, fi, &options(doc, a)?).scaled(s);
            let png = export::png::encode(&img);
            Ok(Query {
                result: Json::obj()
                    .with("width", img.width)
                    .with("height", img.height)
                    .with("mimeType", "image/png")
                    .with("bytes", png.len()),
                binary: png,
            })
        }

        fn rendered_sequence(doc: &Document, a: &Json) -> CmdResult<(Vec<usize>, Vec<Image>, Option<u32>)> {
            let (order, repeat) = sequence(doc, a)?;
            let s = scale(a, 1)?;
            check_output(doc, s)?;
            let mut opts = options(doc, a)?;
            opts.onion = None;
            let images = order.iter().map(|&i| render(doc, i, &opts).scaled(s)).collect();
            Ok((order, images, repeat))
        }

        fn export_gif(doc: &Document, a: &Json) -> CmdResult<Query> {
            let (order, images, repeat) = rendered_sequence(doc, a)?;
            let frames: Vec<GifFrame> =
                order.iter().zip(&images).map(|(&i, img)| GifFrame { image: img, duration_ms: doc.frames[i].duration_ms }).collect();
            let (w, h) = (images[0].width, images[0].height);
            let loops = opt_i64(a, "loop")?.or(repeat.map(|r| r as i64)).unwrap_or(0).clamp(0, 65535) as u16;
            let gif = export::gif::encode(w, h, &frames, loops);
            Ok(Query {
                result: Json::obj()
                    .with("width", w)
                    .with("height", h)
                    .with("frames", frames.len())
                    .with("mimeType", "image/gif")
                    .with("bytes", gif.len()),
                binary: gif,
            })
        }

        fn export_sheet(doc: &Document, a: &Json) -> CmdResult<Query> {
            let (order, images, _) = rendered_sequence(doc, a)?;
            let columns = opt_i64(a, "columns")?.unwrap_or(0).clamp(0, 10_000) as usize;
            let padding = opt_i64(a, "padding")?.unwrap_or(0).clamp(0, 64) as u32;
            let frames: Vec<SheetFrame> = order
                .iter()
                .zip(&images)
                .map(|(&i, img)| SheetFrame { index: i, image: img, duration_ms: doc.frames[i].duration_ms })
                .collect();
            let sheet = export::sprite_sheet(&frames, columns, padding).map_err(|e| CmdError::new("limit", e))?;
            let png = export::png::encode(&sheet.image);
            let meta = sheet
                .metadata
                .with("tags", Json::Arr(doc.tags.iter().map(|t| tag_json(doc, t)).collect()))
                .with("generator", "Pixel Studio")
                .with("mimeType", "image/png")
                .with("bytes", png.len());
            Ok(Query { result: meta, binary: png })
        }
    }
    mod structure {
        //! Layers, frames, animation tags, palette and canvas structure.

        use super::args::{frame_index, layer_index, opt_i64, opt_str, req_i64, req_str};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome};
        use crate::animation::{DEFAULT_FRAME_MS, Direction, Tag, validate_duration};
        use crate::layers::{MAX_LAYER_NAME, validate_name};
        use crate::pixel::{Rgba, TRANSPARENT};
        use crate::project::{Document, MAX_FRAMES, MAX_LAYERS, validate_size};

        fn ok(result: Json) -> CmdResult<Outcome> {
            Ok(Outcome::new(result, 0))
        }

        fn layer_json(doc: &Document, i: usize) -> Json {
            let l = &doc.layers[i];
            Json::obj().with("index", i).with("id", l.id).with("name", l.name.as_str())
        }

        pub fn layer_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            if doc.layers.len() >= MAX_LAYERS {
                return Err(CmdError::new("limit", format!("at most {MAX_LAYERS} layers")));
            }
            let name = match opt_str(a, "name")? {
                Some(n) => validate_name(n)?,
                None => format!("Layer {}", doc.next_layer_id),
            };
            let active = doc.layer_index(doc.active_layer).map(|i| i + 1).unwrap_or(doc.layers.len());
            let index = match opt_i64(a, "index")? {
                Some(i) if i < 0 || i as usize > doc.layers.len() => return Err(CmdError::invalid("layer index out of range")),
                Some(i) => i as usize,
                None => active,
            };
            let id = doc.insert_layer(&name, index);
            doc.active_layer = id;
            ok(layer_json(doc, index))
        }

        pub fn layer_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = layer_index(doc, a, "layer")?;
            if doc.layers.len() == 1 {
                return Err(CmdError::invalid("cannot delete the last layer"));
            }
            let removed = doc.remove_layer(i);
            ok(Json::obj().with("deleted", removed.name).with("id", removed.id))
        }

        pub fn layer_move(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = layer_index(doc, a, "layer")?;
            let to = req_i64(a, "index")?;
            if to < 0 || to as usize >= doc.layers.len() {
                return Err(CmdError::invalid("target index out of range"));
            }
            let layer = doc.layers.remove(i);
            doc.layers.insert(to as usize, layer);
            ok(layer_json(doc, to as usize))
        }

        pub fn layer_select(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = layer_index(doc, a, "layer")?;
            doc.active_layer = doc.layers[i].id;
            ok(layer_json(doc, i))
        }

        fn frame_json(doc: &Document, i: usize) -> Json {
            let f = &doc.frames[i];
            Json::obj().with("index", i).with("id", f.id).with("duration", f.duration_ms)
        }

        fn duration_arg(a: &Json, default: u32) -> CmdResult<u32> {
            match opt_i64(a, "duration")? {
                Some(ms) => Ok(validate_duration(ms)?),
                None => Ok(default),
            }
        }

        pub fn frame_add(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let count = opt_i64(a, "count")?.unwrap_or(1);
            if !(1..=10_000).contains(&count) || doc.frames.len() + count as usize > MAX_FRAMES {
                return Err(CmdError::new("limit", "'count' must be 1-10000 and stay within the frame safety limit"));
            }
            let active = doc.frame_index(doc.active_frame).unwrap_or(0);
            let duration = duration_arg(a, doc.frames.get(active).map(|f| f.duration_ms).unwrap_or(DEFAULT_FRAME_MS))?;
            let index = match opt_i64(a, "index")? {
                Some(i) if i < 0 || i as usize > doc.frames.len() => return Err(CmdError::invalid("frame index out of range")),
                Some(i) => i as usize,
                None => doc.frames.len(),
            };
            let mut added = Vec::new();
            for k in 0..count as usize {
                doc.insert_frame(index + k, duration);
                added.push(frame_json(doc, index + k));
            }
            doc.active_frame = doc.frames[index].id;
            ok(Json::obj().with("added", Json::Arr(added)).with("frameCount", doc.frames.len()))
        }

        pub fn frame_duplicate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            if doc.frames.len() >= MAX_FRAMES {
                return Err(CmdError::new("limit", "frame safety limit reached"));
            }
            let i = frame_index(doc, a, "frame")?;
            let to = match opt_i64(a, "index")? {
                Some(t) if t < 0 || t as usize > doc.frames.len() => return Err(CmdError::invalid("target index out of range")),
                Some(t) => t as usize,
                None => i + 1,
            };
            let id = doc.duplicate_frame(i, to);
            doc.active_frame = id;
            ok(frame_json(doc, to).with("source", i).with("frameCount", doc.frames.len()))
        }

        pub fn frame_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            if a.get("frame").is_none_or(Json::is_null) {
                return Err(CmdError::invalid("'frame' is required for frame_delete"));
            }
            let i = frame_index(doc, a, "frame")?;
            if doc.frames.len() == 1 {
                return Err(CmdError::invalid("cannot delete the last frame"));
            }
            let removed = doc.remove_frame(i);
            ok(Json::obj().with("deleted", i).with("id", removed.id).with("frameCount", doc.frames.len()))
        }

        pub fn frame_move(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = frame_index(doc, a, "frame")?;
            let to = req_i64(a, "index")?;
            if to < 0 || to as usize >= doc.frames.len() {
                return Err(CmdError::invalid("target index out of range"));
            }
            let f = doc.frames.remove(i);
            doc.frames.insert(to as usize, f);
            ok(frame_json(doc, to as usize))
        }

        /// `frame` may be an index, omitted (active frame) or the string "all".
        pub fn frame_set_duration(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let ms = validate_duration(req_i64(a, "duration")?)?;
            if a.get("frame").and_then(Json::as_str) == Some("all") {
                doc.frames.iter_mut().for_each(|f| f.duration_ms = ms);
                return ok(Json::obj().with("frames", doc.frames.len()).with("duration", ms));
            }
            let i = frame_index(doc, a, "frame")?;
            doc.frames[i].duration_ms = ms;
            ok(frame_json(doc, i))
        }

        pub fn frame_select(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = frame_index(doc, a, "frame")?;
            doc.active_frame = doc.frames[i].id;
            ok(frame_json(doc, i))
        }

        fn direction_arg(a: &Json) -> CmdResult<Option<Direction>> {
            match opt_str(a, "direction")? {
                None => Ok(None),
                Some(s) => {
                    Direction::parse(s).map(Some).ok_or_else(|| CmdError::invalid("'direction' must be forward, reverse or pingpong"))
                }
            }
        }

        fn repeat_arg(a: &Json) -> CmdResult<Option<u32>> {
            match opt_i64(a, "repeat")? {
                None => Ok(None),
                Some(r) if (0..=65_535).contains(&r) => Ok(Some(r as u32)),
                Some(_) => Err(CmdError::invalid("'repeat' must be 0 (forever) to 65535")),
            }
        }

        pub fn tag_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?.trim().to_string();
            if name.is_empty() || name.chars().count() > MAX_LAYER_NAME {
                return Err(CmdError::invalid("tag name must be 1-128 characters"));
            }
            if doc.tags.iter().any(|t| t.name == name) {
                return Err(CmdError::invalid(format!("tag '{name}' already exists")));
            }
            let from = frame_index(doc, a, "from")?;
            let to = frame_index(doc, a, "to")?;
            if from > to {
                return Err(CmdError::invalid("'from' must not be after 'to'"));
            }
            let tag = Tag {
                name: name.clone(),
                from_id: doc.frames[from].id,
                to_id: doc.frames[to].id,
                direction: direction_arg(a)?.unwrap_or(Direction::Forward),
                repeat: repeat_arg(a)?.unwrap_or(0),
            };
            let dir = tag.direction.as_str();
            doc.tags.push(tag);
            ok(Json::obj().with("name", name).with("from", from).with("to", to).with("direction", dir))
        }

        pub fn tag_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?;
            let before = doc.tags.len();
            doc.tags.retain(|t| t.name != name);
            if doc.tags.len() == before {
                return Err(CmdError::not_found(format!("no tag named '{name}'")));
            }
            ok(Json::obj().with("deleted", name))
        }

        /// Sets direction/repeat of a tag.
        pub fn set_loop(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "tag")?;
            let direction = direction_arg(a)?;
            let repeat = repeat_arg(a)?;
            let tag = doc.tags.iter_mut().find(|t| t.name == name).ok_or_else(|| CmdError::not_found(format!("no tag named '{name}'")))?;
            if let Some(d) = direction {
                tag.direction = d;
            }
            if let Some(r) = repeat {
                tag.repeat = r;
            }
            ok(Json::obj().with("tag", name).with("direction", tag.direction.as_str()).with("repeat", tag.repeat))
        }

        pub fn palette_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let index = req_i64(a, "index")?;
            let hex = req_str(a, "color")?;
            let c = Rgba::parse_hex(hex).ok_or_else(|| CmdError::invalid(format!("'{hex}' is not a color")))?;
            if index < 0 {
                return Err(CmdError::invalid("swatch index out of range"));
            }
            let old = doc.palette.set(index as usize, c).map_err(CmdError::invalid)?;
            // Swatch edits recolor the art that uses the old color (classic palette-swap workflow).
            let (from, to) = (old.to_pixel(), c.to_pixel());
            let mut changed = 0u64;
            let locked: Vec<u32> = doc.layers.iter().filter(|l| l.locked).map(|l| l.id).collect();
            for ((layer, _), cel) in doc.cels.iter_mut() {
                if from == TRANSPARENT || locked.contains(layer) {
                    continue;
                }
                let grid = std::sync::Arc::make_mut(cel);
                let full = crate::raster::Region::full(grid);
                changed += crate::raster::replace(grid, full, from, to) as u64;
            }
            Ok(Outcome::new(
                Json::obj().with("index", index).with("color", c.to_hex()).with("previous", old.to_hex()).with("recolored", changed),
                changed,
            ))
        }

        pub fn canvas_resize(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let (w, h) = validate_size(req_i64(a, "width")?, req_i64(a, "height")?)?;
            let ox = opt_i64(a, "offsetX")?.unwrap_or(0);
            let oy = opt_i64(a, "offsetY")?.unwrap_or(0);
            if ox.abs() > 8192 || oy.abs() > 8192 {
                return Err(CmdError::invalid("offset out of range"));
            }
            doc.resize(w, h, ox, oy);
            ok(Json::obj().with("width", w).with("height", h))
        }

        pub fn project_rename(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = req_str(a, "name")?.trim();
            if name.is_empty() || name.chars().count() > 256 || name.chars().any(char::is_control) {
                return Err(CmdError::invalid("project name must be 1-256 printable characters"));
            }
            doc.name = name.to_string();
            ok(Json::obj().with("name", name))
        }
    }
    pub mod voxel {
        //! 3D pixel (voxel) commands: modeling, PBR materials and textures, rigging, animation and
        //! rendering (including baking 8-direction sprite animations into the 2D project).

        use std::collections::HashMap;
        use std::sync::Arc;

        use super::args::{color_value, opt_bool, opt_f64, opt_i64, opt_str, opt_vec3, req_i64, req_str, req_vec3};
        use super::json::Json;
        use super::{CmdError, CmdResult, Outcome, Query};
        use crate::animation::validate_duration;
        use crate::export;
        use crate::pixel::{PixelGrid, Rgba};
        use crate::project::{Document, MAX_FRAMES, validate_size};
        use crate::voxel::material::{MAP_SLOTS, MapSlot, TEXTURE_KINDS, generate_set, slot_by_name};
        use crate::voxel::model::{MAX_BONES, MAX_MODEL_SIZE, hash01};
        use crate::voxel::render::{Camera, Outline, Pass, PointLight, Prepared, RenderSettings, pick, prepare, render, view_extent};
        use crate::voxel::rig::{Animation, Bone, CycleParams, Key, auto_bind, idle_cycle, locomotion_cycle};
        use crate::voxel::{Material, Shape, Texture, Vec3, Voxel, VoxelModel};

        pub const MAX_BATCH_VOXELS: usize = 1_000_000;

        /// Prepared geometry keyed by model version (shared by queries and bakes).
        #[derive(Default)]
        pub struct RenderCache {
            entries: Vec<Arc<Prepared>>,
        }

        impl RenderCache {
            pub fn get(&mut self, model: &VoxelModel) -> Arc<Prepared> {
                if let Some(p) = self.entries.iter().find(|p| p.version == model.version) {
                    return Arc::clone(p);
                }
                let p = Arc::new(prepare(model));
                self.entries.push(Arc::clone(&p));
                if self.entries.len() > 6 {
                    self.entries.remove(0);
                }
                p
            }
        }

        // ---- argument helpers -------------------------------------------------------------------

        fn model_index(doc: &Document, a: &Json) -> CmdResult<usize> {
            if doc.models.is_empty() {
                return Err(CmdError::not_found("the project has no 3D models; create one with voxel_model_create"));
            }
            match a.get("model") {
                None | Some(Json::Null) => Ok(0),
                Some(Json::Str(name)) => doc
                    .models
                    .iter()
                    .position(|m| m.name == *name)
                    .or_else(|| name.parse::<usize>().ok().filter(|i| *i < doc.models.len()))
                    .ok_or_else(|| CmdError::not_found(format!("no model named '{name}'"))),
                Some(v) => v
                    .as_i64()
                    .filter(|i| *i >= 0 && (*i as usize) < doc.models.len())
                    .map(|i| i as usize)
                    .ok_or_else(|| CmdError::not_found("model index out of range")),
            }
        }

        fn material_arg(doc: &Document, a: &Json) -> CmdResult<Option<u8>> {
            match a.get("material") {
                None | Some(Json::Null) => Ok(None),
                Some(Json::Str(name)) => doc
                    .materials
                    .iter()
                    .position(|m| m.name == *name)
                    .map(|i| Some(i as u8))
                    .ok_or_else(|| CmdError::not_found(format!("no material named '{name}'"))),
                Some(v) => v
                    .as_i64()
                    .filter(|i| *i >= 0 && (*i as usize) < doc.materials.len())
                    .map(|i| Some(i as u8))
                    .ok_or_else(|| CmdError::not_found("material index out of range")),
            }
        }

        fn bone_arg(model: &VoxelModel, a: &Json, key: &str) -> CmdResult<Option<u8>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(Json::Str(name)) => {
                    model.bone_index(name).map(|i| Some(i as u8)).ok_or_else(|| CmdError::not_found(format!("no bone named '{name}'")))
                }
                Some(v) => v
                    .as_i64()
                    .filter(|i| *i >= 0 && (*i as usize) < model.bones.len().max(1))
                    .map(|i| Some(i as u8))
                    .ok_or_else(|| CmdError::not_found("bone index out of range")),
            }
        }

        fn anim_index(model: &VoxelModel, a: &Json, key: &str) -> CmdResult<Option<usize>> {
            match a.get(key) {
                None | Some(Json::Null) => Ok(None),
                Some(Json::Str(name)) => model
                    .animations
                    .iter()
                    .position(|x| x.name == *name)
                    .map(Some)
                    .ok_or_else(|| CmdError::not_found(format!("no animation named '{name}'"))),
                Some(v) => v
                    .as_i64()
                    .filter(|i| *i >= 0 && (*i as usize) < model.animations.len())
                    .map(|i| Some(i as usize))
                    .ok_or_else(|| CmdError::not_found("animation index out of range")),
            }
        }

        fn name_arg(a: &Json, key: &str, max: usize) -> CmdResult<String> {
            let n = req_str(a, key)?.trim();
            if n.is_empty() || n.chars().count() > max || n.chars().any(char::is_control) {
                return Err(CmdError::invalid(format!("'{key}' must be 1-{max} printable characters")));
            }
            Ok(n.to_string())
        }

        fn v3(a: &Json, key: &str) -> CmdResult<Vec3> {
            req_vec3(a, key).map(Vec3::from_f64)
        }

        pub(crate) fn base64_decode(s: &str) -> CmdResult<Vec<u8>> {
            let s = s.split_once("base64,").map(|(_, b)| b).unwrap_or(s);
            let mut out = Vec::with_capacity(s.len() * 3 / 4);
            let (mut acc, mut bits) = (0u32, 0u32);
            for c in s.bytes() {
                let v = match c {
                    b'A'..=b'Z' => c - b'A',
                    b'a'..=b'z' => c - b'a' + 26,
                    b'0'..=b'9' => c - b'0' + 52,
                    b'+' | b'-' => 62,
                    b'/' | b'_' => 63,
                    b'=' | b'\n' | b'\r' | b' ' => continue,
                    _ => return Err(CmdError::invalid("invalid base64 data")),
                } as u32;
                acc = (acc << 6) | v;
                bits += 6;
                if bits >= 8 {
                    bits -= 8;
                    out.push((acc >> bits) as u8);
                }
            }
            Ok(out)
        }

        /// Binary input: the call payload or a `data_base64` argument.
        fn binary_input(a: &Json, payload: &[u8]) -> CmdResult<Vec<u8>> {
            if !payload.is_empty() {
                return Ok(payload.to_vec());
            }
            match opt_str(a, "data_base64")? {
                Some(b) => base64_decode(b),
                None => Err(CmdError::invalid("provide the PNG as 'data_base64' (or as the binary payload)")),
            }
        }

        // ---- modeling -----------------------------------------------------------------------------

        pub fn model_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = name_arg(a, "name", 128)?;
            if doc.models.iter().any(|m| m.name == name) {
                return Err(CmdError::invalid(format!("model '{name}' already exists")));
            }
            let dim = |k: &str| -> CmdResult<u32> {
                let v = opt_i64(a, k)?.unwrap_or(64);
                if !(1..=MAX_MODEL_SIZE as i64).contains(&v) {
                    return Err(CmdError::invalid(format!("'{k}' must be 1-{MAX_MODEL_SIZE}")));
                }
                Ok(v as u32)
            };
            let size = [dim("width")?, dim("height")?, dim("depth")?];
            let mut m = VoxelModel::new(&name, size);
            if let Some(o) = opt_vec3(a, "origin")? {
                m.origin = Vec3::from_f64(o);
            }
            doc.models.push(m);
            Ok(Outcome::new(
                Json::obj()
                    .with("model", doc.models.len() - 1)
                    .with("name", name)
                    .with("size", Json::Arr(size.iter().map(|&d| Json::from(d)).collect())),
                0,
            ))
        }

        /// Duplicates a model (voxels, rig, animations) — the basis of frame-by-frame 3D animation and VFX.
        pub fn model_clone(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let name = name_arg(a, "name", 128)?;
            if doc.models.iter().any(|m| m.name == name) {
                return Err(CmdError::invalid(format!("model '{name}' already exists")));
            }
            if doc.models.len() >= 4096 {
                return Err(CmdError::new("limit", "at most 4096 models"));
            }
            let mut copy = doc.models[i].clone();
            copy.name = name.clone();
            if opt_bool(a, "empty")?.unwrap_or(false) {
                copy.chunks.clear();
            }
            copy.touch();
            doc.models.push(copy);
            Ok(Outcome::new(Json::obj().with("model", doc.models.len() - 1).with("name", name), 0))
        }

        pub fn model_delete(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let m = doc.models.remove(i);
            Ok(Outcome::new(Json::obj().with("deleted", m.name), 0))
        }

        pub fn model_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            if a.get("name").is_some() {
                let name = name_arg(a, "name", 128)?;
                doc.models[i].name = name;
            }
            if let Some(o) = opt_vec3(a, "origin")? {
                doc.models[i].origin = Vec3::from_f64(o);
            }
            doc.models[i].touch();
            let m = &doc.models[i];
            Ok(Outcome::new(Json::obj().with("model", i).with("name", m.name.as_str()).with("origin", vec_json(m.origin)), 0))
        }

        fn vec_json(v: Vec3) -> Json {
            Json::Arr(vec![Json::from(v.x as f64), Json::from(v.y as f64), Json::from(v.z as f64)])
        }

        pub fn voxel_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let rgba = super::args::color(doc, a, "color")?;
            let material = material_arg(doc, a)?.unwrap_or(0);
            let bone = bone_arg(&doc.models[i], a, "bone")?.unwrap_or(0);
            let (x, y, z) = (req_i64(a, "x")?, req_i64(a, "y")?, req_i64(a, "z")?);
            let m = &mut doc.models[i];
            if !m.in_bounds(x as i32, y as i32, z as i32) {
                return Err(CmdError::invalid(format!("voxel ({x},{y},{z}) is outside the {:?} model", m.size)));
            }
            let changed = m.set(x as i32, y as i32, z as i32, Voxel { rgba, material, bone }) as u64;
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        /// Explicit voxel list: `[[x, y, z, color], {x, y, z, color, material, bone}, ...]`.
        pub fn voxel_set_many(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let items = a.get("voxels").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'voxels' must be an array"))?;
            if items.len() > MAX_BATCH_VOXELS {
                return Err(CmdError::new("limit", format!("at most {MAX_BATCH_VOXELS} voxels per call")));
            }
            let default_color = a.get("color").map(|c| color_value(doc, c, "color")).transpose()?;
            let material = material_arg(doc, a)?.unwrap_or(0);
            let bone = bone_arg(&doc.models[i], a, "bone")?.unwrap_or(0);
            let mut resolved = Vec::with_capacity(items.len());
            for (k, item) in items.iter().enumerate() {
                let (x, y, z, c) = match item {
                    Json::Arr(p) if p.len() == 3 || p.len() == 4 => (p[0].as_i64(), p[1].as_i64(), p[2].as_i64(), p.get(3)),
                    Json::Obj(_) => (
                        item.get("x").and_then(Json::as_i64),
                        item.get("y").and_then(Json::as_i64),
                        item.get("z").and_then(Json::as_i64),
                        item.get("color"),
                    ),
                    _ => (None, None, None, None),
                };
                let (Some(x), Some(y), Some(z)) = (x, y, z) else {
                    return Err(CmdError::invalid(format!("voxels[{k}] needs integer x, y, z")));
                };
                let rgba = match c {
                    Some(c) => color_value(doc, c, &format!("voxels[{k}].color"))?,
                    None => default_color.ok_or_else(|| CmdError::invalid(format!("voxels[{k}] has no color and no default 'color'")))?,
                };
                let mat = match item.get("material") {
                    Some(_) => material_arg(doc, item)?.unwrap_or(material),
                    None => material,
                };
                let b = match item.get("bone") {
                    Some(_) => bone_arg(&doc.models[i], item, "bone")?.unwrap_or(bone),
                    None => bone,
                };
                resolved.push((x, y, z, Voxel { rgba, material: mat, bone: b }));
            }
            let m = &mut doc.models[i];
            let mut changed = 0u64;
            for (x, y, z, v) in resolved {
                changed += m.set(x as i32, y as i32, z as i32, v) as u64;
            }
            Ok(Outcome::new(Json::obj().with("requested", items.len()).with("changed", changed), changed))
        }

        fn parse_shape(a: &Json) -> CmdResult<Shape> {
            let kind = req_str(a, "shape")?;
            let shape = match kind {
                "box" => {
                    let (p, q) = (v3(a, "min")?, v3(a, "max")?);
                    Shape::Box {
                        min: Vec3::new(p.x.min(q.x), p.y.min(q.y), p.z.min(q.z)),
                        max: Vec3::new(p.x.max(q.x) + 1.0, p.y.max(q.y) + 1.0, p.z.max(q.z) + 1.0),
                    }
                }
                "ellipsoid" | "sphere" => {
                    let center = v3(a, "center")?;
                    let radii = match opt_vec3(a, "radii")? {
                        Some(r) => Vec3::from_f64(r),
                        None => {
                            let r = opt_f64(a, "radius")?
                                .ok_or_else(|| CmdError::invalid("ellipsoid needs 'radii' [rx, ry, rz] or 'radius'"))?
                                as f32;
                            Vec3::new(r, r, r)
                        }
                    };
                    Shape::Ellipsoid { center, radii }
                }
                "capsule" | "cone" => {
                    let ra = opt_f64(a, "radius")?.ok_or_else(|| CmdError::invalid("capsule needs 'radius'"))? as f32;
                    let rb = opt_f64(a, "radius_b")?.map(|r| r as f32).unwrap_or(if kind == "cone" { 0.5 } else { ra });
                    Shape::Capsule { a: v3(a, "a")?, b: v3(a, "b")?, ra, rb }
                }
                "cylinder" => Shape::Cylinder {
                    a: v3(a, "a")?,
                    b: v3(a, "b")?,
                    r: opt_f64(a, "radius")?.ok_or_else(|| CmdError::invalid("cylinder needs 'radius'"))? as f32,
                },
                other => return Err(CmdError::invalid(format!("unknown shape '{other}' (box, ellipsoid, capsule, cone, cylinder)"))),
            };
            let (lo, hi) = shape.aabb();
            if (hi - lo).x * (hi - lo).y * (hi - lo).z > 512.0 * 512.0 * 512.0 {
                return Err(CmdError::new("limit", "shape is too large"));
            }
            Ok(shape)
        }

        fn mirrored_bone(model: &VoxelModel, bone: u8) -> u8 {
            let Some(b) = model.bones.get(bone as usize) else { return bone };
            let swapped = if let Some(s) = b.name.strip_suffix("_l") {
                format!("{s}_r")
            } else if let Some(s) = b.name.strip_suffix("_r") {
                format!("{s}_l")
            } else {
                return bone;
            };
            model.bone_index(&swapped).map(|i| i as u8).unwrap_or(bone)
        }

        /// Sculpt or paint with a primitive. Modes: add (fill), paint (recolor existing voxels only),
        /// erase. Optional gradient (`color_to` along `gradient_axis`), `jitter` (0..1 tonal noise),
        /// `mirror_x` (also apply mirrored across the model origin's X, swapping _l/_r bones).
        pub fn voxel_shape(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let shape = parse_shape(a)?;
            let mode = opt_str(a, "mode")?.unwrap_or("add");
            if !matches!(mode, "add" | "paint" | "erase") {
                return Err(CmdError::invalid("'mode' must be add, paint or erase"));
            }
            let color = match a.get("color") {
                Some(c) => Some(color_value(doc, c, "color")?),
                None if mode == "erase" => None,
                None => return Err(CmdError::invalid("missing required color 'color'")),
            };
            let color_to = a.get("color_to").map(|c| color_value(doc, c, "color_to")).transpose()?;
            let axis = match opt_str(a, "gradient_axis")?.unwrap_or("y") {
                "x" => 0,
                "y" => 1,
                "z" => 2,
                _ => return Err(CmdError::invalid("'gradient_axis' must be x, y or z")),
            };
            let jitter = opt_f64(a, "jitter")?.unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            let seed = opt_i64(a, "seed")?.unwrap_or(1) as u32;
            let material = material_arg(doc, a)?;
            let bone = bone_arg(&doc.models[i], a, "bone")?;
            let mirror = opt_bool(a, "mirror_x")?.unwrap_or(false);
            let model = &mut doc.models[i];
            let (lo, hi) = shape.aabb();
            let mut changed = 0u64;
            let mut writes: Vec<(i32, i32, i32, Voxel)> = Vec::new();
            for z in lo.z.floor() as i32..=hi.z.ceil() as i32 {
                for y in lo.y.floor() as i32..=hi.y.ceil() as i32 {
                    for x in lo.x.floor() as i32..=hi.x.ceil() as i32 {
                        let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                        if !shape.contains(p) || !model.in_bounds(x, y, z) {
                            continue;
                        }
                        let existing = model.get(x, y, z);
                        let out = match mode {
                            "erase" => Voxel::EMPTY,
                            "paint" if existing.is_empty() => continue,
                            _ => {
                                let mut c = Rgba::from_u32(color.unwrap_or(0));
                                if let Some(to) = color_to {
                                    let t = shape.axis_t(p, axis);
                                    let to = Rgba::from_u32(to);
                                    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
                                    c = Rgba::new(mix(c.r, to.r), mix(c.g, to.g), mix(c.b, to.b), mix(c.a, to.a));
                                }
                                if jitter > 0.0 {
                                    let n = 1.0 + (hash01(x, y, z, seed) - 0.5) * 2.0 * jitter * 0.35;
                                    let j = |v: u8| (v as f32 * n).clamp(0.0, 255.0) as u8;
                                    c = Rgba::new(j(c.r), j(c.g), j(c.b), c.a);
                                }
                                let base = if existing.is_empty() { Voxel::EMPTY } else { existing };
                                Voxel { rgba: c.to_pixel(), material: material.unwrap_or(base.material), bone: bone.unwrap_or(base.bone) }
                            }
                        };
                        writes.push((x, y, z, out));
                    }
                }
            }
            let ox = model.origin.x;
            for (x, y, z, v) in writes {
                changed += model.set(x, y, z, v) as u64;
                if mirror {
                    let mx = (2.0 * ox - (x as f32 + 0.5) - 0.5).round() as i32;
                    if mx != x {
                        let mv = if v.is_empty() { v } else { Voxel { bone: mirrored_bone(model, v.bone), ..v } };
                        if mode != "paint" || !model.get(mx, y, z).is_empty() {
                            changed += model.set(mx, y, z, mv) as u64;
                        }
                    }
                }
            }
            Ok(Outcome::new(Json::obj().with("changed", changed).with("voxels", model.count()), changed))
        }

        /// Copies one half of the model onto the other across the origin's X plane.
        pub fn voxel_mirror(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let from_positive = match opt_str(a, "from")?.unwrap_or("+") {
                "+" | "right" | "positive" => true,
                "-" | "left" | "negative" => false,
                _ => return Err(CmdError::invalid("'from' must be '+' or '-'")),
            };
            let model = &mut doc.models[i];
            let ox = model.origin.x;
            let mut writes = Vec::new();
            model.for_each(|x, y, z, v| {
                let cx = x as f32 + 0.5;
                if (from_positive && cx > ox) || (!from_positive && cx < ox) {
                    writes.push(((2.0 * ox - cx - 0.5).round() as i32, y, z, v));
                }
            });
            let mut changed = 0;
            for (x, y, z, v) in writes {
                let v = Voxel { bone: mirrored_bone(model, v.bone), ..v };
                changed += model.set(x, y, z, v) as u64;
            }
            Ok(Outcome::new(Json::obj().with("changed", changed), changed))
        }

        // ---- materials & textures -----------------------------------------------------------------

        fn texture_index(doc: &Document, v: &Json) -> CmdResult<Option<usize>> {
            match v {
                Json::Null => Ok(None),
                Json::Str(name) => doc
                    .textures
                    .iter()
                    .position(|t| t.name == *name)
                    .map(Some)
                    .ok_or_else(|| CmdError::not_found(format!("no texture named '{name}'"))),
                v => v
                    .as_i64()
                    .filter(|i| *i >= 0 && (*i as usize) < doc.textures.len())
                    .map(|i| Some(i as usize))
                    .ok_or_else(|| CmdError::not_found("texture index out of range")),
            }
        }

        /// Creates or updates a PBR material. `maps` = {albedo|normal|roughness|metallic|occlusion|height|emission: texture name | null}.
        pub fn material_set(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = name_arg(a, "name", 64)?;
            let idx = match doc.materials.iter().position(|m| m.name == name) {
                Some(i) => i,
                None => {
                    if doc.materials.len() >= 256 {
                        return Err(CmdError::new("limit", "at most 256 materials"));
                    }
                    doc.materials.push(Material::new(&name));
                    doc.materials.len() - 1
                }
            };
            let mut m = doc.materials[idx].clone();
            let unit = |k: &str| -> CmdResult<Option<f32>> { Ok(opt_f64(a, k)?.map(|v| v.clamp(0.0, 1.0) as f32)) };
            if let Some(v) = unit("roughness")? {
                m.roughness = v;
            }
            if let Some(v) = unit("metallic")? {
                m.metallic = v;
            }
            if let Some(v) = opt_f64(a, "emission")? {
                m.emission = v.clamp(0.0, 100.0) as f32;
            }
            if let Some(v) = opt_f64(a, "uv_scale")? {
                m.uv_scale = v.clamp(0.01, 64.0) as f32;
            }
            if let Some(v) = opt_f64(a, "normal_strength")? {
                m.normal_strength = v.clamp(0.0, 4.0) as f32;
            }
            if let Some(Json::Obj(entries)) = a.get("maps") {
                for (k, v) in entries {
                    let slot = slot_by_name(k).ok_or_else(|| CmdError::invalid(format!("unknown map slot '{k}'")))?;
                    m.maps[slot as usize] = texture_index(doc, v)?;
                }
            }
            doc.materials[idx] = m;
            for model in doc.models.iter_mut() {
                model.touch();
            }
            Ok(Outcome::new(Json::obj().with("material", idx).with("name", name), 0))
        }

        fn add_texture(doc: &mut Document, tex: Texture) -> CmdResult<usize> {
            if let Some(i) = doc.textures.iter().position(|t| t.name == tex.name) {
                doc.textures[i] = Arc::new(tex);
                return Ok(i);
            }
            if doc.textures.len() >= 1024 {
                return Err(CmdError::new("limit", "at most 1024 textures"));
            }
            doc.textures.push(Arc::new(tex));
            Ok(doc.textures.len() - 1)
        }

        /// Imports a PNG as a texture map (any PBR slot can reference it).
        pub fn texture_import(doc: &mut Document, a: &Json, payload: &[u8]) -> CmdResult<Outcome> {
            let name = name_arg(a, "name", 64)?;
            let png = binary_input(a, payload)?;
            let img = export::png::decode(&png).map_err(|e| CmdError::new("invalid_file", e))?;
            if img.width > 4096 || img.height > 4096 {
                return Err(CmdError::new("limit", "textures are limited to 4096x4096"));
            }
            let (w, h) = (img.width, img.height);
            let idx = add_texture(doc, Texture { name: name.clone(), width: w, height: h, data: img.pixels })?;
            Ok(Outcome::new(Json::obj().with("texture", idx).with("name", name).with("width", w).with("height", h), 0))
        }

        /// Generates a tileable PBR texture set (albedo, normal, roughness, occlusion, height) and,
        /// by default, a material using it.
        pub fn texture_generate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let name = name_arg(a, "name", 48)?;
            let kind = opt_str(a, "kind")?.unwrap_or("noise");
            if !TEXTURE_KINDS.contains(&kind) {
                return Err(CmdError::invalid(format!("unknown kind '{kind}' ({})", TEXTURE_KINDS.join(", "))));
            }
            let size = opt_i64(a, "size")?.unwrap_or(64).clamp(8, 1024) as u32;
            let cells = opt_i64(a, "cells")?.unwrap_or(4).clamp(1, 64) as i32;
            let seed = opt_i64(a, "seed")?.unwrap_or(1) as u32;
            let roughness = opt_f64(a, "roughness")?.unwrap_or(0.7).clamp(0.02, 1.0) as f32;
            let contrast = opt_f64(a, "contrast")?.unwrap_or(0.35).clamp(0.0, 1.0) as f32;
            let set = generate_set(&name, kind, size, cells, seed, roughness, contrast);
            let mut ids = Json::obj();
            let mut maps = [None; 7];
            for (tex, slot, label) in [
                (set.albedo, MapSlot::Albedo, "albedo"),
                (set.normal, MapSlot::Normal, "normal"),
                (set.roughness, MapSlot::Roughness, "roughness"),
                (set.occlusion, MapSlot::Occlusion, "occlusion"),
                (set.height, MapSlot::Height, "height"),
            ] {
                let tname = tex.name.clone();
                let idx = add_texture(doc, tex)?;
                maps[slot as usize] = Some(idx);
                ids.set(label, tname);
            }
            let mut result = Json::obj().with("textures", ids).with("kind", kind).with("size", size);
            if opt_bool(a, "material")?.unwrap_or(true) {
                let idx = match doc.materials.iter().position(|m| m.name == name) {
                    Some(i) => i,
                    None => {
                        doc.materials.push(Material::new(&name));
                        doc.materials.len() - 1
                    }
                };
                let m = &mut doc.materials[idx];
                m.maps = maps;
                m.roughness = roughness;
                if let Some(v) = opt_f64(a, "metallic")? {
                    m.metallic = v.clamp(0.0, 1.0) as f32;
                }
                if let Some(v) = opt_f64(a, "uv_scale")? {
                    m.uv_scale = v.clamp(0.01, 64.0) as f32;
                }
                result.set("material", idx);
                for model in doc.models.iter_mut() {
                    model.touch();
                }
            }
            Ok(Outcome::new(result, 0))
        }

        // ---- rigging & animation --------------------------------------------------------------------

        pub fn bone_add(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let name = name_arg(a, "name", 64)?;
            let model = &mut doc.models[i];
            if model.bones.len() >= MAX_BONES {
                return Err(CmdError::new("limit", format!("at most {MAX_BONES} bones")));
            }
            if model.bone_index(&name).is_some() {
                return Err(CmdError::invalid(format!("bone '{name}' already exists")));
            }
            let parent = match a.get("parent") {
                None | Some(Json::Null) => None,
                Some(Json::Str(p)) => Some(model.bone_index(p).ok_or_else(|| CmdError::not_found(format!("no bone named '{p}'")))?),
                Some(v) => Some(
                    v.as_i64()
                        .filter(|p| *p >= 0 && (*p as usize) < model.bones.len())
                        .ok_or_else(|| CmdError::not_found("parent bone out of range"))? as usize,
                ),
            };
            let head = v3(a, "head")?;
            let tail = opt_vec3(a, "tail")?.map(Vec3::from_f64).unwrap_or(head);
            model.bones.push(Bone { name: name.clone(), parent, head, tail });
            model.touch();
            Ok(Outcome::new(Json::obj().with("bone", model.bones.len() - 1).with("name", name), 0))
        }

        pub fn rig_clear(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let model = &mut doc.models[i];
            model.bones.clear();
            model.animations.clear();
            let mut writes = Vec::new();
            model.for_each(|x, y, z, v| {
                if v.bone != 0 {
                    writes.push((x, y, z, Voxel { bone: 0, ..v }));
                }
            });
            for (x, y, z, v) in writes {
                model.set(x, y, z, v);
            }
            model.touch();
            Ok(Outcome::new(Json::obj().with("cleared", true), 0))
        }

        pub fn rig_bind(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let only_unbound = opt_bool(a, "only_unbound")?.unwrap_or(false);
            let n = auto_bind(&mut doc.models[i], only_unbound);
            Ok(Outcome::new(Json::obj().with("rebound", n), n as u64))
        }

        pub fn anim_create(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let name = name_arg(a, "name", 64)?;
            let frames = opt_i64(a, "frames")?.unwrap_or(8).clamp(1, 10_000) as u32;
            let frame_ms = validate_duration(opt_i64(a, "frame_ms")?.unwrap_or(100))?;
            let model = &mut doc.models[i];
            let mut anim = Animation::new(&name, frames, frame_ms);
            anim.looped = opt_bool(a, "loop")?.unwrap_or(true);
            model.animations.retain(|x| x.name != name);
            model.animations.push(anim);
            Ok(Outcome::new(Json::obj().with("animation", model.animations.len() - 1).with("name", name), 0))
        }

        pub fn anim_key(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let model = &doc.models[i];
            let ai = anim_index(model, a, "animation")?.ok_or_else(|| CmdError::invalid("missing 'animation'"))?;
            let bone = bone_arg(model, a, "bone")?.ok_or_else(|| CmdError::invalid("missing 'bone'"))? as usize;
            let frame = opt_f64(a, "frame")?.ok_or_else(|| CmdError::invalid("missing 'frame'"))? as f32;
            let rot = opt_vec3(a, "rotation")?.map(Vec3::from_f64).unwrap_or(Vec3::ZERO);
            let pos = opt_vec3(a, "position")?.map(Vec3::from_f64).unwrap_or(Vec3::ZERO);
            let anim = &mut doc.models[i].animations[ai];
            if frame < 0.0 || frame > anim.frames as f32 {
                return Err(CmdError::invalid("'frame' is outside the animation"));
            }
            anim.set_key(Key { bone, frame, rot, pos });
            Ok(Outcome::new(Json::obj().with("keys", anim.keys.len()), 0))
        }

        /// Procedural humanoid cycles from standard bone names (walk, run, idle).
        pub fn anim_generate(doc: &mut Document, a: &Json) -> CmdResult<Outcome> {
            let i = model_index(doc, a)?;
            let kind = opt_str(a, "kind")?.unwrap_or("walk");
            let frames = opt_i64(a, "frames")?.unwrap_or(8).clamp(2, 240) as u32;
            let name = opt_str(a, "name")?.unwrap_or(kind).to_string();
            let model = &doc.models[i];
            if model.bones.is_empty() {
                return Err(CmdError::invalid("the model has no skeleton; add bones (hips, upper_leg_l, ...) first"));
            }
            let mut anim = match kind {
                "walk" | "run" => {
                    let mut p = if kind == "walk" { CycleParams::walk(frames) } else { CycleParams::run(frames) };
                    if let Some(v) = opt_f64(a, "stride")? {
                        p.stride_deg = v.clamp(0.0, 80.0) as f32;
                    }
                    if let Some(v) = opt_f64(a, "arm_swing")? {
                        p.arm_swing_deg = v.clamp(0.0, 80.0) as f32;
                    }
                    if let Some(v) = opt_f64(a, "bob")? {
                        p.bob = v.clamp(0.0, 40.0) as f32;
                    }
                    if let Some(v) = opt_f64(a, "knee")? {
                        p.knee_deg = v.clamp(0.0, 120.0) as f32;
                    }
                    if let Some(v) = opt_f64(a, "lean")? {
                        p.lean_deg = v.clamp(-30.0, 30.0) as f32;
                    }
                    locomotion_cycle(model, &name, &p)
                }
                "idle" => idle_cycle(model, &name, frames),
                other => return Err(CmdError::invalid(format!("unknown kind '{other}' (walk, run, idle)"))),
            };
            if let Some(ms) = opt_i64(a, "frame_ms")? {
                anim.frame_ms = validate_duration(ms)?;
            }
            let keys = anim.keys.len();
            let model = &mut doc.models[i];
            model.animations.retain(|x| x.name != name);
            model.animations.push(anim);
            Ok(Outcome::new(
                Json::obj().with("animation", model.animations.len() - 1).with("name", name).with("frames", frames).with("keys", keys),
                0,
            ))
        }

        // ---- rendering --------------------------------------------------------------------------------

        pub const DIRECTIONS_8: [(&str, f32); 8] =
            [("S", 0.0), ("SE", 45.0), ("E", 90.0), ("NE", 135.0), ("N", 180.0), ("NW", 225.0), ("W", 270.0), ("SW", 315.0)];

        fn directions(n: i64) -> CmdResult<Vec<(&'static str, f32)>> {
            Ok(match n {
                1 => vec![DIRECTIONS_8[0]],
                2 => vec![DIRECTIONS_8[0], DIRECTIONS_8[4]],
                4 => vec![DIRECTIONS_8[0], DIRECTIONS_8[2], DIRECTIONS_8[4], DIRECTIONS_8[6]],
                8 => DIRECTIONS_8.to_vec(),
                16 => (0..16)
                    .map(|i| {
                        (
                            ["S", "SSE", "SE", "ESE", "E", "ENE", "NE", "NNE", "N", "NNW", "NW", "WNW", "W", "WSW", "SW", "SSW"][i],
                            i as f32 * 22.5,
                        )
                    })
                    .collect(),
                _ => return Err(CmdError::invalid("'directions' must be 1, 2, 4, 8 or 16")),
            })
        }

        /// Common render options from arguments (camera size/angles are filled in by the caller).
        fn settings_from(doc: &Document, model: &VoxelModel, a: &Json, width: u32, height: u32) -> CmdResult<RenderSettings> {
            let mut s = RenderSettings::new(Camera {
                yaw: 0.0,
                pitch: 30.0,
                scale: 1.0,
                width,
                height,
                anchor_x: width as f32 / 2.0,
                anchor_y: height as f32,
            });
            s.camera.yaw = opt_f64(a, "yaw")?.unwrap_or(0.0) as f32;
            s.camera.pitch = opt_f64(a, "pitch")?.unwrap_or(30.0).clamp(-89.0, 89.0) as f32;
            if let Some(p) = opt_str(a, "pass")? {
                s.pass = Pass::parse(p).ok_or_else(|| {
                    CmdError::invalid("'pass' must be lit, albedo, normal, depth, occlusion, roughness, metallic or emission")
                })?;
            }
            s.smoothness = opt_f64(a, "smoothness")?.unwrap_or(0.55).clamp(0.0, 1.0) as f32;
            s.toon_steps = opt_i64(a, "toon_steps")?.unwrap_or(0).clamp(0, 16) as u32;
            s.inner_lines = opt_bool(a, "inner_lines")?.unwrap_or(false);
            s.supersample = opt_i64(a, "supersample")?.unwrap_or(1).clamp(1, 4) as u32;
            s.outline = match a.get("outline") {
                None | Some(Json::Null) | Some(Json::Bool(false)) => Outline::None,
                Some(Json::Bool(true)) => Outline::Selective,
                Some(Json::Str(v)) if v == "selective" || v == "selout" => Outline::Selective,
                Some(Json::Str(v)) if v == "none" => Outline::None,
                Some(v) => Outline::Color(Rgba::from_u32(color_value(doc, v, "outline")?)),
            };
            if let Some(bg) = opt_str(a, "background")? {
                s.background = Some(Rgba::parse_hex(bg).ok_or_else(|| CmdError::invalid("'background' must be a hex color"))?);
            }
            if let Some(e) = opt_f64(a, "exposure")? {
                s.lighting.exposure = e.clamp(0.05, 8.0) as f32;
            }
            s.shadows = match a.get("shadows") {
                Some(Json::Bool(b)) => *b as u8 as f32,
                _ => opt_f64(a, "shadows")?.unwrap_or(0.0).clamp(0.0, 1.0) as f32,
            };
            s.ground_shadow = opt_f64(a, "ground_shadow")?.unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            s.bloom = opt_f64(a, "bloom")?.unwrap_or(0.0).clamp(0.0, 4.0) as f32;
            s.bloom_radius = opt_f64(a, "bloom_radius")?.unwrap_or(0.0).clamp(0.0, 256.0) as f32;
            if let Some(dir) = opt_vec3(a, "key_light")? {
                s.lighting.lights[0].dir = Vec3::from_f64(dir).normalized();
            }
            if let Some(v) = opt_f64(a, "key_intensity")? {
                s.lighting.lights[0].intensity = v.clamp(0.0, 20.0) as f32;
            }
            if let Some(v) = opt_f64(a, "ambient")? {
                let k = v.clamp(0.0, 4.0) as f32;
                s.lighting.sky = s.lighting.sky * k;
                s.lighting.ground = s.lighting.ground * k;
            }
            if let Some(Json::Arr(g)) = a.get("background_gradient") {
                if g.len() != 2 {
                    return Err(CmdError::invalid("'background_gradient' must be [top, bottom] colors"));
                }
                let top = Rgba::from_u32(color_value(doc, &g[0], "background_gradient")?);
                let bottom = Rgba::from_u32(color_value(doc, &g[1], "background_gradient")?);
                s.background_gradient = Some((top, bottom));
            }
            if let Some(Json::Arr(lights)) = a.get("lights") {
                if lights.len() > 32 {
                    return Err(CmdError::new("limit", "at most 32 point lights"));
                }
                for (i, l) in lights.iter().enumerate() {
                    let pos =
                        Vec3::from_f64(req_vec3(l, "position").map_err(|e| CmdError::invalid(format!("lights[{i}]: {}", e.message)))?);
                    let c = Rgba::from_u32(match l.get("color") {
                        Some(c) => color_value(doc, c, "lights.color")?,
                        None => Rgba::new(255, 170, 90, 255).to_pixel(),
                    });
                    let lin = |v: u8| (v as f32 / 255.0).powf(2.2);
                    s.lighting.points.push(PointLight {
                        pos,
                        color: Vec3::new(lin(c.r), lin(c.g), lin(c.b)),
                        intensity: opt_f64(l, "intensity")?.unwrap_or(4.0).clamp(0.0, 200.0) as f32,
                        radius: opt_f64(l, "radius")?.unwrap_or(48.0).clamp(1.0, 2048.0) as f32,
                    });
                }
            }
            s.animation = match anim_index(model, a, "animation")? {
                Some(ai) => Some((ai, opt_f64(a, "frame")?.unwrap_or(0.0) as f32)),
                None => None,
            };
            Ok(s)
        }

        /// Picks one scale/anchor that fits every requested direction and frame (sprite consistency).
        fn fit(
            model: &VoxelModel,
            prepared: &Prepared,
            base: &RenderSettings,
            yaws: &[f32],
            frames: &[f32],
            margin: f32,
            scale: Option<f32>,
        ) -> CmdResult<(f32, f32, f32)> {
            let (mut half_w, mut lo_y, mut hi_y) = (0.0f32, f32::MAX, f32::MIN);
            for &yaw in yaws {
                for &t in frames {
                    let mut s = base.clone();
                    s.camera.yaw = yaw;
                    s.animation = s.animation.map(|(ai, _)| (ai, t));
                    if let Some((x0, x1, y0, y1)) = view_extent(model, prepared, &s) {
                        half_w = half_w.max(x0.abs()).max(x1.abs());
                        lo_y = lo_y.min(y0);
                        hi_y = hi_y.max(y1);
                    }
                }
            }
            if half_w == 0.0 {
                return Err(CmdError::invalid("the model is empty"));
            }
            let (w, h) = (base.camera.width as f32, base.camera.height as f32);
            let s = scale.unwrap_or_else(|| ((w / 2.0 - margin) / half_w).min((h - 2.0 * margin) / (hi_y - lo_y))).max(0.05);
            let anchor_y = h - margin + lo_y * s;
            Ok((s, w / 2.0, anchor_y))
        }

        fn render_dims(a: &Json, default: u32) -> CmdResult<(u32, u32)> {
            let w = opt_i64(a, "width")?.unwrap_or(default as i64);
            let h = opt_i64(a, "height")?.unwrap_or(w);
            if !(8..=2048).contains(&w) || !(8..=2048).contains(&h) {
                return Err(CmdError::invalid("render width/height must be 8-2048"));
            }
            Ok((w as u32, h as u32))
        }

        /// Optional pixel-art post-process for 3D renders: `pixel_palette` (library name, "swatches",
        /// or a color count) with `dither`, then a selective outline when `outline` is "pixel".
        fn pixel_art_post(doc: &Document, a: &Json, img: &mut crate::renderer::Image, shared: Option<&[Rgba]>) -> CmdResult<()> {
            let Some(p) = a.get("pixel_palette").filter(|v| !v.is_null()) else { return Ok(()) };
            let dither = super::pixel2d::dither_arg(a)?;
            let strength = opt_f64(a, "dither_strength")?.unwrap_or(20.0) as f32;
            let pal: Vec<Rgba> = match shared {
                Some(s) => s.to_vec(),
                None => {
                    let samples = crate::pixelart::image_samples(img);
                    let args = match p {
                        Json::Num(n) => Json::obj().with("colors", *n),
                        other => Json::obj().with("palette", other.clone()),
                    };
                    super::pixel2d::quantize_palette(doc, &args, &samples)?
                }
            };
            crate::pixelart::quantize_image(img, &pal, dither, strength);
            Ok(())
        }

        /// Renders a model to PNG from any angle/frame/pass.
        pub fn voxel_render(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Query> {
            let mi = model_index(doc, a)?;
            let model = &doc.models[mi];
            let (w, h) = render_dims(a, 256)?;
            let mut s = settings_from(doc, model, a, w, h)?;
            let prepared = cache.get(model);
            let margin = opt_f64(a, "margin")?.unwrap_or(8.0) as f32;
            let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
            let t = s.animation.map(|(_, t)| t).unwrap_or(0.0);
            let (scale, mut ax, mut ay) = fit(model, &prepared, &s, &[s.camera.yaw], &[t], margin, explicit)?;
            // `focus` = model point to center in the image (close-ups); use with an explicit `scale`.
            if let Some(f) = opt_vec3(a, "focus")? {
                let v = s.camera.view_rotation().apply(Vec3::from_f64(f) - model.origin);
                ax = w as f32 / 2.0 - v.x * scale;
                ay = h as f32 / 2.0 + v.y * scale;
            }
            s.camera.scale = scale;
            s.camera.anchor_x = ax;
            s.camera.anchor_y = ay;
            let mut img = render(model, &prepared, &doc.materials, &doc.textures, &s);
            pixel_art_post(doc, a, &mut img, None)?;
            let png = export::png::encode(&img);
            Ok(Query {
                result: Json::obj()
                    .with("model", model.name.as_str())
                    .with("width", w)
                    .with("height", h)
                    .with("scale", scale as f64)
                    .with("faces", prepared.faces.len())
                    .with("mimeType", "image/png")
                    .with("bytes", png.len()),
                binary: png,
            })
        }

        struct Bake {
            images: Vec<(String, f32, u32, crate::renderer::Image)>,
            frame_ms: u32,
            frames: u32,
            scale: f32,
            anim_name: String,
        }

        /// Frame-by-frame 3D animation: every listed model is one frame (VFX, morphs, hand-keyed poses).
        fn bake_sequence(doc: &Document, cache: &mut RenderCache, a: &Json, list: &[Json]) -> CmdResult<Bake> {
            if list.is_empty() || list.len() > 1024 {
                return Err(CmdError::invalid("'models' must list 1-1024 models"));
            }
            let mut indices = Vec::with_capacity(list.len());
            for m in list {
                indices.push(model_index(doc, &Json::obj().with("model", m.clone()))?);
            }
            let (w, h) = render_dims(a, 256)?;
            let dirs = directions(opt_i64(a, "directions")?.unwrap_or(1))?;
            let frame_ms = validate_duration(opt_i64(a, "frame_ms")?.unwrap_or(66))?;
            let margin = opt_f64(a, "margin")?.unwrap_or(6.0) as f32;
            let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
            // One shared camera for the whole sequence so the effect does not jitter: fit the union of
            // every frame's extent, keep the origin centered and the ground line fixed.
            let (mut half_w, mut lo_y, mut hi_y) = (0.0f32, f32::MAX, f32::MIN);
            for &mi in &indices {
                let model = &doc.models[mi];
                let prepared = cache.get(model);
                for (_, yaw) in &dirs {
                    let mut s = settings_from(doc, model, a, w, h)?;
                    s.camera.yaw = *yaw;
                    if let Some((x0, x1, y0, y1)) = view_extent(model, &prepared, &s) {
                        half_w = half_w.max(x0.abs()).max(x1.abs());
                        lo_y = lo_y.min(y0);
                        hi_y = hi_y.max(y1);
                    }
                }
            }
            if half_w == 0.0 {
                return Err(CmdError::invalid("all models in the sequence are empty"));
            }
            let scale =
                explicit.unwrap_or_else(|| ((w as f32 / 2.0 - margin) / half_w).min((h as f32 - 2.0 * margin) / (hi_y - lo_y))).max(0.05);
            let (ax, ay) = (w as f32 / 2.0, h as f32 - margin + lo_y * scale);
            let mut images = Vec::new();
            for (dname, yaw) in &dirs {
                for (f, &mi) in indices.iter().enumerate() {
                    let model = &doc.models[mi];
                    let mut s = settings_from(doc, model, a, w, h)?;
                    s.camera.yaw = *yaw;
                    s.camera.scale = scale;
                    s.camera.anchor_x = ax;
                    s.camera.anchor_y = ay;
                    if let Some(f) = opt_vec3(a, "focus")? {
                        let v = s.camera.view_rotation().apply(Vec3::from_f64(f) - model.origin);
                        s.camera.anchor_x = w as f32 / 2.0 - v.x * scale;
                        s.camera.anchor_y = h as f32 / 2.0 + v.y * scale;
                    }
                    let prepared = cache.get(model);
                    images.push((dname.to_string(), *yaw, f as u32, render(model, &prepared, &doc.materials, &doc.textures, &s)));
                }
            }
            let name = opt_str(a, "name")?.unwrap_or("sequence").to_string();
            Ok(Bake { images, frame_ms, frames: indices.len() as u32, scale, anim_name: name })
        }

        fn bake(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Bake> {
            let mut b = bake_raw(doc, cache, a)?;
            if a.get("pixel_palette").is_some_and(|v| !v.is_null()) {
                // One palette for every sprite keeps colors identical across directions and frames.
                let mut samples: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
                for (_, _, _, img) in &b.images {
                    for (c, n) in crate::pixelart::image_samples(img) {
                        *samples.entry(c.to_u32()).or_default() += n;
                    }
                }
                let mut list: Vec<(Rgba, u32)> = samples.into_iter().map(|(c, n)| (Rgba::from_u32(c), n)).collect();
                list.sort_by_key(|(c, _)| c.to_u32());
                let args = match a.get("pixel_palette") {
                    Some(Json::Num(n)) => Json::obj().with("colors", *n),
                    Some(other) => Json::obj().with("palette", other.clone()),
                    None => Json::obj(),
                };
                let pal = super::pixel2d::quantize_palette(doc, &args, &list)?;
                for (_, _, _, img) in b.images.iter_mut() {
                    pixel_art_post(doc, a, img, Some(&pal))?;
                }
            }
            Ok(b)
        }

        fn bake_raw(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Bake> {
            if let Some(Json::Arr(list)) = a.get("models") {
                return bake_sequence(doc, cache, a, list);
            }
            let mi = model_index(doc, a)?;
            let model = &doc.models[mi];
            let (w, h) = render_dims(a, 256)?;
            let mut base = settings_from(doc, model, a, w, h)?;
            let dirs = directions(opt_i64(a, "directions")?.unwrap_or(8))?;
            let ai = anim_index(model, a, "animation")?;
            let (frames, frame_ms, anim_name) = match ai {
                Some(i) => {
                    let an = &model.animations[i];
                    (an.frames, an.frame_ms, an.name.clone())
                }
                None => (1, 100, "pose".to_string()),
            };
            if dirs.len() as u64 * frames as u64 > 4096 {
                return Err(CmdError::new("limit", "too many sprites in one bake (max 4096)"));
            }
            base.animation = ai.map(|i| (i, 0.0));
            let prepared = cache.get(model);
            let margin = opt_f64(a, "margin")?.unwrap_or(6.0) as f32;
            let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
            let times: Vec<f32> = (0..frames).map(|f| f as f32).collect();
            let yaws: Vec<f32> = dirs.iter().map(|d| d.1).collect();
            let (scale, ax, ay) = fit(model, &prepared, &base, &yaws, &times, margin, explicit)?;
            base.camera.scale = scale;
            base.camera.anchor_x = ax;
            base.camera.anchor_y = ay;
            let mut images = Vec::with_capacity(dirs.len() * frames as usize);
            for (dname, yaw) in &dirs {
                for f in 0..frames {
                    let mut s = base.clone();
                    s.camera.yaw = *yaw;
                    s.animation = ai.map(|i| (i, f as f32));
                    images.push((dname.to_string(), *yaw, f, render(model, &prepared, &doc.materials, &doc.textures, &s)));
                }
            }
            Ok(Bake { images, frame_ms, frames, scale, anim_name })
        }

        /// Renders directions × frames into one sprite sheet (rows = directions) for any G-buffer pass.
        pub fn voxel_render_sheet(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Query> {
            let b = bake(doc, cache, a)?;
            let (cw, ch) = (b.images[0].3.width, b.images[0].3.height);
            let rows = b.images.len() as u32 / b.frames;
            if cw * b.frames > 16384 || ch * rows > 16384 {
                return Err(CmdError::new("limit", "sprite sheet would exceed 16384px"));
            }
            let mut sheet = crate::renderer::Image::new(cw * b.frames, ch * rows);
            let mut cells = Vec::new();
            for (k, (dir, yaw, f, img)) in b.images.iter().enumerate() {
                let (x, y) = ((k as u32 % b.frames) * cw, (k as u32 / b.frames) * ch);
                sheet.blit(img, x, y);
                cells.push(
                    Json::obj()
                        .with("direction", dir.as_str())
                        .with("yaw", *yaw as f64)
                        .with("frame", *f)
                        .with("x", x)
                        .with("y", y)
                        .with("w", cw)
                        .with("h", ch)
                        .with("duration", b.frame_ms),
                );
            }
            let png = export::png::encode(&sheet);
            Ok(Query {
                result: Json::obj()
                    .with("animation", b.anim_name.as_str())
                    .with("pass", opt_str(a, "pass")?.unwrap_or("lit"))
                    .with("columns", b.frames)
                    .with("rows", rows)
                    .with("cellWidth", cw)
                    .with("cellHeight", ch)
                    .with("scale", b.scale as f64)
                    .with("cells", Json::Arr(cells))
                    .with("mimeType", "image/png")
                    .with("bytes", png.len()),
                binary: png,
            })
        }

        /// Bakes directions × frames into the 2D project as frames on one layer, tagged per direction
        /// (e.g. walk_S, walk_SE, ...), so the sprites can be polished pixel by pixel.
        pub fn voxel_render_to_frames(doc: &mut Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Outcome> {
            let b = bake(doc, cache, a)?;
            let (w, h) = (b.images[0].3.width, b.images[0].3.height);
            if doc.frames.len() + b.images.len() > MAX_FRAMES {
                return Err(CmdError::new("limit", "frame safety limit reached"));
            }
            let resize = opt_bool(a, "resize_canvas")?.unwrap_or(true);
            if (doc.width, doc.height) != (w, h) {
                if !resize {
                    return Err(CmdError::invalid(format!(
                        "canvas is {}x{} but the render is {w}x{h}; set resize_canvas",
                        doc.width, doc.height
                    )));
                }
                let (w2, h2) = validate_size(w as i64, h as i64)?;
                doc.resize(w2, h2, 0, 0);
            }
            let layer_name = opt_str(a, "layer")?.unwrap_or("3D Render").to_string();
            let layer_id = match doc.layers.iter().find(|l| l.name == layer_name) {
                Some(l) => l.id,
                None => doc.insert_layer(&layer_name, doc.layers.len()),
            };
            // Reuse a single, completely empty starting frame instead of leaving a blank frame in front.
            let reuse_first = doc.frames.len() == 1 && doc.cels.values().all(|c| c.is_empty());
            let start = if reuse_first { 0 } else { doc.frames.len() };
            let mut tag_ranges: HashMap<String, (u32, u32)> = HashMap::new();
            let mut order: Vec<String> = Vec::new();
            for (k, (dir, _, _, img)) in b.images.iter().enumerate() {
                let index = start + k;
                let fid = if reuse_first && k == 0 {
                    doc.frames[0].duration_ms = b.frame_ms;
                    doc.frames[0].id
                } else {
                    doc.insert_frame(index, b.frame_ms)
                };
                let (lw, lh) = doc.layer_dims(layer_id);
                let mut grid = PixelGrid::new(lw, lh);
                for y in 0..img.height {
                    for x in 0..img.width {
                        let c = img.get(x, y);
                        if c.a > 0 {
                            grid.set(x as i64, y as i64, c.to_pixel());
                        }
                    }
                }
                doc.cels.insert((layer_id, fid), Arc::new(grid));
                let tag = format!("{}_{}", b.anim_name, dir);
                let e = tag_ranges.entry(tag.clone()).or_insert((fid, fid));
                e.1 = fid;
                if !order.contains(&tag) {
                    order.push(tag);
                }
            }
            let mut tags = Vec::new();
            for name in order {
                let (from, to) = tag_ranges[&name];
                let mut unique = name.clone();
                let mut n = 2;
                while doc.tags.iter().any(|t| t.name == unique) {
                    unique = format!("{name}_{n}");
                    n += 1;
                }
                doc.tags.push(crate::animation::Tag {
                    name: unique.clone(),
                    from_id: from,
                    to_id: to,
                    direction: crate::animation::Direction::Forward,
                    repeat: 0,
                });
                tags.push(Json::from(unique));
            }
            doc.active_layer = layer_id;
            doc.active_frame = doc.frames[start].id;
            let n = b.images.len();
            Ok(Outcome::new(
                Json::obj()
                    .with("framesAdded", n)
                    .with("firstFrame", start)
                    .with("tags", Json::Arr(tags))
                    .with("width", w)
                    .with("height", h)
                    .with("scale", b.scale as f64)
                    .with("layer", layer_name),
                (n as u64) * (w as u64) * (h as u64),
            ))
        }

        pub fn voxel_pick(doc: &Document, cache: &mut RenderCache, a: &Json) -> CmdResult<Query> {
            let mi = model_index(doc, a)?;
            let model = &doc.models[mi];
            let (w, h) = render_dims(a, 256)?;
            let mut s = settings_from(doc, model, a, w, h)?;
            let prepared = cache.get(model);
            let explicit = opt_f64(a, "scale")?.map(|v| v.clamp(0.05, 64.0) as f32);
            let t = s.animation.map(|(_, t)| t).unwrap_or(0.0);
            let (scale, ax, ay) = fit(model, &prepared, &s, &[s.camera.yaw], &[t], opt_f64(a, "margin")?.unwrap_or(8.0) as f32, explicit)?;
            s.camera.scale = scale;
            s.camera.anchor_x = ax;
            s.camera.anchor_y = ay;
            let (px, py) = (req_i64(a, "px")?, req_i64(a, "py")?);
            let hit = if px < 0 || py < 0 { None } else { pick(model, &prepared, &s, px as u32, py as u32) };
            Ok(Query {
                result: match hit {
                    Some((p, n)) => {
                        let v = model.get(p[0], p[1], p[2]);
                        Json::obj()
                            .with("hit", true)
                            .with("voxel", Json::Arr(p.iter().map(|&c| Json::from(c)).collect()))
                            .with("normal", Json::Arr(n.iter().map(|&c| Json::from(c)).collect()))
                            .with("color", Rgba::from_u32(v.rgba).to_hex())
                            .with("material", v.material)
                            .with("bone", model.bones.get(v.bone as usize).map(|b| b.name.as_str()))
                    }
                    None => Json::obj().with("hit", false),
                },
                binary: Vec::new(),
            })
        }

        pub fn voxel_inspect(doc: &Document, a: &Json) -> CmdResult<Query> {
            let models: Vec<Json> = doc
                .models
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let mut j = Json::obj()
                        .with("index", i)
                        .with("name", m.name.as_str())
                        .with("size", Json::Arr(m.size.iter().map(|&d| Json::from(d)).collect()))
                        .with("origin", vec_json(m.origin))
                        .with("bones", m.bones.len())
                        .with(
                            "animations",
                            Json::Arr(
                                m.animations
                                    .iter()
                                    .map(|x| Json::obj().with("name", x.name.as_str()).with("frames", x.frames).with("frameMs", x.frame_ms))
                                    .collect(),
                            ),
                        );
                    if a.get("model").is_some() || doc.models.len() == 1 {
                        j.set("voxels", m.count());
                        if let Some((lo, hi)) = m.bounds() {
                            j.set(
                                "bounds",
                                Json::obj()
                                    .with("min", Json::Arr(lo.iter().map(|&v| Json::from(v)).collect()))
                                    .with("max", Json::Arr(hi.iter().map(|&v| Json::from(v)).collect())),
                            );
                        }
                        j.set(
                            "skeleton",
                            Json::Arr(
                                m.bones
                                    .iter()
                                    .map(|b| {
                                        Json::obj()
                                            .with("name", b.name.as_str())
                                            .with("parent", b.parent.map(|p| m.bones[p].name.as_str()))
                                            .with("head", vec_json(b.head))
                                            .with("tail", vec_json(b.tail))
                                    })
                                    .collect(),
                            ),
                        );
                    }
                    j
                })
                .collect();
            let materials = doc
                .materials
                .iter()
                .enumerate()
                .map(|(i, m)| {
                    let mut maps = Json::obj();
                    for (name, slot) in MAP_SLOTS {
                        if let Some(t) = m.maps[slot as usize] {
                            maps.set(name, doc.textures[t].name.as_str());
                        }
                    }
                    Json::obj()
                        .with("index", i)
                        .with("name", m.name.as_str())
                        .with("roughness", m.roughness as f64)
                        .with("metallic", m.metallic as f64)
                        .with("emission", m.emission as f64)
                        .with("uvScale", m.uv_scale as f64)
                        .with("maps", maps)
                })
                .collect();
            let textures = doc
                .textures
                .iter()
                .enumerate()
                .map(|(i, t)| Json::obj().with("index", i).with("name", t.name.as_str()).with("width", t.width).with("height", t.height))
                .collect();
            Ok(Query {
                result: Json::obj()
                    .with("models", Json::Arr(models))
                    .with("materials", Json::Arr(materials))
                    .with("textures", Json::Arr(textures)),
                binary: Vec::new(),
            })
        }
    }

    use json::Json;

    use crate::project::Document;

    /// Machine-readable failure.
    #[derive(Debug, Clone, PartialEq)]
    pub struct CmdError {
        pub code: &'static str,
        pub message: String,
    }

    impl CmdError {
        pub fn new(code: &'static str, message: impl Into<String>) -> CmdError {
            CmdError { code, message: message.into() }
        }
        pub fn invalid(message: impl Into<String>) -> CmdError {
            CmdError::new("invalid_argument", message)
        }
        pub fn not_found(message: impl Into<String>) -> CmdError {
            CmdError::new("not_found", message)
        }
    }

    impl From<String> for CmdError {
        fn from(message: String) -> Self {
            CmdError::invalid(message)
        }
    }

    pub type CmdResult<T> = Result<T, CmdError>;

    /// Result of a successful mutation.
    pub struct Outcome {
        pub result: Json,
        pub changed: u64,
    }

    impl Outcome {
        pub fn new(result: Json, changed: u64) -> Outcome {
            Outcome { result, changed }
        }
    }

    /// Result of a read-only query (optionally with binary data such as PNG bytes).
    pub struct Query {
        pub result: Json,
        pub binary: Vec<u8>,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Kind {
        Read,
        Mutate,
        /// Mutations with engine-level semantics (history, transactions, whole-document swaps).
        Special,
    }

    /// Every operation the engine understands, with its kind. Kept in one place so the
    /// gateway's tool catalogue can be checked against it.
    pub const OPERATIONS: &[(&str, Kind)] = &[
        // queries
        ("status", Kind::Read),
        ("canvas_inspect", Kind::Read),
        ("get_pixel", Kind::Read),
        ("read_region", Kind::Read),
        ("history_list", Kind::Read),
        ("render_frame", Kind::Read),
        ("canvas_snapshot", Kind::Read),
        ("export", Kind::Read),
        ("project_save", Kind::Read),
        ("voxel_inspect", Kind::Read),
        ("voxel_render", Kind::Read),
        ("voxel_render_sheet", Kind::Read),
        ("voxel_pick", Kind::Read),
        ("render_lit", Kind::Read),
        ("tilemap_render", Kind::Read),
        // drawing
        ("set_pixel", Kind::Mutate),
        ("set_pixels", Kind::Mutate),
        ("draw_line", Kind::Mutate),
        ("draw_rectangle", Kind::Mutate),
        ("draw_ellipse", Kind::Mutate),
        ("draw_polygon", Kind::Mutate),
        ("pattern_fill", Kind::Mutate),
        ("fx_generate", Kind::Mutate),
        ("fx_particles", Kind::Mutate),
        ("anim_generate", Kind::Mutate),
        ("fill_region", Kind::Mutate),
        ("flip", Kind::Mutate),
        ("rotate", Kind::Mutate),
        ("move_region", Kind::Mutate),
        ("copy_region", Kind::Mutate),
        ("clear", Kind::Mutate),
        // structure
        ("layer_create", Kind::Mutate),
        ("layer_delete", Kind::Mutate),
        ("layer_move", Kind::Mutate),
        ("layer_select", Kind::Mutate),
        ("frame_add", Kind::Mutate),
        ("frame_duplicate", Kind::Mutate),
        ("frame_delete", Kind::Mutate),
        ("frame_move", Kind::Mutate),
        ("frame_set_duration", Kind::Mutate),
        ("frame_select", Kind::Mutate),
        ("animation_tag_create", Kind::Mutate),
        ("animation_tag_delete", Kind::Mutate),
        ("animation_set_loop", Kind::Mutate),
        ("palette_set", Kind::Mutate),
        ("canvas_resize", Kind::Mutate),
        ("project_rename", Kind::Mutate),
        ("image_import", Kind::Mutate),
        ("draw_ascii", Kind::Mutate),
        ("outline", Kind::Mutate),
        ("color_ramp", Kind::Mutate),
        ("quantize", Kind::Mutate),
        ("dither_fill", Kind::Mutate),
        ("auto_shade", Kind::Mutate),
        ("gradient_map", Kind::Mutate),
        ("draw_sparkle", Kind::Mutate),
        ("pixel_cleanup", Kind::Mutate),
        ("ramp_set", Kind::Mutate),
        ("ramp_delete", Kind::Mutate),
        ("shade", Kind::Mutate),
        ("draw_stroke", Kind::Mutate),
        ("layer_set", Kind::Mutate),
        // game production
        ("slice_set", Kind::Mutate),
        ("slice_delete", Kind::Mutate),
        ("palette_swap", Kind::Mutate),
        ("tileset_create", Kind::Mutate),
        ("tileset_add_tiles", Kind::Mutate),
        ("tileset_generate_blob", Kind::Mutate),
        ("tileset_delete", Kind::Mutate),
        ("tilemap_create", Kind::Mutate),
        ("tilemap_delete", Kind::Mutate),
        ("tilemap_set", Kind::Mutate),
        ("tilemap_terrain", Kind::Mutate),
        ("tilemap_bake", Kind::Mutate),
        // 3D pixel art
        ("voxel_model_create", Kind::Mutate),
        ("voxel_model_delete", Kind::Mutate),
        ("voxel_model_clone", Kind::Mutate),
        ("voxel_model_set", Kind::Mutate),
        ("voxel_set", Kind::Mutate),
        ("voxel_set_many", Kind::Mutate),
        ("voxel_shape", Kind::Mutate),
        ("voxel_mirror", Kind::Mutate),
        ("material_set", Kind::Mutate),
        ("texture_import", Kind::Mutate),
        ("texture_generate", Kind::Mutate),
        ("rig_bone_add", Kind::Mutate),
        ("rig_clear", Kind::Mutate),
        ("rig_bind", Kind::Mutate),
        ("anim3d_create", Kind::Mutate),
        ("anim3d_key", Kind::Mutate),
        ("anim3d_generate", Kind::Mutate),
        ("voxel_render_to_frames", Kind::Mutate),
        // engine-level
        ("project_new", Kind::Special),
        ("project_load", Kind::Special),
        ("history_undo", Kind::Special),
        ("history_redo", Kind::Special),
        ("transaction", Kind::Special),
    ];

    pub fn kind_of(op: &str) -> Option<Kind> {
        OPERATIONS.iter().find(|(name, _)| *name == op).map(|(_, k)| *k)
    }

    /// Applies one plain mutation to a document.
    pub fn apply_mutation(doc: &mut Document, op: &str, a: &Json, payload: &[u8], cache: &mut voxel::RenderCache) -> CmdResult<Outcome> {
        match op {
            "draw_ascii" => pixel2d::draw_ascii(doc, a),
            "outline" => pixel2d::outline(doc, a),
            "color_ramp" => pixel2d::color_ramp(doc, a),
            "quantize" => pixel2d::quantize(doc, a),
            "dither_fill" => pixel2d::dither_fill(doc, a),
            "auto_shade" => charart::auto_shade(doc, a),
            "gradient_map" => charart::gradient_map(doc, a),
            "draw_sparkle" => charart::draw_sparkle(doc, a),
            "pixel_cleanup" => charart::pixel_cleanup(doc, a),
            "ramp_set" => artist::ramp_set(doc, a),
            "ramp_delete" => artist::ramp_delete(doc, a),
            "shade" => artist::shade(doc, a),
            "draw_stroke" => artist::draw_stroke(doc, a),
            "layer_set" => artist::layer_set(doc, a),
            "image_import" => artist::reference_import(doc, a, payload),
            "slice_set" => gamedev::slice_set(doc, a),
            "slice_delete" => gamedev::slice_delete(doc, a),
            "palette_swap" => gamedev::palette_swap(doc, a),
            "tileset_create" => gamedev::tileset_create(doc, a),
            "tileset_add_tiles" => gamedev::tileset_add_tiles(doc, a),
            "tileset_generate_blob" => gamedev::tileset_generate_blob(doc, a),
            "tileset_delete" => gamedev::tileset_delete(doc, a),
            "tilemap_create" => gamedev::tilemap_create(doc, a),
            "tilemap_delete" => gamedev::tilemap_delete(doc, a),
            "tilemap_set" => gamedev::tilemap_set(doc, a),
            "tilemap_terrain" => gamedev::tilemap_terrain(doc, a),
            "tilemap_bake" => gamedev::tilemap_bake(doc, a),
            "voxel_model_create" => voxel::model_create(doc, a),
            "voxel_model_delete" => voxel::model_delete(doc, a),
            "voxel_model_clone" => voxel::model_clone(doc, a),
            "voxel_model_set" => voxel::model_set(doc, a),
            "voxel_set" => voxel::voxel_set(doc, a),
            "voxel_set_many" => voxel::voxel_set_many(doc, a),
            "voxel_shape" => voxel::voxel_shape(doc, a),
            "voxel_mirror" => voxel::voxel_mirror(doc, a),
            "material_set" => voxel::material_set(doc, a),
            "texture_import" => voxel::texture_import(doc, a, payload),
            "texture_generate" => voxel::texture_generate(doc, a),
            "rig_bone_add" => voxel::bone_add(doc, a),
            "rig_clear" => voxel::rig_clear(doc, a),
            "rig_bind" => voxel::rig_bind(doc, a),
            "anim3d_create" => voxel::anim_create(doc, a),
            "anim3d_key" => voxel::anim_key(doc, a),
            "anim3d_generate" => voxel::anim_generate(doc, a),
            "voxel_render_to_frames" => voxel::voxel_render_to_frames(doc, cache, a),
            "set_pixel" => draw::set_pixel(doc, a),
            "set_pixels" => draw::set_pixels(doc, a),
            "draw_line" => draw::draw_line(doc, a),
            "draw_rectangle" => draw::draw_rectangle(doc, a),
            "draw_ellipse" => draw::draw_ellipse(doc, a),
            "draw_polygon" => draw::draw_polygon(doc, a),
            "pattern_fill" => draw::pattern_fill(doc, a),
            "fx_generate" => fx::fx_generate(doc, a),
            "fx_particles" => fx::fx_particles(doc, a),
            "anim_generate" => fx::anim_generate(doc, a),
            "fill_region" => draw::fill_region(doc, a),
            "flip" => match args::opt_str(a, "axis")?.unwrap_or("horizontal") {
                "horizontal" | "x" => draw::flip(doc, a, true),
                "vertical" | "y" => draw::flip(doc, a, false),
                other => Err(CmdError::invalid(format!("unknown axis '{other}' (horizontal, vertical)"))),
            },
            // Quarter turns are exact; any other angle (or method rotsprite) uses RotSprite.
            "rotate" => {
                let deg = args::opt_f64(a, "degrees")?.ok_or_else(|| CmdError::invalid("missing required number 'degrees'"))?;
                let exact = deg.fract() == 0.0 && (deg as i64).rem_euclid(90) == 0;
                match args::opt_str(a, "method")?.unwrap_or("auto") {
                    "auto" if exact => draw::rotate(doc, a),
                    "auto" | "rotsprite" => gamedev::rotsprite_cmd(doc, a),
                    other => Err(CmdError::invalid(format!("unknown method '{other}' (auto, rotsprite)"))),
                }
            }
            "move_region" => draw::move_region(doc, a),
            "copy_region" => draw::copy_region(doc, a),
            "clear" => draw::clear(doc, a),
            "layer_create" => structure::layer_create(doc, a),
            "layer_delete" => structure::layer_delete(doc, a),
            "layer_move" => structure::layer_move(doc, a),
            "layer_select" => structure::layer_select(doc, a),
            "frame_add" => structure::frame_add(doc, a),
            "frame_duplicate" => structure::frame_duplicate(doc, a),
            "frame_delete" => structure::frame_delete(doc, a),
            "frame_move" => structure::frame_move(doc, a),
            "frame_set_duration" => structure::frame_set_duration(doc, a),
            "frame_select" => structure::frame_select(doc, a),
            "animation_tag_create" => structure::tag_create(doc, a),
            "animation_tag_delete" => structure::tag_delete(doc, a),
            "animation_set_loop" => structure::set_loop(doc, a),
            // Swatch edit with `index`; otherwise load a preset/colors (mode replace or add).
            "palette_set" if a.get("index").is_some_and(|v| !v.is_null()) => structure::palette_set(doc, a),
            "palette_set" => pixel2d::palette_load(doc, a),
            "canvas_resize" => structure::canvas_resize(doc, a),
            "project_rename" => structure::project_rename(doc, a),
            _ => Err(CmdError::new("unknown_op", format!("unknown operation '{op}'"))),
        }
    }

    pub use query::run_query;
}
pub mod engine {
    //! The engine: single authority over the document.
    //!
    //! All writers (human UI, MCP agents, REST clients) go through `Engine::execute`, which is
    //! called under one lock by the FFI layer — a single-writer command queue. Optimistic
    //! concurrency comes from `expectedRevision`: a writer that saw an old revision is rejected
    //! instead of overwriting newer work. `operationId` makes retries idempotent.

    use crate::commands::json::Json;
    use crate::commands::{self, CmdError, CmdResult, Kind, Outcome};
    use crate::history::{History, LogEntry, now_ms};
    use crate::project::{Document, validate_size};
    use crate::storage;

    pub const MAX_TRANSACTION_COMMANDS: usize = 10_000;

    pub struct Response {
        pub json: String,
        pub binary: Vec<u8>,
    }

    pub struct Engine {
        doc: Document,
        history: History,
        revision: u64,
        cache: commands::voxel::RenderCache,
    }

    struct Envelope<'a> {
        op: &'a str,
        args: &'a Json,
        actor: String,
        operation_id: Option<String>,
        expected_revision: Option<u64>,
    }

    fn parse_envelope(req: &Json) -> CmdResult<Envelope<'_>> {
        if !matches!(req, Json::Obj(_)) {
            return Err(CmdError::new("invalid_request", "request must be a JSON object"));
        }
        let op = req.get("op").and_then(Json::as_str).ok_or_else(|| CmdError::new("invalid_request", "missing string field 'op'"))?;
        let args = match req.get("args") {
            Some(a @ Json::Obj(_)) => a,
            Some(Json::Null) | None => req,
            Some(_) => return Err(CmdError::new("invalid_request", "'args' must be an object")),
        };
        let actor = match req.get("actor") {
            Some(Json::Str(s)) if !s.trim().is_empty() => s.trim().chars().filter(|c| !c.is_control()).take(64).collect(),
            None | Some(Json::Null) => "unknown".to_string(),
            Some(_) => return Err(CmdError::new("invalid_request", "'actor' must be a non-empty string")),
        };
        let operation_id = match req.get("operationId") {
            Some(Json::Str(s)) if !s.is_empty() && s.len() <= 128 => Some(s.clone()),
            None | Some(Json::Null) => None,
            Some(_) => return Err(CmdError::new("invalid_request", "'operationId' must be a string of 1-128 characters")),
        };
        let expected_revision = match req.get("expectedRevision") {
            None | Some(Json::Null) => None,
            Some(v) => Some(
                v.as_i64()
                    .filter(|r| *r >= 0)
                    .ok_or_else(|| CmdError::new("invalid_request", "'expectedRevision' must be a non-negative integer"))?
                    as u64,
            ),
        };
        Ok(Envelope { op, args, actor, operation_id, expected_revision })
    }

    impl Engine {
        pub fn new(width: u32, height: u32) -> Engine {
            Engine { doc: Document::new(width, height), history: History::default(), revision: 0, cache: Default::default() }
        }

        pub fn revision(&self) -> u64 {
            self.revision
        }

        pub fn document(&self) -> &Document {
            &self.doc
        }

        fn error_json(&self, op: &str, e: &CmdError) -> String {
            Json::obj()
                .with("ok", false)
                .with("op", op)
                .with("revision", self.revision)
                .with("error", Json::obj().with("code", e.code).with("message", e.message.as_str()))
                .to_json_string()
        }

        /// Executes one request. Never panics on bad input; every failure is a JSON error.
        pub fn execute(&mut self, request: &[u8], payload: &[u8]) -> Response {
            let text = match std::str::from_utf8(request) {
                Ok(t) => t,
                Err(_) => return self.fail("", CmdError::new("invalid_request", "request is not valid UTF-8")),
            };
            let req = match Json::parse(text) {
                Ok(j) => j,
                Err(e) => return self.fail("", CmdError::new("invalid_request", format!("malformed JSON: {e}"))),
            };
            let env = match parse_envelope(&req) {
                Ok(e) => e,
                Err(e) => return self.fail(req.get("op").and_then(Json::as_str).unwrap_or(""), e),
            };
            let Some(kind) = commands::kind_of(env.op) else {
                return self.fail(env.op, CmdError::new("unknown_op", format!("unknown operation '{}'", env.op)));
            };

            if kind == Kind::Read {
                return match commands::run_query(&self.doc, &self.history, self.revision, env.op, env.args, &mut self.cache) {
                    Ok(q) => Response {
                        json: Json::obj()
                            .with("ok", true)
                            .with("op", env.op)
                            .with("revision", self.revision)
                            .with("result", q.result)
                            .to_json_string(),
                        binary: q.binary,
                    },
                    Err(e) => self.fail(env.op, e),
                };
            }

            // Idempotent replay: the same operationId never applies twice.
            if let Some(id) = &env.operation_id
                && let Some(cached) = self.history.cached_response(id)
            {
                let mut j = Json::parse(std::str::from_utf8(cached).unwrap_or("{}")).unwrap_or(Json::obj());
                j.set("replayed", true);
                return Response { json: j.to_json_string(), binary: Vec::new() };
            }
            if let Some(expected) = env.expected_revision
                && expected != self.revision
            {
                return self.fail(
                    env.op,
                    CmdError::new(
                        "conflict",
                        format!(
                            "expected revision {expected} but the canvas is at revision {}; re-read the canvas and retry",
                            self.revision
                        ),
                    ),
                );
            }

            let before = self.doc.clone();
            let outcome = match kind {
                Kind::Special => self.apply_special(env.op, env.args, payload),
                _ => commands::apply_mutation(&mut self.doc, env.op, env.args, payload, &mut self.cache),
            };
            match outcome {
                Err(e) => {
                    self.doc = before;
                    self.fail(env.op, e)
                }
                Ok(outcome) => {
                    self.revision += 1;
                    if !matches!(env.op, "history_undo" | "history_redo") {
                        self.history.record(before, self.revision, env.op, &env.actor);
                    }
                    self.history.log(LogEntry {
                        revision: self.revision,
                        actor: env.actor.clone(),
                        op: env.op.to_string(),
                        operation_id: env.operation_id.clone(),
                        changed_pixels: outcome.changed,
                        timestamp_ms: now_ms(),
                    });
                    let json = Json::obj()
                        .with("ok", true)
                        .with("op", env.op)
                        .with("revision", self.revision)
                        .with("actor", env.actor.as_str())
                        .with("result", outcome.result)
                        .to_json_string();
                    if let Some(id) = &env.operation_id {
                        self.history.cache_response(id, json.clone().into_bytes());
                    }
                    Response { json, binary: Vec::new() }
                }
            }
        }

        fn fail(&self, op: &str, e: CmdError) -> Response {
            Response { json: self.error_json(op, &e), binary: Vec::new() }
        }

        fn apply_special(&mut self, op: &str, a: &Json, payload: &[u8]) -> CmdResult<Outcome> {
            match op {
                "history_undo" | "history_redo" => {
                    let steps = commands_args_steps(a)?;
                    let mut done = Vec::new();
                    for _ in 0..steps {
                        let info = if op == "history_undo" {
                            self.history.undo(&mut self.doc, self.revision)
                        } else {
                            self.history.redo(&mut self.doc, self.revision)
                        };
                        match info {
                            Some(i) => done.push(Json::obj().with("op", i.label).with("actor", i.actor)),
                            None => break,
                        }
                    }
                    if done.is_empty() {
                        return Err(CmdError::new(
                            "nothing_to_do",
                            if op == "history_undo" { "nothing to undo" } else { "nothing to redo" },
                        ));
                    }
                    Ok(Outcome::new(Json::obj().with("steps", done.len()).with("operations", Json::Arr(done)), 0))
                }
                "project_new" => {
                    let w = a.get("width").and_then(Json::as_i64).unwrap_or(32);
                    let h = a.get("height").and_then(Json::as_i64).unwrap_or(32);
                    let (w, h) = validate_size(w, h)?;
                    let mut doc = Document::new(w, h);
                    if let Some(name) = a.get("name").and_then(Json::as_str).map(str::trim).filter(|n| !n.is_empty()) {
                        doc.name = name.chars().take(256).collect();
                    }
                    self.doc = doc;
                    Ok(Outcome::new(Json::obj().with("width", w).with("height", h).with("name", self.doc.name.as_str()), 0))
                }
                "project_load" => {
                    let loaded = storage::load(payload).map_err(|e| CmdError::new("invalid_file", e))?;
                    self.doc = loaded.doc;
                    let max_rev = loaded.log.iter().map(|e| e.revision).max().unwrap_or(0);
                    self.history.replace_log(loaded.log);
                    // Keep revisions monotonic so stale writers are still rejected after a load.
                    self.revision = self.revision.max(max_rev);
                    Ok(Outcome::new(
                        Json::obj()
                            .with("name", self.doc.name.as_str())
                            .with("width", self.doc.width)
                            .with("height", self.doc.height)
                            .with("frames", self.doc.frames.len()),
                        0,
                    ))
                }
                "transaction" => {
                    let cmds =
                        a.get("commands").and_then(Json::as_array).ok_or_else(|| CmdError::invalid("'commands' must be an array"))?;
                    if cmds.is_empty() || cmds.len() > MAX_TRANSACTION_COMMANDS {
                        return Err(CmdError::new("limit", format!("a transaction holds 1-{MAX_TRANSACTION_COMMANDS} commands")));
                    }
                    let mut results = Vec::with_capacity(cmds.len());
                    let mut changed = 0;
                    for (i, c) in cmds.iter().enumerate() {
                        let op = c
                            .get("op")
                            .and_then(Json::as_str)
                            .ok_or_else(|| CmdError::invalid(format!("commands[{i}] is missing 'op'")))?;
                        if commands::kind_of(op) != Some(Kind::Mutate) {
                            return Err(CmdError::invalid(format!("commands[{i}]: '{op}' cannot be used inside a transaction")));
                        }
                        let args = match c.get("args") {
                            Some(a @ Json::Obj(_)) => a,
                            _ => c,
                        };
                        let o = commands::apply_mutation(&mut self.doc, op, args, &[], &mut self.cache).map_err(|e| {
                            CmdError::new(e.code, format!("commands[{i}] ({op}) failed, transaction rolled back: {}", e.message))
                        })?;
                        changed += o.changed;
                        results.push(o.result);
                    }
                    Ok(Outcome::new(
                        Json::obj().with("applied", results.len()).with("changedPixels", changed).with("results", Json::Arr(results)),
                        changed,
                    ))
                }
                _ => Err(CmdError::new("unknown_op", format!("unknown operation '{op}'"))),
            }
        }
    }

    fn commands_args_steps(a: &Json) -> CmdResult<usize> {
        match a.get("steps").filter(|v| !v.is_null()) {
            None => Ok(1),
            Some(v) => {
                v.as_i64().filter(|s| (1..=256).contains(s)).map(|s| s as usize).ok_or_else(|| CmdError::invalid("'steps' must be 1-256"))
            }
        }
    }
}
pub mod export {
    //! Export engine: PNG, animated GIF and sprite sheets (+ JSON metadata for game engines).

    pub mod deflate {
        //! zlib (RFC 1950) wrapper around a DEFLATE (RFC 1951) encoder using LZ77 with hash
        //! chains and the fixed Huffman code. Pixel art is extremely repetitive, so this gets
        //! most of the benefit of a full encoder at a fraction of the code.

        const WINDOW: usize = 32 * 1024;
        const MIN_MATCH: usize = 3;
        const MAX_MATCH: usize = 258;
        const HASH_BITS: usize = 15;
        const MAX_CHAIN: usize = 64;

        const LEN_BASE: [u16; 29] =
            [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
        const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
        const DIST_BASE: [u16; 30] = [
            1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289,
            16385, 24577,
        ];
        const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

        struct BitWriter {
            out: Vec<u8>,
            acc: u64,
            nbits: u32,
        }

        impl BitWriter {
            fn put(&mut self, value: u32, bits: u32) {
                self.acc |= (value as u64) << self.nbits;
                self.nbits += bits;
                while self.nbits >= 8 {
                    self.out.push(self.acc as u8);
                    self.acc >>= 8;
                    self.nbits -= 8;
                }
            }

            /// Huffman codes are defined MSB-first; the stream is LSB-first.
            fn put_code(&mut self, code: u32, bits: u32) {
                let mut rev = 0;
                for i in 0..bits {
                    rev |= ((code >> i) & 1) << (bits - 1 - i);
                }
                self.put(rev, bits);
            }

            fn finish(mut self) -> Vec<u8> {
                if self.nbits > 0 {
                    self.out.push(self.acc as u8);
                }
                self.out
            }
        }

        fn put_literal(w: &mut BitWriter, sym: u32) {
            match sym {
                0..=143 => w.put_code(0x30 + sym, 8),
                144..=255 => w.put_code(0x190 + (sym - 144), 9),
                256..=279 => w.put_code(sym - 256, 7),
                _ => w.put_code(0xC0 + (sym - 280), 8),
            }
        }

        fn put_match(w: &mut BitWriter, len: usize, dist: usize) {
            let li = LEN_BASE.iter().rposition(|&b| b as usize <= len).unwrap();
            put_literal(w, 257 + li as u32);
            w.put((len - LEN_BASE[li] as usize) as u32, LEN_EXTRA[li] as u32);
            let di = DIST_BASE.iter().rposition(|&b| b as usize <= dist).unwrap();
            w.put_code(di as u32, 5);
            w.put((dist - DIST_BASE[di] as usize) as u32, DIST_EXTRA[di] as u32);
        }

        #[inline]
        fn hash3(d: &[u8], i: usize) -> usize {
            let v = (d[i] as u32) << 16 | (d[i + 1] as u32) << 8 | d[i + 2] as u32;
            (v.wrapping_mul(0x9E37_79B1) >> (32 - HASH_BITS)) as usize
        }

        pub fn deflate(data: &[u8]) -> Vec<u8> {
            let mut w = BitWriter { out: Vec::with_capacity(data.len() / 4 + 16), acc: 0, nbits: 0 };
            w.put(1, 1); // BFINAL
            w.put(1, 2); // BTYPE = 01 fixed Huffman
            let mut head = vec![usize::MAX; 1 << HASH_BITS];
            let mut prev = vec![usize::MAX; WINDOW];
            let insert = |head: &mut Vec<usize>, prev: &mut Vec<usize>, i: usize| {
                if i + MIN_MATCH <= data.len() {
                    let h = hash3(data, i);
                    prev[i % WINDOW] = head[h];
                    head[h] = i;
                }
            };
            let mut i = 0;
            while i < data.len() {
                let mut best_len = 0;
                let mut best_dist = 0;
                if i + MIN_MATCH <= data.len() {
                    let mut cand = head[hash3(data, i)];
                    let max_len = MAX_MATCH.min(data.len() - i);
                    let mut chain = 0;
                    while cand != usize::MAX && i - cand <= WINDOW && chain < MAX_CHAIN {
                        if data[cand + best_len.min(max_len - 1)] == data[i + best_len.min(max_len - 1)] {
                            let mut l = 0;
                            while l < max_len && data[cand + l] == data[i + l] {
                                l += 1;
                            }
                            if l > best_len {
                                best_len = l;
                                best_dist = i - cand;
                                if l == max_len {
                                    break;
                                }
                            }
                        }
                        let next = prev[cand % WINDOW];
                        if next == usize::MAX || next >= cand {
                            break;
                        }
                        cand = next;
                        chain += 1;
                    }
                }
                if best_len >= MIN_MATCH {
                    put_match(&mut w, best_len, best_dist);
                    for k in i..i + best_len {
                        insert(&mut head, &mut prev, k);
                    }
                    i += best_len;
                } else {
                    put_literal(&mut w, data[i] as u32);
                    insert(&mut head, &mut prev, i);
                    i += 1;
                }
            }
            put_literal(&mut w, 256);
            w.finish()
        }

        pub fn adler32(data: &[u8]) -> u32 {
            let (mut a, mut b) = (1u32, 0u32);
            for chunk in data.chunks(5552) {
                for &x in chunk {
                    a += x as u32;
                    b += a;
                }
                a %= 65521;
                b %= 65521;
            }
            (b << 16) | a
        }

        pub fn zlib_compress(data: &[u8]) -> Vec<u8> {
            let mut out = vec![0x78, 0x01];
            out.extend(deflate(data));
            out.extend_from_slice(&adler32(data).to_be_bytes());
            out
        }

        static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();

        pub fn crc32(data: &[u8]) -> u32 {
            crc32_update(0, data)
        }

        pub fn crc32_update(crc: u32, data: &[u8]) -> u32 {
            let table = CRC_TABLE.get_or_init(|| {
                let mut t = [0u32; 256];
                for (n, slot) in t.iter_mut().enumerate() {
                    let mut c = n as u32;
                    for _ in 0..8 {
                        c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
                    }
                    *slot = c;
                }
                t
            });
            let mut c = !crc;
            for &b in data {
                c = table[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8);
            }
            !c
        }
    }
    pub mod gif {
        //! Animated GIF89a encoder with an in-house LZW compressor.

        use std::collections::HashMap;

        use crate::renderer::Image;

        pub struct GifFrame<'a> {
            pub image: &'a Image,
            pub duration_ms: u32,
        }

        /// Alpha below this threshold becomes the GIF transparent index.
        const ALPHA_CUTOFF: u8 = 128;

        struct Indexed {
            table: Vec<[u8; 3]>, // index 0 is transparent
            pixels: Vec<u8>,
        }

        fn index_frame(img: &Image) -> Indexed {
            let mut freq: HashMap<[u8; 3], u32> = HashMap::new();
            for p in img.pixels.as_chunks::<4>().0 {
                if p[3] >= ALPHA_CUTOFF {
                    *freq.entry([p[0], p[1], p[2]]).or_default() += 1;
                }
            }
            let mut colors: Vec<([u8; 3], u32)> = freq.into_iter().collect();
            // Deterministic order: most frequent first, ties by value.
            colors.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            colors.truncate(255);
            let mut table = vec![[0u8; 3]];
            table.extend(colors.iter().map(|c| c.0));
            let mut lookup: HashMap<[u8; 3], u8> = table.iter().enumerate().skip(1).map(|(i, c)| (*c, i as u8)).collect();
            let mut pixels = Vec::with_capacity(img.pixels.len() / 4);
            for p in img.pixels.as_chunks::<4>().0 {
                if p[3] < ALPHA_CUTOFF {
                    pixels.push(0);
                    continue;
                }
                let key = [p[0], p[1], p[2]];
                let idx = *lookup.entry(key).or_insert_with(|| {
                    // More than 255 colors: map to the nearest kept color.
                    let mut best = (u32::MAX, 1u8);
                    for (i, c) in table.iter().enumerate().skip(1) {
                        let d = (0..3).map(|k| (c[k] as i32 - key[k] as i32).pow(2) as u32).sum::<u32>();
                        if d < best.0 {
                            best = (d, i as u8);
                        }
                    }
                    best.1
                });
                pixels.push(idx);
            }
            Indexed { table, pixels }
        }

        struct BitPacker {
            bytes: Vec<u8>,
            acc: u32,
            n: u32,
        }

        impl BitPacker {
            fn put(&mut self, code: u32, size: u32) {
                self.acc |= code << self.n;
                self.n += size;
                while self.n >= 8 {
                    self.bytes.push(self.acc as u8);
                    self.acc >>= 8;
                    self.n -= 8;
                }
            }
        }

        /// LZW as specified by GIF89a (variable code size, max 12 bits, clear on full table).
        pub fn lzw_encode(min_code_size: u32, pixels: &[u8]) -> Vec<u8> {
            let clear = 1u32 << min_code_size;
            let eoi = clear + 1;
            let mut size = min_code_size + 1;
            let mut next = eoi + 1;
            let mut dict: HashMap<u32, u32> = HashMap::new();
            let mut w = BitPacker { bytes: Vec::new(), acc: 0, n: 0 };
            let emit = |w: &mut BitPacker, code: u32, size: &mut u32, next: u32| {
                w.put(code, *size);
                if next >= (1 << *size) && *size < 12 {
                    *size += 1;
                }
            };
            emit(&mut w, clear, &mut size, next);
            let Some((&first, rest)) = pixels.split_first() else {
                emit(&mut w, eoi, &mut size, next);
                if w.n > 0 {
                    w.bytes.push(w.acc as u8);
                }
                return w.bytes;
            };
            let mut prefix = first as u32;
            for &k in rest {
                let key = (prefix << 8) | k as u32;
                if let Some(&code) = dict.get(&key) {
                    prefix = code;
                    continue;
                }
                emit(&mut w, prefix, &mut size, next);
                if next >= 4095 {
                    emit(&mut w, clear, &mut size, next);
                    dict.clear();
                    next = eoi + 1;
                    size = min_code_size + 1;
                } else {
                    dict.insert(key, next);
                    next += 1;
                }
                prefix = k as u32;
            }
            emit(&mut w, prefix, &mut size, next);
            emit(&mut w, eoi, &mut size, next);
            if w.n > 0 {
                w.bytes.push(w.acc as u8);
            }
            w.bytes
        }

        /// `loop_count` 0 = loop forever.
        pub fn encode(width: u32, height: u32, frames: &[GifFrame], loop_count: u16) -> Vec<u8> {
            let mut out = b"GIF89a".to_vec();
            out.extend_from_slice(&(width.min(65535) as u16).to_le_bytes());
            out.extend_from_slice(&(height.min(65535) as u16).to_le_bytes());
            out.extend_from_slice(&[0x00, 0x00, 0x00]); // no global color table
            // NETSCAPE2.0 application extension: looping.
            out.extend_from_slice(&[0x21, 0xFF, 0x0B]);
            out.extend_from_slice(b"NETSCAPE2.0");
            out.extend_from_slice(&[0x03, 0x01]);
            out.extend_from_slice(&loop_count.to_le_bytes());
            out.push(0x00);
            for frame in frames {
                let indexed = index_frame(frame.image);
                let bits = (1..=8).find(|b| (1usize << b) >= indexed.table.len()).unwrap_or(8);
                let delay = ((frame.duration_ms + 5) / 10).clamp(2, 65535) as u16;
                // Graphic control extension: disposal 2 (restore background), transparent index 0.
                out.extend_from_slice(&[0x21, 0xF9, 0x04, (2 << 2) | 0x01]);
                out.extend_from_slice(&delay.to_le_bytes());
                out.extend_from_slice(&[0x00, 0x00]);
                // Image descriptor with a local color table.
                out.push(0x2C);
                out.extend_from_slice(&[0, 0, 0, 0]);
                out.extend_from_slice(&(frame.image.width.min(65535) as u16).to_le_bytes());
                out.extend_from_slice(&(frame.image.height.min(65535) as u16).to_le_bytes());
                out.push(0x80 | (bits as u8 - 1));
                for i in 0..(1usize << bits) {
                    out.extend_from_slice(indexed.table.get(i).unwrap_or(&[0, 0, 0]));
                }
                let min_code = bits.max(2) as u32;
                out.push(min_code as u8);
                for block in lzw_encode(min_code, &indexed.pixels).chunks(255) {
                    out.push(block.len() as u8);
                    out.extend_from_slice(block);
                }
                out.push(0x00);
            }
            out.push(0x3B);
            out
        }

        #[cfg(test)]
        pub(crate) fn lzw_decode(min_code_size: u32, data: &[u8]) -> Vec<u8> {
            let clear = 1u32 << min_code_size;
            let eoi = clear + 1;
            let (mut pos, mut acc, mut n) = (0usize, 0u32, 0u32);
            let mut size = min_code_size + 1;
            let mut table: Vec<Vec<u8>> = Vec::new();
            let reset = |t: &mut Vec<Vec<u8>>| {
                t.clear();
                for i in 0..clear {
                    t.push(vec![i as u8]);
                }
                t.push(vec![]);
                t.push(vec![]);
            };
            reset(&mut table);
            let mut out = Vec::new();
            let mut prev: Option<Vec<u8>> = None;
            loop {
                while n < size {
                    acc |= (data[pos] as u32) << n;
                    pos += 1;
                    n += 8;
                }
                let code = acc & ((1 << size) - 1);
                acc >>= size;
                n -= size;
                if code == clear {
                    reset(&mut table);
                    size = min_code_size + 1;
                    prev = None;
                    continue;
                }
                if code == eoi {
                    return out;
                }
                let entry = if (code as usize) < table.len() {
                    table[code as usize].clone()
                } else {
                    let p = prev.clone().unwrap();
                    let mut e = p.clone();
                    e.push(p[0]);
                    e
                };
                out.extend_from_slice(&entry);
                if let Some(p) = prev {
                    let mut e = p;
                    e.push(entry[0]);
                    table.push(e);
                    if table.len() >= (1 << size) && size < 12 {
                        size += 1;
                    }
                }
                prev = Some(entry);
            }
        }
    }
    pub mod inflate {
        //! DEFLATE / zlib decoder (RFC 1950/1951) with strict bounds checks — used for PNG import,
        //! texture import and compressed project data. Malformed input returns an error; it never panics.

        const LEN_BASE: [u16; 29] =
            [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
        const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
        const DIST_BASE: [u16; 30] = [
            1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289,
            16385, 24577,
        ];
        const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

        struct Bits<'a> {
            d: &'a [u8],
            pos: usize,
            acc: u64,
            n: u32,
        }

        impl Bits<'_> {
            fn need(&mut self, k: u32) -> Result<(), String> {
                while self.n < k {
                    let b = *self.d.get(self.pos).ok_or("unexpected end of compressed data")?;
                    self.pos += 1;
                    self.acc |= (b as u64) << self.n;
                    self.n += 8;
                }
                Ok(())
            }

            fn get(&mut self, k: u32) -> Result<u32, String> {
                if k == 0 {
                    return Ok(0);
                }
                self.need(k)?;
                let v = (self.acc & ((1u64 << k) - 1)) as u32;
                self.acc >>= k;
                self.n -= k;
                Ok(v)
            }

            fn align(&mut self) {
                let drop = self.n % 8;
                self.acc >>= drop;
                self.n -= drop;
            }
        }

        struct Huffman {
            counts: [u16; 16],
            symbols: Vec<u16>,
        }

        impl Huffman {
            fn new(lengths: &[u8]) -> Result<Huffman, String> {
                let mut counts = [0u16; 16];
                for &l in lengths {
                    if l > 15 {
                        return Err("invalid code length".into());
                    }
                    counts[l as usize] += 1;
                }
                counts[0] = 0;
                let mut left: i32 = 1;
                for &c in counts.iter().skip(1) {
                    left = (left << 1) - c as i32;
                    if left < 0 {
                        return Err("over-subscribed Huffman code".into());
                    }
                }
                let mut offs = [0u16; 16];
                for i in 1..16 {
                    offs[i] = offs[i - 1] + counts[i - 1];
                }
                let mut symbols = vec![0u16; lengths.len()];
                for (s, &l) in lengths.iter().enumerate() {
                    if l != 0 {
                        symbols[offs[l as usize] as usize] = s as u16;
                        offs[l as usize] += 1;
                    }
                }
                Ok(Huffman { counts, symbols })
            }

            fn decode(&self, b: &mut Bits) -> Result<u16, String> {
                let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
                for len in 1..16 {
                    code |= b.get(1)? as i32;
                    let count = self.counts[len] as i32;
                    if code - count < first {
                        return self.symbols.get((index + code - first) as usize).copied().ok_or_else(|| "bad Huffman symbol".to_string());
                    }
                    index += count;
                    first += count;
                    first <<= 1;
                    code <<= 1;
                }
                Err("invalid Huffman code".into())
            }
        }

        /// Inflates raw DEFLATE data, refusing to produce more than `limit` bytes.
        pub fn inflate(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
            let mut b = Bits { d: data, pos: 0, acc: 0, n: 0 };
            let mut out: Vec<u8> = Vec::new();
            loop {
                let last = b.get(1)?;
                match b.get(2)? {
                    0 => {
                        b.align();
                        let len = b.get(16)? as usize;
                        let nlen = b.get(16)? as usize;
                        if len != (!nlen & 0xFFFF) {
                            return Err("stored block length mismatch".into());
                        }
                        if out.len() + len > limit {
                            return Err("decompressed data too large".into());
                        }
                        for _ in 0..len {
                            out.push(b.get(8)? as u8);
                        }
                    }
                    t @ (1 | 2) => {
                        let (lit, dist) = if t == 1 {
                            let mut l = [0u8; 288];
                            l[..144].fill(8);
                            l[144..256].fill(9);
                            l[256..280].fill(7);
                            l[280..].fill(8);
                            (Huffman::new(&l)?, Huffman::new(&[5u8; 30])?)
                        } else {
                            let hlit = b.get(5)? as usize + 257;
                            let hdist = b.get(5)? as usize + 1;
                            let hclen = b.get(4)? as usize + 4;
                            const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
                            let mut cl = [0u8; 19];
                            for &o in ORDER.iter().take(hclen) {
                                cl[o] = b.get(3)? as u8;
                            }
                            let ch = Huffman::new(&cl)?;
                            let mut lens: Vec<u8> = Vec::with_capacity(hlit + hdist);
                            while lens.len() < hlit + hdist {
                                let (value, repeat) = match ch.decode(&mut b)? {
                                    s @ 0..=15 => (s as u8, 1),
                                    16 => (*lens.last().ok_or("repeat without previous length")?, 3 + b.get(2)? as usize),
                                    17 => (0, 3 + b.get(3)? as usize),
                                    _ => (0, 11 + b.get(7)? as usize),
                                };
                                if lens.len() + repeat > hlit + hdist {
                                    return Err("code lengths overflow".into());
                                }
                                lens.extend(std::iter::repeat_n(value, repeat));
                            }
                            (Huffman::new(&lens[..hlit])?, Huffman::new(&lens[hlit..])?)
                        };
                        loop {
                            let sym = lit.decode(&mut b)? as usize;
                            if sym < 256 {
                                if out.len() >= limit {
                                    return Err("decompressed data too large".into());
                                }
                                out.push(sym as u8);
                            } else if sym == 256 {
                                break;
                            } else {
                                let li = sym - 257;
                                if li >= 29 {
                                    return Err("invalid length symbol".into());
                                }
                                let len = LEN_BASE[li] as usize + b.get(LEN_EXTRA[li] as u32)? as usize;
                                let di = dist.decode(&mut b)? as usize;
                                if di >= 30 {
                                    return Err("invalid distance symbol".into());
                                }
                                let d = DIST_BASE[di] as usize + b.get(DIST_EXTRA[di] as u32)? as usize;
                                if d > out.len() {
                                    return Err("distance too far back".into());
                                }
                                if out.len() + len > limit {
                                    return Err("decompressed data too large".into());
                                }
                                let start = out.len() - d;
                                for k in 0..len {
                                    out.push(out[start + k]);
                                }
                            }
                        }
                    }
                    _ => return Err("invalid block type".into()),
                }
                if last == 1 {
                    return Ok(out);
                }
            }
        }

        pub fn zlib_decompress(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
            if data.len() < 6 || (data[0] & 0x0F) != 8 || !((data[0] as u16) << 8 | data[1] as u16).is_multiple_of(31) {
                return Err("not a zlib stream".into());
            }
            if data[1] & 0x20 != 0 {
                return Err("zlib preset dictionaries are not supported".into());
            }
            let out = inflate(&data[2..], limit)?;
            let expected = u32::from_be_bytes(data[data.len() - 4..].try_into().unwrap());
            if super::deflate::adler32(&out) != expected {
                return Err("zlib checksum mismatch".into());
            }
            Ok(out)
        }
    }
    pub mod png {
        //! PNG encoder (RGBA8, non-interlaced) built on the in-house zlib encoder.

        use super::deflate::{crc32_update, zlib_compress};
        use crate::renderer::Image;

        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            out.extend_from_slice(kind);
            out.extend_from_slice(data);
            let crc = crc32_update(crc32_update(0, kind), data);
            out.extend_from_slice(&crc.to_be_bytes());
        }

        fn paeth(a: u8, b: u8, c: u8) -> u8 {
            let p = a as i16 + b as i16 - c as i16;
            let (pa, pb, pc) = ((p - a as i16).abs(), (p - b as i16).abs(), (p - c as i16).abs());
            if pa <= pb && pa <= pc {
                a
            } else if pb <= pc {
                b
            } else {
                c
            }
        }

        /// Encodes with a per-row filter chosen by the minimum-sum-of-absolute-differences heuristic.
        pub fn encode(img: &Image) -> Vec<u8> {
            let stride = img.width as usize * 4;
            let mut raw = Vec::with_capacity((stride + 1) * img.height as usize);
            let zero = vec![0u8; stride];
            let mut candidates: [Vec<u8>; 5] = Default::default();
            for y in 0..img.height as usize {
                let row = &img.pixels[y * stride..(y + 1) * stride];
                let up = if y == 0 { &zero[..] } else { &img.pixels[(y - 1) * stride..y * stride] };
                for (f, cand) in candidates.iter_mut().enumerate() {
                    cand.clear();
                    for i in 0..stride {
                        let a = if i >= 4 { row[i - 4] } else { 0 };
                        let b = up[i];
                        let c = if i >= 4 { up[i - 4] } else { 0 };
                        let pred = match f {
                            0 => 0,
                            1 => a,
                            2 => b,
                            3 => ((a as u16 + b as u16) / 2) as u8,
                            _ => paeth(a, b, c),
                        };
                        cand.push(row[i].wrapping_sub(pred));
                    }
                }
                let score = |v: &Vec<u8>| v.iter().map(|&x| (x as i8).unsigned_abs() as u32).sum::<u32>();
                let best = (0..5).min_by_key(|&f| score(&candidates[f])).unwrap();
                raw.push(best as u8);
                raw.extend_from_slice(&candidates[best]);
            }
            let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            let mut ihdr = Vec::with_capacity(13);
            ihdr.extend_from_slice(&img.width.to_be_bytes());
            ihdr.extend_from_slice(&img.height.to_be_bytes());
            ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit, RGBA, deflate, adaptive filter, no interlace
            chunk(&mut out, b"IHDR", &ihdr);
            chunk(&mut out, b"tEXt", b"Software\0Pixel Studio");
            chunk(&mut out, b"IDAT", &zlib_compress(&raw));
            chunk(&mut out, b"IEND", &[]);
            out
        }

        /// Decodes a PNG (8/16-bit, grayscale/RGB/palette/alpha, non-interlaced) into RGBA8.
        pub fn decode(png: &[u8]) -> Result<Image, String> {
            const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
            if png.len() < 8 || png[..8] != SIG {
                return Err("not a PNG file".into());
            }
            let mut pos = 8;
            let (mut w, mut h, mut depth, mut ctype, mut interlace) = (0u32, 0u32, 0u8, 0u8, 0u8);
            let mut plte: Vec<[u8; 3]> = Vec::new();
            let mut trns: Vec<u8> = Vec::new();
            let mut idat = Vec::new();
            while pos + 12 <= png.len() {
                let len = u32::from_be_bytes(png[pos..pos + 4].try_into().unwrap()) as usize;
                let kind = &png[pos + 4..pos + 8];
                let end = pos.checked_add(12 + len).filter(|&e| e <= png.len()).ok_or("truncated PNG chunk")?;
                let data = &png[pos + 8..pos + 8 + len];
                let crc = u32::from_be_bytes(png[end - 4..end].try_into().unwrap());
                if crc != crc32_update(crc32_update(0, kind), data) {
                    return Err("PNG chunk checksum mismatch".into());
                }
                match kind {
                    b"IHDR" if len == 13 => {
                        w = u32::from_be_bytes(data[0..4].try_into().unwrap());
                        h = u32::from_be_bytes(data[4..8].try_into().unwrap());
                        depth = data[8];
                        ctype = data[9];
                        interlace = data[12];
                    }
                    b"PLTE" => plte = data.as_chunks::<3>().0.iter().map(|c| [c[0], c[1], c[2]]).collect(),
                    b"tRNS" => trns = data.to_vec(),
                    b"IDAT" => idat.extend_from_slice(data),
                    b"IEND" => break,
                    _ => {}
                }
                pos = end;
            }
            if w == 0 || h == 0 || w > 8192 || h > 8192 {
                return Err("unsupported PNG size (max 8192x8192)".into());
            }
            if interlace != 0 {
                return Err("interlaced PNGs are not supported; re-save without interlacing".into());
            }
            let channels = match ctype {
                0 => 1,
                2 => 3,
                3 => 1,
                4 => 2,
                6 => 4,
                _ => return Err("unsupported PNG color type".into()),
            };
            if !matches!(depth, 1 | 2 | 4 | 8 | 16) || (depth < 8 && !matches!(ctype, 0 | 3)) || (depth == 16 && ctype == 3) {
                return Err("unsupported PNG bit depth".into());
            }
            let bits_pp = channels * depth as usize;
            let stride = (w as usize * bits_pp).div_ceil(8);
            let bpp = bits_pp.div_ceil(8).max(1);
            let raw = crate::export::inflate::zlib_decompress(&idat, (stride + 1) * h as usize)?;
            if raw.len() != (stride + 1) * h as usize {
                return Err("PNG image data has the wrong size".into());
            }
            let mut cur = vec![0u8; stride];
            let mut prev = vec![0u8; stride];
            let mut img = Image::new(w, h);
            for y in 0..h as usize {
                let f = raw[y * (stride + 1)];
                let line = &raw[y * (stride + 1) + 1..(y + 1) * (stride + 1)];
                for i in 0..stride {
                    let a = if i >= bpp { cur[i - bpp] } else { 0 };
                    let b = prev[i];
                    let c = if i >= bpp { prev[i - bpp] } else { 0 };
                    let pred = match f {
                        0 => 0,
                        1 => a,
                        2 => b,
                        3 => ((a as u16 + b as u16) / 2) as u8,
                        4 => paeth(a, b, c),
                        _ => return Err("invalid PNG filter".into()),
                    };
                    cur[i] = line[i].wrapping_add(pred);
                }
                for x in 0..w as usize {
                    let sample = |ch: usize| -> u8 {
                        match depth {
                            8 => cur[x * channels + ch],
                            16 => cur[(x * channels + ch) * 2],
                            d => {
                                let bit = x * d as usize;
                                let v = (cur[bit / 8] >> (8 - d as usize - bit % 8)) & ((1u8 << d) - 1);
                                if ctype == 3 { v } else { (v as u32 * 255 / ((1u32 << d) - 1)) as u8 }
                            }
                        }
                    };
                    let px = match ctype {
                        0 => {
                            let g = sample(0);
                            let transparent = depth == 8 && trns.len() >= 2 && trns[1] == g;
                            [g, g, g, if transparent { 0 } else { 255 }]
                        }
                        2 => [sample(0), sample(1), sample(2), 255],
                        3 => {
                            let i = sample(0) as usize;
                            let c = plte.get(i).copied().ok_or("PNG palette index out of range")?;
                            [c[0], c[1], c[2], trns.get(i).copied().unwrap_or(255)]
                        }
                        4 => {
                            let g = sample(0);
                            [g, g, g, sample(1)]
                        }
                        _ => [sample(0), sample(1), sample(2), sample(3)],
                    };
                    let o = (y * w as usize + x) * 4;
                    img.pixels[o..o + 4].copy_from_slice(&px);
                }
                std::mem::swap(&mut cur, &mut prev);
            }
            Ok(img)
        }
    }

    use crate::commands::json::Json;
    use crate::renderer::Image;

    pub struct SheetFrame<'a> {
        pub index: usize,
        pub image: &'a Image,
        pub duration_ms: u32,
    }

    pub struct SpriteSheet {
        pub image: Image,
        /// Engine-neutral metadata (frame rectangles, durations) consumable by Godot/Unity importers.
        pub metadata: Json,
    }

    /// Packs equally sized frames into a grid. `columns = 0` picks a near-square layout.
    pub fn sprite_sheet(frames: &[SheetFrame], columns: usize, padding: u32) -> Result<SpriteSheet, String> {
        let Some(first) = frames.first() else { return Err("no frames to pack".into()) };
        let (fw, fh) = (first.image.width, first.image.height);
        let n = frames.len();
        let cols = if columns == 0 { (n as f64).sqrt().ceil() as usize } else { columns.min(n) }.max(1);
        let rows = n.div_ceil(cols);
        let w = cols as u64 * (fw + padding) as u64 - padding as u64;
        let h = rows as u64 * (fh + padding) as u64 - padding as u64;
        if w > 32_768 || h > 32_768 {
            return Err(format!("sprite sheet would be {w}x{h}; use more/less columns or a smaller scale"));
        }
        let mut sheet = Image::new(w as u32, h as u32);
        let mut rects = Vec::with_capacity(n);
        for (i, f) in frames.iter().enumerate() {
            let x = (i % cols) as u32 * (fw + padding);
            let y = (i / cols) as u32 * (fh + padding);
            sheet.blit(f.image, x, y);
            rects.push(
                Json::obj().with("frame", f.index).with("x", x).with("y", y).with("w", fw).with("h", fh).with("duration", f.duration_ms),
            );
        }
        let metadata = Json::obj()
            .with("image", Json::obj().with("width", w).with("height", h))
            .with("frameSize", Json::obj().with("w", fw).with("h", fh))
            .with("columns", cols)
            .with("rows", rows)
            .with("padding", padding)
            .with("frames", Json::Arr(rects));
        Ok(SpriteSheet { image: sheet, metadata })
    }
}
pub mod ffi {
    //! Hand-written JNI bridge (no `jni` crate).
    //!
    //! Kotlin side: `com.pixel.studio.bridge.NativeBridge`
    //! ```kotlin
    //! external fun abiVersion(): Int
    //! external fun create(width: Int, height: Int): Long
    //! external fun destroy(handle: Long)
    //! external fun revision(handle: Long): Long
    //! external fun call(handle: Long, request: ByteArray, payload: ByteArray?): ByteArray
    //! ```
    //! Safety rules:
    //! * Handles are opaque registry ids, never raw pointers: a stale or forged handle is
    //!   reported as an error instead of dereferencing freed memory.
    //! * Every entry point catches panics; unwinding never crosses into the JVM.
    //! * Only four JNIEnv functions are used, all with bounds-checked lengths.
    //! * Engines are guarded by a mutex: one writer at a time (the single-writer queue).
    //!
    //! `call` responses are framed: `u32 LE json length | UTF-8 JSON | binary payload`.

    // JNI entry points receive raw pointers from the JVM by definition.
    #![allow(clippy::not_unsafe_ptr_arg_deref)]

    pub mod jni {
        //! The minimal subset of the JNI C ABI that Pixel Studio needs.
        //!
        //! `JNIEnv*` points to a pointer to the JNINativeInterface function table. Function slots
        //! are fixed by the JNI specification ("JNI Functions" chapter, interface function table),
        //! which has been append-only since JNI 1.1.

        #![allow(non_camel_case_types)]

        use std::ffi::c_void;

        pub type jint = i32;
        pub type jlong = i64;
        pub type jsize = jint;
        pub type jbyte = i8;
        pub type jobject = *mut c_void;
        pub type jarray = jobject;
        pub type jbyteArray = jarray;

        /// Opaque; only ever used behind a pointer.
        #[repr(C)]
        pub struct JNIEnv {
            functions: *const *const c_void,
        }

        const GET_ARRAY_LENGTH: usize = 171;
        const NEW_BYTE_ARRAY: usize = 176;
        const GET_BYTE_ARRAY_REGION: usize = 200;
        const SET_BYTE_ARRAY_REGION: usize = 208;
        const EXCEPTION_CHECK: usize = 228;

        type GetArrayLength = unsafe extern "system" fn(*mut JNIEnv, jarray) -> jsize;
        type NewByteArray = unsafe extern "system" fn(*mut JNIEnv, jsize) -> jbyteArray;
        type GetByteArrayRegion = unsafe extern "system" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *mut jbyte);
        type SetByteArrayRegion = unsafe extern "system" fn(*mut JNIEnv, jbyteArray, jsize, jsize, *const jbyte);
        type ExceptionCheck = unsafe extern "system" fn(*mut JNIEnv) -> u8;

        /// Looks up a function slot in the table.
        ///
        /// # Safety
        /// `env` must be a valid JNIEnv pointer for the current thread and `F` must be the
        /// exact function type of `slot`.
        unsafe fn slot<F: Copy>(env: *mut JNIEnv, slot: usize) -> F {
            unsafe {
                let table = (*env).functions;
                let f: *const c_void = *table.add(slot);
                std::mem::transmute_copy::<*const c_void, F>(&f)
            }
        }

        /// Copies a Java `byte[]` into a Rust vector. `None` for a null array.
        ///
        /// # Safety
        /// `env` must be valid for this thread and `array` a live local/global reference or null.
        pub unsafe fn read_byte_array(env: *mut JNIEnv, array: jbyteArray) -> Option<Vec<u8>> {
            if env.is_null() || array.is_null() {
                return None;
            }
            unsafe {
                let len = slot::<GetArrayLength>(env, GET_ARRAY_LENGTH)(env, array);
                if len <= 0 {
                    return Some(Vec::new());
                }
                let mut buf = vec![0u8; len as usize];
                slot::<GetByteArrayRegion>(env, GET_BYTE_ARRAY_REGION)(env, array, 0, len, buf.as_mut_ptr() as *mut jbyte);
                if slot::<ExceptionCheck>(env, EXCEPTION_CHECK)(env) != 0 {
                    return None;
                }
                Some(buf)
            }
        }

        /// Allocates a Java `byte[]` holding `data`. Returns null (with a pending Java exception,
        /// e.g. OutOfMemoryError) if allocation fails.
        ///
        /// # Safety
        /// `env` must be valid for this thread.
        pub unsafe fn new_byte_array(env: *mut JNIEnv, data: &[u8]) -> jbyteArray {
            if env.is_null() || data.len() > i32::MAX as usize {
                return std::ptr::null_mut();
            }
            unsafe {
                let array = slot::<NewByteArray>(env, NEW_BYTE_ARRAY)(env, data.len() as jsize);
                if array.is_null() {
                    return array;
                }
                slot::<SetByteArrayRegion>(env, SET_BYTE_ARRAY_REGION)(env, array, 0, data.len() as jsize, data.as_ptr() as *const jbyte);
                array
            }
        }
    }

    use std::collections::HashMap;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{Arc, Mutex, OnceLock};

    use crate::engine::Engine;
    use jni::{JNIEnv, jbyteArray, jint, jlong, jobject};

    /// Bumped whenever the native interface changes shape.
    pub const ABI_VERSION: i32 = 1;

    type Registry = Mutex<HashMap<u64, Arc<Mutex<Engine>>>>;

    fn registry() -> &'static Registry {
        static REG: OnceLock<Registry> = OnceLock::new();
        REG.get_or_init(|| Mutex::new(HashMap::new()))
    }

    static NEXT_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    pub fn create_engine(width: u32, height: u32) -> u64 {
        let handle = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut reg = registry().lock().unwrap_or_else(|p| p.into_inner());
        reg.insert(handle, Arc::new(Mutex::new(Engine::new(width, height))));
        handle
    }

    pub fn destroy_engine(handle: u64) -> bool {
        registry().lock().unwrap_or_else(|p| p.into_inner()).remove(&handle).is_some()
    }

    pub fn with_engine<R>(handle: u64, f: impl FnOnce(&mut Engine) -> R) -> Option<R> {
        let engine = registry().lock().unwrap_or_else(|p| p.into_inner()).get(&handle).cloned()?;
        let mut guard = engine.lock().unwrap_or_else(|p| p.into_inner());
        Some(f(&mut guard))
    }

    pub fn frame_response(json: &str, binary: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + json.len() + binary.len());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(json.as_bytes());
        out.extend_from_slice(binary);
        out
    }

    fn error_frame(code: &str, message: &str) -> Vec<u8> {
        let json = crate::commands::json::Json::obj()
            .with("ok", false)
            .with("error", crate::commands::json::Json::obj().with("code", code).with("message", message))
            .to_json_string();
        frame_response(&json, &[])
    }

    /// Executes a request against an engine handle and returns the framed response.
    pub fn call(handle: u64, request: &[u8], payload: &[u8]) -> Vec<u8> {
        let result = catch_unwind(AssertUnwindSafe(|| with_engine(handle, |e| e.execute(request, payload))));
        match result {
            Ok(Some(r)) => frame_response(&r.json, &r.binary),
            Ok(None) => error_frame("invalid_handle", "engine handle is not valid (destroyed or never created)"),
            Err(_) => error_frame("internal", "engine panicked; the operation was not applied"),
        }
    }

    // ---- JNI exports -------------------------------------------------------------------------

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_pixel_studio_gateway_NativeBridge_abiVersion(_env: *mut JNIEnv, _this: jobject) -> jint {
        ABI_VERSION
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_pixel_studio_gateway_NativeBridge_create(
        _env: *mut JNIEnv,
        _this: jobject,
        width: jint,
        height: jint,
    ) -> jlong {
        catch_unwind(|| match crate::project::validate_size(width as i64, height as i64) {
            Ok((w, h)) => create_engine(w, h) as jlong,
            Err(_) => 0,
        })
        .unwrap_or(0)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_pixel_studio_gateway_NativeBridge_destroy(_env: *mut JNIEnv, _this: jobject, handle: jlong) {
        let _ = catch_unwind(|| destroy_engine(handle as u64));
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_pixel_studio_gateway_NativeBridge_revision(_env: *mut JNIEnv, _this: jobject, handle: jlong) -> jlong {
        catch_unwind(|| with_engine(handle as u64, |e| e.revision() as jlong).unwrap_or(-1)).unwrap_or(-1)
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn Java_com_pixel_studio_gateway_NativeBridge_call(
        env: *mut JNIEnv,
        _this: jobject,
        handle: jlong,
        request: jbyteArray,
        payload: jbyteArray,
    ) -> jbyteArray {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: `env` is the JNIEnv pointer the JVM passed to this native method and
            // the arrays are local references valid for the duration of this call.
            unsafe {
                let req = jni::read_byte_array(env, request).unwrap_or_default();
                let pay = jni::read_byte_array(env, payload).unwrap_or_default();
                let response = call(handle as u64, &req, &pay);
                jni::new_byte_array(env, &response)
            }
        }));
        outcome.unwrap_or(std::ptr::null_mut())
    }
}
pub mod gamedev {
    //! Game-production features: slices (hitboxes, pivots), texture atlas packing, tilesets and
    //! tilemaps with 47-tile blob autotiling, normal-mapped 2D lighting and RotSprite rotation.

    pub mod atlas {
        //! Texture atlas packing: trim transparent borders, de-duplicate identical sprites and pack
        //! with MaxRects (best short side fit), padding and edge extrusion against texture bleeding.

        use crate::renderer::Image;

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Rect {
            pub x: u32,
            pub y: u32,
            pub w: u32,
            pub h: u32,
        }

        /// Where a source sprite ended up.
        #[derive(Debug, Clone, PartialEq)]
        pub struct Placement {
            /// Rectangle in the atlas.
            pub frame: Rect,
            /// Offset of the trimmed rectangle inside the original sprite.
            pub offset_x: u32,
            pub offset_y: u32,
            pub source_w: u32,
            pub source_h: u32,
            pub trimmed: bool,
            /// Index of the sprite whose pixels this entry reuses (duplicates).
            pub same_as: Option<usize>,
        }

        pub struct Atlas {
            pub image: Image,
            pub placements: Vec<Placement>,
        }

        /// Opaque bounds of an image, or None when fully transparent.
        pub fn trim_bounds(img: &Image) -> Option<Rect> {
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
            for y in 0..img.height {
                for x in 0..img.width {
                    if img.pixels[((y * img.width + x) * 4 + 3) as usize] > 0 {
                        x0 = x0.min(x);
                        y0 = y0.min(y);
                        x1 = x1.max(x);
                        y1 = y1.max(y);
                    }
                }
            }
            (x0 != u32::MAX).then(|| Rect { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 })
        }

        fn crop(img: &Image, r: Rect) -> Image {
            let mut out = Image::new(r.w, r.h);
            for y in 0..r.h {
                let s = (((r.y + y) * img.width + r.x) * 4) as usize;
                let d = (y * r.w * 4) as usize;
                out.pixels[d..d + (r.w * 4) as usize].copy_from_slice(&img.pixels[s..s + (r.w * 4) as usize]);
            }
            out
        }

        struct MaxRects {
            free: Vec<Rect>,
        }

        impl MaxRects {
            fn new(w: u32, h: u32) -> MaxRects {
                MaxRects { free: vec![Rect { x: 0, y: 0, w, h }] }
            }

            fn insert(&mut self, w: u32, h: u32) -> Option<Rect> {
                let mut best: Option<(u32, u32, Rect)> = None;
                for f in &self.free {
                    if w <= f.w && h <= f.h {
                        let short = (f.w - w).min(f.h - h);
                        let long = (f.w - w).max(f.h - h);
                        if best.is_none_or(|(s, l, _)| (short, long) < (s, l)) {
                            best = Some((short, long, Rect { x: f.x, y: f.y, w, h }));
                        }
                    }
                }
                let (_, _, r) = best?;
                let mut next = Vec::with_capacity(self.free.len() + 4);
                for f in self.free.drain(..) {
                    let overlaps = r.x < f.x + f.w && r.x + r.w > f.x && r.y < f.y + f.h && r.y + r.h > f.y;
                    if !overlaps {
                        next.push(f);
                        continue;
                    }
                    if r.x > f.x {
                        next.push(Rect { x: f.x, y: f.y, w: r.x - f.x, h: f.h });
                    }
                    if r.x + r.w < f.x + f.w {
                        next.push(Rect { x: r.x + r.w, y: f.y, w: f.x + f.w - r.x - r.w, h: f.h });
                    }
                    if r.y > f.y {
                        next.push(Rect { x: f.x, y: f.y, w: f.w, h: r.y - f.y });
                    }
                    if r.y + r.h < f.y + f.h {
                        next.push(Rect { x: f.x, y: r.y + r.h, w: f.w, h: f.y + f.h - r.y - r.h });
                    }
                }
                // Drop free rectangles contained in others.
                let mut pruned: Vec<Rect> = Vec::with_capacity(next.len());
                for (i, a) in next.iter().enumerate() {
                    let contained = next.iter().enumerate().any(|(j, b)| {
                        i != j && a.x >= b.x && a.y >= b.y && a.x + a.w <= b.x + b.w && a.y + a.h <= b.y + b.h && (a != b || j < i)
                    });
                    if !contained {
                        pruned.push(*a);
                    }
                }
                self.free = pruned;
                Some(r)
            }
        }

        /// Packs sprites into one atlas. `padding` = transparent gap, `extrude` = border pixels copied
        /// outward around each sprite (prevents bleeding with filtering or sub-pixel cameras).
        pub fn pack(sprites: &[Image], trim: bool, padding: u32, extrude: u32, power_of_two: bool) -> Result<Atlas, String> {
            let mut items: Vec<(usize, Image, Rect)> = Vec::new();
            let mut placements: Vec<Option<Placement>> = vec![None; sprites.len()];
            let mut seen: Vec<(usize, Rect, Image)> = Vec::new();
            for (i, img) in sprites.iter().enumerate() {
                let r = if trim {
                    trim_bounds(img).unwrap_or(Rect { x: 0, y: 0, w: 1, h: 1 })
                } else {
                    Rect { x: 0, y: 0, w: img.width, h: img.height }
                };
                let c = crop(img, r);
                if let Some((j, _, _)) = seen.iter().find(|(_, rr, ci)| *rr == r && *ci == c) {
                    placements[i] = Some(Placement {
                        frame: Rect { x: 0, y: 0, w: 0, h: 0 },
                        offset_x: r.x,
                        offset_y: r.y,
                        source_w: img.width,
                        source_h: img.height,
                        trimmed: trim,
                        same_as: Some(*j),
                    });
                    continue;
                }
                seen.push((i, r, c.clone()));
                items.push((i, c, r));
            }
            let gap = padding + extrude * 2;
            let area: u64 = items.iter().map(|(_, c, _)| (c.width + gap) as u64 * (c.height + gap) as u64).sum();
            let widest = items.iter().map(|(_, c, _)| c.width + gap).max().unwrap_or(1);
            let mut side = ((area as f64 * 1.1).sqrt().ceil() as u32).max(widest).max(1);
            if power_of_two {
                side = side.next_power_of_two();
            }
            let mut order: Vec<usize> = (0..items.len()).collect();
            order.sort_by_key(|&k| std::cmp::Reverse((items[k].1.height, items[k].1.width)));
            loop {
                if side > 16384 {
                    return Err("sprites do not fit in a 16384×16384 atlas".into());
                }
                let mut mr = MaxRects::new(side, side);
                let mut spots = vec![Rect { x: 0, y: 0, w: 0, h: 0 }; items.len()];
                let mut ok = true;
                for &k in &order {
                    let (_, c, _) = &items[k];
                    match mr.insert(c.width + gap, c.height + gap) {
                        Some(r) => spots[k] = r,
                        None => {
                            ok = false;
                            break;
                        }
                    }
                }
                if !ok {
                    side = if power_of_two { side * 2 } else { side + side / 4 + 1 };
                    continue;
                }
                // Shrink to the used area.
                let used_w = spots.iter().map(|r| r.x + r.w).max().unwrap_or(1);
                let used_h = spots.iter().map(|r| r.y + r.h).max().unwrap_or(1);
                let (aw, ah) = if power_of_two { (used_w.next_power_of_two(), used_h.next_power_of_two()) } else { (used_w, used_h) };
                let mut atlas = Image::new(aw, ah);
                for (k, (i, c, r)) in items.iter().enumerate() {
                    let s = spots[k];
                    let (dx, dy) = (s.x + extrude, s.y + extrude);
                    atlas.blit(c, dx, dy);
                    if extrude > 0 {
                        for e in 1..=extrude {
                            for y in 0..c.height {
                                copy_px(&mut atlas, c, 0, y, dx - e, dy + y);
                                copy_px(&mut atlas, c, c.width - 1, y, dx + c.width - 1 + e, dy + y);
                            }
                            for x in 0..c.width {
                                copy_px(&mut atlas, c, x, 0, dx + x, dy - e);
                                copy_px(&mut atlas, c, x, c.height - 1, dx + x, dy + c.height - 1 + e);
                            }
                        }
                    }
                    placements[*i] = Some(Placement {
                        frame: Rect { x: dx, y: dy, w: c.width, h: c.height },
                        offset_x: r.x,
                        offset_y: r.y,
                        source_w: sprites[*i].width,
                        source_h: sprites[*i].height,
                        trimmed: trim,
                        same_as: None,
                    });
                }
                let mut out: Vec<Placement> = placements.into_iter().map(|p| p.expect("placed")).collect();
                for i in 0..out.len() {
                    if let Some(j) = out[i].same_as {
                        out[i].frame = out[j].frame;
                    }
                }
                return Ok(Atlas { image: atlas, placements: out });
            }
        }

        fn copy_px(dst: &mut Image, src: &Image, sx: u32, sy: u32, dx: u32, dy: u32) {
            if dx >= dst.width || dy >= dst.height {
                return;
            }
            let s = ((sy * src.width + sx) * 4) as usize;
            let d = ((dy * dst.width + dx) * 4) as usize;
            dst.pixels[d..d + 4].copy_from_slice(&src.pixels[s..s + 4]);
        }
    }
    pub mod light2d {
        //! Normal-mapped 2D lighting for pixel art (the technique behind dynamically lit pixel games):
        //! sprite albedo × (ambient + Σ point lights · N·L), with optional light banding so the result
        //! keeps crisp pixel-art shading steps instead of smooth gradients.

        use crate::pixel::Rgba;
        use crate::renderer::Image;

        #[derive(Debug, Clone, Copy)]
        pub struct Light2D {
            pub x: f32,
            pub y: f32,
            /// Height above the sprite plane in pixels (low = grazing light, high = overhead).
            pub z: f32,
            pub color: [f32; 3],
            pub intensity: f32,
            pub radius: f32,
        }

        pub struct LightSettings {
            pub ambient: [f32; 3],
            pub lights: Vec<Light2D>,
            /// 0 = smooth; N ≥ 2 quantizes light intensity into N bands.
            pub bands: u32,
            /// Strength of the normal map's influence (0 = flat).
            pub normal_strength: f32,
        }

        fn lin(c: u8) -> f32 {
            (c as f32 / 255.0).powf(2.2)
        }

        fn srgb(v: f32) -> u8 {
            (v.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
        }

        /// `normals` is a tangent-space normal map of the same size (RGB = XYZ*0.5+0.5, y up).
        pub fn light(albedo: &Image, normals: &Image, s: &LightSettings) -> Image {
            let mut out = Image::new(albedo.width, albedo.height);
            for y in 0..albedo.height {
                for x in 0..albedo.width {
                    let a = albedo.get(x, y);
                    if a.a == 0 {
                        continue;
                    }
                    let nm = normals.get(x, y);
                    let mut n = [(nm.r as f32 / 255.0) * 2.0 - 1.0, (nm.g as f32 / 255.0) * 2.0 - 1.0, (nm.b as f32 / 255.0) * 2.0 - 1.0];
                    if nm.a == 0 {
                        n = [0.0, 0.0, 1.0];
                    }
                    n[0] *= s.normal_strength;
                    n[1] *= s.normal_strength;
                    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-5);
                    let n = [n[0] / len, n[1] / len, n[2] / len];
                    let mut acc = s.ambient;
                    for l in &s.lights {
                        // Image y grows downward; normal map y points up.
                        let (dx, dy, dz) = (l.x - (x as f32 + 0.5), -(l.y - (y as f32 + 0.5)), l.z);
                        let d = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-3);
                        let planar = (dx * dx + dy * dy).sqrt();
                        if planar > l.radius {
                            continue;
                        }
                        let ndl = ((n[0] * dx + n[1] * dy + n[2] * dz) / d).max(0.0);
                        let att = (1.0 - planar / l.radius).powi(2);
                        let mut k = ndl * att * l.intensity;
                        if s.bands >= 2 {
                            let b = s.bands as f32;
                            k = (k * b).ceil().min(b) / b;
                        }
                        for c in 0..3 {
                            acc[c] += l.color[c] * k;
                        }
                    }
                    let px = [lin(a.r) * acc[0], lin(a.g) * acc[1], lin(a.b) * acc[2]];
                    let o = ((y * out.width + x) * 4) as usize;
                    out.pixels[o..o + 4].copy_from_slice(&[srgb(px[0]), srgb(px[1]), srgb(px[2]), a.a]);
                }
            }
            out
        }

        pub fn rgb_lin(c: Rgba) -> [f32; 3] {
            [lin(c.r), lin(c.g), lin(c.b)]
        }
    }
    pub mod rotsprite {
        //! RotSprite: pixel-art-friendly rotation. The sprite is upscaled 8× with Scale2x (which keeps
        //! diagonal edges clean), rotated with nearest sampling at high resolution, then sampled back
        //! down — no blur, no new colors, far fewer broken lines than a plain nearest rotation.

        /// Scale2x (EPX) on a true-color grid (0 = transparent).
        fn scale2x(src: &[u32], w: usize, h: usize) -> Vec<u32> {
            let at = |x: isize, y: isize| -> u32 {
                let (x, y) = (x.clamp(0, w as isize - 1) as usize, y.clamp(0, h as isize - 1) as usize);
                src[y * w + x]
            };
            let mut out = vec![0u32; w * h * 4];
            for y in 0..h as isize {
                for x in 0..w as isize {
                    let p = at(x, y);
                    let (a, b, c, d) = (at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
                    let (mut e0, mut e1, mut e2, mut e3) = (p, p, p, p);
                    if c == a && c != d && a != b {
                        e0 = a;
                    }
                    if a == b && a != c && b != d {
                        e1 = b;
                    }
                    if d == c && d != b && c != a {
                        e2 = c;
                    }
                    if b == d && b != a && d != c {
                        e3 = d;
                    }
                    let (ox, oy) = (x as usize * 2, y as usize * 2);
                    out[oy * w * 2 + ox] = e0;
                    out[oy * w * 2 + ox + 1] = e1;
                    out[(oy + 1) * w * 2 + ox] = e2;
                    out[(oy + 1) * w * 2 + ox + 1] = e3;
                }
            }
            out
        }

        /// Rotates `src` (w×h) by `degrees` clockwise around (`cx`, `cy`); output has the same size.
        pub fn rotate(src: &[u32], w: usize, h: usize, degrees: f32, cx: f32, cy: f32) -> Vec<u32> {
            let mut big = src.to_vec();
            let (mut bw, mut bh) = (w, h);
            for _ in 0..3 {
                big = scale2x(&big, bw, bh);
                bw *= 2;
                bh *= 2;
            }
            let (s, c) = (-degrees.to_radians()).sin_cos();
            let mut out = vec![0u32; w * h];
            for y in 0..h {
                for x in 0..w {
                    // Sample the center of the destination pixel, rotated back into the source.
                    let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    let (sx, sy) = (c * dx - s * dy + cx, s * dx + c * dy + cy);
                    let (bx, by) = ((sx * 8.0).floor() as isize, (sy * 8.0).floor() as isize);
                    if bx < 0 || by < 0 || bx >= bw as isize || by >= bh as isize {
                        continue;
                    }
                    out[y * w + x] = big[by as usize * bw + bx as usize];
                }
            }
            out
        }
    }
    pub mod tiles {
        //! Tilesets and tilemaps with 47-tile "blob" autotiling (the layout Godot terrains, Tiled
        //! Wang sets, LDtk and GameMaker expect), plus a generator that synthesizes all 47 pixel-art
        //! terrain tiles from one seamless fill tile.

        use crate::pixel::Rgba;
        use crate::renderer::Image;

        // Neighbour bits: N, NE, E, SE, S, SW, W, NW.
        pub const N: u8 = 1;
        pub const NE: u8 = 2;
        pub const E: u8 = 4;
        pub const SE: u8 = 8;
        pub const S: u8 = 16;
        pub const SW: u8 = 32;
        pub const W: u8 = 64;
        pub const NW: u8 = 128;

        /// Corners only count when both adjacent edges are present → 47 distinct masks.
        pub fn reduce(mask: u8) -> u8 {
            let mut m = mask & (N | E | S | W);
            if mask & NE != 0 && mask & N != 0 && mask & E != 0 {
                m |= NE;
            }
            if mask & SE != 0 && mask & S != 0 && mask & E != 0 {
                m |= SE;
            }
            if mask & SW != 0 && mask & S != 0 && mask & W != 0 {
                m |= SW;
            }
            if mask & NW != 0 && mask & N != 0 && mask & W != 0 {
                m |= NW;
            }
            m
        }

        /// The 47 reduced masks in ascending order (tile i of a blob tileset = BLOB_MASKS[i]).
        pub fn blob_masks() -> Vec<u8> {
            let mut v: Vec<u8> = (0..=255u8).map(reduce).collect();
            v.sort_unstable();
            v.dedup();
            v
        }

        #[derive(Debug, Clone, PartialEq)]
        pub enum TilesetKind {
            Plain,
            /// Tile i is the autotile for `blob_masks()[i]`.
            Blob,
        }

        #[derive(Debug, Clone, PartialEq)]
        pub struct Tileset {
            pub name: String,
            pub tile_w: u32,
            pub tile_h: u32,
            pub kind: TilesetKind,
            /// Tiles as true-color pixels (`Rgba::to_pixel`, 0 = transparent).
            pub tiles: Vec<Vec<u32>>,
        }

        impl Tileset {
            /// Atlas image with `columns` tiles per row.
            pub fn atlas(&self, columns: u32) -> Image {
                let n = self.tiles.len().max(1) as u32;
                let cols = columns.clamp(1, n);
                let rows = n.div_ceil(cols);
                let mut img = Image::new(cols * self.tile_w, rows * self.tile_h);
                for (i, t) in self.tiles.iter().enumerate() {
                    let (ox, oy) = ((i as u32 % cols) * self.tile_w, (i as u32 / cols) * self.tile_h);
                    for y in 0..self.tile_h {
                        for x in 0..self.tile_w {
                            let c = Rgba::from_u32(t[(y * self.tile_w + x) as usize]);
                            let o = (((oy + y) * img.width + ox + x) * 4) as usize;
                            img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
                        }
                    }
                }
                img
            }
        }

        #[derive(Debug, Clone, PartialEq)]
        pub struct Tilemap {
            pub name: String,
            pub tileset: usize,
            pub cols: u32,
            pub rows: u32,
            /// 0 = empty, otherwise tile index + 1.
            pub cells: Vec<u32>,
            /// Terrain occupancy used by blob autotiling.
            pub terrain: Vec<bool>,
        }

        impl Tilemap {
            pub fn new(name: &str, tileset: usize, cols: u32, rows: u32) -> Tilemap {
                let n = (cols * rows) as usize;
                Tilemap { name: name.into(), tileset, cols, rows, cells: vec![0; n], terrain: vec![false; n] }
            }

            pub fn in_bounds(&self, x: i64, y: i64) -> bool {
                x >= 0 && y >= 0 && x < self.cols as i64 && y < self.rows as i64
            }

            fn terrain_at(&self, x: i64, y: i64) -> bool {
                self.in_bounds(x, y) && self.terrain[(y as u32 * self.cols + x as u32) as usize]
            }

            /// Recomputes blob tiles from the terrain map (edges of the map count as terrain so
            /// fills reach the border cleanly).
            pub fn autotile(&mut self) {
                let masks = blob_masks();
                for y in 0..self.rows as i64 {
                    for x in 0..self.cols as i64 {
                        let i = (y as u32 * self.cols + x as u32) as usize;
                        if !self.terrain[i] {
                            self.cells[i] = 0;
                            continue;
                        }
                        let t = |dx: i64, dy: i64| {
                            let (nx, ny) = (x + dx, y + dy);
                            !self.in_bounds(nx, ny) || self.terrain_at(nx, ny)
                        };
                        let mut m = 0u8;
                        for (bit, dx, dy) in
                            [(N, 0, -1), (NE, 1, -1), (E, 1, 0), (SE, 1, 1), (S, 0, 1), (SW, -1, 1), (W, -1, 0), (NW, -1, -1)]
                        {
                            if t(dx, dy) {
                                m |= bit;
                            }
                        }
                        let r = reduce(m);
                        self.cells[i] = masks.iter().position(|&k| k == r).unwrap() as u32 + 1;
                    }
                }
            }

            /// Renders the map into an image.
            pub fn render(&self, ts: &Tileset) -> Image {
                let mut img = Image::new(self.cols * ts.tile_w, self.rows * ts.tile_h);
                for y in 0..self.rows {
                    for x in 0..self.cols {
                        let c = self.cells[(y * self.cols + x) as usize];
                        let Some(tile) = c.checked_sub(1).and_then(|t| ts.tiles.get(t as usize)) else { continue };
                        for ty in 0..ts.tile_h {
                            for tx in 0..ts.tile_w {
                                let v = tile[(ty * ts.tile_w + tx) as usize];
                                if v == 0 {
                                    continue;
                                }
                                let col = Rgba::from_u32(v);
                                let o = (((y * ts.tile_h + ty) * img.width + x * ts.tile_w + tx) * 4) as usize;
                                crate::renderer::blend_over(&mut img.pixels[o..o + 4], col, 255);
                            }
                        }
                    }
                }
                img
            }
        }

        /// Parameters for synthesizing a blob terrain tileset.
        pub struct BlobStyle {
            /// Transparent margin (px) on sides that border empty space.
            pub inset: u32,
            /// Width of the decorated edge band inside the terrain.
            pub edge: u32,
            pub light: Rgba,
            pub dark: Rgba,
            pub outline: Rgba,
            /// Extra "cliff lip" rows below the terrain on open south sides (depth illusion).
            pub lip: u32,
            pub lip_color: Rgba,
            pub rounded: bool,
        }

        fn shade(c: Rgba, k: f32) -> Rgba {
            let f = |v: u8| (v as f32 * k).round().clamp(0.0, 255.0) as u8;
            // Darken with a slight shift toward blue, like hue-shifted pixel-art shadows.
            Rgba::new(f(c.r), f(c.g), (c.b as f32 * (k + 0.08).min(1.0)).round() as u8, c.a)
        }

        /// Builds the 47 blob tiles from one seamless fill tile (w×h true-color pixels).
        pub fn synthesize_blob(fill: &[u32], w: u32, h: u32, style: &BlobStyle) -> Vec<Vec<u32>> {
            let masks = blob_masks();
            masks
                .iter()
                .map(|&m| {
                    let open = |bit: u8| m & bit == 0;
                    let inset = style.inset as i32;
                    let lip = style.lip as i32;
                    let (wi, hi) = (w as i32, h as i32);
                    // Signed distance (px) of a pixel to the terrain boundary; negative = outside.
                    let inside_dist = |x: i32, y: i32| -> i32 {
                        let mut d = i32::MAX;
                        if open(N) {
                            d = d.min(y - inset);
                        }
                        if open(S) {
                            d = d.min(hi - 1 - inset - lip - y);
                        }
                        if open(W) {
                            d = d.min(x - inset);
                        }
                        if open(E) {
                            d = d.min(wi - 1 - inset - x);
                        }
                        // Inner corners: neighbour edges present but the diagonal is missing.
                        let notch = |cx: bool, cy: bool| {
                            let dx = if cx { wi - 1 - x } else { x };
                            let dy = if cy { hi - 1 - y } else { y };
                            let ext = inset + if cy { lip } else { 0 };
                            if dx < inset && dy < ext { (dx - inset).max(dy - ext) } else { i32::MAX }
                        };
                        if !open(N) && !open(E) && open(NE) {
                            d = d.min(notch(true, false));
                        }
                        if !open(S) && !open(E) && open(SE) {
                            d = d.min(notch(true, true));
                        }
                        if !open(S) && !open(W) && open(SW) {
                            d = d.min(notch(false, true));
                        }
                        if !open(N) && !open(W) && open(NW) {
                            d = d.min(notch(false, false));
                        }
                        // Rounded outer corners.
                        if style.rounded {
                            for (cx, cy, a, b) in [(false, false, N, W), (true, false, N, E), (false, true, S, W), (true, true, S, E)] {
                                if open(a) && open(b) {
                                    let px = if cx { wi - 1 - inset - x } else { x - inset };
                                    let py = if cy { hi - 1 - inset - lip - y } else { y - inset };
                                    if px < 2 && py < 2 && px + py < 2 {
                                        d = d.min(px + py - 2);
                                    }
                                }
                            }
                        }
                        d
                    };
                    let mut tile = vec![0u32; (w * h) as usize];
                    for y in 0..hi {
                        for x in 0..wi {
                            let d = inside_dist(x, y);
                            let i = (y * wi + x) as usize;
                            if d < 0 {
                                // Cliff lip under an open south edge.
                                if open(S) && lip > 0 {
                                    let below = y - (hi - 1 - inset - lip);
                                    let horiz_ok = (!open(W) || x >= inset) && (!open(E) || x <= wi - 1 - inset);
                                    if below > 0 && below <= lip && horiz_ok {
                                        tile[i] = if below == lip {
                                            style.outline.to_pixel()
                                        } else if below == 1 {
                                            // Shadow under the overhanging top surface.
                                            shade(style.lip_color, 0.62).to_pixel()
                                        } else if (x * 5 + below * 3 + (x / 3) * 7) % 7 == 0 {
                                            // Vertical strata / cracks on the cliff face.
                                            shade(style.lip_color, 0.8).to_pixel()
                                        } else {
                                            style.lip_color.to_pixel()
                                        };
                                    }
                                }
                                continue;
                            }
                            tile[i] = if d == 0 {
                                style.outline.to_pixel()
                            } else if (d as u32) <= style.edge {
                                // Top edges catch the light, bottom/side edges are shaded.
                                let near_top = open(N) && y - inset <= style.edge as i32;
                                if near_top { style.light.to_pixel() } else { style.dark.to_pixel() }
                            } else {
                                fill[(y as u32 % h * w + x as u32 % w) as usize]
                            };
                        }
                    }
                    tile
                })
                .collect()
        }
    }

    /// A named rectangle per frame (Aseprite-compatible): hitbox, hurtbox, pivot, attach point…
    /// A key applies from its frame until the next key.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Slice {
        pub name: String,
        pub color: u32,
        pub keys: Vec<SliceKey>,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct SliceKey {
        pub frame_id: u32,
        pub x: i32,
        pub y: i32,
        pub w: u32,
        pub h: u32,
        pub pivot: Option<(i32, i32)>,
    }
}
pub mod history {
    //! Undo/redo, the operation log and idempotency tracking.
    //!
    //! Undo entries are full `Document` snapshots. Thanks to shared, copy-on-write tiles a
    //! snapshot costs a few pointer copies per cel instead of a copy of every pixel.

    use std::collections::{HashMap, VecDeque};

    use crate::project::Document;

    pub const UNDO_LIMIT: usize = 256;
    pub const LOG_LIMIT: usize = 10_000;
    pub const IDEMPOTENCY_LIMIT: usize = 2048;

    /// One applied mutation, kept for auditing who changed what.
    #[derive(Debug, Clone, PartialEq)]
    pub struct LogEntry {
        pub revision: u64,
        pub actor: String,
        pub op: String,
        pub operation_id: Option<String>,
        pub changed_pixels: u64,
        pub timestamp_ms: u64,
    }

    struct UndoEntry {
        before: Document,
        after_revision: u64,
        label: String,
        actor: String,
    }

    #[derive(Default)]
    pub struct History {
        undo: Vec<UndoEntry>,
        redo: Vec<UndoEntry>,
        log: VecDeque<LogEntry>,
        idempotency: HashMap<String, Vec<u8>>,
        idempotency_order: VecDeque<String>,
    }

    pub struct UndoInfo {
        pub label: String,
        pub actor: String,
    }

    impl History {
        pub fn record(&mut self, before: Document, after_revision: u64, label: &str, actor: &str) {
            self.undo.push(UndoEntry { before, after_revision, label: label.into(), actor: actor.into() });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }

        /// Swaps `current` with the previous state. Returns what was undone.
        pub fn undo(&mut self, current: &mut Document, revision: u64) -> Option<UndoInfo> {
            let entry = self.undo.pop()?;
            let after = std::mem::replace(current, entry.before);
            let info = UndoInfo { label: entry.label.clone(), actor: entry.actor.clone() };
            self.redo.push(UndoEntry { before: after, after_revision: revision, label: entry.label, actor: entry.actor });
            let _ = entry.after_revision;
            Some(info)
        }

        pub fn redo(&mut self, current: &mut Document, revision: u64) -> Option<UndoInfo> {
            let entry = self.redo.pop()?;
            let before = std::mem::replace(current, entry.before);
            let info = UndoInfo { label: entry.label.clone(), actor: entry.actor.clone() };
            self.undo.push(UndoEntry { before, after_revision: revision, label: entry.label, actor: entry.actor });
            Some(info)
        }

        pub fn can_undo(&self) -> usize {
            self.undo.len()
        }

        pub fn can_redo(&self) -> usize {
            self.redo.len()
        }

        pub fn reset(&mut self) {
            self.undo.clear();
            self.redo.clear();
        }

        pub fn log(&mut self, entry: LogEntry) {
            self.log.push_back(entry);
            while self.log.len() > LOG_LIMIT {
                self.log.pop_front();
            }
        }

        pub fn log_entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
            self.log.iter()
        }

        pub fn replace_log(&mut self, entries: Vec<LogEntry>) {
            self.log = entries.into();
        }

        pub fn cached_response(&self, operation_id: &str) -> Option<&Vec<u8>> {
            self.idempotency.get(operation_id)
        }

        pub fn cache_response(&mut self, operation_id: &str, response: Vec<u8>) {
            if self.idempotency.insert(operation_id.to_string(), response).is_none() {
                self.idempotency_order.push_back(operation_id.to_string());
            }
            while self.idempotency_order.len() > IDEMPOTENCY_LIMIT {
                if let Some(old) = self.idempotency_order.pop_front() {
                    self.idempotency.remove(&old);
                }
            }
        }

        pub fn undo_labels(&self) -> Vec<(String, String)> {
            self.undo.iter().rev().map(|e| (e.label.clone(), e.actor.clone())).collect()
        }
    }

    pub fn now_ms() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
    }
}
pub mod layers {
    //! Layer metadata. Pixel content lives in cels (see `project::Document`).

    #[derive(Debug, Clone, PartialEq)]
    pub struct Layer {
        /// Stable identifier; survives reordering.
        pub id: u32,
        pub name: String,
        pub visible: bool,
        pub locked: bool,
        /// 0 = invisible, 255 = fully opaque.
        pub opacity: u8,
        pub blend: Blend,
        /// Pixel density relative to the canvas: 1, 2 or 4. A density-2 layer has half-size
        /// pixels (its grid is 2× the canvas), for faces, eyes and fine highlights.
        pub density: u8,
    }

    impl Layer {
        pub fn new(id: u32, name: impl Into<String>) -> Layer {
            Layer { id, name: name.into(), visible: true, locked: false, opacity: 255, blend: Blend::Normal, density: 1 }
        }
    }

    /// Layer blend modes (lighting, shadow and glow layers).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Blend {
        Normal,
        Multiply,
        Screen,
        Add,
        Overlay,
        Darken,
        Lighten,
        SoftLight,
    }

    impl Blend {
        pub const ALL: [(&'static str, Blend); 8] = [
            ("normal", Blend::Normal),
            ("multiply", Blend::Multiply),
            ("screen", Blend::Screen),
            ("add", Blend::Add),
            ("overlay", Blend::Overlay),
            ("darken", Blend::Darken),
            ("lighten", Blend::Lighten),
            ("soft_light", Blend::SoftLight),
        ];

        pub fn parse(s: &str) -> Option<Blend> {
            Self::ALL.iter().find(|(n, _)| *n == s.to_ascii_lowercase().replace(['-', ' '], "_")).map(|(_, b)| *b)
        }

        pub fn as_str(self) -> &'static str {
            Self::ALL.iter().find(|(_, b)| *b == self).map(|(n, _)| *n).unwrap_or("normal")
        }

        pub fn code(self) -> u8 {
            Self::ALL.iter().position(|(_, b)| *b == self).unwrap_or(0) as u8
        }

        pub fn from_code(c: u8) -> Blend {
            Self::ALL.get(c as usize).map(|(_, b)| *b).unwrap_or(Blend::Normal)
        }

        /// Blends one channel (0..255 values, straight).
        pub fn channel(self, d: u32, s: u32) -> u32 {
            match self {
                Blend::Normal => s,
                Blend::Multiply => d * s / 255,
                Blend::Screen => 255 - (255 - d) * (255 - s) / 255,
                Blend::Add => (d + s).min(255),
                Blend::Overlay => {
                    if d < 128 {
                        2 * d * s / 255
                    } else {
                        255 - 2 * (255 - d) * (255 - s) / 255
                    }
                }
                Blend::Darken => d.min(s),
                Blend::Lighten => d.max(s),
                Blend::SoftLight => {
                    let (df, sf) = (d as f32 / 255.0, s as f32 / 255.0);
                    let r = if sf < 0.5 { df - (1.0 - 2.0 * sf) * df * (1.0 - df) } else { df + (2.0 * sf - 1.0) * (df.sqrt() - df) };
                    (r * 255.0).round().clamp(0.0, 255.0) as u32
                }
            }
        }
    }

    pub const MAX_LAYER_NAME: usize = 128;

    pub fn validate_name(name: &str) -> Result<String, String> {
        let n = name.trim();
        if n.is_empty() {
            return Err("layer name must not be empty".into());
        }
        if n.chars().count() > MAX_LAYER_NAME {
            return Err(format!("layer name longer than {MAX_LAYER_NAME} characters"));
        }
        if n.chars().any(|c| c.is_control()) {
            return Err("layer name contains control characters".into());
        }
        Ok(n.to_string())
    }
}
pub mod pixel {
    //! Pixel data model: colors, palette and the 16×16 sparse tile grid.

    pub(crate) mod color {
        /// Straight (non-premultiplied) 8-bit RGBA color.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct Rgba {
            pub r: u8,
            pub g: u8,
            pub b: u8,
            pub a: u8,
        }

        impl Rgba {
            pub const TRANSPARENT: Rgba = Rgba { r: 0, g: 0, b: 0, a: 0 };

            pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
                Rgba { r, g, b, a }
            }

            pub fn to_u32(self) -> u32 {
                u32::from_be_bytes([self.r, self.g, self.b, self.a])
            }

            /// Canvas pixel value: `0xRRGGBBAA`, with every fully transparent color collapsed to 0.
            pub fn to_pixel(self) -> u32 {
                if self.a == 0 { 0 } else { self.to_u32() }
            }

            pub fn from_u32(v: u32) -> Self {
                let [r, g, b, a] = v.to_be_bytes();
                Rgba { r, g, b, a }
            }

            /// Parses `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA` (the `#` is optional) and `transparent`.
            pub fn parse_hex(text: &str) -> Option<Rgba> {
                let t = text.trim();
                if t.eq_ignore_ascii_case("transparent") {
                    return Some(Rgba::TRANSPARENT);
                }
                let h = t.strip_prefix('#').unwrap_or(t);
                if !h.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                let nib = |i: usize| u8::from_str_radix(&h[i..i + 1], 16).ok().map(|v| v * 17);
                let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
                match h.len() {
                    3 => Some(Rgba::new(nib(0)?, nib(1)?, nib(2)?, 255)),
                    4 => Some(Rgba::new(nib(0)?, nib(1)?, nib(2)?, nib(3)?)),
                    6 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, 255)),
                    8 => Some(Rgba::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
                    _ => None,
                }
            }

            /// `#RRGGBB` when opaque, `#RRGGBBAA` otherwise.
            pub fn to_hex(self) -> String {
                if self.a == 255 {
                    format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
                } else {
                    format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
                }
            }
        }
    }
    pub(crate) mod grid {
        use std::sync::Arc;

        use super::TRANSPARENT;

        pub const TILE_SIZE: u32 = 16;
        pub const TILE_AREA: usize = (TILE_SIZE * TILE_SIZE) as usize;

        /// One 16×16 block of true-color pixels (`Rgba::to_pixel`, 0 = transparent).
        pub type Tile = [u32; TILE_AREA];

        /// Sparse pixel storage for one cel (one layer in one frame).
        ///
        /// Tiles are reference counted and copy-on-write: cloning a grid (for undo snapshots or
        /// frame duplication) only copies pointers, and unchanged tiles stay shared between frames.
        /// A `None` tile is fully transparent and costs no pixel memory.
        #[derive(Debug, Clone, PartialEq)]
        pub struct PixelGrid {
            width: u32,
            height: u32,
            tiles_x: u32,
            tiles_y: u32,
            tiles: Vec<Option<Arc<Tile>>>,
        }

        impl PixelGrid {
            pub fn new(width: u32, height: u32) -> PixelGrid {
                let tiles_x = width.div_ceil(TILE_SIZE);
                let tiles_y = height.div_ceil(TILE_SIZE);
                PixelGrid { width, height, tiles_x, tiles_y, tiles: vec![None; (tiles_x * tiles_y) as usize] }
            }

            pub fn width(&self) -> u32 {
                self.width
            }

            pub fn height(&self) -> u32 {
                self.height
            }

            pub fn tiles_x(&self) -> u32 {
                self.tiles_x
            }

            pub fn tiles_y(&self) -> u32 {
                self.tiles_y
            }

            pub fn in_bounds(&self, x: i64, y: i64) -> bool {
                x >= 0 && y >= 0 && x < self.width as i64 && y < self.height as i64
            }

            #[inline]
            fn locate(&self, x: u32, y: u32) -> (usize, usize) {
                let t = ((y / TILE_SIZE) * self.tiles_x + (x / TILE_SIZE)) as usize;
                let i = ((y % TILE_SIZE) * TILE_SIZE + (x % TILE_SIZE)) as usize;
                (t, i)
            }

            /// Out-of-bounds reads return transparent.
            #[inline]
            pub fn get(&self, x: i64, y: i64) -> u32 {
                if !self.in_bounds(x, y) {
                    return TRANSPARENT;
                }
                let (t, i) = self.locate(x as u32, y as u32);
                match &self.tiles[t] {
                    Some(tile) => tile[i],
                    None => TRANSPARENT,
                }
            }

            /// Writes a pixel; out-of-bounds writes are clipped. Returns true when the value changed.
            #[inline]
            pub fn set(&mut self, x: i64, y: i64, value: u32) -> bool {
                if !self.in_bounds(x, y) {
                    return false;
                }
                let (t, i) = self.locate(x as u32, y as u32);
                match &mut self.tiles[t] {
                    None if value == TRANSPARENT => false,
                    None => {
                        let mut tile = [TRANSPARENT; TILE_AREA];
                        tile[i] = value;
                        self.tiles[t] = Some(Arc::new(tile));
                        true
                    }
                    Some(tile) => {
                        if tile[i] == value {
                            return false;
                        }
                        Arc::make_mut(tile)[i] = value;
                        true
                    }
                }
            }

            pub fn clear(&mut self) {
                self.tiles.iter_mut().for_each(|t| *t = None);
            }

            /// Drops tiles that became fully transparent.
            pub fn compact(&mut self) {
                for t in self.tiles.iter_mut() {
                    if t.as_ref().is_some_and(|tile| tile.iter().all(|&v| v == TRANSPARENT)) {
                        *t = None;
                    }
                }
            }

            pub fn is_empty(&self) -> bool {
                self.tiles.iter().all(|t| t.as_ref().is_none_or(|tile| tile.iter().all(|&v| v == TRANSPARENT)))
            }

            pub fn tile(&self, tx: u32, ty: u32) -> Option<&Arc<Tile>> {
                self.tiles.get((ty * self.tiles_x + tx) as usize).and_then(|t| t.as_ref())
            }

            pub fn tiles(&self) -> &[Option<Arc<Tile>>] {
                &self.tiles
            }

            pub fn set_tile(&mut self, tx: u32, ty: u32, tile: Option<Arc<Tile>>) {
                let idx = (ty * self.tiles_x + tx) as usize;
                if idx < self.tiles.len() {
                    self.tiles[idx] = tile;
                }
            }

            /// Number of allocated tiles (memory diagnostics).
            pub fn allocated_tiles(&self) -> usize {
                self.tiles.iter().filter(|t| t.is_some()).count()
            }

            /// Calls `f(x, y, index)` for every non-transparent pixel, tile by tile.
            pub fn for_each_opaque(&self, mut f: impl FnMut(u32, u32, u32)) {
                for ty in 0..self.tiles_y {
                    for tx in 0..self.tiles_x {
                        let Some(tile) = self.tile(tx, ty) else { continue };
                        for (i, &v) in tile.iter().enumerate() {
                            if v == TRANSPARENT {
                                continue;
                            }
                            let x = tx * TILE_SIZE + (i as u32 % TILE_SIZE);
                            let y = ty * TILE_SIZE + (i as u32 / TILE_SIZE);
                            if x < self.width && y < self.height {
                                f(x, y, v);
                            }
                        }
                    }
                }
            }

            /// Bounding box `(x, y, w, h)` of non-transparent pixels.
            pub fn bounds(&self) -> Option<(u32, u32, u32, u32)> {
                let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
                let mut any = false;
                self.for_each_opaque(|x, y, _| {
                    any = true;
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                });
                any.then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
            }

            /// Returns a copy of this grid resized to `width × height`, keeping pixels at an offset.
            pub fn resized(&self, width: u32, height: u32, offset_x: i64, offset_y: i64) -> PixelGrid {
                let mut out = PixelGrid::new(width, height);
                self.for_each_opaque(|x, y, v| {
                    out.set(x as i64 + offset_x, y as i64 + offset_y, v);
                });
                out
            }
        }
    }
    mod palette {
        use super::Rgba;

        /// Pixel value of "no pixel". Pixels are true-color `0xRRGGBBAA`; any alpha-0 color is stored as 0.
        pub const TRANSPARENT: u32 = 0;
        /// Swatch list limit (a UI convenience, not a color limit: pixels can use any of 2^32 RGBA values).
        pub const MAX_PALETTE: usize = 4096;

        /// The project's color swatches. Pixels do not depend on it: the canvas is true color.
        /// Index 0 is reserved for "transparent" so swatch indices stay stable for agents.
        #[derive(Debug, Clone, PartialEq)]
        pub struct Palette {
            colors: Vec<Rgba>,
        }

        impl Default for Palette {
            fn default() -> Self {
                Palette { colors: vec![Rgba::TRANSPARENT] }
            }
        }

        impl Palette {
            pub fn from_colors(mut colors: Vec<Rgba>) -> Result<Palette, String> {
                if colors.is_empty() {
                    colors.push(Rgba::TRANSPARENT);
                }
                if colors.len() > MAX_PALETTE {
                    return Err(format!("palette exceeds {MAX_PALETTE} swatches"));
                }
                colors[0] = Rgba::TRANSPARENT;
                Ok(Palette { colors })
            }

            pub fn len(&self) -> usize {
                self.colors.len()
            }

            pub fn is_empty(&self) -> bool {
                self.colors.len() <= 1
            }

            pub fn colors(&self) -> &[Rgba] {
                &self.colors
            }

            pub fn get(&self, index: usize) -> Option<Rgba> {
                self.colors.get(index).copied()
            }

            pub fn find(&self, color: Rgba) -> Option<usize> {
                if color.a == 0 {
                    return Some(0);
                }
                self.colors.iter().skip(1).position(|c| *c == color).map(|i| i + 1)
            }

            /// Returns the swatch index of `color`, appending it when new.
            pub fn intern(&mut self, color: Rgba) -> Result<usize, String> {
                if let Some(i) = self.find(color) {
                    return Ok(i);
                }
                if self.colors.len() >= MAX_PALETTE {
                    return Err("swatch list is full".into());
                }
                self.colors.push(color);
                Ok(self.colors.len() - 1)
            }

            /// Replaces a swatch and returns the previous color.
            pub fn set(&mut self, index: usize, color: Rgba) -> Result<Rgba, String> {
                if index == 0 {
                    return Err("swatch 0 is reserved for transparency".into());
                }
                match self.colors.get_mut(index) {
                    Some(slot) => Ok(std::mem::replace(slot, color)),
                    None => Err(format!("swatch {index} does not exist")),
                }
            }
        }
    }

    pub use color::Rgba;
    pub use grid::{PixelGrid, TILE_AREA, TILE_SIZE, Tile};
    pub use palette::{MAX_PALETTE, Palette, TRANSPARENT};
}
pub mod pixelart {
    //! Pixel-art craft tools: perceptual color (OKLab), hue-shifted ramps, palette library,
    //! median-cut quantization with ordered dithering, selective outlines, dither fills and 2D
    //! normal maps for dynamic sprite lighting.

    pub mod charart {
        //! Character-art finishing tools: automatic cel shading of flat color regions (with cast
        //! shadows from the layers above and rim light), sel-out outlines, gradient-map color grading,
        //! sparkle stamps and "pixel perfect" line cleanup.
        //!
        //! The intended workflow mirrors how anime pixel artists work: block the character in flat
        //! colors on a few layers (hair back, body, face, hair front), let `auto_shade` produce
        //! hue-shifted 2-4 tone shading that follows each shape, add sel-out outlines, then paint the
        //! eyes and highlights by hand.

        use std::collections::HashMap;

        use super::{from_lch, hue_toward, lch, outline_color, ramp};
        use crate::pixel::Rgba;
        use crate::renderer::Image;

        #[derive(Debug, Clone)]
        pub struct ShadeSettings {
            /// Direction toward the light in image space (x right, y down), e.g. (-1, -1) = top-left.
            pub light: (f32, f32),
            /// 2 = base + shadow, 3 = + highlight, 4 = + deep shadow.
            pub tones: u32,
            /// Edge roundness in pixels (how far shading reaches into a shape).
            pub bevel: f32,
            /// Weight of the whole-shape gradient (big areas get a soft light→shadow side).
            pub form: f32,
            pub hue_shift: f32,
            pub contrast: f32,
            /// Shift of the shadow threshold: higher = more shadow.
            pub shadow_amount: f32,
            /// Regions smaller than this (pixels) are left untouched (eyes, buttons, details).
            pub min_region: usize,
            /// Explicit ramps: base color → [deep shadow, shadow, base, highlight].
            pub ramps: HashMap<u32, [Rgba; 4]>,
            /// Colors never shaded.
            pub preserve: Vec<u32>,
            /// Rim light on the silhouette edges facing away from the light: (color, strength 0-1).
            pub rim: Option<(Rgba, f32)>,
            /// Only shade colors that have a ramp; never invent new colors.
            pub palette_lock: bool,
        }

        impl Default for ShadeSettings {
            fn default() -> Self {
                ShadeSettings {
                    light: (-1.0, -1.2),
                    tones: 3,
                    bevel: 3.0,
                    form: 0.45,
                    hue_shift: 22.0,
                    contrast: 0.5,
                    shadow_amount: 0.0,
                    min_region: 6,
                    ramps: HashMap::new(),
                    preserve: Vec::new(),
                    rim: None,
                    palette_lock: false,
                }
            }
        }

        fn key(c: Rgba) -> u32 {
            (c.r as u32) << 24 | (c.g as u32) << 16 | (c.b as u32) << 8 | 0xFF
        }

        /// Auto ramp [deep, shadow, base, highlight] around `base` — shadows lean violet and gain a
        /// little saturation, highlights lean warm, as in the classic pastel anime palette.
        pub fn auto_ramp(base: Rgba, hue_shift: f32, contrast: f32) -> [Rgba; 4] {
            let r = ramp(base, 7, hue_shift, contrast);
            [r[1], r[2], base, r[5]]
        }

        /// Cel-shades every flat color region of `img`. `occluders` (same size, may be empty) marks
        /// pixels covered by layers drawn above; they cast shadows `cast` pixels away from the light.
        pub fn auto_shade(img: &mut Image, s: &ShadeSettings, occluders: &[bool], cast: u32) -> usize {
            let (w, h) = (img.width as i32, img.height as i32);
            let n = (w * h) as usize;
            let px =
                |img: &Image, i: usize| Rgba::new(img.pixels[i * 4], img.pixels[i * 4 + 1], img.pixels[i * 4 + 2], img.pixels[i * 4 + 3]);
            // Connected regions of identical color (4-connectivity).
            let mut region = vec![u32::MAX; n];
            let mut regions: Vec<(Rgba, Vec<usize>)> = Vec::new();
            for start in 0..n {
                let c = px(img, start);
                if c.a == 0 || region[start] != u32::MAX {
                    continue;
                }
                let id = regions.len() as u32;
                let mut members = vec![start];
                region[start] = id;
                let mut k = 0;
                while k < members.len() {
                    let i = members[k];
                    k += 1;
                    let (x, y) = ((i as i32) % w, (i as i32) / w);
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= w || ny >= h {
                            continue;
                        }
                        let j = (ny * w + nx) as usize;
                        if region[j] == u32::MAX && px(img, j) == c {
                            region[j] = id;
                            members.push(j);
                        }
                    }
                }
                regions.push((c, members));
            }
            // Chamfer distance to the region border (3-4 metric / 3 ≈ pixels).
            let mut dist = vec![0f32; n];
            for (i, d) in dist.iter_mut().enumerate() {
                if region[i] != u32::MAX {
                    *d = 1e6;
                }
            }
            let same = |a: usize, b: usize| region[a] == region[b];
            for pass in 0..2 {
                let ys: Vec<i32> = if pass == 0 { (0..h).collect() } else { (0..h).rev().collect() };
                for &y in &ys {
                    let xs: Vec<i32> = if pass == 0 { (0..w).collect() } else { (0..w).rev().collect() };
                    for &x in &xs {
                        let i = (y * w + x) as usize;
                        if region[i] == u32::MAX {
                            continue;
                        }
                        let sgn = if pass == 0 { -1 } else { 1 };
                        let mut best = dist[i];
                        for (dx, dy, cost) in
                            [(sgn, 0, 1.0f32), (0, sgn, 1.0), (sgn, sgn, std::f32::consts::SQRT_2), (-sgn, sgn, std::f32::consts::SQRT_2)]
                        {
                            let (nx, ny) = (x + dx, y + dy);
                            let nd = if nx < 0 || ny < 0 || nx >= w || ny >= h || !same(i, (ny * w + nx) as usize) {
                                0.0
                            } else {
                                dist[(ny * w + nx) as usize]
                            };
                            best = best.min(nd + cost);
                        }
                        dist[i] = best;
                    }
                }
            }
            let ll = (s.light.0 * s.light.0 + s.light.1 * s.light.1).sqrt().max(1e-3);
            let (lx, ly) = (s.light.0 / ll, s.light.1 / ll);
            let height = |i: usize| -> f32 {
                let t = (dist[i] / s.bevel.max(0.5)).min(1.0);
                (t * (2.0 - t)).sqrt()
            };
            let mut changed = 0;
            let src = img.pixels.clone();
            let mut tone_map = vec![0i8; n];
            for (rid, (base, members)) in regions.iter().enumerate() {
                let k = key(*base);
                if members.len() < s.min_region || s.preserve.contains(&k) {
                    continue;
                }
                let tones = match s.ramps.get(&k) {
                    Some(t) => *t,
                    None if s.palette_lock => continue,
                    None => auto_ramp(*base, s.hue_shift, s.contrast),
                };
                // Shape centroid and extent for the whole-form gradient.
                let (mut cx, mut cy) = (0f32, 0f32);
                for &i in members {
                    cx += (i as i32 % w) as f32;
                    cy += (i as i32 / w) as f32;
                }
                cx /= members.len() as f32;
                cy /= members.len() as f32;
                let radius = members.iter().map(|&i| ((i as i32 % w) as f32 - cx).hypot((i as i32 / w) as f32 - cy)).fold(1.0f32, f32::max);
                for &i in members {
                    let (x, y) = (i as i32 % w, i as i32 / w);
                    let hh = |xx: i32, yy: i32| -> f32 {
                        if xx < 0 || yy < 0 || xx >= w || yy >= h {
                            return 0.0;
                        }
                        let j = (yy * w + xx) as usize;
                        if region[j] as usize != rid { 0.0 } else { height(j) }
                    };
                    let gx = hh(x + 1, y) - hh(x - 1, y);
                    let gy = hh(x, y + 1) - hh(x, y - 1);
                    // Outward normal (2D part) dotted with the light direction.
                    let edge = -(gx * lx + gy * ly) * 0.9;
                    let fx = (x as f32 - cx) / radius;
                    let fy = (y as f32 - cy) / radius;
                    let form = (fx * lx + fy * ly) * s.form;
                    let v = edge + form - s.shadow_amount;
                    let mut t: i8 = if v < -0.62 && s.tones >= 4 {
                        -2
                    } else if v < -0.12 {
                        -1
                    } else if v > 0.55 && s.tones >= 3 && height(i) < 0.95 {
                        1
                    } else {
                        0
                    };
                    // Cast shadow from layers above (hair over the face, head over the neck...).
                    if cast > 0 && !occluders.is_empty() && !occluders[i] {
                        for step in 1..=cast as i32 {
                            let (ox, oy) = ((x as f32 + lx * step as f32).round() as i32, (y as f32 + ly * step as f32).round() as i32);
                            if ox >= 0 && oy >= 0 && ox < w && oy < h && occluders[(oy * w + ox) as usize] {
                                t = t.min(-1);
                                break;
                            }
                        }
                    }
                    tone_map[i] = t;
                }
                // Remove lonely tone pixels (keeps clusters clean).
                for &i in members {
                    let (x, y) = (i as i32 % w, i as i32 / w);
                    let mut counts: HashMap<i8, u32> = HashMap::new();
                    let mut alone = true;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= w || ny >= h {
                            continue;
                        }
                        let j = (ny * w + nx) as usize;
                        if region[j] as usize == rid {
                            *counts.entry(tone_map[j]).or_default() += 1;
                            if tone_map[j] == tone_map[i] {
                                alone = false;
                            }
                        }
                    }
                    if alone && let Some((&t, _)) = counts.iter().max_by_key(|(_, c)| **c) {
                        tone_map[i] = t;
                    }
                }
                for &i in members {
                    let mut c = tones[(tone_map[i] + 2) as usize];
                    if let Some((rim, k)) = s.rim {
                        let (x, y) = (i as i32 % w, i as i32 / w);
                        let open = [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| {
                            let (nx, ny) = (x + dx, y + dy);
                            let back = (*dx as f32 * lx + *dy as f32 * ly) < -0.3;
                            back && (nx < 0 || ny < 0 || nx >= w || ny >= h || src[((ny * w + nx) * 4 + 3) as usize] == 0)
                        });
                        if open {
                            c = mix(c, rim, k);
                        }
                    }
                    c.a = base.a;
                    let o = i * 4;
                    if img.pixels[o..o + 4] != [c.r, c.g, c.b, c.a] {
                        img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
                        changed += 1;
                    }
                }
            }
            changed
        }

        fn mix(a: Rgba, b: Rgba, t: f32) -> Rgba {
            let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
            Rgba::new(f(a.r, b.r), f(a.g, b.g), f(a.b, b.b), a.a)
        }

        /// Outline color for sel-out: darker hue of the neighbour; `darkness` 0-1 (0.58 = default).
        pub fn selout_color(c: Rgba, darkness: f32) -> Rgba {
            let [l, ch, h] = lch(c);
            let keep = (1.0 - darkness).clamp(0.05, 0.95);
            from_lch((l * keep).max(0.1), (ch * 1.15).min(0.22), hue_toward(h, 330.0, 20.0 * darkness), 255)
        }

        /// Exterior (or interior with `inside`) outline. `selout` lightens the outline on the lit side
        /// so the silhouette reads soft where the light hits — the pastel anime look.
        pub fn outline_ex(
            img: &mut Image,
            color: Option<Rgba>,
            corners: bool,
            selout: bool,
            darkness: f32,
            light: (f32, f32),
            inside: bool,
        ) -> usize {
            let (w, h) = (img.width as i32, img.height as i32);
            let src = img.pixels.clone();
            let alpha = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && src[((y * w + x) * 4 + 3) as usize] > 0;
            let col = |x: i32, y: i32| {
                let o = ((y * w + x) * 4) as usize;
                Rgba::new(src[o], src[o + 1], src[o + 2], 255)
            };
            let ll = (light.0 * light.0 + light.1 * light.1).sqrt().max(1e-3);
            let (lx, ly) = (light.0 / ll, light.1 / ll);
            let n4 = [(1, 0), (-1, 0), (0, 1), (0, -1)];
            let n8 = [(1, 1), (-1, 1), (1, -1), (-1, -1)];
            let mut changed = 0;
            for y in 0..h {
                for x in 0..w {
                    let target = if inside { alpha(x, y) && n4.iter().any(|(dx, dy)| !alpha(x + dx, y + dy)) } else { !alpha(x, y) };
                    if !target {
                        continue;
                    }
                    // Pick the reference interior color and the outward direction.
                    let mut best: Option<(Rgba, f32, f32)> = None;
                    if inside {
                        let mut ox = 0.0;
                        let mut oy = 0.0;
                        for (dx, dy) in n4 {
                            if !alpha(x + dx, y + dy) {
                                ox += dx as f32;
                                oy += dy as f32;
                            }
                        }
                        best = Some((col(x, y), ox, oy));
                    } else {
                        for (dx, dy) in n4.iter().chain(if corners { n8.iter() } else { [].iter() }) {
                            if alpha(x + dx, y + dy) {
                                let c = col(x + dx, y + dy);
                                if best.is_none_or(|(b, _, _)| lch(c)[0] < lch(b)[0]) {
                                    best = Some((c, -(*dx as f32), -(*dy as f32)));
                                }
                            }
                        }
                    }
                    let Some((b, ox, oy)) = best else { continue };
                    let mut c =
                        color.unwrap_or_else(|| if selout || darkness != 0.58 { selout_color(b, darkness) } else { outline_color(b) });
                    if selout {
                        let facing = (ox * lx + oy * ly) / (ox.hypot(oy)).max(1e-3);
                        if facing > 0.3 {
                            c = mix(c, b, 0.45);
                        }
                    }
                    let o = ((y * w + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
                    changed += 1;
                }
            }
            changed
        }

        /// Gradient map: luminance → color along `stops`, mixed in by `strength` (0-1).
        pub fn gradient_map(img: &mut Image, stops: &[Rgba], strength: f32) -> usize {
            if stops.len() < 2 {
                return 0;
            }
            let mut changed = 0;
            for p in img.pixels.as_chunks_mut::<4>().0 {
                if p[3] == 0 {
                    continue;
                }
                let c = Rgba::new(p[0], p[1], p[2], p[3]);
                let l = super::oklab(c)[0].clamp(0.0, 1.0);
                let t = l * (stops.len() - 1) as f32;
                let i = (t.floor() as usize).min(stops.len() - 2);
                let g = mix(stops[i], stops[i + 1], t - i as f32);
                let out = mix(c, g, strength.clamp(0.0, 1.0));
                if [out.r, out.g, out.b] != [p[0], p[1], p[2]] {
                    changed += 1;
                }
                p[0] = out.r;
                p[1] = out.g;
                p[2] = out.b;
            }
            changed
        }

        /// 4-point sparkle / star stamp pixels relative to its center: (dx, dy, is_core).
        pub fn sparkle_shape(size: u32, style: &str) -> Vec<(i32, i32, bool)> {
            let s = size.clamp(1, 8) as i32;
            let mut out = Vec::new();
            match style {
                "dot" => out.push((0, 0, true)),
                "cross" => {
                    for d in -s..=s {
                        out.push((d, d, d == 0));
                        if d != 0 {
                            out.push((d, -d, false));
                        }
                    }
                }
                "diamond" => {
                    for dy in -s..=s {
                        for dx in -s..=s {
                            if dx.abs() + dy.abs() <= s {
                                out.push((dx, dy, dx.abs() + dy.abs() < s / 2 + 1));
                            }
                        }
                    }
                }
                _ => {
                    // star4: thin arms with a fat diamond core.
                    let core = (s / 3).max(if s >= 3 { 1 } else { 0 });
                    for dy in -s..=s {
                        for dx in -s..=s {
                            let arm = (dx == 0 || dy == 0) && dx.abs() + dy.abs() <= s;
                            let body = dx.abs() + dy.abs() <= core;
                            if arm || body {
                                out.push((dx, dy, dx.abs() + dy.abs() <= core.max(0) && s >= 2 || (dx == 0 && dy == 0)));
                            }
                        }
                    }
                }
            }
            out
        }

        /// "Pixel perfect" cleanup: removes L-shaped corner doubles from 1-px lines of each color.
        pub fn pixel_cleanup(img: &mut Image, only: Option<Rgba>) -> usize {
            let (w, h) = (img.width as i32, img.height as i32);
            let mut changed = 0;
            let get = |p: &[u8], x: i32, y: i32| -> Option<[u8; 4]> {
                if x < 0 || y < 0 || x >= w || y >= h {
                    return None;
                }
                let o = ((y * w + x) * 4) as usize;
                Some([p[o], p[o + 1], p[o + 2], p[o + 3]])
            };
            for y in 0..h {
                for x in 0..w {
                    let Some(c) = get(&img.pixels, x, y) else { continue };
                    if c[3] == 0 || only.is_some_and(|o| [o.r, o.g, o.b] != [c[0], c[1], c[2]]) {
                        continue;
                    }
                    let same = |dx: i32, dy: i32| get(&img.pixels, x + dx, y + dy) == Some(c);
                    let hn: Vec<i32> = [1, -1].into_iter().filter(|&dx| same(dx, 0)).collect();
                    let vn: Vec<i32> = [1, -1].into_iter().filter(|&dy| same(0, dy)).collect();
                    // An L corner of a 1-px line: exactly one horizontal and one vertical neighbour,
                    // and the pixel between them diagonally is not part of the line (not a blob).
                    if hn.len() == 1 && vn.len() == 1 && !same(hn[0], vn[0]) {
                        // Replace with the most common other neighbour color (or clear on the edge).
                        let mut counts: HashMap<[u8; 4], u32> = HashMap::new();
                        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                            if let Some(v) = get(&img.pixels, x + dx, y + dy).filter(|v| *v != c) {
                                *counts.entry(v).or_default() += 1;
                            }
                        }
                        let fill = counts.into_iter().max_by_key(|(_, n)| *n).map(|(v, _)| v).unwrap_or([0; 4]);
                        let o = ((y * w + x) * 4) as usize;
                        img.pixels[o..o + 4].copy_from_slice(&fill);
                        changed += 1;
                    }
                }
            }
            changed
        }
    }
    pub mod palettes {
        //! Built-in palettes from the Lospec palette list (https://lospec.com/palette-list).
        //! Each palette is credited to its author; they are widely shared for free use in pixel art.

        pub const PALETTES: &[(&str, &str, &[u32])] = &[
            (
                "aap-64",
                "AAP-64 by Adigun A. Polack",
                &[
                    0x060608FF, 0x141013FF, 0x3B1725FF, 0x73172DFF, 0xB4202AFF, 0xDF3E23FF, 0xFA6A0AFF, 0xF9A31BFF, 0xFFD541FF, 0xFFFC40FF,
                    0xD6F264FF, 0x9CDB43FF, 0x59C135FF, 0x14A02EFF, 0x1A7A3EFF, 0x24523BFF, 0x122020FF, 0x143464FF, 0x285CC4FF, 0x249FDEFF,
                    0x20D6C7FF, 0xA6FCDBFF, 0xFFFFFFFF, 0xFEF3C0FF, 0xFAD6B8FF, 0xF5A097FF, 0xE86A73FF, 0xBC4A9BFF, 0x793A80FF, 0x403353FF,
                    0x242234FF, 0x221C1AFF, 0x322B28FF, 0x71413BFF, 0xBB7547FF, 0xDBA463FF, 0xF4D29CFF, 0xDAE0EAFF, 0xB3B9D1FF, 0x8B93AFFF,
                    0x6D758DFF, 0x4A5462FF, 0x333941FF, 0x422433FF, 0x5B3138FF, 0x8E5252FF, 0xBA756AFF, 0xE9B5A3FF, 0xE3E6FFFF, 0xB9BFFBFF,
                    0x849BE4FF, 0x588DBEFF, 0x477D85FF, 0x23674EFF, 0x328464FF, 0x5DAF8DFF, 0x92DCBAFF, 0xCDF7E2FF, 0xE4D2AAFF, 0xC7B08BFF,
                    0xA08662FF, 0x796755FF, 0x5A4E44FF, 0x423934FF,
                ],
            ),
            (
                "apollo",
                "Apollo by AdamCYounis",
                &[
                    0x172038FF, 0x253A5EFF, 0x3C5E8BFF, 0x4F8FBAFF, 0x73BED3FF, 0xA4DDDBFF, 0x19332DFF, 0x25562EFF, 0x468232FF, 0x75A743FF,
                    0xA8CA58FF, 0xD0DA91FF, 0x4D2B32FF, 0x7A4841FF, 0xAD7757FF, 0xC09473FF, 0xD7B594FF, 0xE7D5B3FF, 0x341C27FF, 0x602C2CFF,
                    0x884B2BFF, 0xBE772BFF, 0xDE9E41FF, 0xE8C170FF, 0x241527FF, 0x411D31FF, 0x752438FF, 0xA53030FF, 0xCF573CFF, 0xDA863EFF,
                    0x1E1D39FF, 0x402751FF, 0x7A367BFF, 0xA23E8CFF, 0xC65197FF, 0xDF84A5FF, 0x090A14FF, 0x10141FFF, 0x151D28FF, 0x202E37FF,
                    0x394A50FF, 0x577277FF, 0x819796FF, 0xA8B5B2FF, 0xC7CFCCFF, 0xEBEDE9FF,
                ],
            ),
            (
                "endesga-32",
                "ENDESGA 32 by ENDESGA",
                &[
                    0xBE4A2FFF, 0xD77643FF, 0xEAD4AAFF, 0xE4A672FF, 0xB86F50FF, 0x733E39FF, 0x3E2731FF, 0xA22633FF, 0xE43B44FF, 0xF77622FF,
                    0xFEAE34FF, 0xFEE761FF, 0x63C74DFF, 0x3E8948FF, 0x265C42FF, 0x193C3EFF, 0x124E89FF, 0x0099DBFF, 0x2CE8F5FF, 0xFFFFFFFF,
                    0xC0CBDCFF, 0x8B9BB4FF, 0x5A6988FF, 0x3A4466FF, 0x262B44FF, 0x181425FF, 0xFF0044FF, 0x68386CFF, 0xB55088FF, 0xF6757AFF,
                    0xE8B796FF, 0xC28569FF,
                ],
            ),
            (
                "endesga-64",
                "ENDESGA 64 by ENDESGA",
                &[
                    0xFF0040FF, 0x131313FF, 0x1B1B1BFF, 0x272727FF, 0x3D3D3DFF, 0x5D5D5DFF, 0x858585FF, 0xB4B4B4FF, 0xFFFFFFFF, 0xC7CFDDFF,
                    0x92A1B9FF, 0x657392FF, 0x424C6EFF, 0x2A2F4EFF, 0x1A1932FF, 0x0E071BFF, 0x1C121CFF, 0x391F21FF, 0x5D2C28FF, 0x8A4836FF,
                    0xBF6F4AFF, 0xE69C69FF, 0xF6CA9FFF, 0xF9E6CFFF, 0xEDAB50FF, 0xE07438FF, 0xC64524FF, 0x8E251DFF, 0xFF5000FF, 0xED7614FF,
                    0xFFA214FF, 0xFFC825FF, 0xFFEB57FF, 0xD3FC7EFF, 0x99E65FFF, 0x5AC54FFF, 0x33984BFF, 0x1E6F50FF, 0x134C4CFF, 0x0C2E44FF,
                    0x00396DFF, 0x0069AAFF, 0x0098DCFF, 0x00CDF9FF, 0x0CF1FFFF, 0x94FDFFFF, 0xFDD2EDFF, 0xF389F5FF, 0xDB3FFDFF, 0x7A09FAFF,
                    0x3003D9FF, 0x0C0293FF, 0x03193FFF, 0x3B1443FF, 0x622461FF, 0x93388FFF, 0xCA52C9FF, 0xC85086FF, 0xF68187FF, 0xF5555DFF,
                    0xEA323CFF, 0xC42430FF, 0x891E2BFF, 0x571C27FF,
                ],
            ),
            (
                "lospec500",
                "Lospec500 by the Lospec community",
                &[
                    0x10121CFF, 0x2C1E31FF, 0x6B2643FF, 0xAC2847FF, 0xEC273FFF, 0x94493AFF, 0xDE5D3AFF, 0xE98537FF, 0xF3A833FF, 0x4D3533FF,
                    0x6E4C30FF, 0xA26D3FFF, 0xCE9248FF, 0xDAB163FF, 0xE8D282FF, 0xF7F3B7FF, 0x1E4044FF, 0x006554FF, 0x26854CFF, 0x5AB552FF,
                    0x9DE64EFF, 0x008B8BFF, 0x62A477FF, 0xA6CB96FF, 0xD3EED3FF, 0x3E3B65FF, 0x3859B3FF, 0x3388DEFF, 0x36C5F4FF, 0x6DEAD6FF,
                    0x5E5B8CFF, 0x8C78A5FF, 0xB0A7B8FF, 0xDECEEDFF, 0x9A4D76FF, 0xC878AFFF, 0xCC99FFFF, 0xFA6E79FF, 0xFFA2ACFF, 0xFFD1D5FF,
                    0xF6E8E0FF, 0xFFFFFFFF,
                ],
            ),
            (
                "na16",
                "Na16 by Nauris",
                &[
                    0x8C8FAEFF, 0x584563FF, 0x3E2137FF, 0x9A6348FF, 0xD79B7DFF, 0xF5EDBAFF, 0xC0C741FF, 0x647D34FF, 0xE4943AFF, 0x9D303BFF,
                    0xD26471FF, 0x70377FFF, 0x7EC4C1FF, 0x34859DFF, 0x17434BFF, 0x1F0E1CFF,
                ],
            ),
            (
                "pico-8",
                "PICO-8 by Lexaloffle Games",
                &[
                    0x000000FF, 0x1D2B53FF, 0x7E2553FF, 0x008751FF, 0xAB5236FF, 0x5F574FFF, 0xC2C3C7FF, 0xFFF1E8FF, 0xFF004DFF, 0xFFA300FF,
                    0xFFEC27FF, 0x00E436FF, 0x29ADFFFF, 0x83769CFF, 0xFF77A8FF, 0xFFCCAAFF,
                ],
            ),
            (
                "resurrect-64",
                "Resurrect 64 by Kerrie Lake",
                &[
                    0x2E222FFF, 0x3E3546FF, 0x625565FF, 0x966C6CFF, 0xAB947AFF, 0x694F62FF, 0x7F708AFF, 0x9BABB2FF, 0xC7DCD0FF, 0xFFFFFFFF,
                    0x6E2727FF, 0xB33831FF, 0xEA4F36FF, 0xF57D4AFF, 0xAE2334FF, 0xE83B3BFF, 0xFB6B1DFF, 0xF79617FF, 0xF9C22BFF, 0x7A3045FF,
                    0x9E4539FF, 0xCD683DFF, 0xE6904EFF, 0xFBB954FF, 0x4C3E24FF, 0x676633FF, 0xA2A947FF, 0xD5E04BFF, 0xFBFF86FF, 0x165A4CFF,
                    0x239063FF, 0x1EBC73FF, 0x91DB69FF, 0xCDDF6CFF, 0x313638FF, 0x374E4AFF, 0x547E64FF, 0x92A984FF, 0xB2BA90FF, 0x0B5E65FF,
                    0x0B8A8FFF, 0x0EAF9BFF, 0x30E1B9FF, 0x8FF8E2FF, 0x323353FF, 0x484A77FF, 0x4D65B4FF, 0x4D9BE6FF, 0x8FD3FFFF, 0x45293FFF,
                    0x6B3E75FF, 0x905EA9FF, 0xA884F3FF, 0xEAADEDFF, 0x753C54FF, 0xA24B6FFF, 0xCF657FFF, 0xED8099FF, 0x831C5DFF, 0xC32454FF,
                    0xF04F78FF, 0xF68181FF, 0xFCA790FF, 0xFDCBB0FF,
                ],
            ),
            (
                "slso8",
                "SLSO8 by Luis Miguel Maldonado",
                &[0x0D2B45FF, 0x203C56FF, 0x544E68FF, 0x8D697AFF, 0xD08159FF, 0xFFAA5EFF, 0xFFD4A3FF, 0xFFECD6FF],
            ),
            (
                "sweetie-16",
                "Sweetie 16 by GrafxKid",
                &[
                    0x1A1C2CFF, 0x5D275DFF, 0xB13E53FF, 0xEF7D57FF, 0xFFCD75FF, 0xA7F070FF, 0x38B764FF, 0x257179FF, 0x29366FFF, 0x3B5DC9FF,
                    0x41A6F6FF, 0x73EFF7FF, 0xF4F4F4FF, 0x94B0C2FF, 0x566C86FF, 0x333C57FF,
                ],
            ),
            (
                "twilioquest-76",
                "TwilioQuest 76 by Kerrie Lake",
                &[
                    0xFFFFFFFF, 0xEAEAE8FF, 0xCECAC9FF, 0xABAFB9FF, 0xA18897FF, 0x756276FF, 0x5D4660FF, 0x4C3250FF, 0x432641FF, 0x28192FFF,
                    0xFB7575FF, 0xFB3B64FF, 0xC83157FF, 0x8E375CFF, 0x4F2351FF, 0x351544FF, 0xF74A53FF, 0xF22F46FF, 0xBC1642FF, 0xFCC539FF,
                    0xF87B1BFF, 0xF8401BFF, 0xBD2709FF, 0x7C122BFF, 0xFFE08BFF, 0xFAC05AFF, 0xEB8F48FF, 0xD17441FF, 0xC75239FF, 0xB12935FF,
                    0xFDBD8FFF, 0xF0886BFF, 0xD36853FF, 0xAE454AFF, 0x8C3132FF, 0x542323FF, 0xA85848FF, 0x83404CFF, 0x67314BFF, 0x3F2323FF,
                    0xD49577FF, 0x9F705AFF, 0x845750FF, 0x633B3FFF, 0x7BD7A9FF, 0x52B281FF, 0x148568FF, 0x146756FF, 0x22474CFF, 0x102F34FF,
                    0xEBFF8BFF, 0xB3E363FF, 0x4CBD56FF, 0x2F8735FF, 0x0B5931FF, 0x97BF6EFF, 0x899F66FF, 0x61855AFF, 0x4C6051FF, 0x73DFF2FF,
                    0x2ABBD0FF, 0x315DCDFF, 0x472A9CFF, 0xA0D8D7FF, 0x7DBEFAFF, 0x668FAFFF, 0x585D81FF, 0x45365DFF, 0xF6BAFEFF, 0xD59FF4FF,
                    0xB070EBFF, 0x7C3CE1FF, 0xDBCFB1FF, 0xA9A48DFF, 0x7B8382FF, 0x5F5F6EFF,
                ],
            ),
            (
                "vinik24",
                "Vinik24 by Vinik",
                &[
                    0x000000FF, 0x6F6776FF, 0x9A9A97FF, 0xC5CCB8FF, 0x8B5580FF, 0xC38890FF, 0xA593A5FF, 0x666092FF, 0x9A4F50FF, 0xC28D75FF,
                    0x7CA1C0FF, 0x416AA3FF, 0x8D6268FF, 0xBE955CFF, 0x68ACA9FF, 0x387080FF, 0x6E6962FF, 0x93A167FF, 0x6EAA78FF, 0x557064FF,
                    0x9D9F7FFF, 0x7E9E99FF, 0x5D6872FF, 0x433455FF,
                ],
            ),
        ];
    }

    use crate::pixel::{PixelGrid, Rgba, TRANSPARENT};
    use crate::renderer::Image;

    // ---- color science ----------------------------------------------------------------------

    fn to_lin(c: u8) -> f32 {
        let v = c as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    }

    fn from_lin(v: f32) -> u8 {
        let v = v.clamp(0.0, 1.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (s * 255.0 + 0.5) as u8
    }

    /// OKLab (L, a, b) of an sRGB color.
    pub fn oklab(c: Rgba) -> [f32; 3] {
        let (r, g, b) = (to_lin(c.r), to_lin(c.g), to_lin(c.b));
        let l = (0.412_221_47 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
        let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
        let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
        [
            0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
            1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
            0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
        ]
    }

    pub fn from_oklab(lab: [f32; 3], alpha: u8) -> Rgba {
        let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
        let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
        let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
        let (l, m, s) = (l * l * l, m * m * m, s * s * s);
        Rgba::new(
            from_lin(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s),
            from_lin(-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s),
            from_lin(-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s),
            alpha,
        )
    }

    pub(crate) fn lch(c: Rgba) -> [f32; 3] {
        let [l, a, b] = oklab(c);
        [l, (a * a + b * b).sqrt(), b.atan2(a).to_degrees().rem_euclid(360.0)]
    }

    fn lab_to_linear(lab: [f32; 3]) -> [f32; 3] {
        let l = lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2];
        let m = lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2];
        let s = lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2];
        let (l, m, s) = (l * l * l, m * m * m, s * s * s);
        [
            4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
            -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
            -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
        ]
    }

    /// OKLCH → sRGB with gamut mapping by chroma reduction (keeps hue and lightness exact).
    pub(crate) fn from_lch(l: f32, c: f32, h: f32, alpha: u8) -> Rgba {
        let hr = h.to_radians();
        let inside = |c: f32| lab_to_linear([l, c * hr.cos(), c * hr.sin()]).iter().all(|v| (-0.001..=1.001).contains(v));
        let mut chroma = c;
        if !inside(chroma) {
            let (mut lo, mut hi) = (0.0f32, c);
            for _ in 0..20 {
                let mid = (lo + hi) / 2.0;
                if inside(mid) { lo = mid } else { hi = mid }
            }
            chroma = lo;
        }
        from_oklab([l, chroma * hr.cos(), chroma * hr.sin()], alpha)
    }

    pub(crate) fn hue_toward(h: f32, target: f32, amount: f32) -> f32 {
        let d = ((target - h + 540.0) % 360.0) - 180.0;
        (h + d.clamp(-amount.abs(), amount.abs())).rem_euclid(360.0)
    }

    /// A hue-shifted color ramp around `base` (the base sits in the middle): shadows drift toward
    /// cool purple-blue and gain saturation, highlights drift toward warm yellow and lose some
    /// saturation — the classic pixel-art shading recipe.
    pub fn ramp(base: Rgba, steps: usize, hue_shift: f32, contrast: f32) -> Vec<Rgba> {
        let steps = steps.clamp(2, 16);
        let [l0, c0, h0] = lch(base);
        let mid = (steps - 1) as f32 / 2.0;
        (0..steps)
            .map(|i| {
                let t = (i as f32 - mid) / mid.max(1.0); // -1 (dark) .. +1 (light)
                let l = (l0 + t * contrast * if t < 0.0 { l0.min(0.55) } else { (1.0 - l0).min(0.45) }).clamp(0.08, 0.98);
                let (h, c) = if t < 0.0 {
                    (hue_toward(h0, 285.0, hue_shift * -t), c0 * (1.0 + 0.25 * -t))
                } else {
                    (hue_toward(h0, 85.0, hue_shift * t), c0 * (1.0 - 0.35 * t))
                };
                from_lch(l, c, h, base.a)
            })
            .collect()
    }

    /// Darker, cooler companion color used for selective outlines.
    pub fn outline_color(c: Rgba) -> Rgba {
        let [l, ch, h] = lch(c);
        from_lch((l * 0.42).max(0.12), (ch * 1.1).min(0.2), hue_toward(h, 285.0, 25.0), 255)
    }

    pub fn palette_by_name(name: &str) -> Option<Vec<Rgba>> {
        palettes::PALETTES
            .iter()
            .find(|(n, _, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, _, c)| c.iter().map(|&v| Rgba::from_u32(v)).collect())
    }

    // ---- quantization -------------------------------------------------------------------------

    /// Median-cut palette of at most `n` colors from (color, weight) samples.
    pub fn median_cut(samples: &[(Rgba, u32)], n: usize) -> Vec<Rgba> {
        if samples.is_empty() {
            return Vec::new();
        }
        let mut boxes: Vec<Vec<(Rgba, u32)>> = vec![samples.to_vec()];
        while boxes.len() < n {
            // Split the box with the widest channel range (weighted by population).
            let (bi, ch, range) = boxes
                .iter()
                .enumerate()
                .map(|(i, b)| {
                    let mut best = (0usize, 0i32);
                    for ch in 0..3 {
                        let get = |c: &Rgba| [c.r, c.g, c.b][ch] as i32;
                        let lo = b.iter().map(|(c, _)| get(c)).min().unwrap();
                        let hi = b.iter().map(|(c, _)| get(c)).max().unwrap();
                        if hi - lo > best.1 {
                            best = (ch, hi - lo);
                        }
                    }
                    let pop: u32 = b.iter().map(|(_, w)| w).sum();
                    (i, best.0, best.1 as f32 * (pop as f32).sqrt())
                })
                .max_by(|a, b| a.2.total_cmp(&b.2))
                .unwrap();
            if range <= 0.0 || boxes[bi].len() < 2 {
                break;
            }
            let mut b = boxes.swap_remove(bi);
            b.sort_by_key(|(c, _)| [c.r, c.g, c.b][ch]);
            let total: u32 = b.iter().map(|(_, w)| w).sum();
            let mut acc = 0;
            let mut cut = 1;
            for (i, (_, w)) in b.iter().enumerate() {
                acc += w;
                if acc * 2 >= total {
                    cut = (i + 1).clamp(1, b.len() - 1);
                    break;
                }
            }
            let tail = b.split_off(cut);
            boxes.push(b);
            boxes.push(tail);
        }
        boxes
            .iter()
            .map(|b| {
                let total: f32 = b.iter().map(|(_, w)| *w as f32).sum();
                let mut acc = [0.0f32; 3];
                for (c, w) in b {
                    let lab = oklab(*c);
                    for k in 0..3 {
                        acc[k] += lab[k] * *w as f32;
                    }
                }
                from_oklab([acc[0] / total, acc[1] / total, acc[2] / total], 255)
            })
            .collect()
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Dither {
        None,
        Bayer2,
        Bayer4,
        Bayer8,
    }

    impl Dither {
        pub fn parse(s: &str) -> Option<Dither> {
            Some(match s {
                "none" | "off" => Dither::None,
                "bayer2" => Dither::Bayer2,
                "bayer4" | "ordered" => Dither::Bayer4,
                "bayer8" => Dither::Bayer8,
                _ => return None,
            })
        }

        /// Threshold in -0.5..0.5 at a pixel.
        fn threshold(self, x: u32, y: u32) -> f32 {
            let n = match self {
                Dither::None => return 0.0,
                Dither::Bayer2 => 2,
                Dither::Bayer4 => 4,
                Dither::Bayer8 => 8,
            };
            bayer(x % n, y % n, n) as f32 / (n * n) as f32 - 0.5 + 0.5 / (n * n) as f32
        }
    }

    /// Recursive Bayer matrix value at (x, y) for an n×n matrix (n power of two).
    fn bayer(x: u32, y: u32, n: u32) -> u32 {
        if n == 1 {
            return 0;
        }
        let h = n / 2;
        let q = match (x >= h, y >= h) {
            (false, false) => 0,
            (true, true) => 1,
            (true, false) => 2,
            (false, true) => 3,
        };
        4 * bayer(x % h, y % h, h) + q
    }

    /// Maps a color to the perceptually nearest palette entry.
    pub struct Mapper {
        palette: Vec<(Rgba, [f32; 3])>,
    }

    impl Mapper {
        pub fn new(palette: &[Rgba]) -> Mapper {
            Mapper { palette: palette.iter().filter(|c| c.a > 0).map(|&c| (c, oklab(c))).collect() }
        }

        pub fn nearest(&self, c: Rgba) -> Rgba {
            let lab = oklab(c);
            let mut best = (f32::MAX, c);
            for (p, pl) in &self.palette {
                let d = (lab[0] - pl[0]).powi(2) * 1.4 + (lab[1] - pl[1]).powi(2) + (lab[2] - pl[2]).powi(2);
                if d < best.0 {
                    best = (d, *p);
                }
            }
            Rgba::new(best.1.r, best.1.g, best.1.b, c.a)
        }

        /// Nearest color after an ordered-dither offset (`strength` in sRGB steps, ~24 typical).
        pub fn dithered(&self, c: Rgba, x: u32, y: u32, dither: Dither, strength: f32) -> Rgba {
            let t = dither.threshold(x, y) * strength;
            let s = |v: u8| (v as f32 + t).clamp(0.0, 255.0) as u8;
            self.nearest(Rgba::new(s(c.r), s(c.g), s(c.b), c.a))
        }
    }

    /// Quantizes an RGBA image in place. Alpha is snapped to 0/255 (pixel art has no soft edges).
    pub fn quantize_image(img: &mut Image, palette: &[Rgba], dither: Dither, strength: f32) {
        let mapper = Mapper::new(palette);
        for y in 0..img.height {
            for x in 0..img.width {
                let o = ((y * img.width + x) * 4) as usize;
                let p = &mut img.pixels[o..o + 4];
                if p[3] < 128 {
                    p.copy_from_slice(&[0, 0, 0, 0]);
                    continue;
                }
                let q = mapper.dithered(Rgba::new(p[0], p[1], p[2], 255), x, y, dither, strength);
                p.copy_from_slice(&[q.r, q.g, q.b, 255]);
            }
        }
    }

    pub fn image_samples(img: &Image) -> Vec<(Rgba, u32)> {
        let mut map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();
        for p in img.pixels.as_chunks::<4>().0 {
            if p[3] >= 128 {
                *map.entry(Rgba::new(p[0], p[1], p[2], 255).to_u32()).or_default() += 1;
            }
        }
        let mut v: Vec<(Rgba, u32)> = map.into_iter().map(|(c, n)| (Rgba::from_u32(c), n)).collect();
        v.sort_by_key(|(c, _)| c.to_u32());
        v
    }

    /// Selective 1-px outline around an RGBA image (exterior), optionally with diagonal corners.
    pub fn outline_image(img: &mut Image, color: Option<Rgba>, corners: bool) {
        let (w, h) = (img.width as i32, img.height as i32);
        let src = img.pixels.clone();
        let alpha = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && src[((y * w + x) * 4 + 3) as usize] > 0;
        for y in 0..h {
            for x in 0..w {
                if alpha(x, y) {
                    continue;
                }
                let mut best: Option<Rgba> = None;
                let n4 = [(1, 0), (-1, 0), (0, 1), (0, -1)];
                let n8 = [(1, 1), (-1, 1), (1, -1), (-1, -1)];
                for (dx, dy) in n4.iter().chain(if corners { n8.iter() } else { [].iter() }) {
                    let (nx, ny) = (x + dx, y + dy);
                    if alpha(nx, ny) {
                        let o = ((ny * w + nx) * 4) as usize;
                        let c = Rgba::new(src[o], src[o + 1], src[o + 2], 255);
                        if best.is_none_or(|b| lch(c)[0] < lch(b)[0]) {
                            best = Some(c);
                        }
                    }
                }
                if let Some(b) = best {
                    let c = color.unwrap_or_else(|| outline_color(b));
                    let o = ((y * w + x) * 4) as usize;
                    img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, 255]);
                }
            }
        }
    }

    // ---- cel operations ---------------------------------------------------------------------------

    pub fn grid_to_image(g: &PixelGrid) -> Image {
        let mut img = Image::new(g.width(), g.height());
        g.for_each_opaque(|x, y, v| {
            let c = Rgba::from_u32(v);
            let o = ((y * g.width() + x) * 4) as usize;
            img.pixels[o..o + 4].copy_from_slice(&[c.r, c.g, c.b, c.a]);
        });
        img
    }

    /// Writes an image back into a cel; returns the number of changed pixels.
    pub fn image_to_grid(img: &Image, g: &mut PixelGrid) -> usize {
        let mut changed = 0;
        for y in 0..img.height.min(g.height()) {
            for x in 0..img.width.min(g.width()) {
                let o = ((y * img.width + x) * 4) as usize;
                let p = &img.pixels[o..o + 4];
                let v = if p[3] == 0 { TRANSPARENT } else { Rgba::new(p[0], p[1], p[2], p[3]).to_pixel() };
                changed += g.set(x as i64, y as i64, v) as usize;
            }
        }
        changed
    }

    /// Height from the distance to the sprite edge plus luminance, turned into a tangent-space
    /// normal map (RGB = normal, A = sprite alpha) for 2D dynamic lighting in game engines.
    pub fn normal_map(img: &Image, strength: f32, bevel: f32) -> Image {
        let (w, h) = (img.width as usize, img.height as usize);
        let solid = |i: usize| img.pixels[i * 4 + 3] > 0;
        // Chamfer distance to transparency.
        let mut dist: Vec<f32> = (0..w * h).map(|i| if solid(i) { 1e6 } else { 0.0 }).collect();
        for pass in 0..2 {
            let ys: Box<dyn Iterator<Item = usize>> = if pass == 0 { Box::new(0..h) } else { Box::new((0..h).rev()) };
            for y in ys {
                let xs: Vec<usize> = if pass == 0 { (0..w).collect() } else { (0..w).rev().collect() };
                for x in xs {
                    let i = y * w + x;
                    if dist[i] == 0.0 {
                        continue;
                    }
                    let mut d = dist[i];
                    for (dx, dy, c) in [(-1i32, 0i32, 1.0f32), (0, -1, 1.0), (-1, -1, 1.414), (1, -1, 1.414)] {
                        let (dx, dy) = if pass == 0 { (dx, dy) } else { (-dx, -dy) };
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        let nd =
                            if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 { 0.0 } else { dist[ny as usize * w + nx as usize] };
                        d = d.min(nd + c);
                    }
                    dist[i] = d;
                }
            }
        }
        let height = |x: i32, y: i32| -> f32 {
            if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
                return 0.0;
            }
            let i = y as usize * w + x as usize;
            if !solid(i) {
                return 0.0;
            }
            let edge = (dist[i] / bevel.max(0.5)).min(1.0);
            let lum = (img.pixels[i * 4] as f32 * 0.3 + img.pixels[i * 4 + 1] as f32 * 0.59 + img.pixels[i * 4 + 2] as f32 * 0.11) / 255.0;
            edge.sqrt() * 0.8 + lum * 0.2
        };
        let mut out = Image::new(w as u32, h as u32);
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                let i = y as usize * w + x as usize;
                if !solid(i) {
                    continue;
                }
                let dx = height(x + 1, y) - height(x - 1, y);
                let dy = height(x, y + 1) - height(x, y - 1);
                let (nx, ny, nz) = (-dx * strength, dy * strength, 1.0f32);
                let l = (nx * nx + ny * ny + nz * nz).sqrt();
                let enc = |v: f32| ((v / l) * 0.5 + 0.5).clamp(0.0, 1.0) * 255.0;
                let o = i * 4;
                out.pixels[o..o + 4].copy_from_slice(&[enc(nx) as u8, enc(ny) as u8, enc(nz) as u8, img.pixels[o + 3]]);
            }
        }
        out
    }

    /// Ordered dither pattern test: true where `color2` should be used for a 0..16 density level.
    pub fn dither_pick(x: i64, y: i64, level: u32) -> bool {
        bayer(x.rem_euclid(4) as u32, y.rem_euclid(4) as u32, 4) < level.min(16)
    }
}
pub mod project {
    //! The project document: canvas size, palette, layers, frames, cels and tags.
    //!
    //! A `Document` is cheap to clone: cels are `Arc<PixelGrid>` whose tiles are themselves
    //! shared, so undo snapshots and transactions copy pointers, not pixels.

    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::animation::{DEFAULT_FRAME_MS, Frame, Tag};
    use crate::gamedev::Slice;
    use crate::gamedev::tiles::{Tilemap, Tileset};
    use crate::layers::Layer;
    use crate::pixel::{Palette, PixelGrid, Rgba};
    use crate::voxel::{Material, Texture, VoxelModel};

    pub const MAX_CANVAS: u32 = 4096;
    pub const MAX_LAYERS: usize = 1024;
    /// Safety valve against runaway requests, not a creative limit.
    pub const MAX_FRAMES: usize = 100_000;

    pub type CelKey = (u32, u32); // (layer id, frame id)

    /// A named material ramp, dark → light (`Rgba::to_pixel` values).
    #[derive(Debug, Clone, PartialEq)]
    pub struct Ramp {
        pub name: String,
        pub colors: Vec<u32>,
    }

    #[derive(Debug, Clone)]
    pub struct Document {
        pub name: String,
        pub width: u32,
        pub height: u32,
        pub palette: Palette,
        /// Bottom-to-top order.
        pub layers: Vec<Layer>,
        pub frames: Vec<Frame>,
        pub cels: HashMap<CelKey, Arc<PixelGrid>>,
        pub tags: Vec<Tag>,
        pub active_layer: u32,
        pub active_frame: u32,
        pub next_layer_id: u32,
        pub next_frame_id: u32,
        /// 3D pixel (voxel) models living in this project.
        pub models: Vec<VoxelModel>,
        /// PBR materials shared by all models. Index 0 is the default material.
        pub materials: Vec<Material>,
        /// Texture maps (imported PNGs or procedural sets).
        pub textures: Vec<Arc<Texture>>,
        /// Named per-frame rectangles: hitboxes, hurtboxes, pivots, attach points.
        pub slices: Vec<Slice>,
        pub tilesets: Vec<Arc<Tileset>>,
        pub tilemaps: Vec<Arc<Tilemap>>,
        /// Material ramps (dark → light): the shade brush and palette-locked shading use them.
        pub ramps: Vec<Ramp>,
    }

    /// Small neutral starter palette so humans have something to pick from immediately.
    pub fn starter_palette() -> Palette {
        let hex = [
            "#000000", "#FFFFFF", "#3A3A46", "#7A7A86", "#BEBEC8", "#5B2C1E", "#A0522D", "#E3A06B", "#F6D7B0", "#C0283C", "#F2545B",
            "#F5A623", "#F7E26B", "#2E8B57", "#7BD389", "#1E3A8A", "#3B82F6", "#7DD3FC", "#6D28D9", "#C084FC",
        ];
        let mut colors = vec![Rgba::TRANSPARENT];
        colors.extend(hex.iter().map(|h| Rgba::parse_hex(h).expect("valid starter color")));
        Palette::from_colors(colors).expect("starter palette fits")
    }

    pub fn validate_size(width: i64, height: i64) -> Result<(u32, u32), String> {
        if width < 1 || height < 1 || width > MAX_CANVAS as i64 || height > MAX_CANVAS as i64 {
            return Err(format!("canvas size must be between 1x1 and {MAX_CANVAS}x{MAX_CANVAS}"));
        }
        Ok((width as u32, height as u32))
    }

    impl Document {
        pub fn new(width: u32, height: u32) -> Document {
            let mut doc = Document {
                name: "Untitled".into(),
                width,
                height,
                palette: starter_palette(),
                layers: Vec::new(),
                frames: Vec::new(),
                cels: HashMap::new(),
                tags: Vec::new(),
                active_layer: 0,
                active_frame: 0,
                next_layer_id: 1,
                next_frame_id: 1,
                models: Vec::new(),
                materials: vec![Material::new("default")],
                textures: Vec::new(),
                slices: Vec::new(),
                tilesets: Vec::new(),
                tilemaps: Vec::new(),
                ramps: Vec::new(),
            };
            let l = doc.insert_layer("Layer 1", 0);
            let f = doc.insert_frame(0, DEFAULT_FRAME_MS);
            doc.active_layer = l;
            doc.active_frame = f;
            doc
        }

        pub fn layer_index(&self, id: u32) -> Option<usize> {
            self.layers.iter().position(|l| l.id == id)
        }

        pub fn frame_index(&self, id: u32) -> Option<usize> {
            self.frames.iter().position(|f| f.id == id)
        }

        pub fn cel(&self, layer_id: u32, frame_id: u32) -> Option<&PixelGrid> {
            self.cels.get(&(layer_id, frame_id)).map(|c| c.as_ref())
        }

        /// Mutable cel, created on first write. Copy-on-write if the cel is shared.
        /// Grid size of a layer (canvas size × its density).
        pub fn layer_dims(&self, layer_id: u32) -> (u32, u32) {
            let d = self.layers.iter().find(|l| l.id == layer_id).map(|l| l.density.max(1) as u32).unwrap_or(1);
            (self.width * d, self.height * d)
        }

        /// Highest density among the given layers (the resolution needed to show all detail).
        pub fn max_density(&self) -> u32 {
            self.layers.iter().map(|l| l.density.max(1) as u32).max().unwrap_or(1)
        }

        pub fn cel_mut(&mut self, layer_id: u32, frame_id: u32) -> &mut PixelGrid {
            let (w, h) = self.layer_dims(layer_id);
            let arc = self.cels.entry((layer_id, frame_id)).or_insert_with(|| Arc::new(PixelGrid::new(w, h)));
            Arc::make_mut(arc)
        }

        pub fn insert_layer(&mut self, name: &str, index: usize) -> u32 {
            let id = self.next_layer_id;
            self.next_layer_id += 1;
            let index = index.min(self.layers.len());
            self.layers.insert(index, Layer::new(id, name));
            id
        }

        pub fn remove_layer(&mut self, index: usize) -> Layer {
            let layer = self.layers.remove(index);
            self.cels.retain(|(l, _), _| *l != layer.id);
            if self.active_layer == layer.id {
                let i = index.min(self.layers.len().saturating_sub(1));
                self.active_layer = self.layers.get(i).map(|l| l.id).unwrap_or(0);
            }
            layer
        }

        pub fn insert_frame(&mut self, index: usize, duration_ms: u32) -> u32 {
            let id = self.next_frame_id;
            self.next_frame_id += 1;
            let index = index.min(self.frames.len());
            self.frames.insert(index, Frame { id, duration_ms });
            id
        }

        /// Copies a frame; its cels are shared until one side is edited.
        pub fn duplicate_frame(&mut self, index: usize, to: usize) -> u32 {
            let src = self.frames[index].clone();
            let id = self.insert_frame(to, src.duration_ms);
            let copies: Vec<(CelKey, Arc<PixelGrid>)> =
                self.cels.iter().filter(|((_, f), _)| *f == src.id).map(|((l, _), grid)| ((*l, id), Arc::clone(grid))).collect();
            self.cels.extend(copies);
            id
        }

        pub fn remove_frame(&mut self, index: usize) -> Frame {
            let frame = self.frames.remove(index);
            self.cels.retain(|(_, f), _| *f != frame.id);
            if self.active_frame == frame.id {
                let i = index.min(self.frames.len().saturating_sub(1));
                self.active_frame = self.frames.get(i).map(|f| f.id).unwrap_or(0);
            }
            // Repair tags whose end points vanished; drop tags that became empty.
            let remaining: Vec<u32> = self.frames.iter().map(|f| f.id).collect();
            let neighbour = remaining.get(index.min(remaining.len().saturating_sub(1))).copied();
            let prev = if index > 0 { remaining.get(index - 1).copied() } else { None };
            self.tags.retain_mut(|t| {
                if t.from_id == frame.id && t.to_id == frame.id {
                    return false;
                }
                if t.from_id == frame.id {
                    match neighbour {
                        Some(n) => t.from_id = n,
                        None => return false,
                    }
                }
                if t.to_id == frame.id {
                    match prev {
                        Some(p) => t.to_id = p,
                        None => return false,
                    }
                }
                true
            });
            // Slice keys on the removed frame move to the next frame unless it has its own key.
            let next = remaining.get(index).copied();
            for s in &mut self.slices {
                let has_next = next.is_some_and(|n| s.keys.iter().any(|k| k.frame_id == n));
                s.keys.retain_mut(|k| {
                    if k.frame_id != frame.id {
                        return true;
                    }
                    match next {
                        Some(n) if !has_next => {
                            k.frame_id = n;
                            true
                        }
                        _ => false,
                    }
                });
            }
            self.slices.retain(|s| !s.keys.is_empty());
            frame
        }

        /// The slice key in effect at a frame index (the latest key at or before it).
        pub fn slice_key_at(&self, slice: &Slice, frame_index: usize) -> Option<crate::gamedev::SliceKey> {
            slice
                .keys
                .iter()
                .filter_map(|k| self.frame_index(k.frame_id).map(|i| (i, *k)))
                .filter(|(i, _)| *i <= frame_index)
                .max_by_key(|(i, _)| *i)
                .map(|(_, k)| k)
        }

        /// Opaque pixel count of a cel, used by inspection tools.
        pub fn cel_stats(&self, layer_id: u32, frame_id: u32) -> (usize, Option<(u32, u32, u32, u32)>) {
            match self.cel(layer_id, frame_id) {
                None => (0, None),
                Some(g) => {
                    let mut n = 0;
                    g.for_each_opaque(|_, _, _| n += 1);
                    (n, g.bounds())
                }
            }
        }

        /// Crops/extends the canvas. Pixels keep their position relative to the top-left
        /// corner shifted by the given offset.
        pub fn resize(&mut self, width: u32, height: u32, offset_x: i64, offset_y: i64) {
            let density: HashMap<u32, u32> = self.layers.iter().map(|l| (l.id, l.density.max(1) as u32)).collect();
            for ((layer, _), cel) in self.cels.iter_mut() {
                let d = density.get(layer).copied().unwrap_or(1);
                *cel = Arc::new(cel.resized(width * d, height * d, offset_x * d as i64, offset_y * d as i64));
            }
            self.width = width;
            self.height = height;
        }
    }
}
pub mod raster {
    //! Deterministic raster algorithms operating on palette indices.
    //!
    //! Every function returns the number of pixels whose value actually changed.

    use crate::pixel::{PixelGrid, TRANSPARENT};

    /// Axis-aligned rectangle in canvas coordinates.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Region {
        pub x: i64,
        pub y: i64,
        pub w: i64,
        pub h: i64,
    }

    impl Region {
        pub fn full(grid: &PixelGrid) -> Region {
            Region { x: 0, y: 0, w: grid.width() as i64, h: grid.height() as i64 }
        }

        /// Intersects with the grid; `None` when nothing remains.
        pub fn clip(self, grid: &PixelGrid) -> Option<Region> {
            let x0 = self.x.max(0);
            let y0 = self.y.max(0);
            let x1 = (self.x + self.w).min(grid.width() as i64);
            let y1 = (self.y + self.h).min(grid.height() as i64);
            (x1 > x0 && y1 > y0).then_some(Region { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
        }
    }

    /// How copied pixels are combined with the destination.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum PasteMode {
        /// Transparent source pixels leave the destination untouched.
        Over,
        /// Destination is overwritten exactly, including transparent pixels.
        Replace,
    }

    /// Bresenham line, inclusive of both end points.
    pub fn line(grid: &mut PixelGrid, x0: i64, y0: i64, x1: i64, y1: i64, value: u32) -> usize {
        let mut changed = 0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        // Guard against absurd coordinates: never step more than the canvas diagonal plus the span.
        let max_steps = dx.max(-dy) + 1;
        for _ in 0..max_steps {
            if grid.set(x, y, value) {
                changed += 1;
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
        changed
    }

    pub fn rectangle(grid: &mut PixelGrid, r: Region, value: u32, filled: bool) -> usize {
        if r.w <= 0 || r.h <= 0 {
            return 0;
        }
        let mut changed = 0;
        if filled {
            let Some(c) = r.clip(grid) else { return 0 };
            for y in c.y..c.y + c.h {
                for x in c.x..c.x + c.w {
                    changed += grid.set(x, y, value) as usize;
                }
            }
        } else {
            let (x1, y1) = (r.x + r.w - 1, r.y + r.h - 1);
            changed += line(grid, r.x, r.y, x1, r.y, value);
            changed += line(grid, r.x, y1, x1, y1, value);
            if r.h > 2 {
                changed += line(grid, r.x, r.y + 1, r.x, y1 - 1, value);
                changed += line(grid, x1, r.y + 1, x1, y1 - 1, value);
            }
        }
        changed
    }

    /// Ellipse inscribed in the bounding box, symmetric and gap-free.
    pub fn ellipse(grid: &mut PixelGrid, r: Region, value: u32, filled: bool) -> usize {
        if r.w <= 0 || r.h <= 0 {
            return 0;
        }
        let cx = r.x as f64 + r.w as f64 / 2.0;
        let cy = r.y as f64 + r.h as f64 / 2.0;
        let rx = r.w as f64 / 2.0;
        let ry = r.h as f64 / 2.0;
        // Span of each row: pixels whose centers lie inside the ellipse.
        let span = |y: i64| -> Option<(i64, i64)> {
            let dy = (y as f64 + 0.5 - cy) / ry;
            let t = 1.0 - dy * dy;
            if t < 0.0 {
                return None;
            }
            let half = rx * t.sqrt();
            let x0 = (cx - half - 0.5).ceil() as i64;
            let x1 = (cx + half - 0.5).floor() as i64;
            let (x0, x1) = (x0.max(r.x), x1.min(r.x + r.w - 1));
            (x1 >= x0).then_some((x0, x1))
        };
        let rows: Vec<Option<(i64, i64)>> = (r.y..r.y + r.h).map(span).collect();
        let mut changed = 0;
        for (i, row) in rows.iter().enumerate() {
            let y = r.y + i as i64;
            let Some((x0, x1)) = *row else { continue };
            if filled {
                for x in x0..=x1 {
                    changed += grid.set(x, y, value) as usize;
                }
                continue;
            }
            // Outline: a pixel is on the edge if a 4-neighbour lies outside the shape.
            let inside = |xx: i64, yy: i64| -> bool {
                if yy < r.y || yy >= r.y + r.h {
                    return false;
                }
                matches!(rows[(yy - r.y) as usize], Some((a, b)) if xx >= a && xx <= b)
            };
            for x in x0..=x1 {
                if !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1) {
                    changed += grid.set(x, y, value) as usize;
                }
            }
        }
        changed
    }

    /// Fill starting at (x, y). `contiguous = false` replaces every pixel of the seed's value.
    pub fn fill(grid: &mut PixelGrid, x: i64, y: i64, value: u32, contiguous: bool) -> usize {
        if !grid.in_bounds(x, y) {
            return 0;
        }
        let target = grid.get(x, y);
        if target == value {
            return 0;
        }
        if !contiguous {
            return replace(grid, Region::full(grid), target, value);
        }
        let (w, h) = (grid.width() as i64, grid.height() as i64);
        let mut changed = 0;
        let mut stack = vec![(x, y)];
        while let Some((px, py)) = stack.pop() {
            // Scanline fill: expand left/right, then queue the rows above and below.
            if grid.get(px, py) != target {
                continue;
            }
            let mut left = px;
            while left > 0 && grid.get(left - 1, py) == target {
                left -= 1;
            }
            let mut right = px;
            while right + 1 < w && grid.get(right + 1, py) == target {
                right += 1;
            }
            for xx in left..=right {
                changed += grid.set(xx, py, value) as usize;
            }
            for ny in [py - 1, py + 1] {
                if ny < 0 || ny >= h {
                    continue;
                }
                let mut xx = left;
                while xx <= right {
                    if grid.get(xx, ny) == target {
                        stack.push((xx, ny));
                        while xx <= right && grid.get(xx, ny) == target {
                            xx += 1;
                        }
                    } else {
                        xx += 1;
                    }
                }
            }
        }
        changed
    }

    pub fn replace(grid: &mut PixelGrid, region: Region, from: u32, to: u32) -> usize {
        if from == to {
            return 0;
        }
        let Some(c) = region.clip(grid) else { return 0 };
        let mut changed = 0;
        for y in c.y..c.y + c.h {
            for x in c.x..c.x + c.w {
                if grid.get(x, y) == from {
                    changed += grid.set(x, y, to) as usize;
                }
            }
        }
        changed
    }

    /// Copies the pixels of a region row-major. Out-of-bounds pixels read as transparent.
    pub fn extract(grid: &PixelGrid, r: Region) -> Vec<u32> {
        let mut out = Vec::with_capacity((r.w.max(0) * r.h.max(0)) as usize);
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                out.push(grid.get(x, y));
            }
        }
        out
    }

    pub fn paste(grid: &mut PixelGrid, x: i64, y: i64, w: i64, h: i64, data: &[u32], mode: PasteMode) -> usize {
        let mut changed = 0;
        for row in 0..h {
            for col in 0..w {
                let v = data[(row * w + col) as usize];
                if v == TRANSPARENT && mode == PasteMode::Over {
                    continue;
                }
                changed += grid.set(x + col, y + row, v) as usize;
            }
        }
        changed
    }

    pub fn clear_region(grid: &mut PixelGrid, r: Region) -> usize {
        rectangle(grid, r, TRANSPARENT, true)
    }

    pub fn flip_horizontal(grid: &mut PixelGrid, r: Region) -> usize {
        let Some(c) = r.clip(grid) else { return 0 };
        let data = extract(grid, c);
        let mut flipped = vec![TRANSPARENT; data.len()];
        for row in 0..c.h {
            for col in 0..c.w {
                flipped[(row * c.w + col) as usize] = data[(row * c.w + (c.w - 1 - col)) as usize];
            }
        }
        paste(grid, c.x, c.y, c.w, c.h, &flipped, PasteMode::Replace)
    }

    pub fn flip_vertical(grid: &mut PixelGrid, r: Region) -> usize {
        let Some(c) = r.clip(grid) else { return 0 };
        let data = extract(grid, c);
        let mut flipped = vec![TRANSPARENT; data.len()];
        for row in 0..c.h {
            for col in 0..c.w {
                flipped[(row * c.w + col) as usize] = data[((c.h - 1 - row) * c.w + col) as usize];
            }
        }
        paste(grid, c.x, c.y, c.w, c.h, &flipped, PasteMode::Replace)
    }

    /// Rotates a region clockwise by 90, 180 or 270 degrees. 90/270 require a square region.
    pub fn rotate(grid: &mut PixelGrid, r: Region, degrees: i64) -> Result<usize, String> {
        let Some(c) = r.clip(grid) else { return Ok(0) };
        let deg = degrees.rem_euclid(360);
        if deg == 0 {
            return Ok(0);
        }
        if deg % 90 != 0 {
            return Err("rotation must be a multiple of 90 degrees".into());
        }
        if deg != 180 && c.w != c.h {
            return Err(format!("90/270 degree rotation needs a square region (got {}x{})", c.w, c.h));
        }
        let n = c.w;
        let data = extract(grid, c);
        let mut out = vec![TRANSPARENT; data.len()];
        for row in 0..c.h {
            for col in 0..c.w {
                let src = data[(row * c.w + col) as usize];
                let (nr, nc) = match deg {
                    90 => (col, n - 1 - row),
                    180 => (c.h - 1 - row, c.w - 1 - col),
                    _ => (n - 1 - col, row),
                };
                out[(nr * c.w + nc) as usize] = src;
            }
        }
        Ok(paste(grid, c.x, c.y, c.w, c.h, &out, PasteMode::Replace))
    }
}
pub mod renderer {
    //! Deterministic integer compositing of layers into straight-alpha RGBA8.
    //!
    //! The same input always produces bit-identical output on every platform, which is what
    //! the cross-platform golden tests rely on.

    use crate::pixel::{Rgba, TILE_SIZE};
    use crate::project::Document;

    #[derive(Debug, Clone)]
    pub struct OnionSkin {
        pub before: usize,
        pub after: usize,
        /// Opacity of the nearest onion frame (0-255); further frames fade out.
        pub opacity: u8,
        pub tint: bool,
    }

    #[derive(Debug, Clone, Default)]
    pub struct RenderOptions {
        /// Restrict to a single layer id (ignores visibility). `None` = all visible layers.
        pub only_layer: Option<u32>,
        pub onion: Option<OnionSkin>,
        pub background: Option<Rgba>,
        /// Output pixels per canvas pixel. `None` = the highest layer density, so half- and
        /// quarter-size detail pixels are never lost.
        pub density: Option<u32>,
    }

    /// An RGBA8 image, row-major, 4 bytes per pixel.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Image {
        pub width: u32,
        pub height: u32,
        pub pixels: Vec<u8>,
    }

    impl Image {
        pub fn new(width: u32, height: u32) -> Image {
            Image { width, height, pixels: vec![0; width as usize * height as usize * 4] }
        }

        pub fn get(&self, x: u32, y: u32) -> Rgba {
            let i = (y as usize * self.width as usize + x as usize) * 4;
            Rgba::new(self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3])
        }

        /// Nearest-neighbour upscale by an integer factor.
        pub fn scaled(&self, factor: u32) -> Image {
            if factor <= 1 {
                return self.clone();
            }
            let (w, h) = (self.width * factor, self.height * factor);
            let mut out = Image::new(w, h);
            let src_row = self.width as usize * 4;
            for y in 0..self.height as usize {
                let mut row = Vec::with_capacity(w as usize * 4);
                for x in 0..self.width as usize {
                    let p = &self.pixels[y * src_row + x * 4..y * src_row + x * 4 + 4];
                    for _ in 0..factor {
                        row.extend_from_slice(p);
                    }
                }
                for k in 0..factor as usize {
                    let start = (y * factor as usize + k) * row.len();
                    out.pixels[start..start + row.len()].copy_from_slice(&row);
                }
            }
            out
        }

        /// Copies `src` into this image at (x, y) without blending.
        pub fn blit(&mut self, src: &Image, x: u32, y: u32) {
            for row in 0..src.height {
                let dy = y + row;
                if dy >= self.height {
                    break;
                }
                let w = src.width.min(self.width.saturating_sub(x)) as usize;
                let s = row as usize * src.width as usize * 4;
                let d = (dy as usize * self.width as usize + x as usize) * 4;
                self.pixels[d..d + w * 4].copy_from_slice(&src.pixels[s..s + w * 4]);
            }
        }

        /// FNV-1a 64-bit hash of size + pixels; used by golden tests.
        pub fn fingerprint(&self) -> u64 {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            let mut eat = |b: u8| {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            };
            self.width.to_le_bytes().into_iter().for_each(&mut eat);
            self.height.to_le_bytes().into_iter().for_each(&mut eat);
            self.pixels.iter().copied().for_each(&mut eat);
            h
        }
    }

    #[inline]
    fn div255(v: u32) -> u32 {
        (v + 128 + ((v + 128) >> 8)) >> 8
    }

    /// Source-over blend of straight-alpha colors with an extra opacity factor.
    #[inline]
    pub fn blend_over(dst: &mut [u8], src: Rgba, opacity: u8) {
        let sa = div255(src.a as u32 * opacity as u32);
        if sa == 0 {
            return;
        }
        let da = dst[3] as u32;
        if sa == 255 || da == 0 {
            dst[0] = src.r;
            dst[1] = src.g;
            dst[2] = src.b;
            dst[3] = sa as u8;
            return;
        }
        let inv = 255 - sa;
        let out_a = sa + div255(da * inv);
        let mix = |s: u8, d: u8| -> u8 {
            let num = s as u32 * sa * 255 + d as u32 * da * inv;
            ((num + out_a * 255 / 2) / (out_a * 255)) as u8
        };
        dst[0] = mix(src.r, dst[0]);
        dst[1] = mix(src.g, dst[1]);
        dst[2] = mix(src.b, dst[2]);
        dst[3] = out_a as u8;
    }

    fn composite_frame(doc: &Document, frame_index: usize, opts: &RenderOptions, img: &mut Image, opacity_scale: u8, tint: Option<Rgba>) {
        let Some(frame) = doc.frames.get(frame_index) else { return };
        for layer in &doc.layers {
            let include = match opts.only_layer {
                Some(id) => id == layer.id,
                None => layer.visible,
            };
            if !include || layer.opacity == 0 {
                continue;
            }
            let Some(grid) = doc.cel(layer.id, frame.id) else { continue };
            let opacity = div255(layer.opacity as u32 * opacity_scale as u32) as u8;
            // Output density D vs layer density d: each layer pixel covers (D/d)² output pixels
            // (or every (d/D)-th layer pixel is shown when rendering below the layer's density).
            let out_d = (img.width / doc.width.max(1)).max(1);
            let ld = layer.density.max(1) as u32;
            let (up, down) = if out_d >= ld { (out_d / ld, 1) } else { (1, ld / out_d) };
            for ty in 0..grid.tiles_y() {
                for tx in 0..grid.tiles_x() {
                    let Some(tile) = grid.tile(tx, ty) else { continue };
                    for (i, &v) in tile.iter().enumerate() {
                        if v == 0 {
                            continue;
                        }
                        let gx = tx * TILE_SIZE + i as u32 % TILE_SIZE;
                        let gy = ty * TILE_SIZE + i as u32 / TILE_SIZE;
                        if down > 1 && (!gx.is_multiple_of(down) || !gy.is_multiple_of(down)) {
                            continue;
                        }
                        for sub in 0..up * up {
                            let x = gx / down * up + sub % up;
                            let y = gy / down * up + sub / up;
                            if x >= img.width || y >= img.height {
                                continue;
                            }
                            let mut c = Rgba::from_u32(v);
                            if let Some(t) = tint {
                                c = Rgba::new(
                                    ((c.r as u16 + t.r as u16) / 2) as u8,
                                    ((c.g as u16 + t.g as u16) / 2) as u8,
                                    ((c.b as u16 + t.b as u16) / 2) as u8,
                                    c.a,
                                );
                            }
                            let o = (y as usize * img.width as usize + x as usize) * 4;
                            if layer.blend == crate::layers::Blend::Normal || img.pixels[o + 3] == 0 {
                                blend_over(&mut img.pixels[o..o + 4], c, opacity);
                            } else {
                                let px = &img.pixels[o..o + 4];
                                let mixed = Rgba::new(
                                    layer.blend.channel(px[0] as u32, c.r as u32) as u8,
                                    layer.blend.channel(px[1] as u32, c.g as u32) as u8,
                                    layer.blend.channel(px[2] as u32, c.b as u32) as u8,
                                    c.a,
                                );
                                blend_over(&mut img.pixels[o..o + 4], mixed, opacity);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Renders one frame. Onion frames are drawn underneath the current frame.
    pub fn render(doc: &Document, frame_index: usize, opts: &RenderOptions) -> Image {
        let d = opts.density.unwrap_or_else(|| match opts.only_layer {
            Some(id) => doc.layers.iter().find(|l| l.id == id).map(|l| l.density.max(1) as u32).unwrap_or(1),
            None => doc.layers.iter().filter(|l| l.visible).map(|l| l.density.max(1) as u32).max().unwrap_or(1),
        });
        let d = d.clamp(1, 4);
        let mut img = Image::new(doc.width * d, doc.height * d);
        if let Some(bg) = opts.background {
            for px in img.pixels.as_chunks_mut::<4>().0 {
                px.copy_from_slice(&[bg.r, bg.g, bg.b, bg.a]);
            }
        }
        if let Some(onion) = &opts.onion {
            let red = Rgba::new(255, 64, 64, 255);
            let blue = Rgba::new(64, 128, 255, 255);
            let fade = |dist: usize, max: usize| -> u8 { (onion.opacity as usize * (max + 1 - dist) / (max + 1).max(1)) as u8 };
            for d in (1..=onion.before).rev() {
                if let Some(i) = frame_index.checked_sub(d) {
                    composite_frame(doc, i, opts, &mut img, fade(d, onion.before), onion.tint.then_some(red));
                }
            }
            for d in (1..=onion.after).rev() {
                let i = frame_index + d;
                if i < doc.frames.len() {
                    composite_frame(doc, i, opts, &mut img, fade(d, onion.after), onion.tint.then_some(blue));
                }
            }
        }
        composite_frame(doc, frame_index, opts, &mut img, 255, None);
        img
    }
}
pub mod storage {
    //! `.pxs` — Pixel Studio's binary project format.
    //!
    //! ```text
    //! "PXS\0" | version u16 | reserved u16
    //! chunk*  = tag[4] | length u32 | payload
    //!   META  name, size, active layer/frame, id counters
    //!   PALT  palette (RGBA)
    //!   LAYR  layers bottom→top
    //!   FRAM  frames in order with durations
    //!   TAGS  animation tags
    //!   TILE  de-duplicated 16×16 tiles, RLE-encoded
    //!   CELS  (layer id, frame id) → tile references
    //!   HIST  operation log (who changed what)
    //!   MATL / TXTR / VOXM  PBR materials, texture maps and voxel models (3D pixel art)
    //!   SLCE  slices: name, color, keys (frame id, rect, optional pivot)            [v3]
    //!   TSET  tilesets: name, tile size, kind, zlib-compressed RGBA tiles           [v3]
    //!   TMAP  tilemaps: name, tileset index, size, cells and terrain bits          [v3]
    //! "END\0" | 4 | CRC-32 of every byte before this chunk
    //! ```
    //! All integers are little-endian. Unknown chunks are skipped for forward compatibility.
    //! Identical tiles are stored once, so duplicated/held animation frames cost almost nothing.

    use std::collections::HashMap;
    use std::sync::Arc;

    use crate::animation::{Direction, Frame, Tag, validate_duration};
    use crate::export::deflate::{crc32, zlib_compress};
    use crate::export::inflate::zlib_decompress;
    use crate::gamedev::tiles::{Tilemap, Tileset, TilesetKind};
    use crate::gamedev::{Slice, SliceKey};
    use crate::history::LogEntry;
    use crate::layers::Layer;
    use crate::pixel::{Palette, PixelGrid, Rgba, TILE_AREA, TILE_SIZE, Tile};
    use crate::project::{Document, MAX_FRAMES, MAX_LAYERS, validate_size};
    use crate::voxel::model::{CHUNK, CHUNK_VOLUME, MAX_BONES, MAX_MODEL_SIZE, Voxel, VoxelModel};
    use crate::voxel::rig::{Animation, Bone, Key};
    use crate::voxel::{Material, Texture, Vec3};

    pub const MAGIC: &[u8; 4] = b"PXS\0";
    pub const VERSION: u16 = 4;

    struct W(Vec<u8>);

    impl W {
        fn u8(&mut self, v: u8) {
            self.0.push(v);
        }
        fn u16(&mut self, v: u16) {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        fn u32(&mut self, v: u32) {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        fn u64(&mut self, v: u64) {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        fn f32(&mut self, v: f32) {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        fn i32(&mut self, v: i32) {
            self.0.extend_from_slice(&v.to_le_bytes());
        }
        fn vec3(&mut self, v: Vec3) {
            self.f32(v.x);
            self.f32(v.y);
            self.f32(v.z);
        }
        fn blob(&mut self, data: &[u8]) {
            let z = zlib_compress(data);
            self.u32(z.len() as u32);
            self.0.extend_from_slice(&z);
        }
        fn str(&mut self, s: &str) {
            self.u32(s.len() as u32);
            self.0.extend_from_slice(s.as_bytes());
        }
    }

    fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: W) {
        out.extend_from_slice(tag);
        out.extend_from_slice(&(body.0.len() as u32).to_le_bytes());
        out.extend_from_slice(&body.0);
    }

    fn rle_tile(tile: &Tile, w: &mut W) {
        let mut runs: Vec<(u16, u32)> = Vec::new();
        for &v in tile.iter() {
            match runs.last_mut() {
                Some((n, last)) if *last == v => *n += 1,
                _ => runs.push((1, v)),
            }
        }
        w.u16(runs.len() as u16);
        for (n, v) in runs {
            w.u16(n);
            w.u32(v);
        }
    }

    pub fn save(doc: &Document, log: &[LogEntry]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());

        let mut m = W(Vec::new());
        m.str(&doc.name);
        m.u32(doc.width);
        m.u32(doc.height);
        m.u32(doc.active_layer);
        m.u32(doc.active_frame);
        m.u32(doc.next_layer_id);
        m.u32(doc.next_frame_id);
        chunk(&mut out, b"META", m);

        let mut p = W(Vec::new());
        p.u32(doc.palette.len() as u32);
        doc.palette.colors().iter().for_each(|c| p.u32(c.to_u32()));
        chunk(&mut out, b"PALT", p);

        let mut l = W(Vec::new());
        l.u32(doc.layers.len() as u32);
        for layer in &doc.layers {
            l.u32(layer.id);
            l.str(&layer.name);
            l.u8(layer.visible as u8);
            l.u8(layer.locked as u8);
            l.u8(layer.opacity);
            l.u8(layer.blend.code());
            l.u8(layer.density);
        }
        chunk(&mut out, b"LAYR", l);

        let mut f = W(Vec::new());
        f.u32(doc.frames.len() as u32);
        for frame in &doc.frames {
            f.u32(frame.id);
            f.u32(frame.duration_ms);
        }
        chunk(&mut out, b"FRAM", f);

        let mut t = W(Vec::new());
        t.u32(doc.tags.len() as u32);
        for tag in &doc.tags {
            t.str(&tag.name);
            t.u32(tag.from_id);
            t.u32(tag.to_id);
            t.u8(tag.direction.code());
            t.u32(tag.repeat);
        }
        chunk(&mut out, b"TAGS", t);

        // De-duplicate tiles by content. Deterministic order: layer order, then frame order.
        let mut tile_ids: HashMap<&Tile, u32> = HashMap::new();
        let mut tiles: Vec<&Tile> = Vec::new();
        let mut cel_body = W(Vec::new());
        let mut cel_count = 0u32;
        let mut cel_entries = W(Vec::new());
        for layer in &doc.layers {
            for frame in &doc.frames {
                let Some(grid) = doc.cel(layer.id, frame.id) else { continue };
                if grid.is_empty() {
                    continue;
                }
                cel_count += 1;
                cel_entries.u32(layer.id);
                cel_entries.u32(frame.id);
                for slot in grid.tiles() {
                    let reference = match slot {
                        Some(tile) if tile.iter().any(|&v| v != 0) => {
                            let key: &Tile = tile.as_ref();
                            *tile_ids.entry(key).or_insert_with(|| {
                                tiles.push(key);
                                tiles.len() as u32
                            })
                        }
                        _ => 0,
                    };
                    cel_entries.u32(reference);
                }
            }
        }
        let mut tb = W(Vec::new());
        tb.u32(tiles.len() as u32);
        tiles.iter().for_each(|t| rle_tile(t, &mut tb));
        chunk(&mut out, b"TILE", tb);
        cel_body.u32(cel_count);
        cel_body.0.extend_from_slice(&cel_entries.0);
        chunk(&mut out, b"CELS", cel_body);

        let mut h = W(Vec::new());
        h.u32(log.len() as u32);
        for e in log {
            h.u64(e.revision);
            h.str(&e.actor);
            h.str(&e.op);
            h.str(e.operation_id.as_deref().unwrap_or(""));
            h.u64(e.changed_pixels);
            h.u64(e.timestamp_ms);
        }
        chunk(&mut out, b"HIST", h);

        save_voxels(doc, &mut out);
        save_gamedev(doc, &mut out);

        let crc = crc32(&out);
        out.extend_from_slice(b"END\0");
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }

    struct R<'a> {
        d: &'a [u8],
        pos: usize,
    }

    impl<'a> R<'a> {
        fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
            let end = self.pos.checked_add(n).filter(|&e| e <= self.d.len()).ok_or("unexpected end of data")?;
            let s = &self.d[self.pos..end];
            self.pos = end;
            Ok(s)
        }
        fn u8(&mut self) -> Result<u8, String> {
            Ok(self.take(1)?[0])
        }
        fn u16(&mut self) -> Result<u16, String> {
            Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
        }
        fn u32(&mut self) -> Result<u32, String> {
            Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
        }
        fn u64(&mut self) -> Result<u64, String> {
            Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
        }
        fn f32(&mut self) -> Result<f32, String> {
            let v = f32::from_le_bytes(self.take(4)?.try_into().unwrap());
            if v.is_finite() { Ok(v) } else { Err("invalid number".into()) }
        }
        fn i32(&mut self) -> Result<i32, String> {
            Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
        }
        fn vec3(&mut self) -> Result<Vec3, String> {
            Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
        }
        fn blob(&mut self, limit: usize) -> Result<Vec<u8>, String> {
            let n = self.u32()? as usize;
            zlib_decompress(self.take(n)?, limit)
        }
        fn str(&mut self) -> Result<String, String> {
            let n = self.u32()? as usize;
            String::from_utf8(self.take(n)?.to_vec()).map_err(|_| "invalid UTF-8 string".into())
        }
        /// Count prefix that must be plausible given the bytes left (each item ≥ `min_item` bytes).
        fn count(&mut self, min_item: usize) -> Result<usize, String> {
            let n = self.u32()? as usize;
            if n.saturating_mul(min_item) > self.d.len() - self.pos {
                return Err("corrupt count".into());
            }
            Ok(n)
        }
    }

    pub struct Loaded {
        pub doc: Document,
        pub log: Vec<LogEntry>,
    }

    pub fn load(data: &[u8]) -> Result<Loaded, String> {
        if data.len() < 8 + 12 || &data[..4] != MAGIC {
            return Err("not a Pixel Studio (.pxs) file".into());
        }
        let version = u16::from_le_bytes([data[4], data[5]]);
        if !(2..=VERSION).contains(&version) {
            return Err(format!("unsupported .pxs version {version}"));
        }
        let end = data.len() - 12;
        if &data[end..end + 4] != b"END\0" || u32::from_le_bytes(data[end + 4..end + 8].try_into().unwrap()) != 4 {
            return Err("missing END chunk (file truncated?)".into());
        }
        let stored = u32::from_le_bytes(data[end + 8..end + 12].try_into().unwrap());
        if crc32(&data[..end]) != stored {
            return Err("checksum mismatch: file is corrupted".into());
        }

        let mut chunks: HashMap<[u8; 4], &[u8]> = HashMap::new();
        let mut r = R { d: &data[..end], pos: 8 };
        while r.pos < end {
            let tag: [u8; 4] = r.take(4)?.try_into().unwrap();
            let len = r.u32()? as usize;
            chunks.insert(tag, r.take(len)?);
        }
        let get = |t: &[u8; 4]| chunks.get(t).copied().ok_or_else(|| format!("missing {} chunk", String::from_utf8_lossy(t)));

        let mut m = R { d: get(b"META")?, pos: 0 };
        let name = m.str()?;
        let (width, height) = validate_size(m.u32()? as i64, m.u32()? as i64)?;
        let (active_layer, active_frame, next_layer_id, next_frame_id) = (m.u32()?, m.u32()?, m.u32()?, m.u32()?);

        let mut p = R { d: get(b"PALT")?, pos: 0 };
        let n = p.count(4)?;
        let colors = (0..n).map(|_| p.u32().map(Rgba::from_u32)).collect::<Result<Vec<_>, _>>()?;
        let palette = Palette::from_colors(colors)?;

        let mut l = R { d: get(b"LAYR")?, pos: 0 };
        let n = l.count(11)?;
        if n == 0 || n > MAX_LAYERS {
            return Err("invalid layer count".into());
        }
        let mut layers = Vec::with_capacity(n);
        for _ in 0..n {
            let id = l.u32()?;
            let name = l.str()?;
            let visible = l.u8()? != 0;
            let locked = l.u8()? != 0;
            let opacity = l.u8()?;
            let blend = if version >= 3 { crate::layers::Blend::from_code(l.u8()?) } else { crate::layers::Blend::Normal };
            let density = if version >= 4 { l.u8()? } else { 1 };
            if !matches!(density, 1 | 2 | 4) {
                return Err("invalid layer density".into());
            }
            if id == 0 || id >= next_layer_id || layers.iter().any(|x: &Layer| x.id == id) {
                return Err("invalid layer id".into());
            }
            layers.push(Layer { id, name, visible, locked, opacity, blend, density });
        }

        let mut f = R { d: get(b"FRAM")?, pos: 0 };
        let n = f.count(8)?;
        if n == 0 || n > MAX_FRAMES {
            return Err("invalid frame count".into());
        }
        let mut frames = Vec::with_capacity(n);
        for _ in 0..n {
            let id = f.u32()?;
            let duration_ms = validate_duration(f.u32()? as i64)?;
            if id == 0 || id >= next_frame_id || frames.iter().any(|x: &Frame| x.id == id) {
                return Err("invalid frame id".into());
            }
            frames.push(Frame { id, duration_ms });
        }

        let mut t = R { d: get(b"TAGS")?, pos: 0 };
        let n = t.count(17)?;
        let mut tags = Vec::with_capacity(n);
        for _ in 0..n {
            let name = t.str()?;
            let (from_id, to_id) = (t.u32()?, t.u32()?);
            let direction = Direction::from_code(t.u8()?).ok_or("invalid tag direction")?;
            let repeat = t.u32()?;
            if !frames.iter().any(|x| x.id == from_id) || !frames.iter().any(|x| x.id == to_id) {
                return Err("tag references unknown frame".into());
            }
            tags.push(Tag { name, from_id, to_id, direction, repeat });
        }

        let mut tb = R { d: get(b"TILE")?, pos: 0 };
        let n = tb.count(8)?;
        let mut tiles: Vec<Arc<Tile>> = Vec::with_capacity(n);
        for _ in 0..n {
            let runs = tb.u16()? as usize;
            let mut tile = [0u32; TILE_AREA];
            let mut i = 0;
            for _ in 0..runs {
                let (len, v) = (tb.u16()? as usize, tb.u32()?);
                if i + len > TILE_AREA {
                    return Err("tile RLE overflow".into());
                }
                tile[i..i + len].fill(v);
                i += len;
            }
            if i != TILE_AREA {
                return Err("tile RLE underflow".into());
            }
            tiles.push(Arc::new(tile));
        }

        let mut doc = Document {
            name,
            width,
            height,
            palette,
            layers,
            frames,
            cels: HashMap::new(),
            tags,
            active_layer,
            active_frame,
            next_layer_id,
            next_frame_id,
            models: Vec::new(),
            materials: vec![Material::new("default")],
            textures: Vec::new(),
            slices: Vec::new(),
            tilesets: Vec::new(),
            tilemaps: Vec::new(),
            ramps: Vec::new(),
        };
        let mut c = R { d: get(b"CELS")?, pos: 0 };
        let n = c.count(8)?;
        for _ in 0..n {
            let (layer_id, frame_id) = (c.u32()?, c.u32()?);
            if doc.layer_index(layer_id).is_none() || doc.frame_index(frame_id).is_none() {
                return Err("cel references unknown layer/frame".into());
            }
            let (lw, lh) = doc.layer_dims(layer_id);
            let mut grid = PixelGrid::new(lw, lh);
            for ty in 0..lh.div_ceil(TILE_SIZE) {
                for tx in 0..lw.div_ceil(TILE_SIZE) {
                    let reference = c.u32()? as usize;
                    if reference > tiles.len() {
                        return Err("cel references missing tile".into());
                    }
                    if reference > 0 {
                        grid.set_tile(tx, ty, Some(Arc::clone(&tiles[reference - 1])));
                    }
                }
            }
            doc.cels.insert((layer_id, frame_id), Arc::new(grid));
        }
        if doc.layer_index(doc.active_layer).is_none() {
            doc.active_layer = doc.layers[0].id;
        }
        if doc.frame_index(doc.active_frame).is_none() {
            doc.active_frame = doc.frames[0].id;
        }

        load_voxels(&mut doc, &chunks)?;
        load_gamedev(&mut doc, &chunks)?;

        let mut log = Vec::new();
        if let Some(hist) = chunks.get(b"HIST") {
            let mut h = R { d: hist, pos: 0 };
            let n = h.count(36)?;
            for _ in 0..n {
                let revision = h.u64()?;
                let actor = h.str()?;
                let op = h.str()?;
                let opid = h.str()?;
                let changed_pixels = h.u64()?;
                let timestamp_ms = h.u64()?;
                log.push(LogEntry { revision, actor, op, operation_id: (!opid.is_empty()).then_some(opid), changed_pixels, timestamp_ms });
            }
        }
        Ok(Loaded { doc, log })
    }

    fn save_gamedev(doc: &Document, out: &mut Vec<u8>) {
        let mut rm = W(Vec::new());
        rm.u32(doc.ramps.len() as u32);
        for r in &doc.ramps {
            rm.str(&r.name);
            rm.u32(r.colors.len() as u32);
            r.colors.iter().for_each(|c| rm.u32(*c));
        }
        chunk(out, b"RAMP", rm);

        let mut s = W(Vec::new());
        s.u32(doc.slices.len() as u32);
        for sl in &doc.slices {
            s.str(&sl.name);
            s.u32(sl.color);
            s.u32(sl.keys.len() as u32);
            for k in &sl.keys {
                s.u32(k.frame_id);
                s.i32(k.x);
                s.i32(k.y);
                s.u32(k.w);
                s.u32(k.h);
                s.u8(k.pivot.is_some() as u8);
                let (px, py) = k.pivot.unwrap_or((0, 0));
                s.i32(px);
                s.i32(py);
            }
        }
        chunk(out, b"SLCE", s);

        let mut t = W(Vec::new());
        t.u32(doc.tilesets.len() as u32);
        for ts in &doc.tilesets {
            t.str(&ts.name);
            t.u32(ts.tile_w);
            t.u32(ts.tile_h);
            t.u8(matches!(ts.kind, TilesetKind::Blob) as u8);
            t.u32(ts.tiles.len() as u32);
            let raw: Vec<u8> = ts.tiles.iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
            t.blob(&raw);
        }
        chunk(out, b"TSET", t);

        let mut m = W(Vec::new());
        m.u32(doc.tilemaps.len() as u32);
        for tm in &doc.tilemaps {
            m.str(&tm.name);
            m.u32(tm.tileset as u32);
            m.u32(tm.cols);
            m.u32(tm.rows);
            let mut raw: Vec<u8> = tm.cells.iter().flat_map(|v| v.to_le_bytes()).collect();
            raw.extend(tm.terrain.iter().map(|&b| b as u8));
            m.blob(&raw);
        }
        chunk(out, b"TMAP", m);
    }

    const MAX_TILES: usize = 4096;
    const MAX_TILE_SIDE: u32 = 256;
    const MAX_MAP_SIDE: u32 = 4096;

    fn load_gamedev(doc: &mut Document, chunks: &HashMap<[u8; 4], &[u8]>) -> Result<(), String> {
        if let Some(data) = chunks.get(b"RAMP") {
            let mut r = R { d: data, pos: 0 };
            for _ in 0..r.count(8)? {
                let name = r.str()?;
                let n = r.count(4)?;
                if n > 64 {
                    return Err("ramp too long".into());
                }
                let colors = (0..n).map(|_| r.u32()).collect::<Result<Vec<_>, _>>()?;
                doc.ramps.push(crate::project::Ramp { name, colors });
            }
        }
        if let Some(data) = chunks.get(b"SLCE") {
            let mut r = R { d: data, pos: 0 };
            for _ in 0..r.count(12)? {
                let name = r.str()?;
                let color = r.u32()?;
                let mut keys = Vec::new();
                for _ in 0..r.count(29)? {
                    let (frame_id, x, y, w, h) = (r.u32()?, r.i32()?, r.i32()?, r.u32()?, r.u32()?);
                    let has = r.u8()? != 0;
                    let (px, py) = (r.i32()?, r.i32()?);
                    keys.push(SliceKey { frame_id, x, y, w, h, pivot: has.then_some((px, py)) });
                }
                doc.slices.push(Slice { name, color, keys });
            }
        }
        if let Some(data) = chunks.get(b"TSET") {
            let mut r = R { d: data, pos: 0 };
            for _ in 0..r.count(21)? {
                let name = r.str()?;
                let (tile_w, tile_h) = (r.u32()?, r.u32()?);
                let kind = if r.u8()? != 0 { TilesetKind::Blob } else { TilesetKind::Plain };
                let n = r.u32()? as usize;
                if !(1..=MAX_TILE_SIDE).contains(&tile_w) || !(1..=MAX_TILE_SIDE).contains(&tile_h) || n > MAX_TILES {
                    return Err("invalid tileset".into());
                }
                let area = (tile_w * tile_h) as usize;
                let raw = r.blob(n * area * 4)?;
                if raw.len() != n * area * 4 {
                    return Err("tileset has the wrong size".into());
                }
                let px: Vec<u32> = raw.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
                let tiles = px.chunks(area.max(1)).map(|c| c.to_vec()).collect();
                doc.tilesets.push(Arc::new(Tileset { name, tile_w, tile_h, kind, tiles }));
            }
        }
        if let Some(data) = chunks.get(b"TMAP") {
            let mut r = R { d: data, pos: 0 };
            for _ in 0..r.count(20)? {
                let name = r.str()?;
                let tileset = r.u32()? as usize;
                let (cols, rows) = (r.u32()?, r.u32()?);
                if tileset >= doc.tilesets.len() || !(1..=MAX_MAP_SIDE).contains(&cols) || !(1..=MAX_MAP_SIDE).contains(&rows) {
                    return Err("invalid tilemap".into());
                }
                let n = (cols * rows) as usize;
                let raw = r.blob(n * 5)?;
                if raw.len() != n * 5 {
                    return Err("tilemap has the wrong size".into());
                }
                let cells = raw[..n * 4].as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
                let terrain = raw[n * 4..].iter().map(|&b| b != 0).collect();
                doc.tilemaps.push(Arc::new(Tilemap { name, tileset, cols, rows, cells, terrain }));
            }
        }
        Ok(())
    }

    const VOXEL_BYTES: usize = 6;

    fn save_voxels(doc: &Document, out: &mut Vec<u8>) {
        let mut m = W(Vec::new());
        m.u32(doc.materials.len() as u32);
        for mat in &doc.materials {
            m.str(&mat.name);
            for v in [mat.roughness, mat.metallic, mat.emission, mat.uv_scale, mat.normal_strength] {
                m.f32(v);
            }
            for slot in mat.maps {
                m.i32(slot.map(|i| i as i32).unwrap_or(-1));
            }
        }
        chunk(out, b"MATL", m);

        let mut t = W(Vec::new());
        t.u32(doc.textures.len() as u32);
        for tex in &doc.textures {
            t.str(&tex.name);
            t.u32(tex.width);
            t.u32(tex.height);
            t.blob(&tex.data);
        }
        chunk(out, b"TXTR", t);

        let mut v = W(Vec::new());
        v.u32(doc.models.len() as u32);
        for model in &doc.models {
            v.str(&model.name);
            for d in model.size {
                v.u32(d);
            }
            v.vec3(model.origin);
            v.u32(model.bones.len() as u32);
            for b in &model.bones {
                v.str(&b.name);
                v.i32(b.parent.map(|p| p as i32).unwrap_or(-1));
                v.vec3(b.head);
                v.vec3(b.tail);
            }
            v.u32(model.animations.len() as u32);
            for a in &model.animations {
                v.str(&a.name);
                v.u32(a.frames);
                v.u32(a.frame_ms);
                v.u8(a.looped as u8);
                v.u32(a.keys.len() as u32);
                for k in &a.keys {
                    v.u32(k.bone as u32);
                    v.f32(k.frame);
                    v.vec3(k.rot);
                    v.vec3(k.pos);
                }
            }
            let mut keys: Vec<_> = model.chunks.keys().copied().collect();
            keys.sort();
            let keys: Vec<_> = keys.into_iter().filter(|k| model.chunks[k].iter().any(|x| !x.is_empty())).collect();
            v.u32(keys.len() as u32);
            for key in keys {
                v.i32(key.0);
                v.i32(key.1);
                v.i32(key.2);
                let mut raw = Vec::with_capacity(CHUNK_VOLUME * VOXEL_BYTES);
                for vox in model.chunks[&key].iter() {
                    raw.extend_from_slice(&vox.rgba.to_le_bytes());
                    raw.push(vox.material);
                    raw.push(vox.bone);
                }
                v.blob(&raw);
            }
        }
        chunk(out, b"VOXM", v);
    }

    fn load_voxels(doc: &mut Document, chunks: &HashMap<[u8; 4], &[u8]>) -> Result<(), String> {
        if let Some(data) = chunks.get(b"TXTR") {
            let mut r = R { d: data, pos: 0 };
            let n = r.count(16)?;
            for _ in 0..n {
                let name = r.str()?;
                let (w, h) = (r.u32()?, r.u32()?);
                if w == 0 || h == 0 || w > 4096 || h > 4096 {
                    return Err("invalid texture size".into());
                }
                let data = r.blob((w * h * 4) as usize)?;
                if data.len() != (w * h * 4) as usize {
                    return Err("texture data has the wrong size".into());
                }
                doc.textures.push(Arc::new(Texture { name, width: w, height: h, data }));
            }
        }
        if let Some(data) = chunks.get(b"MATL") {
            let mut r = R { d: data, pos: 0 };
            let n = r.count(52)?;
            if n == 0 || n > 256 {
                return Err("invalid material count".into());
            }
            doc.materials.clear();
            for _ in 0..n {
                let mut m = Material::new(&r.str()?);
                m.roughness = r.f32()?.clamp(0.0, 1.0);
                m.metallic = r.f32()?.clamp(0.0, 1.0);
                m.emission = r.f32()?.clamp(0.0, 100.0);
                m.uv_scale = r.f32()?.clamp(0.01, 64.0);
                m.normal_strength = r.f32()?.clamp(0.0, 4.0);
                for slot in m.maps.iter_mut() {
                    let i = r.i32()?;
                    *slot = if i < 0 {
                        None
                    } else {
                        Some(i as usize).filter(|&i| i < doc.textures.len()).ok_or("material references a missing texture")?.into()
                    };
                }
                doc.materials.push(m);
            }
        }
        if let Some(data) = chunks.get(b"VOXM") {
            let mut r = R { d: data, pos: 0 };
            let n = r.count(32)?;
            for _ in 0..n {
                let name = r.str()?;
                let size = [r.u32()?, r.u32()?, r.u32()?];
                if size.iter().any(|&d| d == 0 || d > MAX_MODEL_SIZE) {
                    return Err("invalid model size".into());
                }
                let mut model = VoxelModel::new(&name, size);
                model.origin = r.vec3()?;
                let nb = r.count(32)?;
                if nb > MAX_BONES {
                    return Err("too many bones".into());
                }
                for i in 0..nb {
                    let name = r.str()?;
                    let parent = r.i32()?;
                    let parent = if parent < 0 {
                        None
                    } else if (parent as usize) < i {
                        Some(parent as usize)
                    } else {
                        return Err("bone parent must precede the bone".into());
                    };
                    model.bones.push(Bone { name, parent, head: r.vec3()?, tail: r.vec3()? });
                }
                let na = r.count(17)?;
                for _ in 0..na {
                    let mut a = Animation::new(&r.str()?, 1, 100);
                    a.frames = r.u32()?.clamp(1, 100_000);
                    a.frame_ms = r.u32()?.clamp(1, 60_000);
                    a.looped = r.u8()? != 0;
                    let nk = r.count(32)?;
                    for _ in 0..nk {
                        let bone = r.u32()? as usize;
                        if bone >= nb {
                            return Err("animation key references a missing bone".into());
                        }
                        a.keys.push(Key { bone, frame: r.f32()?, rot: r.vec3()?, pos: r.vec3()? });
                    }
                    model.animations.push(a);
                }
                let nc = r.count(16)?;
                let max_chunks = size.iter().map(|&d| (d as i32 + CHUNK - 1) / CHUNK).product::<i32>();
                if nc as i32 > max_chunks {
                    return Err("too many voxel chunks".into());
                }
                for _ in 0..nc {
                    let key = (r.i32()?, r.i32()?, r.i32()?);
                    let raw = r.blob(CHUNK_VOLUME * VOXEL_BYTES)?;
                    if raw.len() != CHUNK_VOLUME * VOXEL_BYTES {
                        return Err("voxel chunk has the wrong size".into());
                    }
                    let mut c = [Voxel::EMPTY; CHUNK_VOLUME];
                    for (i, v) in raw.as_chunks::<VOXEL_BYTES>().0.iter().enumerate() {
                        let vox = Voxel { rgba: u32::from_le_bytes([v[0], v[1], v[2], v[3]]), material: v[4], bone: v[5] };
                        if (vox.bone as usize) >= nb.max(1) || (vox.material as usize) >= doc.materials.len() {
                            return Err("voxel references a missing bone or material".into());
                        }
                        c[i] = if vox.is_empty() { Voxel::EMPTY } else { vox };
                    }
                    model.chunks.insert(key, Arc::new(c));
                }
                doc.models.push(model);
            }
        }
        Ok(())
    }
}
pub mod voxel {
    //! 3D pixel art: sparse voxel models with true color, PBR materials and textures, skeletal
    //! rigs, keyframe animation and a deterministic software renderer that turns models into
    //! pixel-perfect sprites (any angle, any frame, every G-buffer pass).

    pub mod material {
        //! PBR materials and textures for voxels.
        //!
        //! Every voxel has its own true color (albedo) and a material index. A material adds the PBR
        //! parameters (roughness, metallic, emission) and optional texture maps. Textures are mapped onto
        //! voxel faces with planar UVs per face axis (X faces use z/y, Y faces x/z, Z faces x/y), so a
        //! texture "wraps" a voxel model like a UV-unwrapped box, `uv_scale` texels per voxel.

        use std::sync::Arc;

        use super::math::Vec3;
        use super::model::{hash01, value_noise};

        /// Texture map slots of a material.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum MapSlot {
            /// Multiplies the voxel color (RGB) — color variation, painted detail.
            Albedo = 0,
            /// Tangent-space normal map (RGB = XYZ * 0.5 + 0.5).
            Normal = 1,
            /// Roughness multiplier (R channel).
            Roughness = 2,
            /// Metallic multiplier (R channel).
            Metallic = 3,
            /// Ambient occlusion multiplier (R channel).
            Occlusion = 4,
            /// Height (R channel) — deepens crevices in the occlusion term.
            Height = 5,
            /// Emission color (RGB), scaled by the material's emission strength.
            Emission = 6,
        }

        pub const MAP_SLOTS: [(&str, MapSlot); 7] = [
            ("albedo", MapSlot::Albedo),
            ("normal", MapSlot::Normal),
            ("roughness", MapSlot::Roughness),
            ("metallic", MapSlot::Metallic),
            ("occlusion", MapSlot::Occlusion),
            ("height", MapSlot::Height),
            ("emission", MapSlot::Emission),
        ];

        pub fn slot_by_name(name: &str) -> Option<MapSlot> {
            let n = name.to_ascii_lowercase();
            let n = match n.as_str() {
                "ao" | "ambient_occlusion" => "occlusion",
                "basecolor" | "base_color" | "diffuse" | "color" => "albedo",
                "metalness" => "metallic",
                "displacement" | "bump" => "height",
                "emissive" => "emission",
                other => other,
            };
            MAP_SLOTS.iter().find(|(s, _)| *s == n).map(|(_, slot)| *slot)
        }

        /// RGBA8 image used as a material map.
        #[derive(Debug, Clone, PartialEq)]
        pub struct Texture {
            pub name: String,
            pub width: u32,
            pub height: u32,
            pub data: Vec<u8>,
        }

        impl Texture {
            /// Nearest sample with wrapping; returns RGBA in 0..1.
            pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
                let x = (u.floor() as i64).rem_euclid(self.width as i64) as usize;
                let y = (v.floor() as i64).rem_euclid(self.height as i64) as usize;
                let i = (y * self.width as usize + x) * 4;
                let d = &self.data[i..i + 4];
                [d[0] as f32 / 255.0, d[1] as f32 / 255.0, d[2] as f32 / 255.0, d[3] as f32 / 255.0]
            }
        }

        #[derive(Debug, Clone, PartialEq)]
        pub struct Material {
            pub name: String,
            pub roughness: f32,
            pub metallic: f32,
            /// Emission strength (0 = none). Emission color = emission map or the voxel color.
            pub emission: f32,
            /// Texels per voxel for all maps.
            pub uv_scale: f32,
            /// Normal map strength (0..2).
            pub normal_strength: f32,
            /// Texture index (into the project's textures) per slot.
            pub maps: [Option<usize>; 7],
        }

        impl Material {
            pub fn new(name: &str) -> Material {
                Material {
                    name: name.into(),
                    roughness: 0.7,
                    metallic: 0.0,
                    emission: 0.0,
                    uv_scale: 1.0,
                    normal_strength: 1.0,
                    maps: [None; 7],
                }
            }
        }

        /// Everything the shader needs for one voxel face.
        #[derive(Debug, Clone, Copy)]
        pub struct SurfaceSample {
            pub albedo_mul: Vec3,
            pub normal_ts: Vec3,
            pub roughness: f32,
            pub metallic: f32,
            pub occlusion: f32,
            pub emission: Vec3,
            pub has_emission_map: bool,
        }

        /// Samples all maps of a material at a voxel face. `axis` = face axis (0 x, 1 y, 2 z).
        /// `pos` is the model-space point on the face (sub-voxel precision gives texel detail when zoomed in).
        pub fn sample_surface(mat: &Material, textures: &[Arc<Texture>], pos: [f32; 3], axis: usize) -> SurfaceSample {
            let (u, v) = match axis {
                0 => (pos[2], -pos[1]),
                1 => (pos[0], pos[2]),
                _ => (pos[0], -pos[1]),
            };
            let (u, v) = (u * mat.uv_scale, v * mat.uv_scale);
            let tex = |slot: MapSlot| mat.maps[slot as usize].and_then(|i| textures.get(i)).map(|t| t.sample(u, v));
            let albedo_mul = tex(MapSlot::Albedo).map(|c| Vec3::new(c[0], c[1], c[2])).unwrap_or(Vec3::new(1.0, 1.0, 1.0));
            let normal_ts = tex(MapSlot::Normal)
                .map(|c| {
                    let n = Vec3::new((c[0] * 2.0 - 1.0) * mat.normal_strength, (c[1] * 2.0 - 1.0) * mat.normal_strength, c[2] * 2.0 - 1.0);
                    n.normalized()
                })
                .unwrap_or(Vec3::new(0.0, 0.0, 1.0));
            let roughness = (mat.roughness * tex(MapSlot::Roughness).map(|c| c[0] * 2.0).unwrap_or(1.0)).clamp(0.03, 1.0);
            let metallic = (mat.metallic * tex(MapSlot::Metallic).map(|c| c[0] * 2.0).unwrap_or(1.0)).clamp(0.0, 1.0);
            let mut occlusion = tex(MapSlot::Occlusion).map(|c| c[0]).unwrap_or(1.0);
            if let Some(h) = tex(MapSlot::Height) {
                occlusion *= 0.75 + 0.25 * h[0];
            }
            let em = tex(MapSlot::Emission);
            SurfaceSample {
                albedo_mul,
                normal_ts,
                roughness,
                metallic,
                occlusion,
                emission: em.map(|c| Vec3::new(c[0], c[1], c[2])).unwrap_or(Vec3::new(1.0, 1.0, 1.0)) * mat.emission,
                has_emission_map: em.is_some(),
            }
        }

        // ---- procedural PBR texture sets ------------------------------------------------------

        pub const TEXTURE_KINDS: [&str; 9] =
            ["noise", "brushed_metal", "hammered_metal", "leather", "fabric", "scales", "stone", "wood", "chainmail"];

        /// Tileable value noise on a `period`-sized lattice.
        fn periodic_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let s = |t: f32| t * t * (3.0 - 2.0 * t);
            let (sx, sy) = (s(fx), s(fy));
            let p = period.max(1);
            let h = |ix: i32, iy: i32| hash01(ix.rem_euclid(p), iy.rem_euclid(p), 0, seed);
            let (ix, iy) = (x0 as i32, y0 as i32);
            let a = h(ix, iy) * (1.0 - sx) + h(ix + 1, iy) * sx;
            let b = h(ix, iy + 1) * (1.0 - sx) + h(ix + 1, iy + 1) * sx;
            a * (1.0 - sy) + b * sy
        }

        fn fbm(x: f32, y: f32, period: i32, seed: u32, octaves: u32) -> f32 {
            let (mut sum, mut amp, mut norm, mut f) = (0.0, 0.5, 0.0, 1.0);
            for o in 0..octaves {
                sum += amp * periodic_noise(x * f, y * f, period * f as i32, seed.wrapping_add(o * 101));
                norm += amp;
                amp *= 0.5;
                f *= 2.0;
            }
            sum / norm
        }

        /// Tileable Worley (cellular) distance to the nearest feature point.
        fn worley(x: f32, y: f32, period: i32, seed: u32) -> (f32, f32) {
            let (cx, cy) = (x.floor() as i32, y.floor() as i32);
            let (mut d1, mut d2) = (f32::MAX, f32::MAX);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (gx, gy) = (cx + dx, cy + dy);
                    let (wx, wy) = (gx.rem_euclid(period), gy.rem_euclid(period));
                    let px = gx as f32 + hash01(wx, wy, 1, seed);
                    let py = gy as f32 + hash01(wx, wy, 2, seed);
                    let d = ((px - x).powi(2) + (py - y).powi(2)).sqrt();
                    if d < d1 {
                        d2 = d1;
                        d1 = d;
                    } else if d < d2 {
                        d2 = d;
                    }
                }
            }
            (d1, d2)
        }

        /// Height field (0..1) of a procedural surface at texel (x, y).
        fn height_at(kind: &str, x: f32, y: f32, size: f32, cells: i32, seed: u32) -> f32 {
            let c = cells as f32;
            let (u, v) = (x / size * c, y / size * c);
            match kind {
                "brushed_metal" => {
                    0.6 * fbm(u * 0.25, v * 8.0, (cells / 4).max(1), seed, 3) + 0.4 * periodic_noise(u * 0.1, v * 16.0, 1, seed + 9)
                }
                "hammered_metal" => {
                    let (d1, _) = worley(u, v, cells, seed);
                    1.0 - (d1 * 1.2).min(1.0).powf(0.6) * 0.8
                }
                "leather" => {
                    let (d1, d2) = worley(u * 1.5, v * 1.5, (cells as f32 * 1.5) as i32, seed);
                    let crease = ((d2 - d1) * 4.0).min(1.0);
                    0.7 * crease + 0.3 * fbm(u * 2.0, v * 2.0, cells * 2, seed + 3, 3)
                }
                "fabric" => {
                    let threads = 4.0;
                    let wx = ((u * threads * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(0.7);
                    let wy = ((v * threads * std::f32::consts::PI).sin() * 0.5 + 0.5).powf(0.7);
                    let over = ((u * threads).floor() as i32 + (v * threads).floor() as i32) & 1 == 0;
                    0.85 * if over { wx } else { wy } + 0.15 * fbm(u * 4.0, v * 4.0, cells * 4, seed, 2)
                }
                "scales" => {
                    let row = (v * 2.0).floor();
                    let su = u * 2.0 + if row as i32 & 1 == 1 { 0.5 } else { 0.0 };
                    let (fu, fv) = (su.fract() - 0.5, (v * 2.0).fract());
                    let d = (fu * fu + (fv - 0.15).powi(2)).sqrt();
                    (1.0 - d * 1.6).clamp(0.0, 1.0)
                }
                "stone" => {
                    let (d1, d2) = worley(u, v, cells, seed);
                    let edge = ((d2 - d1) * 3.0).min(1.0);
                    0.65 * edge + 0.35 * fbm(u * 3.0, v * 3.0, cells * 3, seed + 5, 4)
                }
                "wood" => {
                    let n = fbm(u * 0.5, v * 4.0, (cells / 2).max(1), seed, 3);
                    ((u * 3.0 + n * 4.0) * std::f32::consts::PI).sin() * 0.35 + 0.5 + (n - 0.5) * 0.3
                }
                "chainmail" => {
                    let (fu, fv) =
                        ((u * 2.0).fract() - 0.5, (v * 2.0 + if (u * 2.0).floor() as i32 & 1 == 1 { 0.5 } else { 0.0 }).fract() - 0.5);
                    let r = (fu * fu + fv * fv).sqrt();
                    (1.0 - ((r - 0.32).abs() * 9.0)).clamp(0.0, 1.0)
                }
                _ => fbm(u, v, cells, seed, 5),
            }
        }

        /// A generated set of PBR maps (all tileable).
        pub struct TextureSet {
            pub albedo: Texture,
            pub normal: Texture,
            pub roughness: Texture,
            pub occlusion: Texture,
            pub height: Texture,
        }

        /// Generates a tileable PBR texture set. `cells` = feature count across the tile.
        pub fn generate_set(name: &str, kind: &str, size: u32, cells: i32, seed: u32, base_roughness: f32, contrast: f32) -> TextureSet {
            let n = size as usize;
            let mut h = vec![0f32; n * n];
            for y in 0..n {
                for x in 0..n {
                    h[y * n + x] = height_at(kind, x as f32 + 0.5, y as f32 + 0.5, size as f32, cells.max(1), seed).clamp(0.0, 1.0);
                }
            }
            let at = |x: i64, y: i64| h[(y.rem_euclid(n as i64) as usize) * n + x.rem_euclid(n as i64) as usize];
            let img = |f: &dyn Fn(usize, usize) -> [u8; 4], suffix: &str| {
                let mut data = Vec::with_capacity(n * n * 4);
                for y in 0..n {
                    for x in 0..n {
                        data.extend_from_slice(&f(x, y));
                    }
                }
                Texture { name: format!("{name}_{suffix}"), width: size, height: size, data }
            };
            let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            let albedo = img(
                &|x, y| {
                    let v = 1.0 - contrast * 0.5
                        + contrast * at(x as i64, y as i64)
                        + (value_noise(Vec3::new(x as f32 * 0.7, y as f32 * 0.7, 0.0), seed + 77) - 0.5) * 0.06;
                    let g = to8(v * 0.92);
                    [g, g, g, 255]
                },
                "albedo",
            );
            let normal = img(
                &|x, y| {
                    let (xi, yi) = (x as i64, y as i64);
                    let dx = (at(xi + 1, yi - 1) + 2.0 * at(xi + 1, yi) + at(xi + 1, yi + 1))
                        - (at(xi - 1, yi - 1) + 2.0 * at(xi - 1, yi) + at(xi - 1, yi + 1));
                    let dy = (at(xi - 1, yi + 1) + 2.0 * at(xi, yi + 1) + at(xi + 1, yi + 1))
                        - (at(xi - 1, yi - 1) + 2.0 * at(xi, yi - 1) + at(xi + 1, yi - 1));
                    let nrm = Vec3::new(-dx * 1.5, dy * 1.5, 1.0).normalized();
                    [to8(nrm.x * 0.5 + 0.5), to8(nrm.y * 0.5 + 0.5), to8(nrm.z * 0.5 + 0.5), 255]
                },
                "normal",
            );
            let roughness = img(
                &|x, y| {
                    let r = to8((base_roughness * (0.8 + 0.4 * (1.0 - at(x as i64, y as i64)))) / 2.0);
                    [r, r, r, 255]
                },
                "roughness",
            );
            let occlusion = img(
                &|x, y| {
                    let (xi, yi) = (x as i64, y as i64);
                    let mut blur = 0.0;
                    for dy in -2..=2 {
                        for dx in -2..=2 {
                            blur += at(xi + dx, yi + dy);
                        }
                    }
                    let cavity = (blur / 25.0 - at(xi, yi)).max(0.0);
                    let a = to8(1.0 - cavity * 2.5);
                    [a, a, a, 255]
                },
                "occlusion",
            );
            let height = img(
                &|x, y| {
                    let v = to8(at(x as i64, y as i64));
                    [v, v, v, 255]
                },
                "height",
            );
            TextureSet { albedo, normal, roughness, occlusion, height }
        }
    }
    pub mod math {
        //! Small, dependency-free 3D math: vectors and rigid affine transforms.

        use std::ops::{Add, Mul, Neg, Sub};

        #[derive(Debug, Clone, Copy, PartialEq, Default)]
        pub struct Vec3 {
            pub x: f32,
            pub y: f32,
            pub z: f32,
        }

        impl Vec3 {
            pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

            pub const fn new(x: f32, y: f32, z: f32) -> Vec3 {
                Vec3 { x, y, z }
            }

            pub fn from_f64(v: [f64; 3]) -> Vec3 {
                Vec3::new(v[0] as f32, v[1] as f32, v[2] as f32)
            }

            pub fn dot(self, o: Vec3) -> f32 {
                self.x * o.x + self.y * o.y + self.z * o.z
            }

            pub fn cross(self, o: Vec3) -> Vec3 {
                Vec3::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
            }

            pub fn length(self) -> f32 {
                self.dot(self).sqrt()
            }

            pub fn normalized(self) -> Vec3 {
                let l = self.length();
                if l > 1e-8 { self * (1.0 / l) } else { Vec3::new(0.0, 0.0, 1.0) }
            }

            pub fn lerp(self, o: Vec3, t: f32) -> Vec3 {
                self + (o - self) * t
            }

            pub fn mul_elem(self, o: Vec3) -> Vec3 {
                Vec3::new(self.x * o.x, self.y * o.y, self.z * o.z)
            }

            pub fn to_array(self) -> [f32; 3] {
                [self.x, self.y, self.z]
            }
        }

        impl Add for Vec3 {
            type Output = Vec3;
            fn add(self, o: Vec3) -> Vec3 {
                Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
            }
        }
        impl Sub for Vec3 {
            type Output = Vec3;
            fn sub(self, o: Vec3) -> Vec3 {
                Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
            }
        }
        impl Mul<f32> for Vec3 {
            type Output = Vec3;
            fn mul(self, s: f32) -> Vec3 {
                Vec3::new(self.x * s, self.y * s, self.z * s)
            }
        }
        impl Neg for Vec3 {
            type Output = Vec3;
            fn neg(self) -> Vec3 {
                Vec3::new(-self.x, -self.y, -self.z)
            }
        }

        /// Affine transform: 3×3 linear part (row-major) + translation.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Affine {
            pub m: [[f32; 3]; 3],
            pub t: Vec3,
        }

        impl Default for Affine {
            fn default() -> Self {
                Affine::IDENTITY
            }
        }

        impl Affine {
            pub const IDENTITY: Affine = Affine { m: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], t: Vec3::ZERO };

            pub fn translation(t: Vec3) -> Affine {
                Affine { t, ..Affine::IDENTITY }
            }

            /// Rotation from Euler angles in degrees, applied X then Y then Z (R = Rz·Ry·Rx).
            pub fn rotation_euler(deg: Vec3) -> Affine {
                let (sx, cx) = deg.x.to_radians().sin_cos();
                let (sy, cy) = deg.y.to_radians().sin_cos();
                let (sz, cz) = deg.z.to_radians().sin_cos();
                let rx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
                let ry = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
                let rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
                Affine { m: mat_mul(rz, mat_mul(ry, rx)), t: Vec3::ZERO }
            }

            /// Rotation by `deg` around `pivot`.
            pub fn rotation_about(pivot: Vec3, deg: Vec3) -> Affine {
                Affine::translation(pivot).then(&Affine::rotation_euler(deg)).then(&Affine::translation(-pivot))
            }

            /// `self ∘ other`: apply `other` first, then `self`.
            pub fn then(&self, other: &Affine) -> Affine {
                Affine { m: mat_mul(self.m, other.m), t: self.apply_vector(other.t) + self.t }
            }

            pub fn apply(&self, p: Vec3) -> Vec3 {
                self.apply_vector(p) + self.t
            }

            pub fn apply_vector(&self, v: Vec3) -> Vec3 {
                let m = &self.m;
                Vec3::new(
                    m[0][0] * v.x + m[0][1] * v.y + m[0][2] * v.z,
                    m[1][0] * v.x + m[1][1] * v.y + m[1][2] * v.z,
                    m[2][0] * v.x + m[2][1] * v.y + m[2][2] * v.z,
                )
            }
        }

        fn mat_mul(a: [[f32; 3]; 3], b: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
            let mut out = [[0.0; 3]; 3];
            for (i, row) in out.iter_mut().enumerate() {
                for (j, cell) in row.iter_mut().enumerate() {
                    *cell = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
                }
            }
            out
        }
    }
    pub mod model {
        //! Sparse voxel models ("3D pixels") with per-voxel true color, material and bone.

        use std::collections::HashMap;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};

        use super::math::Vec3;
        use super::rig::{Animation, Bone};

        pub const CHUNK: i32 = 16;
        pub const CHUNK_VOLUME: usize = (CHUNK * CHUNK * CHUNK) as usize;
        pub const MAX_MODEL_SIZE: u32 = 512;
        pub const MAX_BONES: usize = 255;

        /// One 3D pixel. `rgba` alpha 0 = empty. `material` indexes the project's materials
        /// (0 = default), `bone` indexes the model's skeleton (0 = root / unbound).
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        pub struct Voxel {
            pub rgba: u32,
            pub material: u8,
            pub bone: u8,
        }

        impl Voxel {
            pub const EMPTY: Voxel = Voxel { rgba: 0, material: 0, bone: 0 };

            #[inline]
            pub fn is_empty(&self) -> bool {
                self.rgba & 0xFF == 0
            }
        }

        pub type Chunk = [Voxel; CHUNK_VOLUME];
        pub type ChunkKey = (i32, i32, i32);

        static VERSION: AtomicU64 = AtomicU64::new(1);

        /// Globally unique edit stamp; render caches compare it to know when to rebuild.
        pub fn next_version() -> u64 {
            VERSION.fetch_add(1, Ordering::Relaxed)
        }

        #[derive(Debug, Clone)]
        pub struct VoxelModel {
            pub name: String,
            pub size: [u32; 3],
            pub chunks: HashMap<ChunkKey, Arc<Chunk>>,
            pub bones: Vec<Bone>,
            pub animations: Vec<Animation>,
            /// Model-space point placed at the bottom-center of rendered sprites (usually between the feet).
            pub origin: Vec3,
            pub version: u64,
        }

        impl VoxelModel {
            pub fn new(name: &str, size: [u32; 3]) -> VoxelModel {
                VoxelModel {
                    name: name.into(),
                    size,
                    chunks: HashMap::new(),
                    bones: Vec::new(),
                    animations: Vec::new(),
                    origin: Vec3::new(size[0] as f32 / 2.0, 0.0, size[2] as f32 / 2.0),
                    version: next_version(),
                }
            }

            pub fn in_bounds(&self, x: i32, y: i32, z: i32) -> bool {
                x >= 0 && y >= 0 && z >= 0 && (x as u32) < self.size[0] && (y as u32) < self.size[1] && (z as u32) < self.size[2]
            }

            #[inline]
            fn split(x: i32, y: i32, z: i32) -> (ChunkKey, usize) {
                let key = (x.div_euclid(CHUNK), y.div_euclid(CHUNK), z.div_euclid(CHUNK));
                let (lx, ly, lz) = (x.rem_euclid(CHUNK), y.rem_euclid(CHUNK), z.rem_euclid(CHUNK));
                (key, ((lz * CHUNK + ly) * CHUNK + lx) as usize)
            }

            #[inline]
            pub fn get(&self, x: i32, y: i32, z: i32) -> Voxel {
                if !self.in_bounds(x, y, z) {
                    return Voxel::EMPTY;
                }
                let (key, i) = Self::split(x, y, z);
                self.chunks.get(&key).map(|c| c[i]).unwrap_or(Voxel::EMPTY)
            }

            /// Writes a voxel (clipped to bounds). Returns true when it changed.
            pub fn set(&mut self, x: i32, y: i32, z: i32, v: Voxel) -> bool {
                if !self.in_bounds(x, y, z) {
                    return false;
                }
                let v = if v.is_empty() { Voxel::EMPTY } else { v };
                let (key, i) = Self::split(x, y, z);
                let changed = match self.chunks.get_mut(&key) {
                    None if v.is_empty() => false,
                    None => {
                        let mut c = [Voxel::EMPTY; CHUNK_VOLUME];
                        c[i] = v;
                        self.chunks.insert(key, Arc::new(c));
                        true
                    }
                    Some(c) => {
                        if c[i] == v {
                            false
                        } else {
                            Arc::make_mut(c)[i] = v;
                            true
                        }
                    }
                };
                if changed {
                    self.version = next_version();
                }
                changed
            }

            pub fn touch(&mut self) {
                self.version = next_version();
            }

            /// Drops chunks that became empty.
            pub fn compact(&mut self) {
                self.chunks.retain(|_, c| c.iter().any(|v| !v.is_empty()));
            }

            pub fn for_each(&self, mut f: impl FnMut(i32, i32, i32, Voxel)) {
                let mut keys: Vec<&ChunkKey> = self.chunks.keys().collect();
                keys.sort();
                for key in keys {
                    let c = &self.chunks[key];
                    for (i, v) in c.iter().enumerate() {
                        if v.is_empty() {
                            continue;
                        }
                        let i = i as i32;
                        f(key.0 * CHUNK + i % CHUNK, key.1 * CHUNK + (i / CHUNK) % CHUNK, key.2 * CHUNK + i / (CHUNK * CHUNK), *v);
                    }
                }
            }

            pub fn count(&self) -> usize {
                self.chunks.values().map(|c| c.iter().filter(|v| !v.is_empty()).count()).sum()
            }

            /// Inclusive min/max corner of filled voxels.
            pub fn bounds(&self) -> Option<([i32; 3], [i32; 3])> {
                let mut lo = [i32::MAX; 3];
                let mut hi = [i32::MIN; 3];
                let mut any = false;
                self.for_each(|x, y, z, _| {
                    any = true;
                    for (k, v) in [x, y, z].into_iter().enumerate() {
                        lo[k] = lo[k].min(v);
                        hi[k] = hi[k].max(v);
                    }
                });
                any.then_some((lo, hi))
            }

            pub fn bone_index(&self, name: &str) -> Option<usize> {
                self.bones.iter().position(|b| b.name == name)
            }
        }

        /// Primitive volumes for sculpting. All coordinates are in voxel units; a voxel is inside when
        /// its center is inside the shape.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum Shape {
            Box {
                min: Vec3,
                max: Vec3,
            },
            Ellipsoid {
                center: Vec3,
                radii: Vec3,
            },
            /// Tapered capsule from `a` (radius `ra`) to `b` (radius `rb`) — limbs, horns, fingers.
            Capsule {
                a: Vec3,
                b: Vec3,
                ra: f32,
                rb: f32,
            },
            /// Cylinder from `a` to `b` with radius `r` (flat caps).
            Cylinder {
                a: Vec3,
                b: Vec3,
                r: f32,
            },
        }

        impl Shape {
            pub fn aabb(&self) -> (Vec3, Vec3) {
                match *self {
                    Shape::Box { min, max } => (min, max),
                    Shape::Ellipsoid { center, radii } => (center - radii, center + radii),
                    Shape::Capsule { a, b, ra, rb } => {
                        let r = ra.max(rb);
                        (
                            Vec3::new(a.x.min(b.x) - r, a.y.min(b.y) - r, a.z.min(b.z) - r),
                            Vec3::new(a.x.max(b.x) + r, a.y.max(b.y) + r, a.z.max(b.z) + r),
                        )
                    }
                    Shape::Cylinder { a, b, r } => (
                        Vec3::new(a.x.min(b.x) - r, a.y.min(b.y) - r, a.z.min(b.z) - r),
                        Vec3::new(a.x.max(b.x) + r, a.y.max(b.y) + r, a.z.max(b.z) + r),
                    ),
                }
            }

            pub fn contains(&self, p: Vec3) -> bool {
                match *self {
                    Shape::Box { min, max } => p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y && p.z >= min.z && p.z <= max.z,
                    Shape::Ellipsoid { center, radii } => {
                        let d = p - center;
                        let (rx, ry, rz) = (radii.x.max(0.5), radii.y.max(0.5), radii.z.max(0.5));
                        (d.x / rx).powi(2) + (d.y / ry).powi(2) + (d.z / rz).powi(2) <= 1.0
                    }
                    Shape::Capsule { a, b, ra, rb } => {
                        let ab = b - a;
                        let len2 = ab.dot(ab);
                        let t = if len2 < 1e-6 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
                        let r = ra + (rb - ra) * t;
                        (p - (a + ab * t)).length() <= r.max(0.5)
                    }
                    Shape::Cylinder { a, b, r } => {
                        let ab = b - a;
                        let len2 = ab.dot(ab);
                        if len2 < 1e-6 {
                            return false;
                        }
                        let t = (p - a).dot(ab) / len2;
                        (0.0..=1.0).contains(&t) && (p - (a + ab * t)).length() <= r.max(0.5)
                    }
                }
            }

            /// 0..1 position of a point along the shape's main axis (for gradients).
            pub fn axis_t(&self, p: Vec3, axis: usize) -> f32 {
                let (lo, hi) = self.aabb();
                let (l, h, v) = match axis {
                    0 => (lo.x, hi.x, p.x),
                    1 => (lo.y, hi.y, p.y),
                    _ => (lo.z, hi.z, p.z),
                };
                if h - l < 1e-6 { 0.0 } else { ((v - l) / (h - l)).clamp(0.0, 1.0) }
            }
        }

        /// Integer hash → [0, 1). Deterministic noise for color variation.
        pub fn hash01(x: i32, y: i32, z: i32, seed: u32) -> f32 {
            let mut h = (x as u32).wrapping_mul(0x8DA6_B343)
                ^ (y as u32).wrapping_mul(0xD816_3841)
                ^ (z as u32).wrapping_mul(0xCB1A_B31F)
                ^ seed.wrapping_mul(0x9E37_79B9);
            h ^= h >> 13;
            h = h.wrapping_mul(0x5BD1_E995);
            h ^= h >> 15;
            (h & 0x00FF_FFFF) as f32 / 16_777_216.0
        }

        /// Smooth 3D value noise in [0, 1).
        pub fn value_noise(p: Vec3, seed: u32) -> f32 {
            let (x0, y0, z0) = (p.x.floor(), p.y.floor(), p.z.floor());
            let (fx, fy, fz) = (p.x - x0, p.y - y0, p.z - z0);
            let s = |t: f32| t * t * (3.0 - 2.0 * t);
            let (sx, sy, sz) = (s(fx), s(fy), s(fz));
            let (ix, iy, iz) = (x0 as i32, y0 as i32, z0 as i32);
            let mut acc = 0.0;
            for dz in 0..2 {
                for dy in 0..2 {
                    for dx in 0..2 {
                        let w = (if dx == 1 { sx } else { 1.0 - sx })
                            * (if dy == 1 { sy } else { 1.0 - sy })
                            * (if dz == 1 { sz } else { 1.0 - sz });
                        acc += w * hash01(ix + dx, iy + dy, iz + dz, seed);
                    }
                }
            }
            acc
        }
    }
    pub mod render {
        //! Voxel renderer.
        //!
        //! * Rest-pose surface extraction: exposed faces with smooth normals, two-scale ambient occlusion
        //!   and MagicaVoxel-style per-corner occlusion (soft darkening in creases and inner corners).
        //! * Rigid bone skinning and orthographic rasterization with sub-voxel UVs.
        //! * PBR (or toon) shading in view space: key/fill/rim lights with a shadow map (PCF), point lights
        //!   (fires, windows, magic), studio reflections, emission with bloom.
        //! * G-buffer passes, supersampling, selective outline, inner lines, gradient backgrounds and a
        //!   ground shadow catcher for transparent sprites.

        use std::sync::Arc;

        use super::material::{Material, Texture, sample_surface};
        use super::math::{Affine, Vec3};
        use super::model::{Voxel, VoxelModel};
        use super::rig::pose;
        use crate::pixel::Rgba;
        use crate::renderer::Image;

        /// Face directions: +X, -X, +Y, -Y, +Z, -Z.
        pub const DIRS: [[i32; 3]; 6] = [[1, 0, 0], [-1, 0, 0], [0, 1, 0], [0, -1, 0], [0, 0, 1], [0, 0, -1]];

        /// One exposed voxel face in rest pose.
        #[derive(Debug, Clone, Copy)]
        pub struct Face {
            pub pos: [i32; 3],
            pub dir: u8,
            pub voxel: Voxel,
            /// Smooth surface normal (model space) of the voxel.
            pub smooth: Vec3,
            /// Neighbourhood ambient occlusion 0..1 (1 = open).
            pub ao: f32,
            /// Per-corner occlusion in `face_corners` order, interpolated across the face.
            pub corner_ao: [f32; 4],
        }

        /// Cached render geometry of a model version.
        pub struct Prepared {
            pub version: u64,
            pub faces: Vec<Face>,
        }

        /// Dense occupancy bitset for fast neighbourhood queries.
        struct Occupancy {
            size: [i32; 3],
            bits: Vec<u64>,
        }

        impl Occupancy {
            fn build(model: &VoxelModel) -> Occupancy {
                let size = [model.size[0] as i32, model.size[1] as i32, model.size[2] as i32];
                let total = (size[0] as usize) * (size[1] as usize) * (size[2] as usize);
                let mut occ = Occupancy { size, bits: vec![0; total.div_ceil(64)] };
                model.for_each(|x, y, z, _| {
                    let i = occ.index(x, y, z);
                    occ.bits[i >> 6] |= 1 << (i & 63);
                });
                occ
            }

            #[inline]
            fn index(&self, x: i32, y: i32, z: i32) -> usize {
                ((z as usize * self.size[1] as usize) + y as usize) * self.size[0] as usize + x as usize
            }

            #[inline]
            fn get(&self, x: i32, y: i32, z: i32) -> bool {
                if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
                    return false;
                }
                let i = self.index(x, y, z);
                self.bits[i >> 6] >> (i & 63) & 1 == 1
            }
        }

        /// Coarse (4×) occupancy fractions for large-scale occlusion (armpits, under the cape, ...).
        struct Coarse {
            size: [i32; 3],
            fill: Vec<f32>,
        }

        const COARSE: i32 = 4;

        impl Coarse {
            fn build(occ: &Occupancy) -> Coarse {
                let size = [(occ.size[0] + COARSE - 1) / COARSE, (occ.size[1] + COARSE - 1) / COARSE, (occ.size[2] + COARSE - 1) / COARSE];
                let mut fill = vec![0f32; (size[0] * size[1] * size[2]) as usize];
                for cz in 0..size[2] {
                    for cy in 0..size[1] {
                        for cx in 0..size[0] {
                            let mut n = 0;
                            for z in 0..COARSE {
                                for y in 0..COARSE {
                                    for x in 0..COARSE {
                                        n += occ.get(cx * COARSE + x, cy * COARSE + y, cz * COARSE + z) as i32;
                                    }
                                }
                            }
                            fill[((cz * size[1] + cy) * size[0] + cx) as usize] = n as f32 / (COARSE * COARSE * COARSE) as f32;
                        }
                    }
                }
                Coarse { size, fill }
            }

            fn get(&self, x: i32, y: i32, z: i32) -> f32 {
                if x < 0 || y < 0 || z < 0 || x >= self.size[0] || y >= self.size[1] || z >= self.size[2] {
                    return 0.0;
                }
                self.fill[((z * self.size[1] + y) * self.size[0] + x) as usize]
            }
        }

        const CORNER_AO: [f32; 4] = [0.46, 0.66, 0.84, 1.0];

        /// Extracts exposed faces with smooth normals and ambient occlusion.
        pub fn prepare(model: &VoxelModel) -> Prepared {
            let occ = Occupancy::build(model);
            let coarse = Coarse::build(&occ);
            let mut faces = Vec::new();
            model.for_each(|x, y, z, v| {
                let exposed: Vec<u8> = (0..6u8)
                    .filter(|&d| {
                        let o = DIRS[d as usize];
                        !occ.get(x + o[0], y + o[1], z + o[2])
                    })
                    .collect();
                if exposed.is_empty() {
                    return;
                }
                // Smooth normal: points away from the occupied neighbourhood (radius 2).
                let mut g = Vec3::ZERO;
                for dz in -2..=2i32 {
                    for dy in -2..=2i32 {
                        for dx in -2..=2i32 {
                            if (dx, dy, dz) != (0, 0, 0) && occ.get(x + dx, y + dy, z + dz) {
                                let w = 1.0 / ((dx * dx + dy * dy + dz * dz) as f32).sqrt();
                                g = g - Vec3::new(dx as f32, dy as f32, dz as f32) * w;
                            }
                        }
                    }
                }
                let face_sum = exposed.iter().fold(Vec3::ZERO, |a, &d| {
                    let o = DIRS[d as usize];
                    a + Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32)
                });
                let smooth = if g.length() < 0.5 {
                    face_sum.normalized()
                } else {
                    (g.normalized() * 0.8 + face_sum.normalized() * 0.2).normalized()
                };
                for d in exposed {
                    let o = DIRS[d as usize];
                    // Fine AO: occupancy of the 5×5×5 block in front of the face.
                    let (fx, fy, fz) = (x + o[0] * 2, y + o[1] * 2, z + o[2] * 2);
                    let mut n = 0;
                    for dz in -2..=2 {
                        for dy in -2..=2 {
                            for dx in -2..=2 {
                                n += occ.get(fx + dx, fy + dy, fz + dz) as i32;
                            }
                        }
                    }
                    let fine = 1.0 - (n as f32 / 125.0 * 1.6).min(0.85);
                    // Coarse AO: 3×3×3 coarse cells around a point ~6 voxels in front of the face.
                    let (cx, cy, cz) =
                        ((x + o[0] * 6).div_euclid(COARSE), (y + o[1] * 6).div_euclid(COARSE), (z + o[2] * 6).div_euclid(COARSE));
                    let mut c = 0.0;
                    for dz in -1..=1 {
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                c += coarse.get(cx + dx, cy + dy, cz + dz);
                            }
                        }
                    }
                    let broad = 1.0 - (c / 27.0 * 1.4).min(0.7);
                    // Per-corner AO (classic voxel rule: two sides + diagonal of the cell in front).
                    let front = [x + o[0], y + o[1], z + o[2]];
                    let tangents: Vec<usize> = (0..3).filter(|&k| o[k] == 0).collect();
                    let mut corner_ao = [1.0; 4];
                    for (k, c) in face_corners(d).iter().enumerate() {
                        let (ta, tb) = (tangents[0], tangents[1]);
                        let sa = if c[ta] > 0.5 { 1 } else { -1 };
                        let sb = if c[tb] > 0.5 { 1 } else { -1 };
                        let at = |da: i32, db: i32| {
                            let mut p = front;
                            p[ta] += da;
                            p[tb] += db;
                            occ.get(p[0], p[1], p[2])
                        };
                        let (s1, s2, cn) = (at(sa, 0), at(0, sb), at(sa, sb));
                        let level = if s1 && s2 { 0 } else { 3 - (s1 as usize + s2 as usize + cn as usize) };
                        corner_ao[k] = CORNER_AO[level];
                    }
                    faces.push(Face { pos: [x, y, z], dir: d, voxel: v, smooth, ao: (fine * broad).clamp(0.08, 1.0), corner_ao });
                }
            });
            Prepared { version: model.version, faces }
        }

        // ---- camera, lights, settings ---------------------------------------------------------

        #[derive(Debug, Clone, Copy)]
        pub struct Camera {
            /// Turntable angle in degrees. 0 = the model's front (+Z) faces the viewer.
            pub yaw: f32,
            /// Elevation in degrees (0 = side view, 90 = top-down).
            pub pitch: f32,
            /// Pixels per voxel.
            pub scale: f32,
            pub width: u32,
            pub height: u32,
            /// Screen position (pixels) of the model's origin.
            pub anchor_x: f32,
            pub anchor_y: f32,
        }

        impl Camera {
            /// Rotation from model space (relative to the origin) into view space (x right, y up, z toward viewer).
            pub fn view_rotation(&self) -> Affine {
                let yaw = Affine::rotation_euler(Vec3::new(0.0, self.yaw, 0.0));
                let pitch = Affine::rotation_euler(Vec3::new(self.pitch, 0.0, 0.0));
                pitch.then(&yaw)
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Pass {
            Lit,
            Albedo,
            Normal,
            Depth,
            Occlusion,
            Roughness,
            Metallic,
            Emission,
        }

        impl Pass {
            pub fn parse(s: &str) -> Option<Pass> {
                Some(match s.to_ascii_lowercase().as_str() {
                    "lit" | "beauty" | "final" => Pass::Lit,
                    "albedo" | "basecolor" | "color" => Pass::Albedo,
                    "normal" | "normals" => Pass::Normal,
                    "depth" => Pass::Depth,
                    "ao" | "occlusion" => Pass::Occlusion,
                    "roughness" => Pass::Roughness,
                    "metallic" | "metalness" => Pass::Metallic,
                    "emission" | "emissive" => Pass::Emission,
                    _ => return None,
                })
            }
        }

        #[derive(Debug, Clone, Copy, PartialEq)]
        pub enum Outline {
            None,
            /// Selective outline: each edge pixel is a darkened, cooler version of the color it borders.
            Selective,
            Color(Rgba),
        }

        #[derive(Debug, Clone, Copy)]
        pub struct Light {
            /// Direction the light comes FROM, in view space.
            pub dir: Vec3,
            pub color: Vec3,
            pub intensity: f32,
        }

        /// Local light in model space (campfires, lanterns, windows, magic).
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct PointLight {
            pub pos: Vec3,
            /// Linear RGB.
            pub color: Vec3,
            pub intensity: f32,
            /// Distance (voxels) at which the light fades to zero.
            pub radius: f32,
        }

        #[derive(Debug, Clone)]
        pub struct Lighting {
            pub lights: Vec<Light>,
            pub points: Vec<PointLight>,
            pub sky: Vec3,
            pub ground: Vec3,
            pub exposure: f32,
        }

        impl Default for Lighting {
            fn default() -> Self {
                Lighting {
                    lights: vec![
                        Light { dir: Vec3::new(-0.55, 0.7, 0.55).normalized(), color: Vec3::new(1.0, 0.94, 0.85), intensity: 3.2 },
                        Light { dir: Vec3::new(0.75, 0.15, 0.4).normalized(), color: Vec3::new(0.55, 0.65, 1.0), intensity: 0.9 },
                        Light { dir: Vec3::new(0.3, 0.45, -0.9).normalized(), color: Vec3::new(0.85, 0.9, 1.0), intensity: 2.0 },
                    ],
                    points: Vec::new(),
                    sky: Vec3::new(0.32, 0.36, 0.48),
                    ground: Vec3::new(0.16, 0.13, 0.12),
                    exposure: 1.0,
                }
            }
        }

        #[derive(Debug, Clone)]
        pub struct RenderSettings {
            pub camera: Camera,
            /// (animation index, time in frames)
            pub animation: Option<(usize, f32)>,
            pub pass: Pass,
            /// 0 = flat voxel faces ("3D pixel" look), 1 = fully smooth normals (sprite look).
            pub smoothness: f32,
            /// 0 = physically based; N ≥ 2 = toon shading with N light bands.
            pub toon_steps: u32,
            pub outline: Outline,
            /// Darken depth discontinuities inside the silhouette.
            pub inner_lines: bool,
            pub supersample: u32,
            pub background: Option<Rgba>,
            /// Vertical background gradient (top, bottom) with a soft vignette.
            pub background_gradient: Option<(Rgba, Rgba)>,
            pub lighting: Lighting,
            /// Cast shadows from the key light (0 = off, 1 = full strength).
            pub shadows: f32,
            /// Ground shadow under the model for transparent sprites (alpha 0..1, 0 = off).
            pub ground_shadow: f32,
            /// Glow strength around emissive surfaces (0 = off).
            pub bloom: f32,
            /// Glow radius in output pixels (0 = automatic).
            pub bloom_radius: f32,
        }

        impl RenderSettings {
            pub fn new(camera: Camera) -> RenderSettings {
                RenderSettings {
                    camera,
                    animation: None,
                    pass: Pass::Lit,
                    smoothness: 0.6,
                    toon_steps: 0,
                    outline: Outline::None,
                    inner_lines: false,
                    supersample: 1,
                    background: None,
                    background_gradient: None,
                    lighting: Lighting::default(),
                    shadows: 0.0,
                    ground_shadow: 0.0,
                    bloom: 0.0,
                    bloom_radius: 0.0,
                }
            }
        }

        // ---- rasterization --------------------------------------------------------------------

        struct Target {
            w: usize,
            h: usize,
            depth: Vec<f32>,
            face: Vec<u32>,
            uv: Vec<[f32; 2]>,
        }

        const NO_FACE: u32 = u32::MAX;

        impl Target {
            fn new(w: usize, h: usize, with_uv: bool) -> Target {
                Target {
                    w,
                    h,
                    depth: vec![f32::NEG_INFINITY; w * h],
                    face: vec![NO_FACE; w * h],
                    uv: if with_uv { vec![[0.0; 2]; w * h] } else { Vec::new() },
                }
            }

            #[inline]
            fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
                (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
            }

            fn triangle(&mut self, p: [(f32, f32, f32); 3], uv: [[f32; 2]; 3], id: u32) {
                let area = Self::edge((p[0].0, p[0].1), (p[1].0, p[1].1), (p[2].0, p[2].1));
                if area.abs() < 1e-9 {
                    return;
                }
                let minx = p.iter().map(|v| v.0).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
                let maxx = p.iter().map(|v| v.0).fold(f32::MIN, f32::max).ceil().min(self.w as f32 - 1.0) as i32;
                let miny = p.iter().map(|v| v.1).fold(f32::MAX, f32::min).floor().max(0.0) as i32;
                let maxy = p.iter().map(|v| v.1).fold(f32::MIN, f32::max).ceil().min(self.h as f32 - 1.0) as i32;
                if minx > maxx || miny > maxy {
                    return;
                }
                let inv = 1.0 / area;
                let with_uv = !self.uv.is_empty();
                for y in miny..=maxy {
                    for x in minx..=maxx {
                        let c = (x as f32 + 0.5, y as f32 + 0.5);
                        let w0 = Self::edge((p[1].0, p[1].1), (p[2].0, p[2].1), c) * inv;
                        let w1 = Self::edge((p[2].0, p[2].1), (p[0].0, p[0].1), c) * inv;
                        let w2 = 1.0 - w0 - w1;
                        // Small epsilon keeps shared edges watertight.
                        if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                            continue;
                        }
                        let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
                        let i = y as usize * self.w + x as usize;
                        if z > self.depth[i] {
                            self.depth[i] = z;
                            self.face[i] = id;
                            if with_uv {
                                self.uv[i] = [
                                    (w0 * uv[0][0] + w1 * uv[1][0] + w2 * uv[2][0]).clamp(0.0, 1.0),
                                    (w0 * uv[0][1] + w1 * uv[1][1] + w2 * uv[2][1]).clamp(0.0, 1.0),
                                ];
                            }
                        }
                    }
                }
            }
        }

        /// Face corner offsets (unit cube) for each direction. The four corners form a parallelogram:
        /// corner 0 = uv (0,0), 1 = (1,0), 2 = (1,1), 3 = (0,1).
        fn face_corners(dir: u8) -> [[f32; 3]; 4] {
            match dir {
                0 => [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [1.0, 1.0, 1.0], [1.0, 0.0, 1.0]],
                1 => [[0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 1.0], [0.0, 1.0, 0.0]],
                2 => [[0.0, 1.0, 0.0], [0.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 0.0]],
                3 => [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [1.0, 0.0, 1.0], [0.0, 0.0, 1.0]],
                4 => [[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
                _ => [[0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0], [1.0, 0.0, 0.0]],
            }
        }

        const QUAD_UV: [[f32; 2]; 4] = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];

        /// Model-space point on a face at (u, v).
        fn face_point(f: &Face, u: f32, v: f32) -> Vec3 {
            let c = face_corners(f.dir);
            let at = |k: usize| Vec3::new(c[k][0], c[k][1], c[k][2]);
            let base = Vec3::new(f.pos[0] as f32, f.pos[1] as f32, f.pos[2] as f32);
            base + at(0) + (at(1) - at(0)) * u + (at(3) - at(0)) * v
        }

        /// Camera rotation composed with the origin shift (model space → view space).
        fn camera_view(model: &VoxelModel, settings: &RenderSettings) -> Affine {
            settings.camera.view_rotation().then(&Affine::translation(-model.origin))
        }

        /// Per-bone transform from model space into view space (before projection).
        fn bone_views(model: &VoxelModel, settings: &RenderSettings) -> Vec<Affine> {
            let view = camera_view(model, settings);
            let anim = settings.animation.and_then(|(i, t)| model.animations.get(i).map(|a| (a, t)));
            let world = pose(&model.bones, anim.map(|(a, _)| a), anim.map(|(_, t)| t).unwrap_or(0.0));
            if world.is_empty() { vec![view] } else { world.iter().map(|w| view.then(w)).collect() }
        }

        /// Screen-space extent of the posed model: (min x, max x, min y, max y) in view units.
        pub fn view_extent(model: &VoxelModel, prepared: &Prepared, settings: &RenderSettings) -> Option<(f32, f32, f32, f32)> {
            let views = bone_views(model, settings);
            let mut e = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
            let mut any = false;
            for f in &prepared.faces {
                let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
                let c = m.apply(Vec3::new(f.pos[0] as f32 + 0.5, f.pos[1] as f32 + 0.5, f.pos[2] as f32 + 0.5));
                any = true;
                e = (e.0.min(c.x - 0.9), e.1.max(c.x + 0.9), e.2.min(c.y - 0.9), e.3.max(c.y + 0.9));
            }
            any.then_some(e)
        }

        fn rasterize(model: &VoxelModel, prepared: &Prepared, settings: &RenderSettings, ss: u32, with_uv: bool) -> (Target, Vec<Affine>) {
            let cam = &settings.camera;
            let (w, h) = ((cam.width * ss) as usize, (cam.height * ss) as usize);
            let mut target = Target::new(w, h, with_uv);
            let views = bone_views(model, settings);
            let s = cam.scale * ss as f32;
            let (ax, ay) = (cam.anchor_x * ss as f32, cam.anchor_y * ss as f32);
            for (id, f) in prepared.faces.iter().enumerate() {
                let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
                let o = DIRS[f.dir as usize];
                let n = m.apply_vector(Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32));
                if n.z <= 1e-4 {
                    continue;
                }
                let base = Vec3::new(f.pos[0] as f32, f.pos[1] as f32, f.pos[2] as f32);
                let corners = face_corners(f.dir).map(|c| {
                    let p = m.apply(base + Vec3::new(c[0], c[1], c[2]));
                    (ax + p.x * s, ay - p.y * s, p.z)
                });
                target.triangle([corners[0], corners[1], corners[2]], [QUAD_UV[0], QUAD_UV[1], QUAD_UV[2]], id as u32);
                target.triangle([corners[0], corners[2], corners[3]], [QUAD_UV[0], QUAD_UV[2], QUAD_UV[3]], id as u32);
            }
            (target, views)
        }

        // ---- shadow map -------------------------------------------------------------------------

        /// Orthographic depth map from the key light, in view space.
        struct ShadowMap {
            lx: Vec3,
            ly: Vec3,
            lz: Vec3,
            min_x: f32,
            min_y: f32,
            texel: f32,
            target: Target,
        }

        impl ShadowMap {
            fn build(prepared: &Prepared, views: &[Affine], light_dir: Vec3) -> Option<ShadowMap> {
                let lz = light_dir.normalized();
                let up = if lz.y.abs() > 0.95 { Vec3::new(1.0, 0.0, 0.0) } else { Vec3::new(0.0, 1.0, 0.0) };
                let lx = up.cross(lz).normalized();
                let ly = lz.cross(lx);
                let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
                for f in &prepared.faces {
                    let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
                    let c = m.apply(Vec3::new(f.pos[0] as f32 + 0.5, f.pos[1] as f32 + 0.5, f.pos[2] as f32 + 0.5));
                    let (x, y) = (c.dot(lx), c.dot(ly));
                    x0 = x0.min(x);
                    x1 = x1.max(x);
                    y0 = y0.min(y);
                    y1 = y1.max(y);
                }
                if x0 > x1 {
                    return None;
                }
                let (x0, y0) = (x0 - 2.0, y0 - 2.0);
                let extent = (x1 + 2.0 - x0).max(y1 + 2.0 - y0);
                // Two texels per voxel, capped for very large models.
                let texel = (extent / 2048.0).max(0.5);
                let n = (extent / texel).ceil() as usize + 2;
                let mut target = Target::new(n, n, false);
                for (id, f) in prepared.faces.iter().enumerate() {
                    let m = &views[(f.voxel.bone as usize).min(views.len() - 1)];
                    let base = Vec3::new(f.pos[0] as f32, f.pos[1] as f32, f.pos[2] as f32);
                    let c = face_corners(f.dir).map(|c| {
                        let p = m.apply(base + Vec3::new(c[0], c[1], c[2]));
                        ((p.dot(lx) - x0) / texel, (p.dot(ly) - y0) / texel, p.dot(lz))
                    });
                    target.triangle([c[0], c[1], c[2]], [[0.0; 2]; 3], id as u32);
                    target.triangle([c[0], c[2], c[3]], [[0.0; 2]; 3], id as u32);
                }
                Some(ShadowMap { lx, ly, lz, min_x: x0, min_y: y0, texel, target })
            }

            /// 0 = fully shadowed, 1 = lit. 3×3 percentage-closer filtering.
            fn visibility(&self, p: Vec3) -> f32 {
                let fx = (p.dot(self.lx) - self.min_x) / self.texel;
                let fy = (p.dot(self.ly) - self.min_y) / self.texel;
                let depth = p.dot(self.lz);
                let (cx, cy) = (fx.floor() as i64, fy.floor() as i64);
                let mut lit = 0.0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (x, y) = (cx + dx, cy + dy);
                        if x < 0 || y < 0 || x >= self.target.w as i64 || y >= self.target.h as i64 {
                            lit += 1.0;
                            continue;
                        }
                        let occluder = self.target.depth[y as usize * self.target.w + x as usize];
                        if !occluder.is_finite() || depth >= occluder - 0.9 {
                            lit += 1.0;
                        }
                    }
                }
                lit / 9.0
            }
        }

        // ---- shading ------------------------------------------------------------------------------

        #[inline]
        fn srgb_to_linear(c: u8) -> f32 {
            let v = c as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        }

        #[inline]
        fn linear_to_srgb(v: f32) -> u8 {
            let v = v.clamp(0.0, 1.0);
            let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
            (s * 255.0 + 0.5) as u8
        }

        fn aces(x: f32) -> f32 {
            ((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14)).clamp(0.0, 1.0)
        }

        /// Studio environment for reflections, in view space (y up, z toward the viewer).
        fn environment(r: Vec3, rough: f32, l: &Lighting) -> Vec3 {
            let up = r.y;
            let horizon = Vec3::new(0.85, 0.72, 0.58);
            let base = if up >= 0.0 { horizon.lerp(l.sky * 2.2, up.powf(0.6)) } else { horizon.lerp(l.ground * 0.6, (-up).powf(0.5)) };
            let key = Vec3::new(-0.45, 0.75, 0.48).normalized();
            let sharp = 1.0 - rough;
            let soft = r.dot(key).max(0.0).powf(4.0 + 60.0 * sharp * sharp) * (1.0 + 6.0 * sharp);
            let rim = r.dot(Vec3::new(0.6, 0.35, -0.72).normalized()).max(0.0).powf(6.0 + 30.0 * sharp) * 1.5;
            base + Vec3::new(1.0, 0.95, 0.88) * soft + Vec3::new(0.55, 0.7, 1.0) * rim
        }

        struct Brdf {
            n: Vec3,
            albedo: Vec3,
            rough: f32,
            metal: f32,
            f0: Vec3,
            ndv: f32,
            toon: u32,
        }

        impl Brdf {
            /// Reflected radiance for light arriving from direction `l` with radiance `radiance`.
            fn light(&self, l: Vec3, radiance: Vec3) -> Vec3 {
                let view_dir = Vec3::new(0.0, 0.0, 1.0);
                let mut ndl = self.n.dot(l).max(0.0);
                if self.toon >= 2 {
                    let steps = self.toon as f32;
                    ndl = ((ndl * steps).floor() / (steps - 1.0)).min(1.0);
                }
                if ndl <= 0.0 {
                    return Vec3::ZERO;
                }
                let a = self.rough * self.rough;
                let a2 = a * a;
                let k = (self.rough + 1.0).powi(2) / 8.0;
                let h = (l + view_dir).normalized();
                let ndh = self.n.dot(h).max(0.0);
                let vdh = view_dir.dot(h).max(0.0);
                let d = a2 / (std::f32::consts::PI * ((ndh * ndh) * (a2 - 1.0) + 1.0).powi(2)).max(1e-6);
                let g = (self.ndv / (self.ndv * (1.0 - k) + k)) * (ndl / (ndl * (1.0 - k) + k));
                let f = self.f0 + (Vec3::new(1.0, 1.0, 1.0) - self.f0) * (1.0 - vdh).powi(5);
                let spec = f * (d * g / (4.0 * self.ndv * ndl + 1e-4));
                let spec = if self.toon >= 2 { spec * 0.6 } else { spec };
                let kd = (Vec3::new(1.0, 1.0, 1.0) - f) * (1.0 - self.metal);
                let diffuse = kd.mul_elem(self.albedo) * (1.0 / std::f32::consts::PI);
                (diffuse + spec).mul_elem(radiance) * ndl
            }
        }

        /// Everything known about one visible pixel.
        struct Fragment<'a> {
            face: &'a Face,
            view: &'a Affine,
            u: f32,
            v: f32,
            /// View-space position.
            pos: Vec3,
            /// Key light visibility (1 = lit).
            shadow: f32,
            depth01: f32,
        }

        /// Returns (sRGB color, linear glow for bloom).
        fn shade(
            fr: &Fragment,
            materials: &[Material],
            textures: &[Arc<Texture>],
            settings: &RenderSettings,
            points_view: &[PointLight],
        ) -> ([u8; 4], Vec3) {
            let face = fr.face;
            let v = face.voxel;
            let c = Rgba::from_u32(v.rgba);
            let mat_default = Material::new("default");
            let mat = materials.get(v.material as usize).unwrap_or(materials.first().unwrap_or(&mat_default));
            let axis = (face.dir / 2) as usize;
            let surface_point = face_point(face, fr.u, fr.v);
            let surf = sample_surface(mat, textures, surface_point.to_array(), axis);
            let o = DIRS[face.dir as usize];
            let face_n = Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32);
            let geo = (face_n * (1.0 - settings.smoothness) + face.smooth * settings.smoothness).normalized();
            let (t, b) = match axis {
                0 => (Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 1.0, 0.0)),
                1 => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0)),
                _ => (Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0)),
            };
            let nt = surf.normal_ts;
            let n_model = (t * nt.x + b * nt.y + geo * nt.z).normalized();
            let n = fr.view.apply_vector(n_model).normalized();
            let albedo = Vec3::new(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b)).mul_elem(surf.albedo_mul);
            let ca = face.corner_ao;
            let (u, w) = (fr.u, fr.v);
            let corner = ca[0] * (1.0 - u) * (1.0 - w) + ca[1] * u * (1.0 - w) + ca[2] * u * w + ca[3] * (1.0 - u) * w;
            let ao = face.ao.powf(0.6) * corner * surf.occlusion;
            let emission = if surf.has_emission_map { surf.emission } else { albedo * mat.emission };
            let enc = |x: f32| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let gray = |x: f32| {
                let g = enc(x);
                [g, g, g, 255]
            };
            let rgba = match settings.pass {
                Pass::Albedo => [linear_to_srgb(albedo.x), linear_to_srgb(albedo.y), linear_to_srgb(albedo.z), 255],
                Pass::Normal => [enc(n.x * 0.5 + 0.5), enc(n.y * 0.5 + 0.5), enc(n.z * 0.5 + 0.5), 255],
                Pass::Depth => gray(fr.depth01),
                Pass::Occlusion => gray(ao),
                Pass::Roughness => gray(surf.roughness),
                Pass::Metallic => gray(surf.metallic),
                Pass::Emission => [linear_to_srgb(emission.x), linear_to_srgb(emission.y), linear_to_srgb(emission.z), 255],
                Pass::Lit => {
                    let view_dir = Vec3::new(0.0, 0.0, 1.0);
                    let ndv = n.dot(view_dir).max(1e-3);
                    let brdf = Brdf {
                        n,
                        albedo,
                        rough: surf.roughness,
                        metal: surf.metallic,
                        f0: Vec3::new(0.04, 0.04, 0.04).lerp(albedo, surf.metallic),
                        ndv,
                        toon: settings.toon_steps,
                    };
                    let mut col = Vec3::ZERO;
                    for (i, light) in settings.lighting.lights.iter().enumerate() {
                        let vis = if i == 0 { 1.0 - settings.shadows * (1.0 - fr.shadow) } else { 1.0 };
                        col = col + brdf.light(light.dir, light.color * (light.intensity * vis * (0.55 + 0.45 * ao)));
                    }
                    for p in points_view {
                        let d = p.pos - fr.pos;
                        let dist = d.length();
                        if dist < p.radius && dist > 1e-3 {
                            let fall = (1.0 - dist / p.radius).powi(2);
                            col = col + brdf.light(d * (1.0 / dist), p.color * (p.intensity * fall)) * (0.35 + 0.65 * ao);
                        }
                    }
                    let hemi = settings.lighting.sky * (0.5 + 0.5 * n.y) + settings.lighting.ground * (0.5 - 0.5 * n.y);
                    let ambient_diffuse = albedo.mul_elem(hemi) * (1.0 - surf.metallic);
                    let r = (n * (2.0 * ndv) - view_dir).normalized();
                    let env = environment(r, surf.roughness, &settings.lighting);
                    let fr_env = brdf.f0 + (Vec3::new(1.0, 1.0, 1.0) - brdf.f0) * ((1.0 - ndv).powi(5) * (1.0 - surf.roughness).powi(2));
                    let ambient_spec = fr_env.mul_elem(env) * (1.0 - 0.6 * surf.roughness);
                    col = col + (ambient_diffuse + ambient_spec) * ao + emission;
                    let e = settings.lighting.exposure;
                    [linear_to_srgb(aces(col.x * e)), linear_to_srgb(aces(col.y * e)), linear_to_srgb(aces(col.z * e)), c.a]
                }
            };
            (rgba, emission)
        }

        /// Renders a model.
        pub fn render(
            model: &VoxelModel,
            prepared: &Prepared,
            materials: &[Material],
            textures: &[Arc<Texture>],
            settings: &RenderSettings,
        ) -> Image {
            let ss = settings.supersample.clamp(1, 4);
            let (target, views) = rasterize(model, prepared, settings, ss, true);
            let cam_view = camera_view(model, settings);
            let lit = settings.pass == Pass::Lit;
            let key = settings.lighting.lights.first().map(|l| l.dir);
            let shadow_map = match key {
                Some(dir) if lit && (settings.shadows > 0.0 || settings.ground_shadow > 0.0) => ShadowMap::build(prepared, &views, dir),
                _ => None,
            };
            let points_view: Vec<PointLight> =
                settings.lighting.points.iter().map(|p| PointLight { pos: cam_view.apply(p.pos), ..*p }).collect();
            let (zmin, zmax) = target.depth.iter().filter(|z| z.is_finite()).fold((f32::MAX, f32::MIN), |(a, b), &z| (a.min(z), b.max(z)));
            let zrange = (zmax - zmin).max(1e-3);
            let mut hi = vec![[0u8; 4]; target.w * target.h];
            let mut glow = vec![Vec3::ZERO; if settings.bloom > 0.0 { target.w * target.h } else { 0 }];
            for (i, &fid) in target.face.iter().enumerate() {
                if fid == NO_FACE {
                    continue;
                }
                let f = &prepared.faces[fid as usize];
                let view = &views[(f.voxel.bone as usize).min(views.len() - 1)];
                let [u, v] = target.uv[i];
                let pos = view.apply(face_point(f, u, v));
                let shadow = match &shadow_map {
                    Some(sm) if settings.shadows > 0.0 => {
                        let o = DIRS[f.dir as usize];
                        let n = view.apply_vector(Vec3::new(o[0] as f32, o[1] as f32, o[2] as f32));
                        sm.visibility(pos + n * 0.6)
                    }
                    _ => 1.0,
                };
                let fr = Fragment { face: f, view, u, v, pos, shadow, depth01: (target.depth[i] - zmin) / zrange };
                let (rgba, emission) = shade(&fr, materials, textures, settings, &points_view);
                hi[i] = rgba;
                if !glow.is_empty() {
                    glow[i] = emission;
                }
            }
            let (w, h) = (settings.camera.width as usize, settings.camera.height as usize);
            let mut img = Image::new(w as u32, h as u32);
            let mut depth = vec![f32::NEG_INFINITY; w * h];
            let mut glow_out = vec![Vec3::ZERO; if glow.is_empty() { 0 } else { w * h }];
            let ssu = ss as usize;
            for y in 0..h {
                for x in 0..w {
                    let (mut acc, mut n, mut zmax_px, mut g) = ([0u32; 4], 0u32, f32::NEG_INFINITY, Vec3::ZERO);
                    for sy in 0..ssu {
                        for sx in 0..ssu {
                            let i = (y * ssu + sy) * target.w + x * ssu + sx;
                            if target.face[i] != NO_FACE {
                                for k in 0..4 {
                                    acc[k] += hi[i][k] as u32;
                                }
                                n += 1;
                                zmax_px = zmax_px.max(target.depth[i]);
                                if !glow.is_empty() {
                                    g = g + glow[i];
                                }
                            }
                        }
                    }
                    if !glow_out.is_empty() {
                        glow_out[y * w + x] = g * (1.0 / (ssu * ssu) as f32);
                    }
                    // Sharp silhouette: a pixel is solid when at least half of its samples are covered.
                    if n * 2 >= (ssu * ssu) as u32 && n > 0 {
                        let o = (y * w + x) * 4;
                        for k in 0..4 {
                            img.pixels[o + k] = (acc[k] / n) as u8;
                        }
                        depth[y * w + x] = zmax_px;
                    }
                }
            }
            if lit {
                if settings.inner_lines {
                    inner_lines(&mut img, &depth, settings.camera.scale);
                }
                match settings.outline {
                    Outline::None => {}
                    Outline::Selective => outline(&mut img, None),
                    Outline::Color(c) => outline(&mut img, Some(c)),
                }
                if settings.ground_shadow > 0.0
                    && let Some(sm) = &shadow_map
                {
                    ground_shadow(&mut img, model, settings, sm);
                }
                if settings.bloom > 0.0 {
                    bloom(&mut img, glow_out, settings);
                }
            }
            composite_background(&mut img, settings);
            img
        }

        /// Soft shadow on the ground plane (y = origin) for pixels not covered by the model.
        fn ground_shadow(img: &mut Image, model: &VoxelModel, settings: &RenderSettings, sm: &ShadowMap) {
            let cam = &settings.camera;
            let r = cam.view_rotation().m;
            let det = r[0][0] * r[1][2] - r[0][2] * r[1][0];
            if det.abs() < 1e-3 {
                return;
            }
            let _ = model;
            let up = Vec3::new(r[0][1], r[1][1], r[2][1]) * 0.5;
            let (w, h) = (img.width as usize, img.height as usize);
            for y in 0..h {
                for x in 0..w {
                    let o = (y * w + x) * 4;
                    if img.pixels[o + 3] != 0 {
                        continue;
                    }
                    let vx = (x as f32 + 0.5 - cam.anchor_x) / cam.scale;
                    let vy = (cam.anchor_y - (y as f32 + 0.5)) / cam.scale;
                    let gx = (vx * r[1][2] - vy * r[0][2]) / det;
                    let gz = (vy * r[0][0] - vx * r[1][0]) / det;
                    let vz = r[2][0] * gx + r[2][2] * gz;
                    let vis = sm.visibility(Vec3::new(vx, vy, vz) + up);
                    let a = settings.ground_shadow * (1.0 - vis);
                    if a > 0.01 {
                        img.pixels[o..o + 4].copy_from_slice(&[12, 10, 22, (a * 255.0) as u8]);
                    }
                }
            }
        }

        /// Adds a blurred glow of emissive light; glow spills onto transparent pixels as a halo.
        fn bloom(img: &mut Image, glow: Vec<Vec3>, settings: &RenderSettings) {
            let (w, h) = (img.width as usize, img.height as usize);
            let radius = if settings.bloom_radius > 0.0 { settings.bloom_radius } else { (w.max(h) as f32 / 48.0).max(2.0) };
            let r = radius.round().max(1.0) as usize;
            let mut buf = glow;
            let mut tmp = vec![Vec3::ZERO; w * h];
            // Three box blurs ≈ Gaussian.
            for _ in 0..3 {
                for y in 0..h {
                    let mut acc = Vec3::ZERO;
                    let row = y * w;
                    for x in 0..(r.min(w)) {
                        acc = acc + buf[row + x];
                    }
                    for x in 0..w {
                        if x + r < w {
                            acc = acc + buf[row + x + r];
                        }
                        if x > r {
                            acc = acc - buf[row + x - r - 1];
                        }
                        tmp[row + x] = acc * (1.0 / (2 * r + 1) as f32);
                    }
                }
                for x in 0..w {
                    let mut acc = Vec3::ZERO;
                    for y in 0..(r.min(h)) {
                        acc = acc + tmp[y * w + x];
                    }
                    for y in 0..h {
                        if y + r < h {
                            acc = acc + tmp[(y + r) * w + x];
                        }
                        if y > r {
                            acc = acc - tmp[(y - r - 1) * w + x];
                        }
                        buf[y * w + x] = acc * (1.0 / (2 * r + 1) as f32);
                    }
                }
            }
            let strength = settings.bloom * 1.2;
            for (i, g) in buf.iter().enumerate() {
                let g = *g * strength;
                let o = i * 4;
                let p = &mut img.pixels[o..o + 4];
                if p[3] > 0 {
                    let lin = |c: u8, add: f32| linear_to_srgb(srgb_to_linear(c) + add);
                    p[0] = lin(p[0], g.x);
                    p[1] = lin(p[1], g.y);
                    p[2] = lin(p[2], g.z);
                } else {
                    let a = g.x.max(g.y).max(g.z).min(1.0);
                    if a > 1.0 / 255.0 {
                        p.copy_from_slice(&[linear_to_srgb(g.x / a), linear_to_srgb(g.y / a), linear_to_srgb(g.z / a), (a * 255.0) as u8]);
                    }
                }
            }
        }

        fn composite_background(img: &mut Image, settings: &RenderSettings) {
            if settings.background.is_none() && settings.background_gradient.is_none() {
                return;
            }
            let (w, h) = (img.width as f32, img.height as f32);
            for y in 0..img.height {
                for x in 0..img.width {
                    let bg = match (settings.background_gradient, settings.background) {
                        (Some((top, bottom)), _) => {
                            let t = y as f32 / h.max(1.0);
                            let (dx, dy) = (x as f32 / w - 0.5, y as f32 / h - 0.5);
                            let vignette = 1.0 - (dx * dx + dy * dy).sqrt() * 0.55;
                            let mix = |a: u8, b: u8| ((a as f32 + (b as f32 - a as f32) * t) * vignette).clamp(0.0, 255.0) as u8;
                            Rgba::new(mix(top.r, bottom.r), mix(top.g, bottom.g), mix(top.b, bottom.b), 255)
                        }
                        (None, Some(c)) => c,
                        _ => unreachable!(),
                    };
                    let o = ((y * img.width + x) * 4) as usize;
                    let px = &mut img.pixels[o..o + 4];
                    let mut dst = [bg.r, bg.g, bg.b, bg.a];
                    crate::renderer::blend_over(&mut dst, Rgba::new(px[0], px[1], px[2], px[3]), 255);
                    px.copy_from_slice(&dst);
                }
            }
        }

        /// Darkens pixels that sit in front of a large depth step (separates overlapping limbs).
        fn inner_lines(img: &mut Image, depth: &[f32], scale: f32) {
            let (w, h) = (img.width as usize, img.height as usize);
            let threshold = 4.0 / scale.max(0.25) * scale.clamp(1.0, 4.0);
            let src = img.pixels.clone();
            for y in 0..h {
                for x in 0..w {
                    let i = y * w + x;
                    if !depth[i].is_finite() {
                        continue;
                    }
                    let mut step = false;
                    for (dx, dy) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                        if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                            continue;
                        }
                        let j = ny as usize * w + nx as usize;
                        if depth[j].is_finite() && depth[i] - depth[j] < -threshold {
                            step = true;
                        }
                    }
                    if step {
                        let o = i * 4;
                        for k in 0..3 {
                            img.pixels[o + k] = (src[o + k] as f32 * 0.62) as u8;
                        }
                    }
                }
            }
        }

        /// 1-pixel exterior outline. `None` = selective outline derived from the bordering color.
        fn outline(img: &mut Image, color: Option<Rgba>) {
            let (w, h) = (img.width as i32, img.height as i32);
            let src = img.pixels.clone();
            for y in 0..h {
                for x in 0..w {
                    let o = ((y * w + x) * 4) as usize;
                    if src[o + 3] != 0 {
                        continue;
                    }
                    let mut best: Option<[u8; 4]> = None;
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, ny) = (x + dx, y + dy);
                        if nx < 0 || ny < 0 || nx >= w || ny >= h {
                            continue;
                        }
                        let j = ((ny * w + nx) * 4) as usize;
                        if src[j + 3] > 0 {
                            let cand = [src[j], src[j + 1], src[j + 2], 255];
                            let lum = |p: [u8; 4]| p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32;
                            if best.is_none_or(|b| lum(cand) < lum(b)) {
                                best = Some(cand);
                            }
                        }
                    }
                    if let Some(b) = best {
                        let c = match color {
                            Some(c) => [c.r, c.g, c.b, c.a],
                            None => {
                                [(b[0] as f32 * 0.28) as u8, (b[1] as f32 * 0.26) as u8, (b[2] as f32 * 0.34 + 10.0).min(255.0) as u8, 255]
                            }
                        };
                        img.pixels[o..o + 4].copy_from_slice(&c);
                    }
                }
            }
        }

        /// Voxel + face under a screen pixel (for click-to-edit in 3D viewports).
        pub fn pick(model: &VoxelModel, prepared: &Prepared, settings: &RenderSettings, px: u32, py: u32) -> Option<([i32; 3], [i32; 3])> {
            if px >= settings.camera.width || py >= settings.camera.height {
                return None;
            }
            let (target, _) = rasterize(model, prepared, settings, 1, false);
            let fid = target.face[py as usize * target.w + px as usize];
            (fid != NO_FACE).then(|| {
                let f = &prepared.faces[fid as usize];
                (f.pos, DIRS[f.dir as usize])
            })
        }
    }
    pub mod rig {
        //! Skeletons, rigid voxel skinning and keyframe animation.

        use std::f32::consts::TAU;

        use super::math::{Affine, Vec3};
        use super::model::{Voxel, VoxelModel};

        /// A joint. Voxels bound to a bone follow it rigidly; `head` is the pivot, `tail` the end
        /// (used for automatic binding and for drawing the skeleton).
        #[derive(Debug, Clone, PartialEq)]
        pub struct Bone {
            pub name: String,
            pub parent: Option<usize>,
            pub head: Vec3,
            pub tail: Vec3,
        }

        /// Local bone pose at a frame: Euler rotation (degrees, around the head) and translation.
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct Key {
            pub bone: usize,
            pub frame: f32,
            pub rot: Vec3,
            pub pos: Vec3,
        }

        #[derive(Debug, Clone, PartialEq)]
        pub struct Animation {
            pub name: String,
            pub frames: u32,
            pub frame_ms: u32,
            pub looped: bool,
            pub keys: Vec<Key>,
        }

        impl Animation {
            pub fn new(name: &str, frames: u32, frame_ms: u32) -> Animation {
                Animation { name: name.into(), frames: frames.max(1), frame_ms, looped: true, keys: Vec::new() }
            }

            /// Inserts or replaces the key of `bone` at `frame`.
            pub fn set_key(&mut self, key: Key) {
                self.keys.retain(|k| !(k.bone == key.bone && (k.frame - key.frame).abs() < 1e-4));
                self.keys.push(key);
                self.keys.sort_by(|a, b| a.bone.cmp(&b.bone).then(a.frame.total_cmp(&b.frame)));
            }

            /// Interpolated (rotation, translation) of a bone at time `t` (in frames).
            pub fn sample(&self, bone: usize, t: f32) -> (Vec3, Vec3) {
                let keys: Vec<&Key> = self.keys.iter().filter(|k| k.bone == bone).collect();
                match keys.len() {
                    0 => (Vec3::ZERO, Vec3::ZERO),
                    1 => (keys[0].rot, keys[0].pos),
                    _ => {
                        let len = self.frames as f32;
                        let t = if self.looped { t.rem_euclid(len) } else { t.clamp(0.0, len) };
                        let next = keys.iter().position(|k| k.frame > t);
                        let (k0, k1, span) = match next {
                            Some(0) => {
                                let last = keys[keys.len() - 1];
                                if self.looped {
                                    (last, keys[0], keys[0].frame + len - last.frame)
                                } else {
                                    return (keys[0].rot, keys[0].pos);
                                }
                            }
                            Some(i) => (keys[i - 1], keys[i], keys[i].frame - keys[i - 1].frame),
                            None => {
                                let last = keys[keys.len() - 1];
                                if self.looped { (last, keys[0], keys[0].frame + len - last.frame) } else { return (last.rot, last.pos) }
                            }
                        };
                        let local = (t - k0.frame).rem_euclid(len.max(1.0));
                        let u = if span <= 1e-6 { 0.0 } else { (local / span).clamp(0.0, 1.0) };
                        let s = u * u * (3.0 - 2.0 * u);
                        (k0.rot.lerp(k1.rot, s), k0.pos.lerp(k1.pos, s))
                    }
                }
            }
        }

        /// World transforms of every bone for an animation time (rest pose when `anim` is None).
        pub fn pose(bones: &[Bone], anim: Option<&Animation>, t: f32) -> Vec<Affine> {
            let mut world: Vec<Affine> = Vec::with_capacity(bones.len());
            for (i, b) in bones.iter().enumerate() {
                let (rot, pos) = anim.map(|a| a.sample(i, t)).unwrap_or((Vec3::ZERO, Vec3::ZERO));
                let local = Affine::translation(pos).then(&Affine::rotation_about(b.head, rot));
                let w = match b.parent {
                    Some(p) if p < i => world[p].then(&local),
                    _ => local,
                };
                world.push(w);
            }
            world
        }

        /// Distance from a point to a bone segment.
        fn segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
            let ab = b - a;
            let len2 = ab.dot(ab);
            let t = if len2 < 1e-6 { 0.0 } else { ((p - a).dot(ab) / len2).clamp(0.0, 1.0) };
            (p - (a + ab * t)).length()
        }

        /// Binds voxels to their nearest bone segment. `only_unbound` keeps explicit bindings.
        pub fn auto_bind(model: &mut VoxelModel, only_unbound: bool) -> usize {
            if model.bones.is_empty() {
                return 0;
            }
            let bones = model.bones.clone();
            let mut changes: Vec<(i32, i32, i32, Voxel)> = Vec::new();
            model.for_each(|x, y, z, v| {
                if only_unbound && v.bone != 0 {
                    return;
                }
                let p = Vec3::new(x as f32 + 0.5, y as f32 + 0.5, z as f32 + 0.5);
                let mut best = (f32::MAX, 0usize);
                for (i, b) in bones.iter().enumerate() {
                    let d = segment_distance(p, b.head, b.tail);
                    if d < best.0 {
                        best = (d, i);
                    }
                }
                if v.bone as usize != best.1 {
                    changes.push((x, y, z, Voxel { bone: best.1 as u8, ..v }));
                }
            });
            let n = changes.len();
            for (x, y, z, v) in changes {
                model.set(x, y, z, v);
            }
            n
        }

        /// Procedural humanoid motion cycles (walk, run, idle) driven by standard bone names:
        /// hips, spine, chest, neck, head, upper_arm_l/r, lower_arm_l/r, hand_l/r,
        /// upper_leg_l/r, lower_leg_l/r, foot_l/r, and optional cape, weapon. Missing bones are skipped.
        pub struct CycleParams {
            pub frames: u32,
            pub frame_ms: u32,
            pub stride_deg: f32,
            pub knee_deg: f32,
            pub arm_swing_deg: f32,
            pub bob: f32,
            pub lean_deg: f32,
        }

        impl CycleParams {
            pub fn walk(frames: u32) -> CycleParams {
                CycleParams { frames, frame_ms: 100, stride_deg: 28.0, knee_deg: 38.0, arm_swing_deg: 24.0, bob: 2.0, lean_deg: 3.0 }
            }
            pub fn run(frames: u32) -> CycleParams {
                CycleParams { frames, frame_ms: 70, stride_deg: 42.0, knee_deg: 75.0, arm_swing_deg: 40.0, bob: 4.0, lean_deg: 10.0 }
            }
        }

        pub fn locomotion_cycle(model: &VoxelModel, name: &str, p: &CycleParams) -> Animation {
            let mut a = Animation::new(name, p.frames, p.frame_ms);
            let bone = |n: &str| model.bone_index(n);
            let n = p.frames.max(2);
            let length = |n: &str| bone(n).map(|i| (model.bones[i].tail - model.bones[i].head).length()).unwrap_or(0.0);
            let (thigh, shin) = (length("upper_leg_l").max(length("upper_leg_r")), length("lower_leg_l").max(length("lower_leg_r")));
            // Vertical reach of a leg for hip angle `h` and knee bend `k` (degrees).
            let reach = |h: f32, k: f32| thigh * h.to_radians().cos() + shin * (h + k).to_radians().cos();
            let rest = reach(0.0, 0.0);
            for f in 0..n {
                let phi = TAU * f as f32 / n as f32;
                let (s, c) = phi.sin_cos();
                let mut key = |b: Option<usize>, rot: Vec3, pos: Vec3| {
                    if let Some(bone) = b {
                        a.set_key(Key { bone, frame: f as f32, rot, pos });
                    }
                };
                // Character faces +Z: negative X rotation swings a hanging limb forward.
                let leg_l = -p.stride_deg * s;
                let leg_r = p.stride_deg * s;
                let knee_l = 6.0 + p.knee_deg * c.max(0.0).powf(1.5);
                let knee_r = 6.0 + p.knee_deg * (-c).max(0.0).powf(1.5);
                // Foot planting: lower the hips so the longer (stance) leg still reaches the ground.
                let plant = rest - reach(leg_l, knee_l).max(reach(leg_r, knee_r));
                key(bone("hips"), Vec3::new(0.0, 6.0 * s, 2.0 * c), Vec3::new(0.0, -plant + p.bob * 0.5 * ((2.0 * phi).cos() - 1.0), 0.0));
                key(bone("spine"), Vec3::new(p.lean_deg, -3.0 * s, 0.0), Vec3::ZERO);
                key(bone("chest"), Vec3::new(1.5 * (2.0 * phi).cos(), -6.0 * s, -1.5 * c), Vec3::ZERO);
                key(bone("neck"), Vec3::new(-p.lean_deg * 0.5, 2.0 * s, 0.0), Vec3::ZERO);
                key(bone("head"), Vec3::new(-1.5 * (2.0 * phi).cos(), 3.0 * s, 0.0), Vec3::ZERO);
                key(bone("upper_leg_l"), Vec3::new(leg_l, 0.0, 0.0), Vec3::ZERO);
                key(bone("upper_leg_r"), Vec3::new(leg_r, 0.0, 0.0), Vec3::ZERO);
                key(bone("lower_leg_l"), Vec3::new(knee_l, 0.0, 0.0), Vec3::ZERO);
                key(bone("lower_leg_r"), Vec3::new(knee_r, 0.0, 0.0), Vec3::ZERO);
                key(bone("foot_l"), Vec3::new(-(leg_l + knee_l) * 0.75, 0.0, 0.0), Vec3::ZERO);
                key(bone("foot_r"), Vec3::new(-(leg_r + knee_r) * 0.75, 0.0, 0.0), Vec3::ZERO);
                // Character's left is +X; positive Z rotation opens the left arm away from the body.
                key(bone("upper_arm_l"), Vec3::new(p.arm_swing_deg * s, 0.0, 5.0), Vec3::ZERO);
                key(bone("upper_arm_r"), Vec3::new(-p.arm_swing_deg * s, 0.0, -5.0), Vec3::ZERO);
                key(bone("lower_arm_l"), Vec3::new(-14.0 - 12.0 * s.max(0.0), 0.0, 0.0), Vec3::ZERO);
                key(bone("lower_arm_r"), Vec3::new(-14.0 - 12.0 * (-s).max(0.0), 0.0, 0.0), Vec3::ZERO);
                key(bone("cape"), Vec3::new(8.0 + 6.0 * (1.0 - (2.0 * phi).cos()) * 0.5 + p.lean_deg, 0.0, 3.0 * s), Vec3::ZERO);
                key(bone("weapon"), Vec3::new(-3.0 * s, 0.0, 0.0), Vec3::ZERO);
            }
            a
        }

        pub fn idle_cycle(model: &VoxelModel, name: &str, frames: u32) -> Animation {
            let mut a = Animation::new(name, frames, 160);
            let n = frames.max(2);
            for f in 0..n {
                let phi = TAU * f as f32 / n as f32;
                let s = phi.sin();
                for (bone, rot, pos) in [
                    ("hips", Vec3::ZERO, Vec3::new(0.0, -0.6 * (1.0 - phi.cos()) * 0.5, 0.0)),
                    ("chest", Vec3::new(-1.5 * s, 0.0, 0.0), Vec3::ZERO),
                    ("head", Vec3::new(1.0 * s, 1.5 * s, 0.0), Vec3::ZERO),
                    ("upper_arm_l", Vec3::new(2.0 * s, 0.0, 6.0 + 1.5 * s), Vec3::ZERO),
                    ("upper_arm_r", Vec3::new(2.0 * s, 0.0, -6.0 - 1.5 * s), Vec3::ZERO),
                    ("lower_arm_l", Vec3::new(-10.0, 0.0, 0.0), Vec3::ZERO),
                    ("lower_arm_r", Vec3::new(-10.0, 0.0, 0.0), Vec3::ZERO),
                    ("cape", Vec3::new(4.0 + 2.0 * s, 0.0, 1.5 * phi.cos()), Vec3::ZERO),
                ] {
                    if let Some(b) = model.bone_index(bone) {
                        a.set_key(Key { bone: b, frame: f as f32, rot, pos });
                    }
                }
            }
            a
        }
    }

    pub use material::{Material, Texture};
    pub use math::{Affine, Vec3};
    pub use model::{Shape, Voxel, VoxelModel};
}

pub use engine::{Engine, Response};

#[cfg(test)]
#[path = "Tests.rs"]
mod tests;
