# UI Parity & ArtCraft Desktop Specification (`plex-app`)

`crates/plex-app` is the L3 native `egui 0.37` (`eframe 0.37`) desktop frontend for `rust-plex-mcp`. It replaces the legacy Python/Next.js/React `plex-mcp/webapp` with a zero-daemon, zero-port native desktop executable.

## 1. Visual & Interaction Standards

| Requirement | Implementation in `crates/plex-app/src/app.rs` | Verification |
|---|---|---|
| **Harsh Dark & Crisp Light Themes** | `Theme::Dark` (`#0B0F17` bg, `#111827` panel, `#F59E0B` amber accent) & `Theme::Light` (`#F8FAFC` bg, `#FFFFFF` cards) | Topbar Sun/Moon `egui::Painter` toggle |
| **Safe `Ctrl+Wheel` Zoom** | Reads `ctx.input(\|i\| i.zoom_delta())` inside closure, calls `ctx.set_zoom_factor(self.zoom)` **outside** closure | `50%` – `200%` with one-click reset badge |
| **Strict Glyph Allowlist** | Only ASCII + `['°', '×', '·', '…']` permitted in `app.rs` (zero tofu boxes in default font) | `tests::glyph_source_scan` + `glyph_allowlist` |
| **Async Background Worker** | Dedicated `tokio` runtime thread + `mpsc` channel keeps UI rendering at 60 FPS during network requests | `self.drain_jobs(&ctx)` |
| **Headless CI Smoke Mode** | `plex-app --smoke` runs `status` + `server-status` + `libraries` and exits `0` without opening a window | `cargo run -p plex-app -- --smoke` |

## 2. 19-Route Clone Parity Specification

All 19 navigation routes from `plex-mcp/webapp/frontend/components/layout/sidebar.tsx` are fully cloned:

1. **Overview (`/`)**:
   - Server Connection Card: Server name, version, host:port, platform, and live connected badge.
   - KPI Grid: Total Libraries, Recently Added count, Active Streaming Sessions, Playlists count.
   - Active Streams Panel: Real-time transcode/direct-play streams, player client, and user.
   - Recently Added Grid: Media titles, release years, library sections, and media types.
   - Quick Operations: Telemetry refresh, section scanner trigger, search shortcut.

2. **Inbox (`/inbox`)**:
   - Ingest queue and pending transcoding/matching tasks.
   - Unresolved match notifications and corrupt file alerts.

3. **Libraries (`/libraries`)**:
   - Configured library sections (Movies, Shows, Music, Photos).
   - Agent, scanner, and language details.
   - 1-click `Browse Items` transition directly into Media Inspector.

4. **Movies & Media Inspector (`/movies`, `/media`)**:
   - Paginated section item browsing with title and year filters.
   - Media Item Inspector: Title, tagline, plot summary, runtime, video resolution (4K/1080p), video codec, and audio channels.
   - Action triggers: Metadata enrichment and playback tests.

5. **Media Repair (`/repair`)**:
   - Diagnostic status for unplayable containers, missing codecs, and unmatched titles.
   - Automated repair scan trigger.

6. **Search (`/search`)**:
   - Universal search across movies, television series, seasons, and tracks.
   - Instant 1-click inspection transition.

7. **Semantic Search (`/search/semantic`)**:
   - Natural-language plot and synopsis vector similarity search.
   - Configurable Top-K slider and match ranking scores.

8. **RAG Management (`/rag`)**:
   - Vector database telemetry (indexed documents, vector dimensions, index health).
   - Re-index trigger for media synopsis vector depot.

9. **Canon Coverage (`/canon`)**:
   - Collection completeness against IMDB Top 250, AFI 100, and curated franchise lists.
   - Percentage bars and owned vs total volume counts.

10. **Enrichment (`/enrichment`)**:
    - Single-item metadata enrichment engine for posters, subtitle tracks, and ratings.

11. **Taste (`/taste`)**:
    - Viewing history analytics, top genres, favorite directors, and personalized recommendations.

12. **Chat (`/chat`)**:
    - Conversational AI assistant with full Plex collection context.
    - Provider and model selection, conversation bubble history, clear and send controls.

13. **Tools (`/tools`)**:
    - Interactive in-process execution runner for all 36 MCP tools.
    - JSON parameter payload editor and live response viewer.

14. **Skills (`/skills`)**:
    - Catalog of automated maintenance skills (Auto-organize Ingest, Subtitles Fetcher, Transcode Optimizer, Smart Playlists).

15. **API Docs (`/api-docs`)**:
    - Reference for Plex REST endpoints and FastMCP stdio JSON-RPC tool contracts.

16. **Server (`/server`)**:
    - Deep system telemetry: Platform, architecture, machine ID, Intel QuickSync/NVIDIA NVENC hardware acceleration, and ports.

17. **Logs (`/logs`)**:
    - Real-time event log viewer with search filter and level filtering.

18. **Help (`/help`)**:
    - Plex token acquisition guide and MCP client configuration snippets for Claude Desktop and Antigravity.

19. **Settings (`/settings`)**:
    - **Fleet Standard LLM Settings Card**: Provider selector, base URL, OS Credential Manager vaulting, dynamic model elicitation, and ping test button.
    - **Plex Connection Card**: Server URL and Plex Token management.

### Extensions
20. **Crossconnect (`/crossconnect`)**:
    - Fleet bridge dispatch to `speech-mcp` TTS (:10909) and `aiwatcher` telemetry (:10946).
21. **Raw JSON (`/raw`)**:
    - Syntax-formatted raw wire JSON inspector for debugging server responses.
