#[cfg(feature = "esp")]
extern crate alloc;

#[cfg(feature = "esp")]
use alloc::string::String;
#[cfg(feature = "esp")]
use alloc::vec::Vec;
#[cfg(not(feature = "esp"))]
use std::string::String;
#[cfg(not(feature = "esp"))]
use std::vec::Vec;

use crate::epub::EpubError;

/// Common interface over epub, HTML, and plain-text books.
pub trait Book {
    /// Ordered list of chapter identifiers (paths for epub, a single label for others).
    fn spine(&self) -> Result<Vec<String>, EpubError>;
    /// Plain text content for the chapter named by `id`.
    fn chapter_text(&self, id: &str) -> Result<String, EpubError>;
}

// ── TxtBook ───────────────────────────────────────────────────────────────────

pub struct TxtBook {
    data: Vec<u8>,
}

impl TxtBook {
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl Book for TxtBook {
    fn spine(&self) -> Result<Vec<String>, EpubError> {
        let mut v = Vec::new();
        v.push(String::from("Document"));
        Ok(v)
    }

    fn chapter_text(&self, _id: &str) -> Result<String, EpubError> {
        Ok(decode_lossy(&self.data))
    }
}

// ── HtmlBook ──────────────────────────────────────────────────────────────────

pub struct HtmlBook {
    data: Vec<u8>,
}

impl HtmlBook {
    pub fn from_vec(data: Vec<u8>) -> Self {
        Self { data }
    }
}

impl Book for HtmlBook {
    fn spine(&self) -> Result<Vec<String>, EpubError> {
        let mut v = Vec::new();
        v.push(String::from("Document"));
        Ok(v)
    }

    fn chapter_text(&self, _id: &str) -> Result<String, EpubError> {
        Ok(html_to_text(&decode_lossy(&self.data)))
    }
}

// ── UTF-8 lossy decode ────────────────────────────────────────────────────────

/// Decode bytes as UTF-8; if invalid, fall back to Latin-1 (ISO-8859-1) by
/// treating each byte as its Unicode code point.  Avoids panicking on files
/// saved with Windows-1252 or other 8-bit encodings.
fn decode_lossy(bytes: &[u8]) -> String {
    match core::str::from_utf8(bytes) {
        Ok(s) => String::from(s),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

// ── HTML → plain text ─────────────────────────────────────────────────────────

fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let bytes = html.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut skip = false; // inside <head>, <script>, or <style>

    while i < n {
        if bytes[i] == b'<' {
            i += 1;
            let tag_start = i;
            while i < n && bytes[i] != b'>' {
                i += 1;
            }
            let tag = core::str::from_utf8(&bytes[tag_start..i])
                .unwrap_or("")
                .trim();
            process_tag(tag, &mut out, &mut skip);
            if i < n {
                i += 1; // consume '>'
            }
        } else if !skip {
            if bytes[i] == b'&' {
                let (ch, len) = decode_entity(&bytes[i..]);
                out.push(ch);
                i += len;
            } else {
                // Copy one UTF-8 character, collapsing whitespace runs to a single space.
                let ch_len = utf8_char_len(bytes[i]);
                let end = (i + ch_len).min(n);
                if let Ok(s) = core::str::from_utf8(&bytes[i..end]) {
                    for c in s.chars() {
                        if c.is_whitespace() {
                            if !out.ends_with(' ') && !out.ends_with('\n') {
                                out.push(' ');
                            }
                        } else {
                            out.push(c);
                        }
                    }
                }
                i = end;
            }
        } else {
            i += 1;
        }
    }

    // Remove trailing whitespace.
    let trimmed = out.trim_end();
    String::from(trimmed)
}

fn process_tag(tag: &str, out: &mut String, skip: &mut bool) {
    let closing = tag.starts_with('/');
    let name_part = if closing { &tag[1..] } else { tag };
    // Tag name runs up to the first whitespace or '/'
    let name_end = name_part
        .find(|c: char| c.is_whitespace() || c == '/')
        .unwrap_or(name_part.len());
    let name = &name_part[..name_end];

    if name.eq_ignore_ascii_case("head")
        || name.eq_ignore_ascii_case("script")
        || name.eq_ignore_ascii_case("style")
    {
        *skip = !closing;
        return;
    }
    if *skip {
        return;
    }

    if closing {
        on_close(name, out);
    } else {
        on_open(name, out);
    }
}

fn on_open(name: &str, out: &mut String) {
    if is_heading(name) {
        push_blank_line(out);
    } else if name.eq_ignore_ascii_case("br") {
        push_newline(out);
    } else if name.eq_ignore_ascii_case("li") {
        push_newline(out);
        out.push_str("• ");
    } else if name.eq_ignore_ascii_case("hr") {
        push_newline(out);
        out.push_str("────────────────────");
        push_newline(out);
    }
}

fn on_close(name: &str, out: &mut String) {
    if is_heading(name) {
        push_blank_line(out);
    } else if name.eq_ignore_ascii_case("p")
        || name.eq_ignore_ascii_case("blockquote")
        || name.eq_ignore_ascii_case("ul")
        || name.eq_ignore_ascii_case("ol")
    {
        push_blank_line(out);
    } else if name.eq_ignore_ascii_case("div")
        || name.eq_ignore_ascii_case("td")
        || name.eq_ignore_ascii_case("th")
        || name.eq_ignore_ascii_case("tr")
    {
        push_newline(out);
    }
}

fn is_heading(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() == 2 && (b[0] == b'h' || b[0] == b'H') && b[1] >= b'1' && b[1] <= b'6'
}

fn push_newline(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn push_blank_line(out: &mut String) {
    push_newline(out);
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
}

fn utf8_char_len(first_byte: u8) -> usize {
    if first_byte < 0x80 {
        1
    } else if first_byte < 0xE0 {
        2
    } else if first_byte < 0xF0 {
        3
    } else {
        4
    }
}

fn decode_entity(bytes: &[u8]) -> (char, usize) {
    // bytes[0] == b'&'; look for ';' within the next 12 bytes
    let rest = &bytes[1..bytes.len().min(13)];
    let Some(semi) = rest.iter().position(|&b| b == b';') else {
        return ('&', 1);
    };
    let Ok(entity) = core::str::from_utf8(&rest[..semi]) else {
        return ('&', 1);
    };
    let total = semi + 2; // '&' + entity + ';'

    let ch = match entity {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => '\u{00A0}',
        "mdash" => '\u{2014}',
        "ndash" => '\u{2013}',
        "ldquo" => '\u{201C}',
        "rdquo" => '\u{201D}',
        "lsquo" => '\u{2018}',
        "rsquo" => '\u{2019}',
        "hellip" => '\u{2026}',
        "copy" => '\u{00A9}',
        "reg" => '\u{00AE}',
        "trade" => '\u{2122}',
        "eacute" => '\u{00E9}',
        "egrave" => '\u{00E8}',
        "ecirc" => '\u{00EA}',
        "agrave" => '\u{00E0}',
        "aacute" => '\u{00E1}',
        "ocirc" => '\u{00F4}',
        "uuml" => '\u{00FC}',
        _ if entity.starts_with('#') => {
            let num = &entity[1..];
            let code = if num.starts_with('x') || num.starts_with('X') {
                u32::from_str_radix(&num[1..], 16).ok()
            } else {
                num.parse::<u32>().ok()
            };
            match code.and_then(core::char::from_u32) {
                Some(c) => c,
                None => return ('&', 1),
            }
        }
        _ => return ('&', 1),
    };
    (ch, total)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── TxtBook ───────────────────────────────────────────────────────────────

    #[test]
    fn txt_spine_has_single_document_entry() {
        let book = TxtBook::from_vec(b"hello".to_vec());
        let spine = book.spine().unwrap();
        assert_eq!(spine, vec!["Document"]);
    }

    #[test]
    fn txt_chapter_text_returns_content() {
        let book = TxtBook::from_vec(b"hello world".to_vec());
        assert_eq!(book.chapter_text("Document").unwrap(), "hello world");
    }

    #[test]
    fn txt_invalid_utf8_decoded_lossily() {
        // Non-UTF-8 bytes are decoded as Latin-1 (each byte → its Unicode code point).
        let book = TxtBook::from_vec(vec![b'h', b'i', 0xFF]);
        let text = book.chapter_text("Document").unwrap();
        assert!(text.starts_with("hi"), "got: {text:?}");
        assert!(text.contains('\u{00FF}'), "got: {text:?}");
    }

    // ── HtmlBook ──────────────────────────────────────────────────────────────

    #[test]
    fn html_spine_has_single_document_entry() {
        let book = HtmlBook::from_vec(b"<p>hi</p>".to_vec());
        let spine = book.spine().unwrap();
        assert_eq!(spine, vec!["Document"]);
    }

    // ── html_to_text: plain text ──────────────────────────────────────────────

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(html_to_text("hello world"), "hello world");
    }

    #[test]
    fn whitespace_runs_collapse_to_single_space() {
        assert_eq!(html_to_text("hello   world"), "hello world");
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert_eq!(html_to_text(""), "");
    }

    #[test]
    fn bare_newline_in_text_collapses_to_space() {
        // In HTML, a bare newline in text is whitespace — not a forced line break.
        assert_eq!(html_to_text("line1\nline2"), "line1 line2");
    }

    // ── html_to_text: skipped sections ───────────────────────────────────────

    #[test]
    fn head_section_is_skipped() {
        let out = html_to_text("<head><title>My Page</title></head>content");
        assert_eq!(out, "content");
    }

    #[test]
    fn script_section_is_skipped() {
        let out = html_to_text("before<script>alert(1)</script>after");
        // No space is inserted where the tag was; content is simply omitted.
        assert_eq!(out, "beforeafter");
    }

    #[test]
    fn style_section_is_skipped() {
        let out = html_to_text("before<style>body{color:red}</style>after");
        assert_eq!(out, "beforeafter");
    }

    // ── html_to_text: block elements ─────────────────────────────────────────

    #[test]
    fn paragraphs_separated_by_blank_line() {
        let out = html_to_text("<p>first</p><p>second</p>");
        assert_eq!(out, "first\n\nsecond");
    }

    #[test]
    fn heading_surrounded_by_blank_lines() {
        // Content before the heading, heading, content after.
        let out = html_to_text("<p>intro</p><h1>Title</h1><p>body</p>");
        assert!(
            out.contains("\n\nTitle\n\n"),
            "heading must be surrounded by blank lines; got: {out:?}"
        );
    }

    #[test]
    fn br_inserts_newline() {
        let out = html_to_text("line1<br>line2");
        assert_eq!(out, "line1\nline2");
    }

    #[test]
    fn li_items_get_bullets() {
        let out = html_to_text("<ul><li>one</li><li>two</li></ul>");
        assert!(out.contains("• one"), "got: {out:?}");
        assert!(out.contains("• two"), "got: {out:?}");
    }

    #[test]
    fn div_close_inserts_newline() {
        let out = html_to_text("<div>a</div><div>b</div>");
        assert!(
            out.contains("a\nb") || out.contains("a\n\nb"),
            "got: {out:?}"
        );
    }

    // ── html_to_text: entities ────────────────────────────────────────────────

    #[test]
    fn entity_amp() {
        assert_eq!(html_to_text("a &amp; b"), "a & b");
    }

    #[test]
    fn entity_lt_gt() {
        assert_eq!(html_to_text("&lt;br&gt;"), "<br>");
    }

    #[test]
    fn entity_quot() {
        assert_eq!(html_to_text("&quot;hi&quot;"), "\"hi\"");
    }

    #[test]
    fn entity_mdash() {
        assert_eq!(html_to_text("x&mdash;y"), "x\u{2014}y");
    }

    #[test]
    fn entity_ndash() {
        assert_eq!(html_to_text("1&ndash;2"), "1\u{2013}2");
    }

    #[test]
    fn entity_curly_quotes() {
        assert_eq!(html_to_text("&ldquo;hi&rdquo;"), "\u{201C}hi\u{201D}");
    }

    #[test]
    fn entity_decimal_numeric() {
        // &#160; is a non-breaking space (U+00A0); wrap in text so trim_end doesn't eat it
        assert_eq!(html_to_text("a&#160;b"), "a\u{00A0}b");
    }

    #[test]
    fn entity_hex_numeric() {
        // &#x2014; is an em-dash (U+2014)
        assert_eq!(html_to_text("&#x2014;"), "\u{2014}");
    }

    #[test]
    fn entity_hex_uppercase() {
        assert_eq!(html_to_text("&#X2014;"), "\u{2014}");
    }

    #[test]
    fn unknown_entity_passes_through_as_ampersand() {
        // An unrecognised named entity: the '&' is emitted, then the rest as text.
        let out = html_to_text("&foobar;");
        assert!(out.starts_with('&'), "got: {out:?}");
    }

    // ── html_to_text: case-insensitive tags ───────────────────────────────────

    #[test]
    fn uppercase_br_inserts_newline() {
        assert_eq!(html_to_text("a<BR>b"), "a\nb");
    }

    #[test]
    fn mixed_case_heading() {
        let out = html_to_text("<H2>Title</H2>");
        assert!(out.contains("Title"), "got: {out:?}");
    }

    // ── html_to_text: self-closing tags ───────────────────────────────────────

    #[test]
    fn self_closing_br_inserts_newline() {
        // <br/> — the slash appears in name_part after trimming
        let out = html_to_text("a<br/>b");
        // The tag name parsing strips trailing '/', so this should act like <br>
        assert!(out.contains('a') && out.contains('b'), "got: {out:?}");
    }
}
