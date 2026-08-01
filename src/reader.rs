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

use crate::epub::{EpubArchive, EpubError};
use crate::layout::{layout_chapter, Layout, LayoutConfig};

/// Stateful e-reader: holds one chapter's text, its paginated layout, and the
/// current position expressed both as a page index and an anchor byte offset.
///
/// The anchor byte lets `relayout()` (called on font-size change) find the
/// page that contains the same sentence after re-pagination.
pub struct ReaderState {
    pub chapter_text: String,
    pub layout:       Layout,
    pub current_page: usize,
    /// Byte offset into `chapter_text` of the first character of the current page.
    /// Survives `relayout()` — the new page is the one whose range contains this offset.
    pub anchor_byte:  usize,
}

impl ReaderState {
    /// Create a new reader for `chapter_text` with the given layout config.
    pub fn new(chapter_text: String, cfg: &LayoutConfig) -> Self {
        let layout = layout_chapter(&chapter_text, cfg);
        Self {
            chapter_text,
            layout,
            current_page: 0,
            anchor_byte:  0,
        }
    }

    /// Re-paginate after a config change (e.g. font size). The current page is
    /// updated to the one containing `anchor_byte` so the reader lands on the
    /// same passage.
    pub fn relayout(&mut self, cfg: &LayoutConfig) {
        self.layout = layout_chapter(&self.chapter_text, cfg);
        self.current_page = self.layout.pages
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
            None    => "",
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
pub struct BookSession {
    pub chapter_idx: usize,
    pub reader: ReaderState,
    spine: Vec<String>,
}

impl BookSession {
    /// Open the EPUB and load the first chapter.
    pub fn new(epub: &EpubArchive, cfg: &LayoutConfig) -> Result<Self, EpubError> {
        let spine = epub.spine()?;
        if spine.is_empty() { return Err(EpubError::MissingSpine); }
        let text = epub.chapter_text(&spine[0])?;
        Ok(Self { chapter_idx: 0, reader: ReaderState::new(text, cfg), spine })
    }

    /// Restore a previously saved position. Loads `chapter_idx` and seeks to
    /// the page containing `anchor_byte` without re-running layout twice.
    pub fn restore(
        epub:        &EpubArchive,
        cfg:         &LayoutConfig,
        chapter_idx: usize,
        anchor_byte: usize,
    ) -> Result<Self, EpubError> {
        let spine = epub.spine()?;
        if spine.is_empty() { return Err(EpubError::MissingSpine); }
        let idx  = chapter_idx.min(spine.len().saturating_sub(1));
        let text = epub.chapter_text(&spine[idx])?;
        let mut reader = ReaderState::new(text, cfg);
        reader.anchor_byte = anchor_byte;
        reader.current_page = reader.layout.pages
            .iter()
            .position(|p| p.start <= anchor_byte && anchor_byte < p.end)
            .unwrap_or(0);
        Ok(Self { chapter_idx: idx, reader, spine })
    }

    /// Load the chapter at `idx` (clamped to the spine length).
    /// Resets the reader to the first page of that chapter.
    pub fn go_to_chapter(
        &mut self,
        idx:  usize,
        epub: &EpubArchive,
        cfg:  &LayoutConfig,
    ) -> Result<(), EpubError> {
        let idx  = idx.min(self.spine.len().saturating_sub(1));
        let text = epub.chapter_text(&self.spine[idx])?;
        self.chapter_idx = idx;
        self.reader = ReaderState::new(text, cfg);
        Ok(())
    }

    /// Advance to the next chapter. Returns `false` if already at the last.
    pub fn next_chapter(
        &mut self,
        epub: &EpubArchive,
        cfg:  &LayoutConfig,
    ) -> Result<bool, EpubError> {
        if self.chapter_idx + 1 >= self.spine.len() { return Ok(false); }
        self.go_to_chapter(self.chapter_idx + 1, epub, cfg)?;
        Ok(true)
    }

    /// Return to the previous chapter. Returns `false` if already at the first.
    pub fn prev_chapter(
        &mut self,
        epub: &EpubArchive,
        cfg:  &LayoutConfig,
    ) -> Result<bool, EpubError> {
        if self.chapter_idx == 0 { return Ok(false); }
        self.go_to_chapter(self.chapter_idx - 1, epub, cfg)?;
        Ok(true)
    }

    /// Total number of chapters in the spine.
    pub fn chapter_count(&self) -> usize { self.spine.len() }

    /// The spine paths in order.
    pub fn spine(&self) -> &[String] { &self.spine }
}
