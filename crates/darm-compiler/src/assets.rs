//! Asset inspection: identify a file by its content (never by its
//! extension), measure it, hash it and reject unsafe or unsupported input.
//!
//! Files are read as a stream so that a multi-gigabyte video is hashed without
//! being loaded into memory.

use std::io::{Read, Seek, SeekFrom};

use darm_ir::ResourceKind;
use sha2::{Digest, Sha256};

/// Largest single asset the compiler accepts.
pub const MAX_ASSET_BYTES: u64 = 8 * 1024 * 1024 * 1024;
/// Largest SVG / WebVTT file (they are parsed in memory).
pub const MAX_TEXT_ASSET_BYTES: u64 = 8 * 1024 * 1024;
/// Assets above this size produce warning W0606.
pub const LARGE_ASSET_BYTES: u64 = 64 * 1024 * 1024;

pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

#[derive(Debug, Clone, PartialEq)]
pub struct Inspected {
    pub kind: ResourceKind,
    pub media_type: &'static str,
    /// Canonical extension including the dot.
    pub ext: &'static str,
    /// Human name of the format, e.g. `PNG image`.
    pub label: &'static str,
    pub size: u64,
    pub sha256: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AssetError {
    pub code: &'static str,
    pub message: String,
    pub help: Option<String>,
}

fn err(code: &'static str, message: impl Into<String>) -> AssetError {
    AssetError {
        code,
        message: message.into(),
        help: None,
    }
}

pub fn accepted_formats(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Image => "PNG, JPEG, GIF, WebP or SVG",
        ResourceKind::Video => "MP4 or WebM",
        ResourceKind::Audio => "MP3, M4A/AAC, Ogg, WAV or FLAC",
        ResourceKind::Captions => "WebVTT",
        ResourceKind::Font => "WOFF2, TTF or OTF",
    }
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn be64(b: &[u8]) -> u64 {
    u64::from_be_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]])
}

fn le16(b: &[u8]) -> u32 {
    u16::from_le_bytes([b[0], b[1]]) as u32
}

struct Format {
    kind: ResourceKind,
    media_type: &'static str,
    ext: &'static str,
    label: &'static str,
}

const fn fmt(
    kind: ResourceKind,
    media_type: &'static str,
    ext: &'static str,
    label: &'static str,
) -> Format {
    Format {
        kind,
        media_type,
        ext,
        label,
    }
}

fn looks_like_svg(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg")
        || ((t.starts_with("<?xml") || t.starts_with("<!--") || t.starts_with("<!DOCTYPE"))
            && t.contains("<svg"))
}

/// Identify the container/format from the first bytes. `want` disambiguates
/// containers that can hold either audio or video.
fn sniff(head: &[u8], want: ResourceKind) -> Option<Format> {
    use ResourceKind::*;
    let h = head;
    let starts = |sig: &[u8]| h.starts_with(sig);
    let at = |off: usize, sig: &[u8]| h.len() >= off + sig.len() && &h[off..off + sig.len()] == sig;
    Some(if starts(b"\x89PNG\r\n\x1a\n") {
        fmt(Image, "image/png", ".png", "PNG image")
    } else if starts(b"\xff\xd8\xff") {
        fmt(Image, "image/jpeg", ".jpg", "JPEG image")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        fmt(Image, "image/gif", ".gif", "GIF image")
    } else if starts(b"RIFF") && at(8, b"WEBP") {
        fmt(Image, "image/webp", ".webp", "WebP image")
    } else if starts(b"RIFF") && at(8, b"WAVE") {
        fmt(Audio, "audio/wav", ".wav", "WAV audio")
    } else if at(4, b"ftyp") {
        let audio_brand = at(8, b"M4A ") || at(8, b"M4B ");
        if want == Audio || (audio_brand && want != Video) {
            fmt(Audio, "audio/mp4", ".m4a", "MPEG-4 audio")
        } else {
            fmt(Video, "video/mp4", ".mp4", "MP4 video")
        }
    } else if starts(b"\x1a\x45\xdf\xa3") {
        if want == Audio {
            fmt(Audio, "audio/webm", ".weba", "WebM audio")
        } else {
            fmt(Video, "video/webm", ".webm", "WebM video")
        }
    } else if starts(b"OggS") {
        fmt(Audio, "audio/ogg", ".ogg", "Ogg audio")
    } else if starts(b"fLaC") {
        fmt(Audio, "audio/flac", ".flac", "FLAC audio")
    } else if starts(b"ID3") {
        fmt(Audio, "audio/mpeg", ".mp3", "MP3 audio")
    } else if h.len() >= 2 && h[0] == 0xff && h[1] & 0xf6 == 0xf0 {
        fmt(Audio, "audio/aac", ".aac", "AAC audio")
    } else if h.len() >= 2 && h[0] == 0xff && h[1] & 0xe0 == 0xe0 && h[1] & 0x06 != 0 {
        fmt(Audio, "audio/mpeg", ".mp3", "MP3 audio")
    } else if starts(b"wOF2") {
        fmt(Font, "font/woff2", ".woff2", "WOFF2 font")
    } else if starts(b"\x00\x01\x00\x00") || starts(b"true") {
        fmt(Font, "font/ttf", ".ttf", "TrueType font")
    } else if starts(b"OTTO") {
        fmt(Font, "font/otf", ".otf", "OpenType font")
    } else if starts(b"WEBVTT") || starts(b"\xef\xbb\xbfWEBVTT") {
        fmt(Captions, "text/vtt", ".vtt", "WebVTT captions")
    } else if looks_like_svg(h) {
        fmt(Image, "image/svg+xml", ".svg", "SVG image")
    } else {
        return None;
    })
}

fn jpeg_size(data: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < data.len() {
        if data[i] != 0xff {
            i += 1;
            continue;
        }
        let marker = data[i + 1];
        if marker == 0xff {
            i += 1;
            continue;
        }
        if (0xd0..=0xd9).contains(&marker) || marker == 0x01 {
            i += 2;
            continue;
        }
        let len = u16::from_be_bytes([data[i + 2], data[i + 3]]) as usize;
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let h = u16::from_be_bytes([data[i + 5], data[i + 6]]) as u32;
            let w = u16::from_be_bytes([data[i + 7], data[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 2 + len.max(2);
    }
    None
}

fn webp_size(h: &[u8]) -> Option<(u32, u32)> {
    if h.len() < 30 {
        return None;
    }
    match &h[12..16] {
        b"VP8 " => Some((le16(&h[26..28]) & 0x3fff, le16(&h[28..30]) & 0x3fff)),
        b"VP8L" => {
            let bits = u32::from_le_bytes([h[21], h[22], h[23], h[24]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        b"VP8X" => {
            let w = u32::from_le_bytes([h[24], h[25], h[26], 0]) + 1;
            let hh = u32::from_le_bytes([h[27], h[28], h[29], 0]) + 1;
            Some((w, hh))
        }
        _ => None,
    }
}

/// Facts read from an MP4 `moov` box.
#[derive(Default)]
struct Mp4Info {
    duration_ms: Option<u64>,
    video: Option<(u32, u32)>,
}

fn mp4_walk(data: &[u8], depth: usize, info: &mut Mp4Info) {
    let mut i = 0usize;
    while i + 8 <= data.len() && depth < 6 {
        let mut size = be32(&data[i..]) as usize;
        let ty = &data[i + 4..i + 8];
        let mut header = 8;
        if size == 1 && i + 16 <= data.len() {
            size = usize::try_from(be64(&data[i + 8..])).unwrap_or(usize::MAX);
            header = 16;
        } else if size == 0 {
            size = data.len() - i;
        }
        if size < header || i.checked_add(size).is_none_or(|e| e > data.len()) {
            return;
        }
        let body = &data[i + header..i + size];
        match ty {
            b"trak" | b"mdia" => mp4_walk(body, depth + 1, info),
            b"mvhd" if body.len() >= 20 => {
                let (timescale, duration) = if body[0] == 1 && body.len() >= 32 {
                    (be32(&body[20..]) as u64, be64(&body[24..]))
                } else {
                    (be32(&body[12..]) as u64, be32(&body[16..]) as u64)
                };
                if timescale > 0 && duration != u64::MAX && duration != u32::MAX as u64 {
                    info.duration_ms = duration.checked_mul(1000).map(|d| d / timescale);
                }
            }
            b"tkhd" if body.len() >= 8 => {
                // Width and height are the last two 16.16 fixed-point fields.
                let w = be32(&body[body.len() - 8..]) >> 16;
                let h = be32(&body[body.len() - 4..]) >> 16;
                if w > 0 && h > 0 {
                    info.video = Some((w, h));
                }
            }
            _ => {}
        }
        i += size;
    }
}

fn mp4_info(r: &mut dyn ReadSeek, len: u64) -> Mp4Info {
    let mut info = Mp4Info::default();
    let mut pos = 0u64;
    let mut header = [0u8; 16];
    // Walk top-level boxes without reading media data.
    for _ in 0..4096 {
        if pos + 8 > len
            || r.seek(SeekFrom::Start(pos)).is_err()
            || r.read_exact(&mut header[..8]).is_err()
        {
            break;
        }
        let mut size = be32(&header) as u64;
        let mut head = 8u64;
        if size == 1 {
            if r.read_exact(&mut header[8..16]).is_err() {
                break;
            }
            size = be64(&header[8..]);
            head = 16;
        } else if size == 0 {
            size = len - pos;
        }
        if size < head || pos.checked_add(size).is_none_or(|e| e > len) {
            break;
        }
        if &header[4..8] == b"moov" {
            let body = size - head;
            if body <= 64 * 1024 * 1024 {
                let mut buf = vec![0u8; body as usize];
                if r.read_exact(&mut buf).is_ok() {
                    mp4_walk(&buf, 0, &mut info);
                }
            }
            break;
        }
        pos += size;
    }
    info
}

// --- SVG --------------------------------------------------------------------

fn svg_attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    attributes(tag)
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
}

/// Split the inside of a start tag into `(name, value)` pairs.
fn attributes(tag: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    // Skip the element name.
    while i < b.len() && !b[i].is_ascii_whitespace() {
        i += 1;
    }
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let name = &tag[start..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            let value = if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                i = vs;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                let v = &tag[vs..i.min(b.len())];
                i += 1;
                v
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                &tag[vs..i]
            };
            if !name.is_empty() {
                out.push((name, value));
            }
        } else if !name.is_empty() {
            out.push((name, ""));
        } else {
            i += 1;
        }
    }
    out
}

/// Characters that browsers strip or decode before interpreting a URL scheme.
fn squash(value: &str) -> String {
    value
        .replace("&#58;", ":")
        .replace("&#x3a;", ":")
        .replace("&#x3A;", ":")
        .replace("&colon;", ":")
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect::<String>()
        .to_ascii_lowercase()
}

fn css_is_unsafe(css: &str) -> Option<&'static str> {
    let c = squash(css);
    if c.contains("@import") {
        return Some("CSS `@import`");
    }
    if c.contains("expression(")
        || c.contains("javascript:")
        || c.contains("-moz-binding")
        || c.contains("behavior:")
    {
        return Some("scriptable CSS");
    }
    let mut rest = c.as_str();
    while let Some(i) = rest.find("url(") {
        let arg = rest[i + 4..].trim_start_matches(['"', '\'']);
        if !(arg.starts_with('#')
            || arg.starts_with("data:image/png")
            || arg.starts_with("data:image/jpeg"))
        {
            return Some("CSS `url()` pointing outside the file");
        }
        rest = &rest[i + 4..];
    }
    None
}

/// Check that an SVG is static and self-contained. Returns the intrinsic size.
pub fn check_svg(text: &str) -> Result<(Option<u32>, Option<u32>), AssetError> {
    let unsafe_svg = |what: &str| {
        AssetError {
        code: "E0405",
        message: format!("SVG contains {what}"),
        help: Some("DARM only packages static, self-contained SVG: remove scripts, event handlers, animation elements, `foreignObject` and references to other files".into()),
    }
    };
    let mut size = (None, None);
    let mut seen_root = false;
    let mut i = 0usize;
    let lower_all = text.to_ascii_lowercase();
    if lower_all.contains("<!entity") {
        return Err(unsafe_svg("an XML entity declaration"));
    }
    while let Some(off) = text[i..].find('<') {
        let start = i + off;
        let rest = &text[start..];
        if rest.starts_with("<!--") {
            i = start + rest.find("-->").map_or(rest.len(), |e| e + 3);
            continue;
        }
        if rest.starts_with("<![CDATA[") {
            i = start + rest.find("]]>").map_or(rest.len(), |e| e + 3);
            continue;
        }
        if rest.starts_with("<?") {
            if rest.to_ascii_lowercase().starts_with("<?xml-stylesheet") {
                return Err(unsafe_svg("an external stylesheet"));
            }
            i = start + rest.find("?>").map_or(rest.len(), |e| e + 2);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("</") {
            i = start + rest.find('>').map_or(rest.len(), |e| e + 1);
            continue;
        }
        // Start tag: find its end outside quotes.
        let bytes = rest.as_bytes();
        let mut j = 1;
        let mut quote = 0u8;
        while j < bytes.len() {
            let c = bytes[j];
            if quote != 0 {
                if c == quote {
                    quote = 0;
                }
            } else if c == b'"' || c == b'\'' {
                quote = c;
            } else if c == b'>' {
                break;
            }
            j += 1;
        }
        if j >= bytes.len() {
            return Err(err(
                "E0403",
                "SVG is not well-formed: a tag is never closed",
            ));
        }
        let tag = &rest[1..j];
        let name_end = tag
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tag.len());
        let full = tag[..name_end].to_ascii_lowercase();
        let local = full.rsplit(':').next().unwrap_or("");
        match local {
            "script" => return Err(unsafe_svg("a `<script>` element")),
            "foreignobject" => return Err(unsafe_svg("a `<foreignObject>` element")),
            "iframe" | "object" | "embed" | "audio" | "video" | "canvas" | "handler"
            | "listener" => {
                return Err(unsafe_svg(&format!("a `<{local}>` element")));
            }
            "animate" | "set" | "animatetransform" | "animatemotion" | "animatecolor"
            | "discard" => {
                return Err(unsafe_svg(&format!("an animation element `<{local}>`")));
            }
            _ => {}
        }
        for (name, value) in attributes(tag) {
            let n = name.to_ascii_lowercase();
            let n_local = n.rsplit(':').next().unwrap_or("");
            let v = squash(value);
            if n.starts_with("on") && n.len() > 2 {
                return Err(unsafe_svg(&format!("an event handler attribute `{name}`")));
            }
            if v.contains("javascript:")
                || v.contains("vbscript:")
                || v.starts_with("data:text/html")
            {
                return Err(unsafe_svg("a scriptable URL"));
            }
            if matches!(n_local, "href" | "src")
                && !(v.is_empty()
                    || v.starts_with('#')
                    || v.starts_with("data:image/png")
                    || v.starts_with("data:image/jpeg"))
            {
                return Err(unsafe_svg(&format!(
                    "a reference to an external resource (`{name}=\"{value}\"`)"
                )));
            }
            if (n == "style"
                || matches!(
                    n_local,
                    "fill"
                        | "stroke"
                        | "filter"
                        | "mask"
                        | "clip-path"
                        | "marker-start"
                        | "marker-mid"
                        | "marker-end"
                ))
                && let Some(what) = css_is_unsafe(value)
            {
                return Err(unsafe_svg(what));
            }
        }
        if local == "style" {
            let body_start = start + j + 1;
            let body_end = lower_all[body_start..]
                .find("</style")
                .map_or(text.len(), |e| body_start + e);
            if let Some(what) = css_is_unsafe(&text[body_start..body_end]) {
                return Err(unsafe_svg(what));
            }
        }
        if !seen_root {
            seen_root = true;
            if local != "svg" {
                return Err(err(
                    "E0403",
                    "the file is XML but its root element is not `<svg>`",
                ));
            }
            let num = |s: &str| -> Option<f64> {
                let t = s.trim().trim_end_matches("px");
                t.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
            };
            let w = svg_attr(tag, "width").and_then(num);
            let h = svg_attr(tag, "height").and_then(num);
            let vb: Vec<f64> = svg_attr(tag, "viewBox")
                .map(|v| {
                    v.split(|c: char| c.is_whitespace() || c == ',')
                        .filter_map(|p| p.parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            let (vw, vh) = if vb.len() == 4 && vb[2] > 0.0 && vb[3] > 0.0 {
                (Some(vb[2]), Some(vb[3]))
            } else {
                (None, None)
            };
            if w.is_none() && h.is_none() && vw.is_none() {
                return Err(AssetError {
                    code: "E0403",
                    message: "SVG has neither a `viewBox` nor `width`/`height`, so its proportions are unknown".into(),
                    help: Some("add `viewBox=\"0 0 W H\"` to the root `<svg>` element".into()),
                });
            }
            let as_px = |v: Option<f64>| v.map(|v| v.round().clamp(1.0, 100_000.0) as u32);
            size = (as_px(w.or(vw)), as_px(h.or(vh)));
        }
        i = start + j + 1;
    }
    if !seen_root {
        return Err(err("E0403", "the file does not contain an `<svg>` element"));
    }
    Ok(size)
}

fn check_vtt(text: &str) -> Result<(), AssetError> {
    let body = text.trim_start_matches('\u{feff}');
    let first = body.lines().next().unwrap_or("");
    if !(first == "WEBVTT" || first.starts_with("WEBVTT ") || first.starts_with("WEBVTT\t")) {
        return Err(err("E0407", "captions must start with the line `WEBVTT`"));
    }
    if !body.contains("-->") {
        return Err(err(
            "E0407",
            "captions contain no cues (no `-->` timing line)",
        ));
    }
    Ok(())
}

/// Inspect one asset. `want` is the kind implied by the constructor
/// (`image(...)`, `video(...)`, …); a mismatch is an error.
pub fn inspect(r: &mut dyn ReadSeek, want: ResourceKind) -> Result<Inspected, AssetError> {
    let io = |e: std::io::Error| err("E0404", format!("cannot read the file: {e}"));
    let size = r.seek(SeekFrom::End(0)).map_err(io)?;
    if size == 0 {
        return Err(err("E0403", "the file is empty"));
    }
    if size > MAX_ASSET_BYTES {
        return Err(err(
            "E0404",
            format!(
                "the file is larger than the {} GiB limit",
                MAX_ASSET_BYTES >> 30
            ),
        ));
    }
    r.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut head = vec![0u8; size.min(512 * 1024) as usize];
    r.read_exact(&mut head).map_err(io)?;

    let Some(format) = sniff(&head[..head.len().min(4096)], want) else {
        return Err(AssetError {
            code: "E0403",
            message: "unrecognised file format".into(),
            help: Some(format!("this argument needs {}", accepted_formats(want))),
        });
    };
    if format.kind != want {
        return Err(AssetError {
            code: "E0403",
            message: format!(
                "expected {} but the file is {}",
                accepted_formats(want),
                format.label
            ),
            help: None,
        });
    }

    let mut out = Inspected {
        kind: format.kind,
        media_type: format.media_type,
        ext: format.ext,
        label: format.label,
        size,
        sha256: String::new(),
        width: None,
        height: None,
        duration_ms: None,
    };
    let dims = match format.media_type {
        "image/png" if head.len() >= 24 => Some((be32(&head[16..]), be32(&head[20..]))),
        "image/gif" if head.len() >= 10 => Some((le16(&head[6..]), le16(&head[8..]))),
        "image/jpeg" => jpeg_size(&head),
        "image/webp" => webp_size(&head),
        _ => None,
    };
    if let Some((w, h)) = dims {
        if w == 0 || h == 0 || w > 65_535 || h > 65_535 {
            return Err(err(
                "E0403",
                format!("image dimensions {w}×{h} are not supported"),
            ));
        }
        out.width = Some(w);
        out.height = Some(h);
    } else if matches!(
        format.media_type,
        "image/png" | "image/gif" | "image/jpeg" | "image/webp"
    ) {
        return Err(err(
            "E0403",
            format!(
                "the {} is damaged: its dimensions cannot be read",
                format.label
            ),
        ));
    }
    if matches!(format.media_type, "image/svg+xml" | "text/vtt") {
        if size > MAX_TEXT_ASSET_BYTES {
            return Err(err(
                "E0404",
                format!(
                    "{} files are limited to {} MiB",
                    format.label,
                    MAX_TEXT_ASSET_BYTES >> 20
                ),
            ));
        }
        // `head` holds the whole file when it is under 512 KiB; otherwise read the rest.
        let mut all = head.clone();
        r.read_to_end(&mut all).map_err(io)?;
        let text = String::from_utf8(all)
            .map_err(|_| err("E0403", format!("the {} is not valid UTF-8", format.label)))?;
        if format.media_type == "text/vtt" {
            check_vtt(&text)?;
        } else {
            let (w, h) = check_svg(&text)?;
            out.width = w;
            out.height = h;
        }
    }
    if matches!(format.media_type, "video/mp4" | "audio/mp4") {
        let info = mp4_info(r, size);
        out.duration_ms = info.duration_ms;
        if format.kind == ResourceKind::Video {
            match info.video {
                Some((w, h)) => {
                    out.width = Some(w);
                    out.height = Some(h);
                }
                None => {
                    return Err(AssetError {
                        code: "E0403",
                        message: "the MP4 file has no video track".into(),
                        help: Some(
                            "declare it with `audio(...)`, or re-encode it with a video stream"
                                .into(),
                        ),
                    });
                }
            }
        }
    }

    // Hash the whole file as a stream.
    r.seek(SeekFrom::Start(0)).map_err(io)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut total = 0u64;
    loop {
        let n = r.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    if total != size {
        return Err(err("E0404", "the file changed while it was being read"));
    }
    out.sha256 = hex(&hasher.finalize());
    Ok(out)
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

#[cfg(test)]
pub(crate) mod testdata {
    /// A valid 1×1 PNG.
    pub const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, 0x49, 0x48, 0x44, 0x52, 0,
        0, 0, 3, 0, 0, 0, 2, 8, 2, 0, 0, 0, 0x12, 0x16, 0xf1, 0x4d, 0, 0, 0, 0x0c, 0x49, 0x44,
        0x41, 0x54, 0x78, 0x9c, 0x63, 0x60, 0x40, 0x05, 0, 0, 0x12, 0, 0x01, 0x8b, 0x9e, 0x62,
        0x3d, 0, 0, 0, 0, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];
    pub const SVG: &str = r##"<?xml version="1.0"?><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 400 300"><rect width="400" height="300" fill="#234"/></svg>"##;
    pub const VTT: &str = "WEBVTT\n\n00:00.000 --> 00:02.000\nHello\n";

    /// Minimal MP4 with an `mvhd` (2.5 s) and one track of the given size.
    pub fn mp4(width: u16, height: u16) -> Vec<u8> {
        fn bx(ty: &[u8; 4], body: &[u8]) -> Vec<u8> {
            let mut v = ((body.len() + 8) as u32).to_be_bytes().to_vec();
            v.extend_from_slice(ty);
            v.extend_from_slice(body);
            v
        }
        let mut mvhd = vec![0u8; 100];
        mvhd[12..16].copy_from_slice(&1000u32.to_be_bytes());
        mvhd[16..20].copy_from_slice(&2500u32.to_be_bytes());
        let mut tkhd = vec![0u8; 84];
        tkhd[76..78].copy_from_slice(&width.to_be_bytes());
        tkhd[80..82].copy_from_slice(&height.to_be_bytes());
        let trak = bx(b"trak", &bx(b"tkhd", &tkhd));
        let mut moov_body = bx(b"mvhd", &mvhd);
        moov_body.extend(trak);
        let mut out = bx(b"ftyp", b"isom\0\0\x02\0isomiso2");
        out.extend(bx(b"moov", &moov_body));
        out.extend(bx(b"mdat", &[0u8; 32]));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testdata::*;
    use super::*;
    use std::io::Cursor;

    fn run(data: &[u8], want: ResourceKind) -> Result<Inspected, AssetError> {
        inspect(&mut Cursor::new(data.to_vec()), want)
    }

    #[test]
    fn identifies_by_content() {
        let png = run(PNG, ResourceKind::Image).unwrap();
        assert_eq!(
            (png.media_type, png.width, png.height),
            ("image/png", Some(3), Some(2))
        );
        assert_eq!(png.sha256.len(), 64);
        let svg = run(SVG.as_bytes(), ResourceKind::Image).unwrap();
        assert_eq!(
            (svg.media_type, svg.width, svg.height),
            ("image/svg+xml", Some(400), Some(300))
        );
        let video = run(&mp4(640, 360), ResourceKind::Video).unwrap();
        assert_eq!(
            (
                video.media_type,
                video.width,
                video.height,
                video.duration_ms
            ),
            ("video/mp4", Some(640), Some(360), Some(2500))
        );
        let m4a = run(&mp4(0, 0), ResourceKind::Audio).unwrap();
        assert_eq!(m4a.media_type, "audio/mp4");
        assert_eq!(
            run(VTT.as_bytes(), ResourceKind::Captions)
                .unwrap()
                .media_type,
            "text/vtt"
        );
    }

    #[test]
    fn rejects_mismatch_and_garbage() {
        assert_eq!(run(PNG, ResourceKind::Video).unwrap_err().code, "E0403");
        assert_eq!(
            run(b"hello world", ResourceKind::Image).unwrap_err().code,
            "E0403"
        );
        assert_eq!(run(b"", ResourceKind::Image).unwrap_err().code, "E0403");
        assert_eq!(
            run(&mp4(0, 0), ResourceKind::Video).unwrap_err().code,
            "E0403"
        );
        assert_eq!(
            run(&PNG[..20], ResourceKind::Image).unwrap_err().code,
            "E0403"
        );
        assert_eq!(
            run(b"WEBVTT\n\nno cues", ResourceKind::Captions)
                .unwrap_err()
                .code,
            "E0407"
        );
    }

    #[test]
    fn svg_safety() {
        let bad = [
            r#"<svg viewBox="0 0 1 1"><script>alert(1)</script></svg>"#,
            r#"<svg viewBox="0 0 1 1" onload="x()"/>"#,
            r#"<svg viewBox="0 0 1 1"><a href="javascript:x()"><rect/></a></svg>"#,
            r#"<svg viewBox="0 0 1 1"><a href="java&#x3a;script:x()"><rect/></a></svg>"#,
            r#"<svg viewBox="0 0 1 1"><image href="https://example.org/a.png"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><image xlink:href="other.svg"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><foreignObject><p>x</p></foreignObject></svg>"#,
            r#"<svg viewBox="0 0 1 1"><style>@import url(x.css);</style></svg>"#,
            r#"<svg viewBox="0 0 1 1"><rect style="fill:url(https://e.org/x)"/></svg>"#,
            r#"<svg viewBox="0 0 1 1"><animate attributeName="x"/></svg>"#,
            r#"<!DOCTYPE svg [<!ENTITY x "y">]><svg viewBox="0 0 1 1"/>"#,
            r#"<?xml-stylesheet href="a.css"?><svg viewBox="0 0 1 1"/>"#,
            r#"<svg viewBox="0 0 1 1"><SCRIPT>x</SCRIPT></svg>"#,
        ];
        for b in bad {
            assert_eq!(check_svg(b).unwrap_err().code, "E0405", "{b}");
        }
        let good = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20px" height="10"><defs><linearGradient id="g"/></defs><rect fill="url(#g)" style="stroke:#000"/><use href="#g"/><!-- <script> in a comment --></svg>"##;
        assert_eq!(check_svg(good).unwrap(), (Some(20), Some(10)));
        assert_eq!(check_svg("<svg><rect/></svg>").unwrap_err().code, "E0403");
        assert_eq!(check_svg("<html></html>").unwrap_err().code, "E0403");
    }
}
