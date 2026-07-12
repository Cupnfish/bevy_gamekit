use anyhow::Result;
use bevy_minigame::cli::{PackageArgs, print_report};
use bevy_minigame_wechat::package::WeChatPlatform;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "bevy-minigame-wechat",
    version,
    about = "Package Bevy Wasm for WeChat Mini Game"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Convert a raw wasm32 build into a WeChat Mini Game project.
    Package(PackageArgs),
}

fn main() -> Result<()> {
    let Cli { command } = Cli::parse();
    match command {
        Commands::Package(args) => print_report(args.run(&WeChatPlatform)?),
    }
    Ok(())
}
