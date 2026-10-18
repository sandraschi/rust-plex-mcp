# Product Requirements Document (PRD) — `rust-plex-mcp`

## 1. Overview & Purpose
`rust-plex-mcp` is a pure-Rust conversion of the legacy `plex-mcp` server and Next.js web application. It delivers a fast, memory-safe, single-binary distribution featuring a 36-tool FastMCP stdio server and a native `egui 0.37` GPU desktop dashboard with 100% clone parity across all 19 views.

## 2. Core Architecture
- **Layer 1: `plex-core`**: Core Plex REST client, state machine, universal `Command` bus, and LLM chat/model integration. Zero UI or MCP transport dependencies.
- **Layer 2: `rust-plex-mcp`**: High-performance MCP stdio server implementing the `2024-11-05` JSON-RPC specification. Features 36 canonical tools, legacy portmanteau dispatchers, and clean stdio wire purity.
- **Layer 3: `plex-app`**: Native desktop GUI built on `egui` and `eframe 0.37`. Binds zero network ports and communicates directly with `plex-core` in-process.

## 3. Key Functional Requirements
1. **Plex Server REST Integration**:
   - Outbound HTTPS client with token authentication.
   - Robust error handling for server connection failures with automatic Demo mode fallback.
   - Strict body size limits and timeout safeguards.
2. **MCP Tool Wire Compatibility**:
   - 36 FastMCP tools supporting both canonical dot notation (`plex.status`), underscore notation (`plex_status`), and legacy grouped operations (`plex_server`, `plex_library`, etc.).
3. **UI/UX Clone Parity**:
   - Complete 19-route navigation clone matching the legacy Next.js webapp (`Overview`, `Inbox`, `Libraries`, `Movies`, `Media Repair`, `Search`, `Semantic Search`, `RAG Management`, `Canon Coverage`, `Enrichment`, `Taste`, `Chat`, `Tools`, `Skills`, `API Docs`, `Server`, `Logs`, `Help`, `Settings`).
   - Signature Plex amber theme styling with instant Dark/Light vector toggle.
4. **Fleet Standard LLM Settings Card**:
   - Provider selection: Ollama, LM Studio, OpenAI, Anthropic, Gemini, LocalAI, Custom.
   - Dynamic model elicitation.
   - Secure OS Credential Manager vaulting via Windows `keyring`.
   - Latency-measured connection test button with status pill.
5. **10-Point Uncrashable Binary Contract**:
   - Zero panics, zero unwraps/expects.
   - Asynchronous worker channel for non-blocking UI rendering.
   - Strict glyph allowlist (`°`, `×`, `·`, `…`).
   - Safe Ctrl+Wheel zoom factor clamping.

## 4. Verification & Release Gates
- `cargo test --workspace`: 24/24 unit and UI tests passing.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: Zero warnings.
- `cargo fmt --all --check`: Clean formatting.
- `cargo run -p plex-app -- --smoke`: Headless parity verification passing.
- NSIS installer: `Plex-Rust-0.4.0-setup.exe` compiled and verified with SHA256 hashes.
