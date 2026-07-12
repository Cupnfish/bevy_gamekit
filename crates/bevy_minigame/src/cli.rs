use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, ValueEnum};

use crate::package::{self, MiniGamePlatform, Orientation, PackageOptions, PackageReport};

/// Shared command-line arguments for platform packaging frontends.
#[derive(Args)]
pub struct PackageArgs {
    /// Raw Cargo-produced Wasm input.
    #[arg(long)]
    pub wasm: PathBuf,
    /// Asset directory copied to the platform's configured asset root.
    #[arg(long)]
    pub assets: Option<PathBuf>,
    /// Generated Mini Game project directory.
    #[arg(long)]
    pub output: PathBuf,
    /// Platform application id or a local-test id.
    #[arg(long)]
    pub app_id: String,
    /// Project name shown by developer tooling.
    #[arg(long)]
    pub project_name: String,
    /// Initial device orientation.
    #[arg(long, value_enum, default_value = "landscape")]
    pub orientation: CliOrientation,
    /// Path or command name for the wasm-bindgen CLI.
    #[arg(long, default_value = "wasm-bindgen")]
    pub wasm_bindgen: PathBuf,
    /// Asset file extension to omit. May be repeated.
    #[arg(long = "exclude-extension")]
    pub exclude_extensions: Vec<String>,
    /// Raw decoder Wasm built from `bevy_minigame_wasm_split`.
    #[arg(long)]
    pub wasm_split_decoder: Option<PathBuf>,
    /// wasm-opt executable used to optimize split modules.
    #[arg(long)]
    pub wasm_opt: Option<PathBuf>,
    /// Brotli quality used for the main module and child modules.
    #[arg(long, default_value_t = 11)]
    pub compression_quality: u32,
    /// Fail packaging when the generated project exceeds this many MiB.
    #[arg(long)]
    pub max_total_mib: Option<u64>,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum CliOrientation {
    Landscape,
    Portrait,
}

impl PackageArgs {
    pub fn run(self, platform: &dyn MiniGamePlatform) -> Result<PackageReport> {
        package::package(
            platform,
            &PackageOptions {
                wasm: self.wasm,
                assets: self.assets,
                output: self.output,
                app_id: self.app_id,
                project_name: self.project_name,
                orientation: match self.orientation {
                    CliOrientation::Landscape => Orientation::Landscape,
                    CliOrientation::Portrait => Orientation::Portrait,
                },
                wasm_bindgen: self.wasm_bindgen,
                exclude_extensions: self.exclude_extensions,
                wasm_split_decoder: self.wasm_split_decoder,
                wasm_opt: self.wasm_opt,
                compression_quality: self.compression_quality,
                max_total_bytes: self.max_total_mib.map(|value| value * 1024 * 1024),
            },
        )
    }
}

pub fn print_report(report: PackageReport) {
    println!("Generated {}", report.output.display());
    println!(
        "Wasm {:.1} MiB + {:.1} MiB compressed, assets {:.1} MiB, total {:.1} MiB",
        report.wasm_bytes as f64 / (1024.0 * 1024.0),
        report.compressed_wasm_bytes as f64 / (1024.0 * 1024.0),
        report.asset_bytes as f64 / (1024.0 * 1024.0),
        report.total_bytes as f64 / (1024.0 * 1024.0)
    );
    if report.subpackage_bytes > 0 {
        println!(
            "Main package {:.1} MiB, runtime subpackage {:.1} MiB",
            report.main_package_bytes as f64 / (1024.0 * 1024.0),
            report.subpackage_bytes as f64 / (1024.0 * 1024.0),
        );
    }
    for warning in report.warnings {
        eprintln!("warning: {warning}");
    }
}
