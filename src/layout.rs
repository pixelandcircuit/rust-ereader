#[cfg(feature = "esp")]
extern crate alloc;

#[cfg(feature = "esp")]
use alloc::{boxed::Box, vec::Vec};
#[cfg(not(feature = "esp"))]
use std::vec::Vec;

/// Extra vertical space added between paragraphs (after a blank line in the
/// source text), expressed as a multiple of `line_height_px`.  Set to 1 for
/// one full extra line of breathing room, 0 to disable, 2 for double, etc.
pub const PARA_GAP_LINES: u32 = 2;

// ── Public types ──────────────────────────────────────────────────────────────

/// A half-open byte range [start, end) into the chapter text string.
#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub start: usize,
    pub end: usize,
}

/// The result of laying out one chapter: an ordered list of pages.
#[derive(Debug, Clone)]
pub struct Layout {
    pub pages: Vec<Page>,
}

/// Metrics needed to measure text for layout.
pub struct FontMetrics {
    pub line_height_px: u32,
    /// Width of a single space character, in pixels. If 0, the cached ' ' width is used.
    pub space_width_px: u32,
    /// Returns the pixel advance width of the given UTF-8 string at the current font size.
    pub measure: Box<dyn Fn(&str) -> u32>,
}

/// Configuration for the layout engine.
pub struct LayoutConfig {
    pub screen_width: u32,  // physical display width  (e.g. 960)
    pub screen_height: u32, // physical display height (e.g. 540)
    pub margin_x: u32,      // horizontal margin on each side
    pub margin_y: u32,      // vertical margin on each side
    pub font: FontMetrics,
}

// ── Core layout function ──────────────────────────────────────────────────────

/// Word-wrap `text` into pages using pixel-accurate font metrics.
///
/// Rules:
/// - Words are runs of non-whitespace characters.
/// - A word that does not fit on the current line wraps to the next.
/// - A word wider than `content_width` is placed alone on its line (no infinite loop).
/// - `'\n'` is a forced line break; `'\n\n'` is a paragraph break (adds
///   `PARA_GAP_LINES × line_h` of extra vertical space between paragraphs).
/// - Leading spaces at the start of a line are silently dropped.
/// - `Page.start` / `Page.end` are byte offsets into `text` (UTF-8 safe).
pub fn layout_chapter(text: &str, cfg: &LayoutConfig) -> Layout {
    let content_w = cfg.screen_width.saturating_sub(2 * cfg.margin_x);
    let content_h = cfg.screen_height.saturating_sub(2 * cfg.margin_y);
    let line_h = cfg.font.line_height_px;
    let para_gap = line_h * PARA_GAP_LINES;

    // Degenerate / zero-sized config: one page for everything.
    if content_w == 0 || content_h == 0 || line_h == 0 {
        let pages = if text.is_empty() {
            Vec::new()
        } else {
            {
                let mut v = Vec::new();
                v.push(Page {
                    start: 0,
                    end: text.len(),
                });
                v
            }
        };
        return Layout { pages };
    }

    // ── ASCII glyph width cache ───────────────────────────────────────────────
    // Pre-measuring every printable ASCII character avoids calling `measure`
    // repeatedly for the overwhelmingly common case of ASCII text.
    let mut gcache = [0u32; 128];
    {
        let mut buf = [0u8; 4];
        for b in 32u8..127u8 {
            let s = char::from(b).encode_utf8(&mut buf);
            gcache[b as usize] = (cfg.font.measure)(s);
        }
    }
    let space_w = if cfg.font.space_width_px > 0 {
        cfg.font.space_width_px
    } else {
        gcache[b' ' as usize]
    };

    let bytes = text.as_bytes();
    let total = bytes.len();

    let mut pages = Vec::new();
    let mut page_start = 0usize;
    let mut line_y = 0u32; // top of current line, relative to page top
    let mut line_px = 0u32; // pixel width consumed so far on this line
    let mut pos = 0usize;
    let mut pending_space = false; // space token seen before the next word

    // Advance to the next line.  `extra` adds paragraph spacing.  `at` is the
    // byte offset that will become page_start if a page boundary is crossed.
    // Callers are responsible for resetting line_px and pending_space afterwards.
    macro_rules! next_line {
        ($extra:expr, $at:expr) => {{
            line_y += line_h + $extra;
            if line_y + line_h > content_h {
                pages.push(Page {
                    start: page_start,
                    end: $at,
                });
                page_start = $at;
                line_y = 0;
            }
        }};
    }

    while pos < total {
        let b = bytes[pos];

        // ── Newlines ──────────────────────────────────────────────────────────
        if b == b'\n' {
            let double = pos + 1 < total && bytes[pos + 1] == b'\n';
            let skip = if double { 2 } else { 1 };
            let extra = if double { para_gap } else { 0 };
            next_line!(extra, pos + skip);
            line_px = 0;
            pending_space = false;
            pos += skip;
            continue;
        }

        // ── Spaces ────────────────────────────────────────────────────────────
        if b == b' ' {
            // Consume all consecutive spaces; only set pending_space when we're
            // mid-line so that lines never start with whitespace.
            if line_px > 0 {
                pending_space = true;
            }
            while pos < total && bytes[pos] == b' ' {
                pos += 1;
            }
            continue;
        }

        // ── Word ──────────────────────────────────────────────────────────────
        let word_start = pos;
        let mut word_px = 0u32;

        while pos < total && bytes[pos] != b' ' && bytes[pos] != b'\n' {
            let wb = bytes[pos];
            if wb < 128 {
                // Fast path: ASCII — look up cache.
                word_px += gcache[wb as usize];
                pos += 1;
            } else {
                // Slow path: multi-byte UTF-8 — scan to the end of the codepoint,
                // then measure the whole character as a string.
                let cs = pos;
                pos += 1;
                while pos < total && (bytes[pos] & 0xC0) == 0x80 {
                    pos += 1;
                }
                word_px += (cfg.font.measure)(&text[cs..pos]);
            }
        }

        if word_start == pos {
            pos += 1;
            continue;
        } // safety skip for unexpected bytes

        // How much horizontal space does this word need?
        let gap = if pending_space && line_px > 0 {
            space_w
        } else {
            0
        };
        let needed = line_px + gap + word_px;

        if needed > content_w && line_px > 0 {
            // Word doesn't fit on the current line: wrap, then place at line start.
            next_line!(0, word_start);
            line_px = word_px;
            pending_space = false;
        } else {
            // Fits (or is alone on an empty line — overflowing is unavoidable).
            line_px = needed;
            pending_space = false;
        }
    }

    // Emit the final (possibly partial) page.
    if page_start < total || pages.is_empty() {
        pages.push(Page {
            start: page_start,
            end: total,
        });
    }

    Layout { pages }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Fixed-width metrics: every character is `px` wide, space included.
    fn fixed_cfg(char_px: u32, line_h: u32, w: u32, h: u32) -> LayoutConfig {
        LayoutConfig {
            screen_width: w,
            screen_height: h,
            margin_x: 0,
            margin_y: 0,
            font: FontMetrics {
                line_height_px: line_h,
                space_width_px: char_px,
                measure: Box::new(move |s: &str| s.chars().count() as u32 * char_px),
            },
        }
    }

    fn pages(text: &str, cfg: &LayoutConfig) -> Vec<(usize, usize)> {
        layout_chapter(text, cfg)
            .pages
            .iter()
            .map(|p| (p.start, p.end))
            .collect()
    }

    // ── Basic word wrapping ───────────────────────────────────────────────────

    #[test]
    fn single_short_line_one_page() {
        // "hello" = 5 chars × 10 px = 50 px; fits in 100 px wide, 50 px tall
        let cfg = fixed_cfg(10, 20, 100, 50);
        let p = pages("hello", &cfg);
        assert_eq!(p.len(), 1);
        assert_eq!(p[0], (0, 5));
    }

    #[test]
    fn word_wrap_splits_at_space() {
        // 10 px/char, 100 px wide → 10 chars per line.
        // "hello world" — "hello" fits, then space, "world" would make 11 chars → wraps.
        // Both words fit on their own lines; single page tall enough for 2 lines.
        let cfg = fixed_cfg(10, 20, 100, 60);
        let layout = layout_chapter("hello world", &cfg);
        assert_eq!(layout.pages.len(), 1, "both lines fit one page");
        // The full text is covered.
        assert_eq!(layout.pages[0].end, "hello world".len());
    }

    #[test]
    fn long_word_placed_alone_on_line() {
        // "superlongword" = 13 chars × 10 px = 130 px > 100 px wide.
        // Must not loop forever; placed alone on its line.
        let cfg = fixed_cfg(10, 20, 100, 50);
        let layout = layout_chapter("superlongword", &cfg);
        assert_eq!(layout.pages.len(), 1);
        assert_eq!(layout.pages[0].end, "superlongword".len());
    }

    // ── Hard newlines ─────────────────────────────────────────────────────────

    #[test]
    fn hard_newline_forces_line_break() {
        // "a\nb" — two lines of 1 char each. Both fit in one page.
        let cfg = fixed_cfg(10, 20, 100, 60);
        let layout = layout_chapter("a\nb", &cfg);
        assert_eq!(layout.pages.len(), 1);
        assert_eq!(layout.pages[0].end, "a\nb".len());
    }

    #[test]
    fn double_newline_paragraph_gap_causes_page_break() {
        // line_h=20, para_gap=10. Screen height = 30 → room for exactly one line.
        // After the first line, a double-newline adds para_gap (10) then another line_h (20):
        // total 50 > 30, so the second paragraph starts a new page.
        let cfg = fixed_cfg(10, 20, 100, 30);
        let text = "first\n\nsecond";
        let layout = layout_chapter(text, &cfg);
        assert!(
            layout.pages.len() >= 2,
            "double newline should push second para to new page"
        );
        // First page starts at 0.
        assert_eq!(layout.pages[0].start, 0);
        // Second page contains "second".
        let last = layout.pages.last().unwrap();
        assert_eq!(&text[last.start..last.end], "second");
    }

    // ── Pagination ────────────────────────────────────────────────────────────

    #[test]
    fn text_splits_across_pages() {
        // 10 px/char, 100 px wide (10 chars/line), line_h=20, screen_h=60 → 3 lines/page.
        // Feed 9 words of 8 chars each → 9 lines → should span 3 pages.
        let cfg = fixed_cfg(10, 20, 100, 60);
        // Each "wordXXXX" is 8 chars; they each fit on their own line (80 px < 100).
        let text =
            "aaaaaaaa bbbbbbbb cccccccc dddddddd eeeeeeee ffffffff gggggggg hhhhhhhh iiiiiiii";
        let layout = layout_chapter(text, &cfg);
        assert!(layout.pages.len() >= 3, "9 lines at 3 lines/page = 3 pages");
    }

    #[test]
    fn pages_cover_full_text_no_gaps() {
        let cfg = fixed_cfg(10, 20, 100, 60);
        let text = "one two three four five six seven eight nine ten eleven twelve";
        let layout = layout_chapter(text, &cfg);
        // Pages must be contiguous and cover the full text.
        let mut expected_start = 0usize;
        for page in &layout.pages {
            assert_eq!(page.start, expected_start, "pages must be contiguous");
            expected_start = page.end;
        }
        assert_eq!(expected_start, text.len(), "pages must cover full text");
    }

    #[test]
    fn last_page_end_equals_text_len() {
        let cfg = fixed_cfg(10, 20, 100, 40);
        let text = "the quick brown fox jumps over the lazy dog";
        let layout = layout_chapter(text, &cfg);
        assert!(!layout.pages.is_empty());
        assert_eq!(layout.pages.last().unwrap().end, text.len());
    }

    // ── Edge cases ────────────────────────────────────────────────────────────

    #[test]
    fn empty_text_gives_one_empty_page() {
        // The engine always emits at least one page so the reader always has
        // a page to display; an empty-text page has start == end == 0.
        let cfg = fixed_cfg(10, 20, 100, 100);
        let layout = layout_chapter("", &cfg);
        assert_eq!(layout.pages.len(), 1);
        assert_eq!(layout.pages[0].start, 0);
        assert_eq!(layout.pages[0].end, 0);
    }

    #[test]
    fn leading_spaces_are_dropped() {
        // "   hello" — leading spaces on first line should not affect page count.
        let cfg = fixed_cfg(10, 20, 100, 50);
        let layout = layout_chapter("   hello", &cfg);
        assert_eq!(layout.pages.len(), 1);
    }

    #[test]
    fn zero_height_gives_single_page() {
        // Degenerate config: one page for everything.
        let cfg = fixed_cfg(10, 0, 100, 100);
        let layout = layout_chapter("hello world", &cfg);
        assert_eq!(layout.pages.len(), 1);
    }
}
