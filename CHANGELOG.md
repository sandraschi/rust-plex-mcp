# Changelog

All notable changes to `rust-plex-mcp` are documented in this file.
The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.0] - 2026-10-18

### Added
- **Pure-Rust Architecture**: Completed 3-crate ArtCraft architecture (`plex-core`, `rust-plex-mcp`, `plex-app`) replacing 80,000+ lines of Python/Node/Next.js/React.
- **Movie Poster Art & Visual Parity**:
  - Asynchronous poster thumbnail fetching and RGBA image decoding (`image` crate) off the UI thread via `fetch_image_bytes`.
  - 2:3 aspect ratio responsive `movie_card` grid with hover effects and selection highlights matching the original webapp.
  - Movie cards integrated into `Movies` view (with grid/list toggle), `Overview` view (horizontal Recently Added rail), `Search` view, and the `Media Inspector` detail card.
  - Painted vector fallback clapperboard icon when posters are offline or loading.
- **Interactive Movie Playback**:
  - Double-clicking any movie card or collection item immediately starts playback.
  - Translucent circular play icon badge overlay rendered on hover over poster cards matching the original webapp PlayIcon.
  - Native media player integration (`open` crate): automatically detects physical file path on disk (`Media.Part.file`) and launches it directly in the system default media player (VLC, MPV, Windows Media Player) with zero UI thread blocking.
  - Remote/Web fallback: if physical file is remote or unmounted, automatically opens the movie's Plex Web player URL in the default browser.
  - Prominent `> Play Movie` button in the Media Item Inspector pane and search results rows.
- **Full UI/UX Clone Parity**: Native `egui 0.37` desktop dashboard implementing all 19 views from the legacy Next.js webapp:
  - `Overview` (`/`): Real-time server telemetry, KPI grid, active playback streams, recently added media carousels.
  - `Inbox` (`/inbox`): Ingest queue, task statuses, unmatched file alerts.
  - `Libraries` (`/libraries`): Configured sections, metadata agent inspection, browse trigger.
  - `Movies & Media Inspector` (`/movies`): Paginated browsing, stream details, audio/video codecs, container paths.
  - `Media Repair` (`/repair`): Broken file diagnostics, unplayable containers, rematching.
  - `Search` (`/search`): Universal search across all media libraries.
  - `Semantic Search` (`/search/semantic`): Natural-language similarity queries over plot summaries.
  - `RAG Management` (`/rag`): Vector depot health, embedding models, re-index controls.
  - `Canon Coverage` (`/canon`): Coverage progress for IMDB Top 250, AFI 100, and franchise sets.
  - `Enrichment` (`/enrichment`): Single-item metadata enhancement, poster and subtitle fetching.
  - `Taste` (`/taste`): Watch habits, top directors, genres, and personalized recommendations.
  - `Chat` (`/chat`): Conversational AI assistant with library awareness.
  - `Tools` (`/tools`): Interactive in-process execution for all 36 MCP tools.
  - `Skills` (`/skills`): Directory of automated Plex maintenance workflows.
  - `API Docs` (`/api-docs`): REST endpoints and FastMCP stdio JSON-RPC reference.
  - `Server` (`/server`): Hardware acceleration detection, network interfaces, platform telemetry.
  - `Logs` (`/logs`): Real-time event log viewer with search and level filters.
  - `Help` (`/help`): Plex token retrieval tutorial and MCP configuration snippets.
  - `Settings` (`/settings`): Fleet Standard LLM Settings Card and Plex connection card.
  - `Crossconnect`: Fleet bridge to `speech-mcp` TTS (:10909) and `aiwatcher` (:10946).
  - `Raw JSON`: Live wire inspector for debugging command responses.
- **Fleet Standard LLM Settings Card**:
  - Provider selector supporting Ollama, LM Studio, OpenAI, Anthropic, Gemini, LocalAI, and Custom OpenAI-compatible endpoints.
  - Base URL configuration with provider defaults.
  - Windows Credential Manager OS Vault integration via `keyring` with show/hide masking.
  - Dynamic model elicitation via `LlmModels` command.
  - Non-streaming connection test button with response latency and status pill.
- **36-Tool FastMCP Stdio Server**:
  - Full wire-compatible JSON-RPC `2024-11-05` protocol implementation.
  - Dual tool naming: canonical dot notation (`plex.status`), underscore notation (`plex_status`), and legacy portmanteau dispatchers (`plex_server`, `plex_library`, etc.).
  - Clean stdio wire purity (logs on stderr, JSON-RPC strictly on stdout).
- **10-Point Uncrashable Binary Contract**:
  - Zero panics, zero unwraps/expects.
  - Strictly non-blocking UI thread using async job channels (`spawn` and `drain_jobs`).
  - Strict glyph allowlist (`°`, `×`, `·`, `…`) validated by compile-time source scanning.
  - Safe Ctrl+Wheel zoom factor clamping (0.75x to 2.0x).
- **Windows Packaging**:
  - Nullsoft NSIS installer `Plex-Rust-0.4.0-setup.exe` with desktop and start menu shortcuts.
  - Standalone release binaries `plex-app.exe` and `rust-plex-mcp.exe`.
  - Cryptographic checksums `SHA256SUMS.txt`.
