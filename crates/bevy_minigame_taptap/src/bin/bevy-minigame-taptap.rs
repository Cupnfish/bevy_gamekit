use anyhow::Result;
use bevy_minigame::cli::{PackageArgs, print_report};
use bevy_minigame_taptap::package::TapTapPlatform;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bevy-minigame-taptap",
    version,
    about = "Package Bevy Wasm for TapTap H5"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a raw wasm32 build into a TapTap H5 web package.
    Package(PackageArgs),
}

fn main() -> Result<()> {
    let Cli { command } = Cli::parse();
    match command {
        Commands::Package(args) => print_report(args.run(&TapTapPlatform)?),
    }
    Ok(())
}
