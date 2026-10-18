# Architecture & Engine Design — `rust-plex-mcp`

This document details the internal architecture, async command bus, wire purity, and workspace layering for `rust-plex-mcp`.

## 1. The 3-Crate ArtCraft Seam

```text
rust-plex-mcp/
├── Cargo.toml                  # Virtual workspace + #![deny(unwrap_used, expect_used, panic)]
├── justfile                    # check / test / smoke / release recipes
├── AGENTS.md                   # ArtCraft containment harness
└── crates/
    ├── plex-core/              # L1: Pure domain library, REST client, dispatch bus
    ├── plex-mcp/               # L2: CLI binary + MCP 2024-11-05 JSON-RPC stdio server
    └── plex-app/               # L3: Native egui 0.37 desktop application
```

| Layer | Crate | Dependencies | Responsibility |
|---|---|---|---|
| **L1 Core** | `crates/plex-core` | `reqwest`, `serde`, `tokio`, `uuid` | Outbound Plex REST client with token auth, fallback fixtures, LLM chat client, and universal `Command::dispatch`. |
| **L2 CLI + MCP** | `crates/plex-mcp` | `clap`, `serde_json`, `plex-core` | CLI subcommands + stdin/stdout JSON-RPC loop (`mcp`). Dual-aliased routing for all 36 MCP tools. Clean stdio wire purity. |
| **L3 Desktop GUI** | `crates/plex-app` | `eframe`, `egui 0.37`, `plex-core` | Native GPU-rendered UI. Calls `plex_core::dispatch` in-process via an async background worker channel (`mpsc`), zero HTTP overhead. |

---

## 2. Universal Command Bus (`Command::dispatch`)

All 36 capabilities are defined as variants of `plex_core::Command`:

```rust
pub enum Command {
    Status,
    ServerStatus,
    LibrariesList,
    LibraryBrowse { section_key: String, limit: Option<usize>, offset: Option<usize> },
    LibraryRecentlyAdded { limit: Option<usize> },
    LibrarySearch { query: String },
    MediaGet { rating_key: String },
    SessionsList,
    PlaylistsList,
    ClientsList,
    PlaybackControl { client_id: String, command: String, confirm: bool },
    CanonCoverage { library: Option<String> },
    TasteProfile,
    MediaEnrich { rating_key: String },
    RagSearch { query: String },
    PerfStatus,
    RepairReport,
    InboxList,
    ToolsList,
    SkillsList,
    BridgeSpeak { text: String, voice: Option<String>, url: Option<String> },
    BridgeEvent { kind: String, device: String, payload: Value, url: Option<String> },
    LlmChat { provider: String, model: Option<String>, prompt: String, system: Option<String>, context: Option<Value>, base_url: Option<String>, api_key: Option<String> },
    LlmModels { provider: String, base_url: Option<String>, api_key: Option<String> },
}
```

Every MCP tool and desktop UI action routes through this bus. In live mode with credentials, `PlexClient` executes requests against the real Plex server. In headless CI or unconfigured environments, built-in demo fixtures allow 100% test coverage without network dependencies.

---

## 3. Stdio Wire Purity & Panic Hook

FastMCP stdio servers communicate with AI hosts (Claude Desktop, Antigravity) over stdin and stdout. Any stray `println!` or unhandled panic text written to stdout corrupts the JSON-RPC framing and crashes the client.

`rust-plex-mcp` guarantees wire purity:
1. **JSON-RPC Strictly on stdout**: Only validated JSON-RPC responses are written to stdout.
2. **Diagnostics on stderr**: All log output and diagnostics route to stderr.
3. **Forensic Panic Hook**: If an unexpected panic occurs, it writes to stderr and `%LOCALAPPDATA%\rust-plex-mcp\crash.log` without emitting partial JSON to stdout.

---

## 4. 10-Point Uncrashable Binary Contract

1. `#![deny(clippy::unwrap_used)]`, `#![deny(clippy::expect_used)]`, `#![deny(clippy::panic)]` enforced across all crates.
2. Zero synchronous network or keyring I/O on the UI thread (`self.spawn(...)`).
3. Zero calls to `ctx.set_visuals()` or `ctx.set_zoom_factor()` inside `ctx.input(...)` closures to prevent deadlocks.
4. Strict glyph allowlist: ONLY ASCII and `['°', '×', '·', '…']` to prevent font rendering artifacts.
5. Windows Credential Manager integration via `keyring` for OS-level secret vaulting.
