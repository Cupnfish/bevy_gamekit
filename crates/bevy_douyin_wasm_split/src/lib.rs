//! Runtime split-point support and host-side packaging for Bevy Wasm builds.

#[cfg(feature = "runtime")]
pub use upstream_wasm_split::{
    LazyLoader, LazySplitLoader, Result, SplitLoaderError, lazy_loader, wasm_split,
};

#[cfg(feature = "cli")]
pub mod package;

#[cfg(all(feature = "decoder", target_arch = "wasm32"))]
use wasm_bindgen::prelude::*;

/// Brotli decoder compiled as a small, independent bootstrap Wasm module.
#[cfg(all(feature = "decoder", target_arch = "wasm32"))]
#[wasm_bindgen(js_name = decompress)]
pub fn decompress_brotli(input: Box<[u8]>) -> Result<Box<[u8]>, JsError> {
    let mut output = Vec::new();
    brotli_decompressor::BrotliDecompress(&mut input.as_ref(), &mut output)
        .map_err(|_| JsError::new("Brotli decompression failed"))?;
    Ok(output.into_boxed_slice())
}
