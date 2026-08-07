//! Smoke test for the EPUB library modules: epub, layout, reader.
//!
//! Run on ESP device:  cargo run --example epub_test
//! Run locally (std):  cargo run --example epub_test --no-default-features --target aarch64-apple-darwin

#![cfg_attr(feature = "esp", no_std)]
#![cfg_attr(feature = "esp", no_main)]

#[cfg(feature = "esp")]
extern crate alloc;

#[cfg(feature = "esp")]
use esp_backtrace as _;
#[cfg(feature = "esp")]
use esp_hal::{clock::CpuClock, main};
#[cfg(feature = "esp")]
use esp_println::println;

#[cfg(feature = "esp")]
esp_bootloader_esp_idf::esp_app_desc!();

use ereader::epub::EpubArchive;
use ereader::layout::{FontMetrics, LayoutConfig};
use ereader::reader::ReaderState;

const EPUB_DATA: &[u8] = include_bytes!("test.epub");

fn fixed_measure(s: &str) -> u32 {
    s.chars().count() as u32 * 10
}

fn run_tests() {
    println!("=== epub_test ===");

    // ── 1. Parse the EPUB archive ────────────────────────────────────────────
    let archive = EpubArchive::new(EPUB_DATA).expect("EpubArchive::new failed");
    let spine = archive.spine().expect("spine() failed");
    println!("Spine ({} chapter(s)):", spine.len());
    for (i, path) in spine.iter().enumerate() {
        println!("  [{}] {}", i, path);
    }

    // ── 2. Extract and strip chapter 1 ──────────────────────────────────────
    let text = archive
        .chapter_text(&spine[0])
        .expect("chapter_text failed");
    println!("\nChapter 1 plain text ({} bytes):", text.len());
    let preview_end = text
        .char_indices()
        .nth(400)
        .map(|(i, _)| i)
        .unwrap_or(text.len());
    println!("{}", &text[..preview_end]);
    if text.len() > preview_end {
        println!("... [{} bytes more]", text.len() - preview_end);
    }

    // ── 3. Lay out chapter 1 with fixed-width metrics ────────────────────────
    let cfg = LayoutConfig {
        screen_width: 960,
        screen_height: 540,
        margin_x: 40,
        margin_y: 30,
        font: FontMetrics {
            line_height_px: 28,
            space_width_px: 10,
            measure: Box::new(fixed_measure),
        },
        heading_font: None,
    };

    let mut reader = ReaderState::new(text, &cfg);
    println!("\nLayout: {} pages", reader.page_count());
    println!("Page 1 of {}:", reader.page_count());
    println!("---");
    println!("{}", reader.current_text());
    println!("---");

    // ── 4. Turn pages forward through all pages ──────────────────────────────
    let total = reader.page_count();
    for p in 1..total {
        reader.turn_page(true);
        println!("\nPage {} of {}:", p + 1, total);
        println!("---");
        println!("{}", reader.current_text());
        println!("---");
    }

    // ── 5. Simulate a font-size change via relayout ──────────────────────────
    let larger_cfg = LayoutConfig {
        screen_width: 960,
        screen_height: 540,
        margin_x: 40,
        margin_y: 30,
        font: FontMetrics {
            line_height_px: 36,
            space_width_px: 13,
            measure: Box::new(|s: &str| s.chars().count() as u32 * 13),
        },
        heading_font: None,
    };
    reader.go_to_page(0);
    reader.anchor_byte = reader
        .layout
        .pages
        .get(1)
        .map(|p| p.start)
        .unwrap_or_else(|| (reader.chapter_text.len() / 2).min(200));
    reader.relayout(&larger_cfg);
    println!(
        "\nAfter relayout (larger font): {} pages, landed on page {}",
        reader.page_count(),
        reader.current_page + 1,
    );

    // ── 6. Chapter 2 ─────────────────────────────────────────────────────────
    let text2 = archive.chapter_text(&spine[1]).expect("chapter 2 failed");
    println!("\nChapter 2 plain text ({} bytes):", text2.len());
    let end2 = text2
        .char_indices()
        .nth(300)
        .map(|(i, _)| i)
        .unwrap_or(text2.len());
    println!("{}", &text2[..end2]);

    println!("\n=== epub_test PASSED ===");
}

#[cfg(not(feature = "esp"))]
fn main() {
    run_tests();
}

#[cfg(feature = "esp")]
#[main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let _p = esp_hal::init(config);
    esp_alloc::heap_allocator!(size: 65536);
    run_tests();
    loop {}
}
