# Conversion Dossier: `rust-plex-mcp` (from `plex-mcp`)

**Generated:** 2026-10-18T03:00:00+02:00
**Legacy Repo Path (100% READ-ONLY):** `D:\Dev\repos\plex-mcp`
**Legacy Git Baseline:** branch `main` @ commit `973bc4e`
**Target Rust Repo:** `D:\Dev\repos\rust-plex-mcp`

---

## 1. Bucket & Privilege-Split / Hidden Binary Audit
- **Subdirectories / Native Wrappers Detected in `D:\Dev\repos\plex-mcp`:**
  - `native/` (Tauri 2 wrapper: sidecar builder, `tauri.conf.json`, `node_modules/`, `target/`, `windows/`, `resources/`, `capabilities/`, `gen/`, `icons/`)
  - `src-tauri/` (second Tauri 2 wrapper: same shape — `tauri.conf.json`, `build.rs`, `windows/`, `resources/`)
  - `zed-extension/` (Rust `.wasm` stub: `Cargo.toml`, `Cargo.lock`, `extension.toml`, `target/`, `build.ps1`)
  - No root `Cargo.toml`. No elevated `LocalSystem` C++/C# Windows services, no kernel drivers.
- **Triage Classification:** **Bucket A (100% Pure-Rust HTTP REST + Optional `plex-store` SQLite Historian).**
  - All device access is outbound HTTPS to Plex Media Server (`PLEX_SERVER_URL`, default local `:32400`).
  - The two Tauri wrappers and the Zed stub are *packaging*, not privilege — Rust `egui` + hand-written NSIS replaces all three with zero capability loss. No `C1` split needed.
  - `ffmpeg_mgr.py` (probe/clip/transcode via ffmpeg subprocess) maps to `tokio::process` in Phase 2; no elevated rights required.

---

## 2. Live Service, Port & Database Contention Audit
- **Live check 2026-10-18 ~03:00 UTC+2 (read-only `Get-NetTCPConnection`):**
  - Plex Media Server `:32400` — **LISTENING** (live oracle available).
  - Legacy backend `:10740` — **LISTENING** (Python stack running).
  - Legacy frontend `:10741` — **LISTENING** (Python stack running).
- **Contention Strategy (Gürtel und Hosenträger):**
  - `rust-plex-mcp` binds **zero** ports: MCP is stdio-only, Plex access is outbound `reqwest` HTTPS. No contention with `:10740`/`:10741`/`:32400` by construction.
  - No shared SQLite databases in `plex-mcp` (stateless REST proxy). If a Phase-2 `plex-store` historian lands, it gets its own `%LOCALAPPDATA%\rust-plex-mcp\devices.db`-style file — never touches Python data.
  - `D:\Dev\repos\plex-mcp` (`main@973bc4e`) remains 100% untouched (verified clean `git status`).

---

## 3. Complete 1:1 MCP Tool & Portmanteau Mapping Table

Legacy `@mcp.tool` flat tools map 1:1; portmanteau tools map per-`operation` variant. Rust side exposes clean `plex.<action>` names **plus** legacy aliases (`call_tool` accepts both).

| Legacy Python Tool | `operation=` Variants | Rust `Command` Variant | CLI Subcommand | Rust MCP Tool + Legacy Alias |
|---|---|---|---|---|
| `tools/library.py`: scan/refresh/optimize/get/list/add/update/delete/location-add/location-remove/items/empty-trash/clean-bundles (13) | — | `LibrariesList`, `LibraryStats`, `LibraryMutate` | `libraries`, `library-stats` | `plex.libraries_list` + flat aliases |
| `tools/media.py`: search_media, get_media_info, get_library_items (3) | — | `MediaSearch`, `MediaGet` | `media-search`, `media-get` | `plex.media_search` + flat aliases |
| `tools/organization.py`: organize/analyze/fix-match/refresh/clean/optimize (6) | — | `LibraryOrganize`, `LibraryAnalyze` | `organize`, `analyze` | `plex.library_organize` + flat aliases |
| `tools/playlists.py`: create/get/list/update/delete/add/remove/analytics (8) | — | `PlaylistsList`, `PlaylistMutate` | `playlists` | `plex.playlists_list` + flat aliases |
| `tools/quality.py`: transcode-settings/status, bandwidth, stream-quality, throttling, profiles (10) | — | `QualityGet`, `TranscodeStatus` | `quality` | `plex.quality_get` + flat aliases |
| `tools/users.py`: create/update/delete/list/get/permissions (6) | — | `UsersList`, `UserMutate` | `users` | `plex.users_list` + flat aliases |
| `tools/server.py`: api-docs/status/libraries/info (4) | — | `ServerStatus` | `status` | `plex.server_status` + flat aliases |
| `tools/sessions.py`: sessions/clients/playback-control (3) | — | `SessionsList`, `PlaybackControl` | `sessions`, `control` | `plex.sessions_list` + flat aliases |
| `api/core_original.py` (6 defs), `api/vienna.py` (3 defs) | — | folded into above | — | legacy aliases retained |
| `tools/agentic.py`: `agentic_plex_workflow`, `plex_natural_assistant` (2) | — | `LlmChat` | `chat` | `llm.chat` + aliases |
| `arr_stack` | `status` | `ArrStatus` | `arr-status` | `plex.arr_status` + alias `arr_stack` |
| `audio_mgr` | `list_streams`, `set_volume`, `get_volume`, `mute`, `unmute`, `select_stream`, `handover` | `AudioControl` | `audio` | `plex.audio_control` + alias `plex_audio_mgr` |
| `canon` | `list_canons`, `oscar_best_picture`, `afi_100`, `tmdb_top`, `check_list` | `CanonCheck` | `canon` | `plex.canon_check` + alias `plex_canon` |
| `collections` | `list`, `get`, `create`, `update`, `delete`, `add_items`, `remove_items` | `CollectionsList`, `CollectionMutate` | `collections` | `plex.collections_list` + alias `plex_collections` |
| `completeness` | `status`, `scan_library`, `show_gaps` | `CompletenessGaps` | `gaps` | `plex.completeness_gaps` + alias `plex_completeness` |
| `enrichment` | `enrich_item`, `get_external_metadata`, `analyze_trends`, `enrich`, `bulk_enrich`, `get_enrichment`, `get_related`, `search_by_theme`, `search_by_person`, `stale_report` | `MediaEnrich` | `enrich` | `plex.media_enrich` + alias `plex_media_enrichment` |
| `ffmpeg_mgr` | probe/clip/transcode/rotate/audio-sync/subtitle/aspect ops | `FfmpegOp` | `ffmpeg` | `plex.ffmpeg_op` + alias `plex_ffmpeg_mgr` |
| `help` | `help`, `list_tools`, `tool_info`, `examples`, `api_docs` | `Status` | `status` | `calibre.status`-pattern `plex.status` + alias `plex_help` |
| `integration` | `list_integrations`, `vienna_recommendations`, `european_content`, `anime_season_info`, `configure`, `sync` | `IntegrationOp` | `integration` | `plex.integration_op` + alias `plex_integration` |
| `library` (portmanteau) | `browse`, `search`, `get_details`, `get_recent`, `update_metadata`, `get_stream_url` (+ movie/show/music/photo filters) | `LibraryBrowse`, `LibrarySearch` | `browse`, `search` | `plex.library_browse` + alias `plex_library` |
| `media` (portmanteau) | `browse`, `search`, `get_details`, `get_recent`, `update_metadata`, `get_stream_url` | folded with `LibraryBrowse` | — | alias `plex_media` |
| `metadata` | `refresh`, `refresh_all`, `fix_match`, `update`, `analyze`, `match`, `organize` (+ movie/show/season/episode/artist/album/track/photo filters) | `MetadataOp` | `metadata` | `plex.metadata_op` + alias `plex_metadata` |
| `organization` (portmanteau) | `organize`, `analyze`, `clean_bundles`, `optimize_database`, `fix_issues` | folded with `LibraryOrganize` | — | alias `plex_organization` |
| `performance` | `get_transcode_settings`, `update_transcode_settings`, `get_transcoding_status`, `get_bandwidth`, `set_quality`, `get_throttling`, `set_throttling`, `list_profiles`, `create_profile`, `delete_profile`, `get_server_status`, `get_server_info` | `PerfStatus` | `perf` | `plex.perf_status` + alias `plex_performance` |
| `playlist` (portmanteau) | `list`, `get`, `create`, `update`, `delete`, `add_items`, `remove_items`, `get_analytics` | folded with `PlaylistsList`/`PlaylistMutate` | — | alias `plex_playlist` |
| `quality` (portmanteau) | `list_profiles`, `get_profile`, `create_profile`, `update_profile`, `delete_profile`, `set_default` | folded with quality tools | — | alias `plex_quality` |
| `rag` | `semantic_search`, `sync_metadata`, `sync_subtitles`, `search_subtitles`, `status` | `RagSearch` | `rag-search` | `plex.rag_search` + alias `plex_rag` |
| `reporting` | `library_stats`, `usage_report`, `content_report`, `user_activity`, `performance_report`, `export_report` | `ReportOp` | `report` | `plex.report_op` + alias `plex_reporting` |
| `search` | `search`, `advanced_search`, `suggest`, `recent_searches`, `save_search` | `LibrarySearch` | `search` | `plex.library_search` + alias `plex_search` |
| `server` (portmanteau) | `status`, `info`, `health`, `maintenance`, `restart`, `update` | `ServerStatus` | `status` | alias `plex_server` |
| `shutdown` | `status`, `shutdown` | `ServerStatus` (+ guarded shutdown op) | `status` | alias `plex_shutdown` |
| `streaming` | `list_sessions`, `list_clients`, `play`, `pause`, `stop`, `seek`, `skip_next`, `skip_previous`, `set_quality`, `control` | `PlaybackControl` | `control` | `plex.playback_control` + alias `plex_streaming` |
| `taste` | profile ops | `TasteProfile` | `taste` | `plex.taste_profile` + alias `plex_taste` |
| `user` (portmanteau) | `list`, `get`, `create`, `update`, `delete`, `update_permissions` (+ owner/admin/user/managed/shared roles) | folded with `UsersList`/`UserMutate` | — | alias `plex_user` |
| *(Fleet standard crossconnect & LLM)* | — | `BridgeEvent`, `BridgeSpeak`, `BridgeWebhook`, `LlmChat`, `LlmModels` | `bridge`, `chat`, `models` | `bridge.event`, `bridge.speak`, `bridge.webhook`, `llm.chat`, `llm.models` |

Excluded/delegated: local LanceDB/fastembed indexing (`rag_index_build` semantics → SQLite FTS where possible, else documented gap); Next.js frontend (replaced by `egui`).

---

## 4. Credential, `.env`, Config & Data-Path Inventory (`import.rs` Spec)
`crates/plex-core/src/import.rs` will auto-discover settings in strict priority order:
1. Explicit CLI/MCP parameter (`server_url`, `token`)
2. Process environment variables (`PLEX_SERVER_URL`, `PLEX_TOKEN`, `PLEX_USERNAME`, `PLEX_PASSWORD`, `PLEX_TIMEOUT`, `PLEX_DEBUG`)
3. Sibling `D:\Dev\repos\plex-mcp\.env` (same 6 keys; values redacted here, never logged)
4. `D:\Dev\repos\plex-mcp\.env.example` for key-shape reference only
5. Deterministic `Vendor::Demo` (built-in 3-library/12-item sample catalog so CI and `--smoke` pass on any naked machine without a Plex server).
- Plex server default: `http://127.0.0.1:32400` + `X-Plex-Token` header auth; `Accept: application/json` (Plex defaults to XML — always request JSON).

---

## 5. Write-Safety & Destructive-Action Fence Design
- **Default 100% read-only:** library/browse/search/metadata-read/session-list/user-list/stats paths never mutate server state.
- **Mutating Operations Gate:** `delete_library`, `delete_playlist`, `empty_trash`, `clean_bundles`, `update_user_permissions`, `control_playback`, server `restart`/`update`, and `shutdown` require explicit confirmation flags (`--confirm` on CLI, `confirm: true` on MCP) and are rejected by default with a typed error naming the flag.
- Playback control targets explicit `client_id` only (never "all clients" broadcast).

---

## 6. Write-Safety Duplicate Note
- *(Consolidated into §5 above; kept as section anchor for SOP checklist compatibility.)*

---

## 7. L3 `egui` Desktop Page & Golden Snapshot Plan
- **Domain Pages in `crates/plex-app/src/app.rs`:**
  1. `Page::Libraries` — library sections grid (movies/shows/music/photos counts), 1-click active section switcher, storage bars per section.
  2. `Page::Media` — instant search + filter bar (title, genre, year, unwatched), media grid with poster art (`/photo/:/transcode` thumbnails), right-hand detail inspector (summary, cast, file media info, `Play`/`Mark watched` via `plex.playback_control`/`plex_metadata`).
  3. `Page::Analytics` — `canon_coverage`/`completeness` gaps, duplicate/quality profiles, bandwidth/transcode KPIs, playlist analytics.
- **Inherited Kernel Pages:** `Page::Crossconnect`, `Page::Chat` (`gemma4:12b` default), `Page::Settings` (OS Credential Manager `vault.rs` for `PLEX_TOKEN`), `Page::Help`, `Page::RawJson`.
- **Golden `egui_kittest` Snapshots:** `libraries_dark`, `media_dark`, `analytics_dark`, `help_dark`, `libraries_light`.
