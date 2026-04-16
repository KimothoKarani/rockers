use clap::{Args, Parser, Subcommand};

pub mod pull;
pub mod run;

#[derive(Debug, Parser)]
#[command(name = "mini-docker", about = "Pull and run container images")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Download an image from a registry
    Pull(PullArgs),
    /// Create and run a new container from an image
    Run(RunArgs),
}

#[derive(Debug, Args, Clone)]
pub struct PullArgs {
    pub image: String,
}

#[derive(Debug, Args, Clone)]
pub struct RunArgs {
    pub image: String,
    pub command: String,
}
