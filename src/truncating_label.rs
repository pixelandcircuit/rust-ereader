#[cfg(feature = "esp")]
extern crate alloc;

#[cfg(feature = "esp")]
use alloc::{string::String};

use embedded_graphics_core::pixelcolor::Rgb565;
use iris_ui::view::Flex::{Grow, Shrink};
use iris_ui::view::{View, ViewId};

pub fn make_truncating_label(name: &ViewId, title: &str) -> View<Rgb565> {
    View {
        name: name.clone(),
        title: title.into(),
        h_flex: Grow,
        v_flex: Shrink,
        layout: Some(|e| {
            let space_w = e.space.w;
            if let Some(view) = e.scene.get_view_mut(e.target) {
                let font = e.theme.font;
                let ch = font.char_height();
                view.bounds.size.w = space_w;
                view.bounds.size.h = ch + (ch / 2) * 2;
            }
        }),
        draw: Some(|e| {
            let font = e.theme.font;
            let style = iris_ui::gfx::TextStyle::new(font, &e.theme.standard.text);
            let pad = font.char_width();
            let available = (e.view.bounds.size.w - pad * 2).max(0);
            if font.str_width(&e.view.title) <= available {
                e.ctx.fill_text(&e.view.bounds, &e.view.title, &style);
            } else {
                let ellipsis_w = font.str_width("...");
                let max_text_w = (available - ellipsis_w).max(0);
                let mut truncated = String::new();
                let mut used = 0i32;
                for c in e.view.title.chars() {
                    let mut buf = [0u8; 4];
                    let cw = font.str_width(c.encode_utf8(&mut buf));
                    if used + cw > max_text_w {
                        break;
                    }
                    truncated.push(c);
                    used += cw;
                }
                truncated.push_str("…");
                e.ctx.fill_text(&e.view.bounds, &truncated, &style);
            }
        }),
        ..Default::default()
    }
}
