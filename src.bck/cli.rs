use clap::Parser;

#[derive(Parser)]
#[command(name = "scanner")]
#[command(about = "Solana transaction scanner via Yellowstone gRPC", long_about = None)]
pub struct Cli {
    /// Path to config file (TOML or JSON)
    #[arg(default_value = "config.toml")]
    pub config: String,

    /// Run in terminal-only mode without dashboard (logs to stdout)
    #[arg(long)]
    pub term: bool,
}
