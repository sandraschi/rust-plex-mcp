# AGENTS.md - rust-plex-mcp (ArtCraft containment harness, adapted)

Pure-Rust conversion of `plex-mcp` (Plex Media Server control: libraries,
media, playlists, users, sessions, streaming). Wave-0 pilot #2 — proves the
`repconv` workflow end to end on a pure-HTTP-REST daily driver.

## 1. Safety and error handling (never crash)

- Workspace lint gate (enforced in root `Cargo.toml`):
  `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`,
  `clippy::unimplemented`, `clippy::todo` are all `deny`.
- Every fallible function returns `Result<T, PlexError>` and bubbles with `?`.
  Use `ok_or_else` / explicit `match`, never `unwrap` / `expect`.
- Treat all external input as hostile (CLI args, MCP payloads, Plex XML/JSON):
  - No direct indexing (`buf[i]`); use `.get()` with a typed error on `None`.
  - Integer math on sizes / durations / offsets uses `checked_*` / `saturating_*`.
  - String slicing only at `char` boundaries (`str::get`, never byte slicing).
  - Plex XML attributes may be absent on any item — every field is `Option`
    until proven present.
- Every HTTP call carries `with_timeout_secs` + payload cap (Plex libraries
  return megabytes of XML; never buffer unbounded).
- Every bug fix MUST ship a minimal regression test that fails before the fix.
- Stdio MCP invariant: **stdout is pure JSON-RPC only**. All logs go to stderr
  via `eprintln!`.

## 2. Architecture and seams

- L1 `plex-core`: Plex REST protocol, state machine, command bus. Zero UI,
  zero MCP dependencies. Pure `reqwest` + `serde_json` (+ `rusqlite` only if
  a local historian lands).
- L2 `rust-plex-mcp`: CLI-first binary + MCP `mcp` subcommand over stdio
  (MCP 2024-11-05, hand-rolled framing). Dual tool-name aliasing: clean
  `plex.<action>` names AND legacy portmanteau names (`plex_library` with
  `args["operation"]`) so existing skills keep working with zero edits.
- L3 `plex-app`: egui dashboard calling `plex-core` in-process. Pages:
  `Libraries`, `Books`→Media grid, `Analytics` + kernel tabs
  (`Crossconnect`, `Chat`, `Settings`, `Help`, `RawJson`).
- No destructive Plex actuation (delete library items, empty trash, kill
  sessions) without explicit confirmation flags. Read paths are default.

## 3. MCP surface contract

- Portmanteau parity: every legacy `operation=` variant in
  `docs/CONVERSION_DOSSIER.md` §3 maps to a `Command` variant AND a legacy
  alias in `call_tool`. `rustc`'s exhaustive `match` catches omissions.
- Descriptions are the agent contract: short, imperative, with units and
  required params (`media_key`, `client_id`, `rating_key`).
- Notifications get no response. Unknown methods `-32601`, bad params
  `-32602`, parse failures `-32700`.

## 4. Definition of done

- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings` green.
- [ ] `cargo test --workspace` green (incl. `dispatch_future_is_send` guard).
- [ ] `cargo fmt --all --check` green.
- [ ] Live oracle probe: at least one read-only CLI + MCP call against the
  real Plex server (`:32400`) with auto-imported `PLEX_URL`/`PLEX_TOKEN`.
- [ ] Legacy `plex-mcp` untouched (`git -C D:\Dev\repos\plex-mcp status -s` clean).

## 5. Fleet notes

- Windows-first: all `just` recipes use `windows-shell` + explicit
  `$env:USERPROFILE\.cargo\bin` prefix.
- Never hardcode ports, IPs, tokens, or machine paths. Test fixtures use
  documentation values only; production values come from args → env →
  imported `.env` → config.
- Plex credentials resolve: explicit args > `PLEX_URL`/`PLEX_TOKEN` env >
  sibling `plex-mcp/.env`. Secrets never logged.
- Shadow mode: never bind `:10740`/`:10741`/`:32400`; MCP is stdio-only and
  Plex access is outbound HTTP. The Python stack keeps running untouched.
