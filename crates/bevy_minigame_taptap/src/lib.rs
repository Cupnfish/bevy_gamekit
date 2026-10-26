//! TapTap H5 platform backend for `bevy_minigame`.
//!
//! TapTap H5 games run as standard web pages under TapTap's own domain, so
//! unlike the Douyin/WeChat backends there is no DOM-less host to emulate: the
//! packaging pipeline emits a plain `wasm-bindgen --target web` module plus a
//! generated `index.html`, and `glue.js` implements the `BevyMiniGame` surface
//! over browser APIs (`localStorage`, `navigator`, `visibilitychange`) with
//! the TapTap H5 SDK integration (`TapBridge`: rewarded ads, audio, build
//! info) implemented over the runtime-injected `tap` global with
//! local-development fallbacks.

pub use bevy_minigame::*;

pub mod bridge;

#[cfg(feature = "bevy")]
pub mod ads;

#[cfg(feature = "cli")]
pub mod package;
