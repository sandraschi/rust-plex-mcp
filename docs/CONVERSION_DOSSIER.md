# Conversion Dossier: `rust-plex-mcp` (from `plex-mcp`)

**Generated:** 2026-10-18T03:05:00+02:00
**Legacy Repo Path (100% READ-ONLY):** `D:\Dev\repos\plex-mcp`
**Legacy Git Baseline:** branch `main` @ commit `973bc4e`
**Target Rust Repo:** `D:\Dev\repos\rust-plex-mcp`

---

## 1. Bucket & Privilege-Split / Hidden Binary Audit
- **Subdirectories / Native Wrappers Detected:**
  - `native/` (Tauri 2 wrapper: sidecar builder, `tauri.conf.json`, `node_modules/`, `target/`, `windows/`, `resources/`, `capabilities/`, `gen/`, `icons/`)
  - `src-tauri/` (second Tauri 2 wrapper, same shape)
  - `zed-extension/` (Rust `.wasm` stub: `Cargo.toml`, `extension.toml` — packaging only, not logic)
  - No root `Cargo.toml`. No elevated `LocalSystem` services, no kernel drivers, no C++ MFT service.
- **Triage Classification:** **Bucket A (100% Pure-Rust HTTP REST).**
  - All device access is outbound HTTPS to Plex Media Server. The two Tauri wrappers + Zed stub are *packaging*, replaced by `egui` + hand-written NSIS with zero capability loss. No `C1` split needed.
  - `ffmpeg_mgr.py` (probe/clip/transcode via ffmpeg subprocess) maps to `tokio::process` in Phase 2; no elevated rights required.

---

## 2. Live Service, Port & Database Contention Audit
- **Live check 2026-10-18 ~03:05 UTC+2 (read-only `Get-NetTCPConnection`):**
  - Plex Media Server `:32400` — **LISTENING** (live oracle available).
  - Legacy backend `:10740` — **LISTENING**. Legacy frontend `:10741` — **LISTENING**.
- **Contention Strategy (Gürtel und Hosenträger):**
  - `rust-plex-mcp` binds **zero** ports: MCP is stdio-only, Plex access is outbound `reqwest` HTTPS. No contention with `:10740`/`:10741`/`:32400` by construction.
  - No shared SQLite databases in `plex-mcp` (stateless REST proxy). A Phase-2 `plex-store` historian (watch history, session log) gets its own `%LOCALAPPDATA%\rust-plex-mcp` file.
  - `D:\Dev\repos\plex-mcp` (`main@973bc4e`) remains 100% untouched.

---

## 3. Complete 1:1 MCP Tool & Portmanteau Mapping Table

Flat `@mcp.tool` tools map 1:1; portmanteau tools map per-`operation` variant. Rust exposes clean `plex.<action>` names **plus** legacy aliases so existing skills keep working with zero edits.

**Flat tools** (`tools/*.py`, `api/*.py`):

| Legacy Python Tool | Rust `Command` Variant | CLI | Rust MCP Tool + Legacy Alias |
|---|---|---|---|
| `tools/library.py` (13): scan/refresh/optimize/get/list/add/update/delete/location-add/location-remove/items/empty-trash/clean-bundles | `LibrariesList`, `LibraryMutate` | `libraries` | `plex.libraries_list` + flat aliases |
| `tools/media.py` (3): search_media, get_media_info, get_library_items | `MediaSearch`, `MediaGet` | `media-search`, `media-get` | `plex.media_search` + flat aliases |
| `tools/organization.py` (6): organize/analyze/fix-match/refresh/clean/optimize | `LibraryOrganize` | `organize` | `plex.library_organize` + flat aliases |
| `tools/playlists.py` (8): create/get/list/update/delete/add/remove/analytics | `PlaylistsList`, `PlaylistMutate` | `playlists` | `plex.playlists_list` + flat aliases |
| `tools/quality.py` (10): transcode-settings/status, bandwidth, stream-quality, throttling, profiles | `QualityGet`, `TranscodeStatus` | `quality` | `plex.quality_get` + flat aliases |
| `tools/users.py` (6): create/update/delete/list/get/permissions | `UsersList`, `UserMutate` | `users` | `plex.users_list` + flat aliases |
| `tools/server.py` (4): api-docs/status/libraries/info | `ServerStatus` | `status` | `plex.server_status` + flat aliases |
| `tools/sessions.py` (3): sessions/clients/playback-control | `SessionsList`, `PlaybackControl` | `sessions`, `control` | `plex.sessions_list` + flat aliases |
| `api/core_original.py` (6), `api/vienna.py` (3) | folded into above | — | legacy aliases retained |
| `tools/agentic.py` (2): `agentic_plex_workflow`, `plex_natural_assistant` | `LlmChat` | `chat` | `llm.chat` + aliases |

**Portmanteau tools** (`tools/portmanteau/*.py`, `operation=` variants):

| Legacy Tool | `operation=` Variants | Rust `Command` | Rust MCP Tool + Alias |
|---|---|---|---|
| `arr_stack` | `status` | `ArrStatus` | `plex.arr_status` + `arr_stack` |
| `audio_mgr` | `list_streams`, `set_volume`, `get_volume`, `mute`, `unmute`, `select_stream`, `handover` | `AudioControl` | `plex.audio_control` + `plex_audio_mgr` |
| `canon` | `list_canons`, `oscar_best_picture`, `afi_100`, `tmdb_top`, `check_list` | `CanonCheck` | `plex.canon_check` + `plex_canon` |
| `collections` | `list`, `get`, `create`, `update`, `delete`, `add_items`, `remove_items` | `CollectionsList`, `CollectionMutate` | `plex.collections_list` + `plex_collections` |
| `completeness` | `status`, `scan_library`, `show_gaps` | `CompletenessGaps` | `plex.completeness_gaps` + `plex_completeness` |
| `enrichment` | `enrich_item`, `get_external_metadata`, `analyze_trends`, `enrich`, `bulk_enrich`, `get_enrichment`, `get_related`, `search_by_theme`, `search_by_person`, `stale_report` | `MediaEnrich` | `plex.media_enrich` + `plex_media_enrichment` |
| `ffmpeg_mgr` | probe, clip, transcode, rotate, audio-sync, subtitles, aspect, revert, extract | `FfmpegOp` | `plex.ffmpeg_op` + `plex_ffmpeg_mgr` |
| `help` | `help`, `list_tools`, `tool_info`, `examples`, `api_docs` | `Status` | `plex.status` + `plex_help` |
| `integration` | `list_integrations`, `vienna_recommendations`, `european_content`, `anime_season_info`, `configure`, `sync` | `IntegrationOp` | `plex.integration_op` + `plex_integration` |
| `library` | `browse`, `search`, `get_details`, `get_recent`, `update_metadata`, `get_stream_url` (+ movie/show/music/photo) | `LibraryBrowse`, `LibrarySearch` | `plex.library_browse` + `plex_library` |
| `media` | `browse`, `search`, `get_details`, `get_recent`, `update_metadata`, `get_stream_url` | folded with `LibraryBrowse` | alias `plex_media` |
| `metadata` | `refresh`, `refresh_all`, `fix_match`, `update`, `analyze`, `match`, `organize` (+ 8 media-type filters) | `MetadataOp` | `plex.metadata_op` + `plex_metadata` |
| `organization` | `organize`, `analyze`, `clean_bundles`, `optimize_database`, `fix_issues` | folded with `LibraryOrganize` | alias `plex_organization` |
| `performance` | transcode-settings/status, bandwidth, quality, throttling, profiles, server-status/info (12 ops) | `PerfStatus` | `plex.perf_status` + `plex_performance` |
| `playlist` | `list`, `get`, `create`, `update`, `delete`, `add_items`, `remove_items`, `get_analytics` | folded with `PlaylistsList` | alias `plex_playlist` |
| `quality` | `list_profiles`, `get_profile`, `create_profile`, `update_profile`, `delete_profile`, `set_default` | folded with quality tools | alias `plex_quality` |
| `rag` | `semantic_search`, `sync_metadata`, `sync_subtitles`, `search_subtitles`, `status` | `RagSearch` | `plex.rag_search` + `plex_rag` |
| `reporting` | `library_stats`, `usage_report`, `content_report`, `user_activity`, `performance_report`, `export_report` | `ReportOp` | `plex.report_op` + `plex_reporting` |
| `search` | `search`, `advanced_search`, `suggest`, `recent_searches`, `save_search` | `LibrarySearch` | `plex.library_search` + `plex_search` |
| `server` | `status`, `info`, `health`, `maintenance`, `restart`, `update` | `ServerStatus` | alias `plex_server` |
| `shutdown` | `status`, `shutdown` | `ServerStatus` (+ guarded shutdown) | alias `plex_shutdown` |
| `streaming` | `list_sessions`, `list_clients`, `play`, `pause`, `stop`, `seek`, `skip_next`, `skip_previous`, `set_quality`, `control` | `PlaybackControl` | `plex.playback_control` + `plex_streaming` |
| `taste` | profile ops | `TasteProfile` | `plex.taste_profile` + `plex_taste` |
| `user` | `list`, `get`, `create`, `update`, `delete`, `update_permissions` (+ owner/admin/user/managed/shared) | folded with `UsersList` | alias `plex_user` |
| *(Fleet standard)* | — | `BridgeEvent`, `BridgeSpeak`, `BridgeWebhook`, `LlmChat`, `LlmModels` | `bridge.*`, `llm.*` |

Excluded/delegated: local LanceDB/fastembed indexing (→ SQLite FTS where possible, else documented gap); Next.js frontend (replaced by `egui`); `dev/prefab` preview route (dev-only, no user counterpart).

---

## 4. Credential, `.env`, Config & Data-Path Inventory (`import.rs` Spec)
`crates/plex-core/src/import.rs` auto-discovers settings in strict priority order:
1. Explicit CLI/MCP parameter (`server_url`, `token`)
2. Process environment (`PLEX_URL` → `PLEX_SERVER_URL` → legacy `PLEX_BASE_URL`; `PLEX_TOKEN`; `PLEX_USERNAME`, `PLEX_PASSWORD`, `PLEX_TIMEOUT`, `PLEX_DEBUG`). Root `.env` carries `PLEX_BASE_URL` + `PLEX_TOKEN`; `webapp/backend/.env` carries `PLEX_URL` + `PLEX_TOKEN`.
3. Sibling `D:\Dev\repos\plex-mcp\.env` (same 6 keys; values redacted here, never logged)
4. Deterministic `Vendor::Demo` (built-in sample server + 3 libraries / 12 items so CI and `--smoke` pass on any naked machine without Plex).
- Auth: `X-Plex-Token` header on every call; `Accept: application/json` (Plex defaults to XML — always request JSON).
- Image pipeline (Phase 4): `plexImageUrl()` pattern → backend `/image/{path}` proxy with `follow_redirects=True` semantics (reqwest follows up to 10 by default); Plex thumb endpoints 307-redirect to `/photo/:/transcode`.

---

## 5. Write-Safety & Destructive-Action Fence Design
- **Default 100% read-only:** browse/search/metadata-read/session-list/user-list/stats/rag-search paths never mutate server state.
- **Mutating Operations Gate:** `delete_library`, `delete_playlist`, `empty_trash`, `clean_bundles`, `update_user_permissions`, `control_playback`, server `restart`/`update`, and `shutdown` require explicit confirmation (`--confirm` on CLI, `confirm: true` on MCP) and are rejected by default with a typed error naming the flag.
- Playback control targets explicit `client_id` only (never "all clients" broadcast).

---

## 6. Privileged Operations Confirmation Matrix
| Operation family | Guard | Notes |
|---|---|---|
| Library delete / empty trash / clean bundles | `--confirm` + typed refuse-by-default | Irreversible media-library changes |
| Playlist delete / user delete / permission change | `--confirm` + typed refuse-by-default | Affects other Plex users |
| Playback control (play/pause/stop/seek) | explicit `client_id` required | No broadcast; interrupts viewing |
| Server restart / update / shutdown | `--confirm` + typed refuse-by-default | Takes the household server down |
| ffmpeg transcode/clip/rotate in place | operate on copy + `--confirm`; `_handle_revert` parity | Matches legacy revert path |
| Token/auth handling | env/`.env`/vault only; never logged, never in errors | Same discipline as fleet `import.rs` |

---

## 7. L3 `egui` Desktop Full Structural Parity & Clone Plan
Legacy sidebar (`webapp/frontend/components/layout/sidebar.tsx`, v2.4.1) is a **flat 19-item list** (no CONTENT/DISCOVER groups — groups are calibre-flavored; plex is flat): Overview, Inbox, Libraries, Movies, Media Repair, Search, Semantic search, RAG Management, Canon Coverage, Enrichment, Taste, Chat, Tools, Skills, API Docs, Server, Logs, Help, Settings. Page routes (`webapp/frontend/app/**/page.tsx`, 21 incl. `/media` + `/search/semantic` + dev-only `/dev/prefab`): every route except `/dev/prefab` gets a typed `Page` variant.
- **Domain Pages in `crates/plex-app/src/app.rs`:**
  1. `Page::Overview` (+ `Inbox`) — server status card, active sessions, recently added rail.
  2. `Page::Libraries` (+ `Movies`, `Media`) — sections grid, media grid with poster art via `/image/{path}` proxy pattern, right-hand detail inspector (summary, cast, file info, `Play`/`Mark watched`).
  3. `Page::Search` (+ `SemanticSearch`, `Rag`) — instant + semantic search, RAG sync status.
  4. `Page::Playlists` (+ `Canon`, `Enrichment`, `Taste`, `Repair`) — playlist CRUD + analytics, canon gaps, enrichment runs, taste profile, ffmpeg repair ops.
  5. `Page::Users` (+ `Server`, `Tools`, `Skills`, `ApiDocs`, `Logs`) — users/permissions, server info + maintenance, tool registry, skill list, API docs viewer, log tail.
- **Inherited Kernel Pages:** `Page::Crossconnect`, `Page::Chat` (`gemma4:12b` default + `Inject Library Context` button), `Page::Settings` (OS Credential Manager `vault.rs` for `PLEX_TOKEN` + fleet LLM card: Ollama/LM Studio/OpenAI/Anthropic/Custom, editable base URL, keyring-stored key, live model elicitation, Ping test pill), `Page::Help`, `Page::RawJson`.
- **Golden `egui_kittest` Snapshots:** `libraries_dark`, `media_dark`, `analytics_dark`, `help_dark`, `libraries_light`.
