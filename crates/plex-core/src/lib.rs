//! L1 Plex core: protocols, state machine, command bus.
//!
//! UI runtimes, transports (MCP stdio), and CLIs are forbidden from touching
//! device state directly. Everything routes through `Command::dispatch`
//! with typed params, so the MCP server is a thin frame around one async
//! function. Phase 1 scaffold: kernel modules only (`error`, `creds`,
//! `bridge`, `llm`, `import`); domain modules (`plex.rs`, `Command`, full
//! `dispatch`) land in Phase 2 per `docs/CONVERSION_DOSSIER.md`.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

pub mod bridge;
pub mod creds;
pub mod error;
pub mod import;
pub mod llm;
