use anyhow::Result;
use bevy_minigame::cli::{PackageArgs, print_report};
use bevy_minigame_douyin::package::DouyinPlatform;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bevy-minigame-douyin",
    version,
    about = "Package Bevy Wasm for Douyin Mini Game"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a raw wasm32 build into a Douyin Mini Game project.
    Package(PackageArgs),
}

fn main() -> Result<()> {
    let Cli { command } = Cli::parse();
    match command {
        Commands::Package(args) => print_report(args.run(&DouyinPlatform)?),
    }
    Ok(())
}
