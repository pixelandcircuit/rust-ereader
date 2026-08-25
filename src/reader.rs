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

use crate::book::Book;
use crate::epub::EpubError;
use crate::layout::{layout_chapter, Layout, LayoutConfig};

/// Stateful e-reader: holds one chapter's text, its paginated layout, and the
/// current position expressed both as a page index and an anchor byte offset.
///
/// The anchor byte lets `relayout()` (called on font-size change) find the
/// page that contains the same sentence after re-pagination.
pub struct ReaderState {
    pub chapter_text: String,
    pub layout: Layout,
    pub current_page: usize,
    /// Byte offset into `chapter_text` of the first character of the current page.
    /// Survives `relayout()` — the new page is the one whose range contains this offset.
    pub anchor_byte: usize,
}

impl ReaderState {
    /// Create a new reader for `chapter_text` with the given layout config.
    pub fn new(chapter_text: String, cfg: &LayoutConfig) -> Self {
        let layout = layout_chapter(&chapter_text, cfg);
        Self {
            chapter_text,
            layout,
            current_page: 0,
            anchor_byte: 0,
        }
    }

    /// Re-paginate after a config change (e.g. font size). The current page is
    /// updated to the one containing `anchor_byte` so the reader lands on the
    /// same passage.
    pub fn relayout(&mut self, cfg: &LayoutConfig) {
        self.layout = layout_chapter(&self.chapter_text, cfg);
        self.current_page = self
            .layout
            .pages
            .iter()
            .position(|p| p.start <= self.anchor_byte && self.anchor_byte < p.end)
            .unwrap_or(0);
    }

    /// Turn one page forward or backward. No-op at the first/last page.
    pub fn turn_page(&mut self, forward: bool) {
        if forward {
            if self.current_page + 1 < self.layout.pages.len() {
                self.current_page += 1;
            }
        } else if self.current_page > 0 {
            self.current_page -= 1;
        }
        if let Some(p) = self.layout.pages.get(self.current_page) {
            self.anchor_byte = p.start;
        }
    }

    /// The slice of `chapter_text` that belongs to the current page.
    pub fn current_text(&self) -> &str {
        match self.layout.pages.get(self.current_page) {
            Some(p) => &self.chapter_text[p.start..p.end],
            None => "",
        }
    }

    /// Total number of pages in the current layout.
    pub fn page_count(&self) -> usize {
        self.layout.pages.len()
    }

    /// Jump directly to a page by index (clamped to valid range).
    pub fn go_to_page(&mut self, page: usize) {
        self.current_page = page.min(self.layout.pages.len().saturating_sub(1));
        if let Some(p) = self.layout.pages.get(self.current_page) {
            self.anchor_byte = p.start;
        }
    }
}

// ── BookSession ───────────────────────────────────────────────────────────────

/// Combines the book's spine (chapter list) with the current chapter and page
/// position. Wraps [`ReaderState`] and adds inter-chapter navigation.
///
/// Use `session.reader` to access page-level operations (`turn_page`,
/// `current_text`, `relayout`, etc.). Save `session.chapter_idx` and
/// `session.reader.anchor_byte` to persist the reading position.
#[derive(DebugInspect)]
pub struct BookSession {
    pub chapter_idx: usize,
    #[inspect]
    pub reader: ReaderState,
    spine: Vec<String>,
}

impl BookSession {
    /// Open the EPUB and load the first chapter.
    pub fn new(epub: &dyn Book, cfg: &LayoutConfig) -> Result<Self, EpubError> {
        let spine = epub.spine()?;
        if spine.is_empty() {
            return Err(EpubError::MissingSpine);
        }
        let text = epub.chapter_text(&spine[0])?;
        Ok(Self {
            chapter_idx: 0,
            reader: ReaderState::new(text, cfg),
            spine,
        })
    }

    /// Restore a previously saved position. Loads `chapter_idx` and seeks to
    /// the page containing `anchor_byte` without re-running layout twice.
    pub fn restore(
        epub: &dyn Book,
        cfg: &LayoutConfig,
        chapter_idx: usize,
        anchor_byte: usize,
    ) -> Result<Self, EpubError> {
        let spine = epub.spine()?;
        if spine.is_empty() {
            return Err(EpubError::MissingSpine);
        }
        let idx = chapter_idx.min(spine.len().saturating_sub(1));
        let text = epub.chapter_text(&spine[idx])?;
        let mut reader = ReaderState::new(text, cfg);
        reader.anchor_byte = anchor_byte;
        reader.current_page = reader
            .layout
            .pages
            .iter()
            .position(|p| p.start <= anchor_byte && anchor_byte < p.end)
            .unwrap_or(0);
        Ok(Self {
            chapter_idx: idx,
            reader,
            spine,
        })
    }

    /// Load the chapter at `idx` (clamped to the spine length).
    /// Resets the reader to the first page of that chapter.
    pub fn go_to_chapter(
        &mut self,
        idx: usize,
        epub: &dyn Book,
        cfg: &LayoutConfig,
    ) -> Result<(), EpubError> {
        let idx = idx.min(self.spine.len().saturating_sub(1));
        let text = epub.chapter_text(&self.spine[idx])?;
        self.chapter_idx = idx;
        self.reader = ReaderState::new(text, cfg);
        Ok(())
    }

    /// Advance to the next chapter. Returns `false` if already at the last.
    pub fn next_chapter(&mut self, epub: &dyn Book, cfg: &LayoutConfig) -> Result<bool, EpubError> {
        if self.chapter_idx + 1 >= self.spine.len() {
            return Ok(false);
        }
        self.go_to_chapter(self.chapter_idx + 1, epub, cfg)?;
        Ok(true)
    }

    /// Return to the previous chapter. Returns `false` if already at the first.
    pub fn prev_chapter(&mut self, epub: &dyn Book, cfg: &LayoutConfig) -> Result<bool, EpubError> {
        if self.chapter_idx == 0 {
            return Ok(false);
        }
        self.go_to_chapter(self.chapter_idx - 1, epub, cfg)?;
        Ok(true)
    }

    /// Total number of chapters in the spine.
    pub fn chapter_count(&self) -> usize {
        self.spine.len()
    }

    /// The spine paths in order.
    pub fn spine(&self) -> &[String] {
        &self.spine
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{HtmlBook, TxtBook};
    use crate::layout::FontMetrics;

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
            bold_font: None,
            italic_font: None,
            heading_font: None,
        }
    }

    // ── ReaderState ───────────────────────────────────────────────────────────

    #[test]
    fn single_page_text_has_one_page() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let rs = ReaderState::new("hello world".into(), &cfg);
        assert_eq!(rs.page_count(), 1);
    }

    #[test]
    fn long_text_splits_into_multiple_pages() {
        // 10 px/char, 100 px wide (10 chars/line), 40 px tall (2 lines/page).
        // 8 single-word lines → 4 pages.
        let cfg = fixed_cfg(10, 20, 100, 40);
        let text = "aaaaaaaa bbbbbbbb cccccccc dddddddd eeeeeeee ffffffff gggggggg hhhhhhhh";
        let rs = ReaderState::new(text.into(), &cfg);
        assert!(rs.page_count() >= 2);
    }

    #[test]
    fn turn_page_forward_advances() {
        let cfg = fixed_cfg(10, 20, 100, 40);
        let text = "aaaaaaaa bbbbbbbb cccccccc dddddddd";
        let mut rs = ReaderState::new(text.into(), &cfg);
        assert!(rs.page_count() >= 2);
        rs.turn_page(true);
        assert_eq!(rs.current_page, 1);
    }

    #[test]
    fn turn_page_backward_goes_back() {
        let cfg = fixed_cfg(10, 20, 100, 40);
        let text = "aaaaaaaa bbbbbbbb cccccccc dddddddd";
        let mut rs = ReaderState::new(text.into(), &cfg);
        rs.turn_page(true);
        rs.turn_page(false);
        assert_eq!(rs.current_page, 0);
    }

    #[test]
    fn turn_page_clamps_at_last() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let mut rs = ReaderState::new("hello".into(), &cfg);
        let last = rs.page_count() - 1;
        rs.turn_page(true);
        rs.turn_page(true); // already at last
        assert_eq!(rs.current_page, last);
    }

    #[test]
    fn turn_page_clamps_at_first() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let mut rs = ReaderState::new("hello".into(), &cfg);
        rs.turn_page(false); // already at first
        assert_eq!(rs.current_page, 0);
    }

    #[test]
    fn current_text_matches_page_slice() {
        let cfg = fixed_cfg(10, 20, 100, 40);
        let text = "aaaaaaaa bbbbbbbb cccccccc dddddddd";
        let mut rs = ReaderState::new(text.to_string(), &cfg);
        rs.turn_page(true);
        let page = &rs.layout.pages[rs.current_page];
        assert_eq!(rs.current_text(), &text[page.start..page.end]);
    }

    #[test]
    fn relayout_preserves_anchor_byte() {
        let cfg = fixed_cfg(10, 20, 100, 40);
        let text = "aaaaaaaa bbbbbbbb cccccccc dddddddd eeeeeeee ffffffff";
        let mut rs = ReaderState::new(text.into(), &cfg);
        rs.turn_page(true);
        let saved_anchor = rs.anchor_byte;
        // Re-layout with a wider screen (fewer pages).
        let wide_cfg = fixed_cfg(10, 20, 200, 200);
        rs.relayout(&wide_cfg);
        // The page should contain the saved anchor byte.
        let page = &rs.layout.pages[rs.current_page];
        assert!(
            page.start <= saved_anchor && saved_anchor <= page.end,
            "anchor {saved_anchor} not within page {}..{}",
            page.start,
            page.end
        );
    }

    #[test]
    fn go_to_page_clamps_to_last() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let mut rs = ReaderState::new("hello".into(), &cfg);
        rs.go_to_page(999);
        assert_eq!(rs.current_page, rs.page_count() - 1);
    }

    // ── BookSession with TxtBook ──────────────────────────────────────────────

    #[test]
    fn book_session_opens_txt_book() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = TxtBook::from_vec(b"hello world".to_vec());
        let session = BookSession::new(&book, &cfg).unwrap();
        assert_eq!(session.chapter_count(), 1);
        assert_eq!(session.chapter_idx, 0);
    }

    #[test]
    fn book_session_txt_current_text_contains_content() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = TxtBook::from_vec(b"hello world".to_vec());
        let session = BookSession::new(&book, &cfg).unwrap();
        assert!(session.reader.current_text().contains("hello"));
    }

    #[test]
    fn book_session_next_chapter_false_for_single_chapter() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let mut book_session = {
            let book = TxtBook::from_vec(b"hello".to_vec());
            BookSession::new(&book, &cfg).unwrap()
        };
        let book = TxtBook::from_vec(b"hello".to_vec());
        let advanced = book_session.next_chapter(&book, &cfg).unwrap();
        assert!(!advanced);
    }

    #[test]
    fn book_session_prev_chapter_false_at_first() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = TxtBook::from_vec(b"hello".to_vec());
        let mut session = BookSession::new(&book, &cfg).unwrap();
        let went_back = session.prev_chapter(&book, &cfg).unwrap();
        assert!(!went_back);
    }

    // ── BookSession with HtmlBook ─────────────────────────────────────────────

    #[test]
    fn book_session_opens_html_book() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = HtmlBook::from_vec(b"<p>hello world</p>".to_vec());
        let session = BookSession::new(&book, &cfg).unwrap();
        assert_eq!(session.chapter_count(), 1);
        assert!(session.reader.current_text().contains("hello"));
    }

    // ── BookSession with multi-chapter MockBook ───────────────────────────────

    struct MockBook {
        chapters: Vec<(&'static str, &'static str)>,
    }

    impl Book for MockBook {
        fn spine(&self) -> Result<Vec<String>, EpubError> {
            Ok(self.chapters.iter().map(|(id, _)| id.to_string()).collect())
        }
        fn chapter_text(&self, id: &str) -> Result<String, EpubError> {
            self.chapters
                .iter()
                .find(|(ch_id, _)| *ch_id == id)
                .map(|(_, text)| text.to_string())
                .ok_or(EpubError::EntryNotFound)
        }
    }

    fn two_chapter_book() -> MockBook {
        MockBook {
            chapters: vec![("ch1", "Chapter one text."), ("ch2", "Chapter two text.")],
        }
    }

    #[test]
    fn next_chapter_advances_chapter_idx() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = two_chapter_book();
        let mut session = BookSession::new(&book, &cfg).unwrap();
        let advanced = session.next_chapter(&book, &cfg).unwrap();
        assert!(advanced);
        assert_eq!(session.chapter_idx, 1);
    }

    #[test]
    fn next_chapter_loads_new_text() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = two_chapter_book();
        let mut session = BookSession::new(&book, &cfg).unwrap();
        session.next_chapter(&book, &cfg).unwrap();
        assert!(session.reader.current_text().contains("two"));
    }

    #[test]
    fn next_chapter_false_at_last() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = two_chapter_book();
        let mut session = BookSession::new(&book, &cfg).unwrap();
        session.next_chapter(&book, &cfg).unwrap();
        let advanced = session.next_chapter(&book, &cfg).unwrap();
        assert!(!advanced);
    }

    #[test]
    fn prev_chapter_goes_back() {
        let cfg = fixed_cfg(10, 20, 200, 100);
        let book = two_chapter_book();
        let mut session = BookSession::new(&book, &cfg).unwrap();
        session.next_chapter(&book, &cfg).unwrap();
        let went_back = session.prev_chapter(&book, &cfg).unwrap();
        assert!(went_back);
        assert_eq!(session.chapter_idx, 0);
        assert!(session.reader.current_text().contains("one"));
    }

    #[test]
    fn restore_positions_to_correct_page() {
        let cfg = fixed_cfg(10, 20, 100, 40);
        let book = MockBook {
            chapters: vec![(
                "ch1",
                "aaaaaaaa bbbbbbbb cccccccc dddddddd eeeeeeee ffffffff",
            )],
        };
        // First create a session and advance one page to get an anchor.
        let mut session = BookSession::new(&book, &cfg).unwrap();
        session.reader.turn_page(true);
        let anchor = session.reader.anchor_byte;
        // Restore to that anchor.
        let restored = BookSession::restore(&book, &cfg, 0, anchor).unwrap();
        assert_eq!(restored.chapter_idx, 0);
        assert_eq!(restored.reader.anchor_byte, anchor);
        assert!(restored.reader.current_page > 0);
    }
}
