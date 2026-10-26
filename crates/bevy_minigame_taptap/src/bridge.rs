//! Direct `window.TapBridge` calls, installed by the packaged `glue.js`
//! (loaded via script tag before the wasm module instantiates).
//!
//! On non-wasm targets every function is a safe no-op / default so game code
//! can call them unconditionally.

/// Packager-injected build number (`window.__GAME_BUILD`, inlined into
/// `index.html`). 0 in local preview and on native targets.
#[cfg(target_arch = "wasm32")]
pub fn build_number() -> u32 {
    bridge_js::build_number_js()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn build_number() -> u32 {
    0
}

/// Start the looping BGM through the glue's `HTMLAudioElement`.
///
/// Browser autoplay policy requires the first call to happen after a user
/// gesture; the glue keeps its own started state, so the call is idempotent.
#[cfg(target_arch = "wasm32")]
pub fn bgm_start() {
    bridge_js::bgm_start_js();
}

#[cfg(not(target_arch = "wasm32"))]
pub fn bgm_start() {}

#[cfg(target_arch = "wasm32")]
mod bridge_js {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(js_namespace = TapBridge)]
    extern "C" {
        // TapBridge is installed by glue.js before this module instantiates;
        // every member has a dev-mode fallback when the H5 SDK is absent.
        #[wasm_bindgen(js_name = buildNumber)]
        pub(super) fn build_number_js() -> u32;
        #[wasm_bindgen(js_name = bgmStart)]
        pub(super) fn bgm_start_js();
        #[wasm_bindgen(js_name = showRewarded)]
        pub(super) fn show_rewarded_js(callback: &JsValue);
    }
}

/// Raw callback entry used by [`crate::ads`]. `callback` receives a truthy
/// value when the reward was granted.
#[cfg(target_arch = "wasm32")]
pub(crate) fn show_rewarded_raw(callback: &wasm_bindgen::JsValue) {
    bridge_js::show_rewarded_js(callback);
}
