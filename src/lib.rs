#![cfg_attr(feature = "esp", no_std)]

// `embedded-inspect-derive`'s generated code references `::alloc::string::String`
// unconditionally for `String`-typed inspected fields/command params, so `alloc`
// must be linked even under the (std) `simulator` build once `debug-inspect` is
// enabled — harmless/idempotent alongside the `esp` feature, which already needs it.
#[cfg(any(feature = "esp", feature = "debug-inspect"))]
extern crate alloc;
pub mod appstate;
pub mod book;
pub mod bookview;
#[cfg(feature = "esp")]
pub mod driver;
pub mod epub;
pub mod font;
pub mod h_spacer;
pub mod hardware;
#[cfg(feature = "debug-inspect")]
pub mod inspect_shared;
#[cfg(all(feature = "esp", feature = "debug-inspect"))]
pub mod inspect_esp;
#[cfg(all(feature = "simulator", feature = "debug-inspect"))]
pub mod inspect_sim;
pub mod layout;
pub mod reader;
pub mod truncating_label;
pub mod fast_paging;
