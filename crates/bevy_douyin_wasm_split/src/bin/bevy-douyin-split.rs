use std::path::PathBuf;

use anyhow::Result;
use bevy_douyin_wasm_split::package::{SplitOptions, split};
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "bevy-douyin-split",
    version,
    about = "Split and compress a wasm-bindgen Bevy build"
)]
struct Cli {
    /// Cargo-produced Wasm containing relocation metadata.
    #[arg(long)]
    original: PathBuf,
    /// Wasm emitted by wasm-bindgen. This file is replaced with the split main module.
    #[arg(long)]
    bindgen_wasm: PathBuf,
    /// JavaScript emitted by wasm-bindgen. Split imports are injected into this file.
    #[arg(long)]
    bindgen_js: PathBuf,
    /// Directory that receives the split runtime and compressed modules.
    #[arg(long)]
    output: PathBuf,
    /// Raw Cargo-built decoder Wasm from this crate's `decoder` feature.
    #[arg(long)]
    decoder_wasm: PathBuf,
    /// wasm-bindgen executable path.
    #[arg(long, default_value = "wasm-bindgen")]
    wasm_bindgen: PathBuf,
    /// Optional wasm-opt executable path. Omit to preserve emitted module bytes.
    #[arg(long)]
    wasm_opt: Option<PathBuf>,
    /// Brotli quality from 1 through 11.
    #[arg(long, default_value_t = 11)]
    compression_quality: u32,
    /// Fail if the compressed main module exceeds this many MiB.
    #[arg(long)]
    max_main_mib: Option<u64>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let report = split(&SplitOptions {
        original_wasm: cli.original,
        bindgen_wasm: cli.bindgen_wasm,
        bindgen_js: cli.bindgen_js,
        output: cli.output,
        decoder_wasm: cli.decoder_wasm,
        wasm_bindgen: cli.wasm_bindgen,
        wasm_opt: cli.wasm_opt,
        compression_quality: cli.compression_quality,
        max_main_bytes: cli.max_main_mib.map(|value| value * 1024 * 1024),
    })?;
    println!(
        "Split main {:.2} MiB -> {:.2} MiB; {} compressed modules {:.2} MiB; decoder {:.2} MiB",
        report.main_bytes as f64 / (1024.0 * 1024.0),
        report.compressed_main_bytes as f64 / (1024.0 * 1024.0),
        report.modules.len(),
        report.compressed_module_bytes as f64 / (1024.0 * 1024.0),
        report.decoder_bytes as f64 / (1024.0 * 1024.0),
    );
    Ok(())
}
