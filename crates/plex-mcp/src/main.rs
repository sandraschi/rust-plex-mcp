//! L2 binary: CLI-first + MCP `mcp` subcommand over stdio (Phase 1 scaffold).
//!
//! Stdout is pure JSON-RPC. Every log line goes to stderr via `eprintln!`.
//! Phase 1 exposes only `status` (identity reporting — the one honest thing
//! a scaffold can do). Full CLI subcommands + `tools/list` dispatch land in
//! Phase 3 per `docs/CONVERSION_DOSSIER.md`.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use clap::{Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(
    name = "rust-plex-mcp",
    about = "Pure-Rust Plex Media Server control (CLI-first, MCP over stdio)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show app status (human or --json for agents)
    Status {
        #[arg(long)]
        json: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { json: _ } => {
            println!(
                "{}",
                json!({
                    "app": "rust-plex-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                    "phase": "1-scaffold",
                    "next": "Phase 2 crates/plex-core (Command bus + Plex REST)",
                })
            );
        }
    }
}
