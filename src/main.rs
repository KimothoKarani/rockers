use clap::Parser;
use mini_docker::cli::{Cli, Command};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::Pull(args) => args.run().await,
        Command::Run(_) => anyhow::bail!("run subcommand is not implemented yet"),
    }
}
