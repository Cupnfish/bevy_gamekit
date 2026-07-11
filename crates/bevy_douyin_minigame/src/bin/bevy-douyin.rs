use std::path::PathBuf;

use anyhow::Result;
use bevy_douyin_minigame::package::{self, Orientation, PackageOptions};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "bevy-douyin",
    version,
    about = "Package Bevy Wasm for Douyin Mini Game"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a raw wasm32 build and copy assets into a Mini Game project.
    Package {
        /// Raw Cargo-produced Wasm input.
        #[arg(long)]
        wasm: PathBuf,
        /// Asset directory copied to `assets/`.
        #[arg(long)]
        assets: Option<PathBuf>,
        /// Generated Douyin Mini Game project directory.
        #[arg(long, default_value = "dist-douyin")]
        output: PathBuf,
        /// Douyin application id or `testAppId` for local import.
        #[arg(long, default_value = "testAppId")]
        app_id: String,
        /// Project name shown by developer tooling.
        #[arg(long, default_value = "bevy-douyin-minigame")]
        project_name: String,
        /// Initial device orientation.
        #[arg(long, value_enum, default_value = "landscape")]
        orientation: CliOrientation,
        /// Path or command name for the wasm-bindgen CLI.
        #[arg(long, default_value = "wasm-bindgen")]
        wasm_bindgen: PathBuf,
        /// Asset file extension to omit. May be repeated, for example `--exclude-extension ogg`.
        #[arg(long = "exclude-extension")]
        exclude_extensions: Vec<String>,
        /// Raw decoder Wasm built from `bevy_douyin_wasm_split` with its `decoder` feature.
        #[arg(long)]
        wasm_split_decoder: Option<PathBuf>,
        /// wasm-opt executable used to optimize the split main and child modules.
        #[arg(long)]
        wasm_opt: Option<PathBuf>,
        /// Brotli quality used for child modules.
        #[arg(long, default_value_t = 11)]
        compression_quality: u32,
        /// Fail packaging when the generated project exceeds this many MiB.
        #[arg(long)]
        max_total_mib: Option<u64>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum CliOrientation {
    Landscape,
    Portrait,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Package {
            wasm,
            assets,
            output,
            app_id,
            project_name,
            orientation,
            wasm_bindgen,
            exclude_extensions,
            wasm_split_decoder,
            wasm_opt,
            compression_quality,
            max_total_mib,
        } => {
            let report = package::package(&PackageOptions {
                wasm,
                assets,
                output,
                app_id,
                project_name,
                orientation: match orientation {
                    CliOrientation::Landscape => Orientation::Landscape,
                    CliOrientation::Portrait => Orientation::Portrait,
                },
                wasm_bindgen,
                exclude_extensions,
                wasm_split_decoder,
                wasm_opt,
                compression_quality,
                max_total_bytes: max_total_mib.map(|value| value * 1024 * 1024),
            })?;
            println!("Generated {}", report.output.display());
            println!(
                "Wasm {:.1} MiB + {:.1} MiB compressed, assets {:.1} MiB, total {:.1} MiB",
                report.wasm_bytes as f64 / (1024.0 * 1024.0),
                report.compressed_wasm_bytes as f64 / (1024.0 * 1024.0),
                report.asset_bytes as f64 / (1024.0 * 1024.0),
                report.total_bytes as f64 / (1024.0 * 1024.0)
            );
            for warning in report.warnings {
                eprintln!("warning: {warning}");
            }
        }
    }
    Ok(())
}
