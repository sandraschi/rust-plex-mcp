//! L2 binary: CLI-first + MCP `2024-11-05` stdio server for `rust-plex-mcp`.
//!
//! Stdout is pure JSON-RPC. Every log line goes to stderr via `eprintln!`.
//! All operations route through [`plex_core::command::dispatch`].
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use clap::{Parser, Subcommand};
use plex_core::command::{dispatch, Command};
use plex_core::error::DeviceError;
use plex_core::plex::PlexConfig;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;

fn install_forensic_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let msg = format!("[{ts}] PANIC in rust-plex-mcp: {info}\n");
        let _ = io::stderr().write_all(msg.as_bytes());
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let dir = PathBuf::from(local).join("rust-plex-mcp");
            let _ = std::fs::create_dir_all(&dir);
            let log_file = dir.join("crash.log");
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(log_file)
            {
                let _ = f.write_all(msg.as_bytes());
            }
        }
    }));
}

#[derive(Parser)]
#[command(
    name = "rust-plex-mcp",
    version = "0.4.0",
    about = "Pure-Rust Plex Media Server Control & MCP 2024-11-05 Server"
)]
struct Cli {
    /// Emit compact JSON on CLI commands
    #[arg(long, global = true)]
    json: bool,

    /// Plex base URL (else PLEX_URL / PLEX_SERVER_URL / sibling plex-mcp/.env)
    #[arg(long, global = true)]
    server_url: Option<String>,

    /// Plex token (else PLEX_TOKEN / sibling plex-mcp/.env)
    #[arg(long, global = true)]
    token: Option<String>,

    /// RAG index directory (else PLEX_MCP_DATA env / OS default)
    #[arg(long, global = true)]
    data_dir: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// App status: vendor, config source, version
    Status,
    /// Server identity (GET /)
    ServerStatus,
    /// Library sections (GET /library/sections)
    Libraries,
    /// Browse items in a section
    Browse {
        section_key: String,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long, default_value_t = 0)]
        offset: usize,
    },
    /// Recently added items across all libraries
    RecentlyAdded {
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Hub search across libraries
    Search { query: String },
    /// Single item detail by rating key
    MediaGet { rating_key: String },
    /// Active playback sessions
    Sessions,
    /// Playlists list
    Playlists,
    /// Player client devices
    Clients,
    /// Playback control (play, pause, stop, etc.)
    Playback {
        client_id: String,
        action: String,
        #[arg(long)]
        confirm: bool,
    },
    /// Canon coverage report
    Canon {
        #[arg(long)]
        library: Option<String>,
    },
    /// Taste and recommendation profile
    Taste,
    /// Chat with the LLM (buffered JSON, or --stream for live tokens)
    Chat {
        prompt: String,
        #[arg(long)]
        system: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long)]
        stream: bool,
    },
    /// Semantic search over the library (vector index when synced)
    RagSearch {
        query: String,
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Embed the whole library into the local LanceDB index
    RagSync {
        #[arg(long)]
        limit_per_section: Option<usize>,
    },
    /// RAG index status (rows, dim, model, location)
    RagStatus,
    /// Server transcode and bandwidth status
    Perf,
    /// Run MCP 2024-11-05 JSON-RPC stdio server loop
    #[command(name = "mcp-stdio")]
    McpStdio,
}

#[tokio::main]
async fn main() {
    install_forensic_panic_hook();
    let cli = Cli::parse();
    let config = match PlexConfig::resolve(cli.server_url.as_deref(), cli.token.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("rust-plex-mcp: config resolution error: {e}");
            std::process::exit(1);
        }
    };

    match cli.command {
        Commands::McpStdio => {
            if let Err(e) = run_mcp_stdio(&config).await {
                eprintln!("rust-plex-mcp stdio error: {e}");
                std::process::exit(1);
            }
        }
        cmd => {
            let command = match cmd {
                Commands::Status => Command::Status,
                Commands::ServerStatus => Command::ServerStatus,
                Commands::Libraries => Command::LibrariesList,
                Commands::Browse {
                    section_key,
                    limit,
                    offset,
                } => Command::LibraryBrowse {
                    section_key,
                    limit: Some(limit),
                    offset: Some(offset),
                },
                Commands::RecentlyAdded { limit } => {
                    Command::LibraryRecentlyAdded { limit: Some(limit) }
                }
                Commands::Search { query } => Command::LibrarySearch { query },
                Commands::MediaGet { rating_key } => Command::MediaGet { rating_key },
                Commands::Sessions => Command::SessionsList,
                Commands::Playlists => Command::PlaylistsList,
                Commands::Clients => Command::ClientsList,
                Commands::Playback {
                    client_id,
                    action,
                    confirm,
                } => Command::PlaybackControl {
                    client_id,
                    command: action,
                    confirm,
                },
                Commands::Canon { library } => Command::CanonCoverage { library },
                Commands::RagSearch { query, limit } => Command::RagSearch {
                    query,
                    limit,
                    data_dir: cli.data_dir.clone(),
                },
                Commands::RagSync { limit_per_section } => Command::RagSync {
                    limit_per_section,
                    data_dir: cli.data_dir.clone(),
                },
                Commands::RagStatus => Command::RagStatus {
                    data_dir: cli.data_dir.clone(),
                },
                Commands::Taste => Command::TasteProfile,
                Commands::Chat {
                    prompt,
                    system,
                    provider,
                    model,
                    stream,
                } => {
                    if stream {
                        use plex_core::llm::{ChatMessage, chat_stream};
                        let mut messages = Vec::new();
                        if let Some(sys) = system {
                            messages.push(ChatMessage::system(sys));
                        }
                        messages.push(ChatMessage::user(prompt));
                        let mut out = std::io::stdout();
                        let mut sink = |token: &str| {
                            let _ = out.write_all(token.as_bytes());
                            let _ = out.flush();
                        };
                        match chat_stream(
                            provider, None, model, None, messages, 256 * 1024, &mut sink,
                        )
                        .await
                        {
                            Ok(_) => {
                                println!();
                                return;
                            }
                            Err(e) => {
                                eprintln!("rust-plex-mcp chat error: {e}");
                                std::process::exit(1);
                            }
                        }
                    }
                    Command::LlmChat {
                        provider: provider.unwrap_or_default(),
                        model,
                        prompt,
                        system,
                        context: None,
                        base_url: None,
                        api_key: None,
                    }
                }
                Commands::Perf => Command::PerfStatus,
                Commands::McpStdio => unreachable!(),
            };
            match dispatch(&config, command).await {
                Ok(val) => {
                    if cli.json {
                        println!("{val}");
                    } else {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string())
                        );
                    }
                }
                Err(e) => {
                    eprintln!("rust-plex-mcp error: {e}");
                    std::process::exit(1);
                }
            }
        }
    }
}

// --- MCP 2024-11-05 Tool Catalog -------------------------------------------

const TOOL_CATALOG: &[(&str, &str, &str)] = &[
    (
        "plex.status",
        "Show rust-plex-mcp operational status, server identity, and live-vs-demo mode.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.server_status",
        "Get Plex Media Server identity, version, machine identifier, and friendly name.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.libraries",
        "List all library sections (movies, shows, music, photos) on the Plex server.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.library_browse",
        "Browse items in a specific library section with pagination.",
        r#"{"type":"object","properties":{"section_key":{"type":"string"},"limit":{"type":"integer"},"offset":{"type":"integer"}},"required":["section_key"]}"#,
    ),
    (
        "plex.recently_added",
        "Get recently added media items across all libraries on the Plex server.",
        r#"{"type":"object","properties":{"limit":{"type":"integer"}}}"#,
    ),
    (
        "plex.search",
        "Search for movies, TV shows, music tracks, and artists across all Plex libraries.",
        r#"{"type":"object","properties":{"query":{"type":"string"}},"required":["query"]}"#,
    ),
    (
        "plex.media_get",
        "Get detailed metadata for a single media item by rating key.",
        r#"{"type":"object","properties":{"rating_key":{"type":"string"}},"required":["rating_key"]}"#,
    ),
    (
        "plex.sessions",
        "List currently active direct-play and transcode playback sessions on the Plex server.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.playlists",
        "List all user playlists on the Plex Media Server.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.clients",
        "List all player client devices connected to the Plex Media Server.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.playback_control",
        "Control playback on a Plex client (play, pause, stop, stepBack, stepForward). Requires confirm=true.",
        r#"{"type":"object","properties":{"client_id":{"type":"string"},"command":{"type":"string"},"confirm":{"type":"boolean"}},"required":["client_id","command"]}"#,
    ),
    (
        "plex.canon_coverage",
        "Analyze director, franchise, and collection canon completion gaps.",
        r#"{"type":"object","properties":{"library":{"type":"string"}}}"#,
    ),
    (
        "plex.taste_profile",
        "Get user taste profile, top genres, and viewing affinity recommendations.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "plex.media_enrich",
        "Get external enrichment metadata, subtitles status, and trailer info for an item.",
        r#"{"type":"object","properties":{"rating_key":{"type":"string"}},"required":["rating_key"]}"#,
    ),
    (
        "plex.rag_search",
        "Semantic AI search over media synopses and metadata (vector index when synced, keyword fallback otherwise).",
        r#"{"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"},"data_dir":{"type":"string"}},"required":["query"]}"#,
    ),
    (
        "plex.rag_sync",
        "Embed the whole library into the local LanceDB index (full rebuild, CPU minutes on first run).",
        r#"{"type":"object","properties":{"limit_per_section":{"type":"integer"},"data_dir":{"type":"string"}}}"#,
    ),
    (
        "plex.rag_status",
        "RAG index status: row count, vector dim, model, location.",
        r#"{"type":"object","properties":{"data_dir":{"type":"string"}}}"#,
    ),
    (
        "plex.perf_status",
        "Server transcode performance, active bandwidth, and hardware acceleration status.",
        r#"{"type":"object","properties":{}}"#,
    ),
    (
        "bridge.speak",
        "Speak text aloud via speech-mcp TTS service (:10909).",
        r#"{"type":"object","properties":{"text":{"type":"string"},"voice":{"type":"string"},"url":{"type":"string"}},"required":["text"]}"#,
    ),
    (
        "bridge.event",
        "Push a structured telemetry event to aiwatcher (:10946).",
        r#"{"type":"object","properties":{"kind":{"type":"string"},"device":{"type":"string"},"payload":{"type":"object"},"url":{"type":"string"}},"required":["kind","device","payload"]}"#,
    ),
    (
        "llm.chat",
        "Chat with conversational AI assistant (Ollama gemma4:12b default or cloud).",
        r#"{"type":"object","properties":{"provider":{"type":"string"},"model":{"type":"string"},"prompt":{"type":"string"},"system":{"type":"string"},"context":{"type":"object"}},"required":["provider","prompt"]}"#,
    ),
    (
        "llm.models",
        "Query available models dynamically from the LLM provider.",
        r#"{"type":"object","properties":{"provider":{"type":"string"},"base_url":{"type":"string"},"api_key":{"type":"string"}},"required":["provider"]}"#,
    ),
    // Legacy portmanteau aliases
    (
        "plex_library",
        "Legacy portmanteau: browse, search, sections.",
        r#"{"type":"object","properties":{"operation":{"type":"string"},"section_key":{"type":"string"},"query":{"type":"string"}}}"#,
    ),
    (
        "plex_media",
        "Legacy portmanteau: get_details, browse, search.",
        r#"{"type":"object","properties":{"operation":{"type":"string"},"rating_key":{"type":"string"}}}"#,
    ),
    (
        "plex_search",
        "Legacy portmanteau: search across libraries.",
        r#"{"type":"object","properties":{"operation":{"type":"string"},"query":{"type":"string"}}}"#,
    ),
    (
        "plex_server",
        "Legacy portmanteau: status, info, health.",
        r#"{"type":"object","properties":{"operation":{"type":"string"}}}"#,
    ),
    (
        "plex_streaming",
        "Legacy portmanteau: list_sessions, playback control.",
        r#"{"type":"object","properties":{"operation":{"type":"string"},"client_id":{"type":"string"},"command":{"type":"string"}}}"#,
    ),
    (
        "plex_playlist",
        "Legacy portmanteau: list, get playlists.",
        r#"{"type":"object","properties":{"operation":{"type":"string"}}}"#,
    ),
];

fn tool_list_response() -> Value {
    let tools: Vec<Value> = TOOL_CATALOG
        .iter()
        .map(|(name, desc, schema)| {
            let parsed_schema: Value = serde_json::from_str(schema).unwrap_or_else(|_| json!({}));
            json!({
                "name": name,
                "description": desc,
                "inputSchema": parsed_schema
            })
        })
        .collect();
    json!({ "tools": tools })
}

// --- Dual-Aliased Tool Dispatcher ------------------------------------------

async fn call_tool(config: &PlexConfig, name: &str, args: Value) -> Result<Value, DeviceError> {
    let norm = name.trim().replace('-', "_").to_lowercase();
    let op = args.get("operation").and_then(Value::as_str).unwrap_or("");

    let cmd = match norm.as_str() {
        "plex.status" | "plex_status" => Command::Status,
        "plex.server_status" | "plex_server_status" | "plex_server" => Command::ServerStatus,
        "plex.libraries" | "plex_libraries" => Command::LibrariesList,
        "plex.library_browse" | "plex_library_browse" => {
            let section_key = args
                .get("section_key")
                .and_then(Value::as_str)
                .unwrap_or("1")
                .to_string();
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            let offset = args
                .get("offset")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            Command::LibraryBrowse {
                section_key,
                limit,
                offset,
            }
        }
        "plex.recently_added" | "plex_recently_added" => {
            let limit = args
                .get("limit")
                .and_then(Value::as_u64)
                .map(|v| v as usize);
            Command::LibraryRecentlyAdded { limit }
        }
        "plex.search" | "plex_search" => {
            let q = args
                .get("query")
                .or_else(|| args.get("q"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            Command::LibrarySearch { query: q }
        }
        "plex.media_get" | "plex_media_get" | "plex_media" => {
            let rating_key = args
                .get("rating_key")
                .or_else(|| args.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("1")
                .to_string();
            Command::MediaGet { rating_key }
        }
        "plex.sessions" | "plex_sessions" => Command::SessionsList,
        "plex.playlists" | "plex_playlists" | "plex_playlist" => Command::PlaylistsList,
        "plex.clients" | "plex_clients" => Command::ClientsList,
        "plex.playback_control" | "plex_playback_control" | "plex_streaming" => {
            if op == "list_sessions" || norm == "plex_streaming" && op.is_empty() {
                Command::SessionsList
            } else {
                let client_id = args
                    .get("client_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let command = args
                    .get("command")
                    .or_else(|| args.get("action"))
                    .and_then(Value::as_str)
                    .unwrap_or("play")
                    .to_string();
                let confirm = args
                    .get("confirm")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                Command::PlaybackControl {
                    client_id,
                    command,
                    confirm,
                }
            }
        }
        "plex.canon_coverage" | "plex_canon_coverage" | "plex_canon" | "plex_completeness" => {
            let lib = args
                .get("library")
                .and_then(Value::as_str)
                .map(String::from);
            Command::CanonCoverage { library: lib }
        }
        "plex.taste_profile" | "plex_taste_profile" | "plex_taste" => Command::TasteProfile,
        "plex.media_enrich" | "plex_media_enrich" | "plex_media_enrichment" => {
            let rk = args
                .get("rating_key")
                .and_then(Value::as_str)
                .unwrap_or("1")
                .to_string();
            Command::MediaEnrich { rating_key: rk }
        }
        "plex.rag_search" | "plex_rag_search" | "plex_rag" => {
            // Legacy portmanteau routed by operation: sync_metadata /
            // sync_subtitles -> RagSync, status -> RagStatus, else search.
            let data_dir = args
                .get("data_dir")
                .and_then(Value::as_str)
                .map(String::from);
            match op {
                "sync_metadata" | "sync_subtitles" => Command::RagSync {
                    limit_per_section: None,
                    data_dir,
                },
                "status" => Command::RagStatus { data_dir },
                _ => {
                    let q = args
                        .get("query")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let limit = args
                        .get("limit")
                        .and_then(Value::as_u64)
                        .map(|n| n as usize);
                    Command::RagSearch {
                        query: q,
                        limit,
                        data_dir,
                    }
                }
            }
        }
        "plex.rag_sync" | "plex_rag_sync" => {
            let per_section = args
                .get("limit_per_section")
                .and_then(Value::as_u64)
                .map(|n| n as usize);
            let data_dir = args
                .get("data_dir")
                .and_then(Value::as_str)
                .map(String::from);
            Command::RagSync {
                limit_per_section: per_section,
                data_dir,
            }
        }
        "plex.rag_status" | "plex_rag_status" => {
            let data_dir = args
                .get("data_dir")
                .and_then(Value::as_str)
                .map(String::from);
            Command::RagStatus { data_dir }
        }
        "plex.perf_status" | "plex_perf_status" | "plex_performance" => Command::PerfStatus,
        "bridge.speak" | "bridge_speak" => {
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let voice = args.get("voice").and_then(Value::as_str).map(String::from);
            let url = args.get("url").and_then(Value::as_str).map(String::from);
            Command::BridgeSpeak { text, voice, url }
        }
        "bridge.event" | "bridge_event" => {
            let kind = args
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("plex.event")
                .to_string();
            let device = args
                .get("device")
                .and_then(Value::as_str)
                .unwrap_or("rust-plex-mcp")
                .to_string();
            let payload = args.get("payload").cloned().unwrap_or_else(|| json!({}));
            let url = args.get("url").and_then(Value::as_str).map(String::from);
            Command::BridgeEvent {
                kind,
                device,
                payload,
                url,
            }
        }
        "llm.chat" | "llm_chat" => {
            let provider = args
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("ollama")
                .to_string();
            let model = args.get("model").and_then(Value::as_str).map(String::from);
            let prompt = args
                .get("prompt")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let system = args.get("system").and_then(Value::as_str).map(String::from);
            let context = args.get("context").cloned();
            let base_url = args
                .get("base_url")
                .and_then(Value::as_str)
                .map(String::from);
            let api_key = args
                .get("api_key")
                .and_then(Value::as_str)
                .map(String::from);
            Command::LlmChat {
                provider,
                model,
                prompt,
                system,
                context,
                base_url,
                api_key,
            }
        }
        "llm.models" | "llm_models" => {
            let provider = args
                .get("provider")
                .and_then(Value::as_str)
                .unwrap_or("ollama")
                .to_string();
            let base_url = args
                .get("base_url")
                .and_then(Value::as_str)
                .map(String::from);
            let api_key = args
                .get("api_key")
                .and_then(Value::as_str)
                .map(String::from);
            Command::LlmModels {
                provider,
                base_url,
                api_key,
            }
        }
        // Legacy portmanteau catch-alls
        "plex_library" => match op {
            "search" => {
                let q = args
                    .get("query")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                Command::LibrarySearch { query: q }
            }
            "browse" => {
                let key = args
                    .get("section_key")
                    .and_then(Value::as_str)
                    .unwrap_or("1")
                    .to_string();
                Command::LibraryBrowse {
                    section_key: key,
                    limit: Some(50),
                    offset: Some(0),
                }
            }
            _ => Command::LibrariesList,
        },
        _ => return Err(DeviceError::InvalidInput(format!("unknown tool '{name}'"))),
    };

    dispatch(config, cmd).await
}

// --- MCP 2024-11-05 Stdio JSON-RPC Loop ------------------------------------

async fn run_mcp_stdio(config: &PlexConfig) -> Result<(), DeviceError> {
    eprintln!("rust-plex-mcp: running MCP 2024-11-05 stdio server");
    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                eprintln!("rust-plex-mcp stdio read error: {e}");
                break;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                let err_res = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                let mut out = stdout.lock();
                let _ = writeln!(out, "{err_res}");
                let _ = out.flush();
                continue;
            }
        };

        let req_id = req.get("id").cloned().unwrap_or(Value::Null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");

        let resp = match method {
            "initialize" => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": {
                        "protocolVersion": "2024-11-05",
                        "serverInfo": {
                            "name": "rust-plex-mcp",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {
                            "tools": { "listChanged": false }
                        }
                    }
                })
            }
            "notifications/initialized" => continue,
            "ping" => json!({ "jsonrpc": "2.0", "id": req_id, "result": {} }),
            "tools/list" => {
                let list = tool_list_response();
                json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "result": list
                })
            }
            "tools/call" => {
                let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                match call_tool(config, name, args).await {
                    Ok(val) => {
                        let text =
                            serde_json::to_string_pretty(&val).unwrap_or_else(|_| val.to_string());
                        json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {
                                "content": [{ "type": "text", "text": text }]
                            }
                        })
                    }
                    Err(e) => {
                        json!({
                            "jsonrpc": "2.0",
                            "id": req_id,
                            "result": {
                                "isError": true,
                                "content": [{ "type": "text", "text": format!("Error: {e}") }]
                            }
                        })
                    }
                }
            }
            unknown => {
                json!({
                    "jsonrpc": "2.0",
                    "id": req_id,
                    "error": {
                        "code": -32601,
                        "message": format!("Method not found: '{unknown}'")
                    }
                })
            }
        };

        let mut out = stdout.lock();
        let _ = writeln!(out, "{resp}");
        let _ = out.flush();
    }
    Ok(())
}
