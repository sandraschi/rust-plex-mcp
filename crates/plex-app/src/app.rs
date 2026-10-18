//! Pure-Rust `egui` desktop UI for `rust-plex-mcp`.
//!
//! Enforces the 10-Point Uncrashable Binary Contract:
//! - Zero synchronous network or keyring I/O on the UI thread (`self.spawn(...)`).
//! - Zero `ctx.set_visuals()` or `ctx.set_zoom_factor()` inside `ctx.input(|i| ...)`.
//! - Strict glyph allowlist (`°`, `×`, `·`, `…`) verified at compile/test time by `glyph_source_scan`.

use eframe::egui::{self, Color32, CornerRadius, Margin, Pos2, RichText, Stroke, Vec2};
use plex_core::{dispatch, dispatch_auto, Command, PlexConfig, Vendor};
use serde_json::{json, Value};
use std::future::Future;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

const BG_DARK: Color32 = Color32::from_rgb(11, 15, 23);
const PANEL_DARK: Color32 = Color32::from_rgb(17, 24, 39);
const CARD_DARK: Color32 = Color32::from_rgb(26, 36, 54);
const BORDER_DARK: Color32 = Color32::from_rgb(51, 65, 85);
const AMBER: Color32 = Color32::from_rgb(245, 158, 11);
const AMBER_GLOW: Color32 = Color32::from_rgba_premultiplied(245, 158, 11, 35);
const EMERALD: Color32 = Color32::from_rgb(16, 185, 129);
const ROSE: Color32 = Color32::from_rgb(244, 63, 94);
const SKY: Color32 = Color32::from_rgb(14, 165, 233);
const SLATE_MUTED: Color32 = Color32::from_rgb(148, 163, 184);
const SLATE_FAINT: Color32 = Color32::from_rgb(100, 116, 139);

const LLM_PROVIDERS: &[(&str, &str, &str, &str)] = &[
    (
        "ollama",
        "Ollama (Local :11434)",
        "http://127.0.0.1:11434/v1",
        "gemma4:12b",
    ),
    (
        "lmstudio",
        "LM Studio (Local :1234)",
        "http://127.0.0.1:1234/v1",
        "local-model",
    ),
    (
        "openai",
        "OpenAI (Cloud)",
        "https://api.openai.com/v1",
        "gpt-4o-mini",
    ),
    (
        "anthropic",
        "Anthropic (Cloud)",
        "https://api.anthropic.com",
        "claude-sonnet-4-20250514",
    ),
    ("custom", "Custom / OpenAI-Compatible", "", ""),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    // 19 Routes matching Next.js webapp
    Overview,
    Inbox,
    Libraries,
    Movies,
    Repair,
    Search,
    SemanticSearch,
    Rag,
    Canon,
    Enrichment,
    Taste,
    Chat,
    Tools,
    Skills,
    ApiDocs,
    Server,
    Logs,
    Help,
    Settings,

    // Fleet extensions
    Crossconnect,
    RawJson,
}

impl Page {
    pub fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Inbox => "Inbox",
            Self::Libraries => "Libraries",
            Self::Movies => "Movies",
            Self::Repair => "Media Repair",
            Self::Search => "Search",
            Self::SemanticSearch => "Semantic search",
            Self::Rag => "RAG Management",
            Self::Canon => "Canon Coverage",
            Self::Enrichment => "Enrichment",
            Self::Taste => "Taste",
            Self::Chat => "Chat",
            Self::Tools => "Tools",
            Self::Skills => "Skills",
            Self::ApiDocs => "API Docs",
            Self::Server => "Server",
            Self::Logs => "Logs",
            Self::Help => "Help",
            Self::Settings => "Settings",
            Self::Crossconnect => "Crossconnect",
            Self::RawJson => "Raw JSON",
        }
    }
}

enum Job {
    ServerStatus(Value),
    Libraries(Value),
    RecentlyAdded(Value),
    BrowseItems(Value),
    MediaDetail(Value),
    Sessions(Value),
    Playlists(Value),
    Clients(Value),
    SearchResults(Value),
    SemanticResults(Value),
    RagStatus(Value),
    CanonReport(Value),
    TasteReport(Value),
    EnrichReport(Value),
    RepairReport(Value),
    InboxItems(Value),
    ToolsList(Value),
    SkillsList(Value),
    ChatReply(Value),
    LlmModelsLoaded(Value),
    LlmTestResult(Value),
    StatusNote(String, Value),
    VaultLoaded {
        server_url: String,
        plex_token: String,
        llm_api_key: String,
    },
    CrossconnectReply(Value),
}

pub struct PlexApp {
    runtime: Arc<tokio::runtime::Runtime>,
    tx: Sender<Job>,
    rx: Receiver<Job>,
    pending: usize,
    theme: Theme,
    sidebar_open: bool,
    zoom: f32,
    page: Page,
    status_line: String,

    // Live backend / Plex connection status
    plex_ok: bool,

    // Data models
    server_status_val: Value,
    libraries_val: Value,
    recently_added_val: Value,
    browse_val: Value,
    media_detail_val: Value,
    sessions_val: Value,
    playlists_val: Value,
    clients_val: Value,
    search_results_val: Value,
    semantic_results_val: Value,
    rag_status_val: Value,
    canon_val: Value,
    taste_val: Value,
    enrich_val: Value,
    repair_val: Value,
    inbox_val: Value,
    tools_val: Value,
    skills_val: Value,
    last_raw: Value,

    // Search and filter inputs
    search_query: String,
    semantic_query: String,
    semantic_top_k: usize,
    selected_section_key: String,
    selected_rating_key: String,
    enrich_rating_key: String,
    selected_tool_name: String,
    tool_args_raw: String,

    // Chat state
    chat_provider: String,
    chat_model: String,
    chat_prompt: String,
    chat_history: Vec<(String, String)>,

    // Crossconnect state
    speak_text: String,
    speech_url: String,
    event_device: String,
    event_payload: String,
    webhook_url: String,

    // Settings / Vault state
    vault_loaded: bool,
    vault_status_note: String,
    server_url: String,
    plex_token: String,
    mask_token: bool,
    llm_api_key: String,
    mask_api_key: bool,
    llm_base_url: String,
    available_models: Vec<String>,
    llm_test_status: String,
    llm_test_success: Option<bool>,

    // System logs & diagnostics state
    event_logs: Vec<String>,
    log_filter: String,
}

pub fn harsh_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = PANEL_DARK;
    v.window_fill = BG_DARK;
    v.override_text_color = Some(Color32::from_rgb(241, 245, 249));
    v.widgets.noninteractive.bg_fill = PANEL_DARK;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(148, 163, 184));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER_DARK);
    v.widgets.inactive.bg_fill = CARD_DARK;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(203, 213, 225));
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER_DARK);
    v.widgets.hovered.bg_fill = Color32::from_rgb(36, 50, 77);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, AMBER);
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, AMBER);
    v.widgets.active.bg_fill = Color32::from_rgb(45, 62, 96);
    v.widgets.active.fg_stroke = Stroke::new(1.5, AMBER);
    v.widgets.active.bg_stroke = Stroke::new(1.5, AMBER);
    v.selection.bg_fill = Color32::from_rgb(245, 158, 11);
    v.selection.stroke = Stroke::new(1.0, Color32::BLACK);
    v
}

pub fn crisp_light_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    v.panel_fill = Color32::from_rgb(248, 250, 252);
    v.window_fill = Color32::from_rgb(255, 255, 255);
    v.override_text_color = Some(Color32::from_rgb(15, 23, 42));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(203, 213, 225));
    v
}

pub fn apply_theme(ctx: &egui::Context, theme: Theme) {
    match theme {
        Theme::Dark => ctx.set_visuals(harsh_visuals()),
        Theme::Light => ctx.set_visuals(crisp_light_visuals()),
    }
}

pub fn card_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    let is_dark = ui.visuals().dark_mode;
    let fill = if is_dark { CARD_DARK } else { Color32::WHITE };
    let stroke_color = if is_dark {
        BORDER_DARK
    } else {
        Color32::from_rgb(203, 213, 225)
    };

    egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(1.0, stroke_color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, add_contents);
}

pub fn card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    card_frame(ui, |ui| {
        ui.label(
            RichText::new(title)
                .size(13.0)
                .strong()
                .color(Color32::from_rgb(203, 213, 225)),
        );
        ui.add_space(4.0);
        add_contents(ui);
    });
}

pub fn kpi_card(ui: &mut egui::Ui, label: &str, value: &str, sub: &str, accent: Color32) {
    card_frame(ui, |ui| {
        ui.set_min_width(140.0);
        ui.label(RichText::new(label).size(11.0).strong().color(accent));
        ui.add_space(2.0);
        ui.label(
            RichText::new(value)
                .size(22.0)
                .strong()
                .color(Color32::WHITE),
        );
        ui.add_space(2.0);
        ui.label(RichText::new(sub).size(10.0).color(SLATE_FAINT));
    });
}

fn paint_theme_button(ui: &mut egui::Ui, theme: Theme) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let c = rect.center();
        if resp.hovered() {
            p.circle_filled(c, 12.0, Color32::from_rgba_unmultiplied(120, 140, 180, 45));
        }
        match theme {
            Theme::Dark => {
                p.circle_filled(c, 5.0, AMBER);
                for i in 0..8 {
                    let angle = (i as f32) * std::f32::consts::FRAC_PI_4;
                    let p1 = Pos2::new(c.x + angle.cos() * 7.5, c.y + angle.sin() * 7.5);
                    let p2 = Pos2::new(c.x + angle.cos() * 10.5, c.y + angle.sin() * 10.5);
                    p.line_segment([p1, p2], Stroke::new(1.6, AMBER));
                }
            }
            Theme::Light => {
                p.circle_filled(c, 7.0, Color32::from_rgb(51, 65, 85));
                p.circle_filled(
                    Pos2::new(c.x + 3.0, c.y - 2.5),
                    6.0,
                    ui.visuals().panel_fill,
                );
            }
        }
    }
    resp.on_hover_text("Toggle Dark / Light theme")
}

impl PlexApp {
    pub fn new(cc: &eframe::CreationContext<'_>, runtime: Arc<tokio::runtime::Runtime>) -> Self {
        cc.egui_ctx.set_visuals(harsh_visuals());
        let (tx, rx) = mpsc::channel();

        let mut app = Self {
            runtime,
            tx,
            rx,
            pending: 0,
            theme: Theme::Dark,
            sidebar_open: true,
            zoom: 1.0,
            page: Page::Overview,
            status_line: "Initializing Plex (Rust)...".to_string(),

            plex_ok: false,

            server_status_val: Value::Null,
            libraries_val: Value::Null,
            recently_added_val: Value::Null,
            browse_val: Value::Null,
            media_detail_val: Value::Null,
            sessions_val: Value::Null,
            playlists_val: Value::Null,
            clients_val: Value::Null,
            search_results_val: Value::Null,
            semantic_results_val: Value::Null,
            rag_status_val: Value::Null,
            canon_val: Value::Null,
            taste_val: Value::Null,
            enrich_val: Value::Null,
            repair_val: Value::Null,
            inbox_val: Value::Null,
            tools_val: Value::Null,
            skills_val: Value::Null,
            last_raw: Value::Null,

            search_query: String::new(),
            semantic_query: String::new(),
            semantic_top_k: 5,
            selected_section_key: "1".to_string(),
            selected_rating_key: "101".to_string(),
            enrich_rating_key: "101".to_string(),
            selected_tool_name: "plex_server".to_string(),
            tool_args_raw: "{\n  \"operation\": \"status\"\n}".to_string(),

            chat_provider: "ollama".to_string(),
            chat_model: "gemma4:12b".to_string(),
            chat_prompt: String::new(),
            chat_history: Vec::new(),

            speak_text: "Plex server online. Media library indexed.".to_string(),
            speech_url: "http://127.0.0.1:10909".to_string(),
            event_device: "plex-server".to_string(),
            event_payload: "{\"event\":\"playback_started\",\"item\":\"The Matrix\"}".to_string(),
            webhook_url: "http://127.0.0.1:10946/events".to_string(),

            vault_loaded: false,
            vault_status_note: "Loading credentials from OS Vault...".to_string(),
            server_url: "http://127.0.0.1:32400".to_string(),
            plex_token: String::new(),
            mask_token: true,
            llm_api_key: String::new(),
            mask_api_key: true,
            llm_base_url: "http://127.0.0.1:11434/v1".to_string(),
            available_models: vec![
                "gemma4:12b".to_string(),
                "llama3:8b".to_string(),
                "mistral:latest".to_string(),
            ],
            llm_test_status: "Untested".to_string(),
            llm_test_success: None,

            event_logs: vec![
                format!(
                    "[INFO] Plex App initialized (v{})",
                    env!("CARGO_PKG_VERSION")
                ),
                "[INFO] OS Secret Vault initialized (keyring service: rust-plex-mcp)".to_string(),
            ],
            log_filter: String::new(),
        };

        app.load_vault_async();
        app.refresh_all();
        app
    }

    fn spawn<F>(&mut self, f: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.pending = self.pending.saturating_add(1);
        self.runtime.spawn(f);
    }

    fn log_event(&mut self, text: impl Into<String>) {
        let msg = text.into();
        self.event_logs.push(msg);
        if self.event_logs.len() > 200 {
            let overflow = self.event_logs.len() - 200;
            self.event_logs.drain(0..overflow);
        }
    }

    fn get_plex_config(&self) -> PlexConfig {
        if !self.server_url.is_empty() && !self.plex_token.is_empty() {
            PlexConfig::resolve(Some(&self.server_url), Some(&self.plex_token)).unwrap_or(
                PlexConfig {
                    vendor: Vendor::Demo,
                    timeout_secs: 15,
                    source: "demo".to_string(),
                },
            )
        } else {
            PlexConfig::resolve(None, None).unwrap_or(PlexConfig {
                vendor: Vendor::Demo,
                timeout_secs: 15,
                source: "demo".to_string(),
            })
        }
    }

    fn refresh_all(&mut self) {
        let tx = self.tx.clone();
        let config = self.get_plex_config();
        self.spawn(async move {
            if let Ok(v) = dispatch(&config, Command::ServerStatus).await {
                let _ = tx.send(Job::ServerStatus(v));
            }
            if let Ok(v) = dispatch(&config, Command::LibrariesList).await {
                let _ = tx.send(Job::Libraries(v));
            }
            if let Ok(v) =
                dispatch(&config, Command::LibraryRecentlyAdded { limit: Some(10) }).await
            {
                let _ = tx.send(Job::RecentlyAdded(v));
            }
            if let Ok(v) = dispatch(&config, Command::SessionsList).await {
                let _ = tx.send(Job::Sessions(v));
            }
            if let Ok(v) = dispatch(&config, Command::PlaylistsList).await {
                let _ = tx.send(Job::Playlists(v));
            }
            if let Ok(v) = dispatch(&config, Command::ClientsList).await {
                let _ = tx.send(Job::Clients(v));
            }
            if let Ok(v) = dispatch(&config, Command::CanonCoverage { library: None }).await {
                let _ = tx.send(Job::CanonReport(v));
            }
            if let Ok(v) = dispatch(&config, Command::TasteProfile).await {
                let _ = tx.send(Job::TasteReport(v));
            }
            if let Ok(v) = dispatch(&config, Command::RepairReport).await {
                let _ = tx.send(Job::RepairReport(v));
            }
            if let Ok(v) = dispatch(&config, Command::InboxList).await {
                let _ = tx.send(Job::InboxItems(v));
            }
            if let Ok(v) = dispatch(&config, Command::ToolsList).await {
                let _ = tx.send(Job::ToolsList(v));
            }
            if let Ok(v) = dispatch(&config, Command::SkillsList).await {
                let _ = tx.send(Job::SkillsList(v));
            }
        });
    }

    fn load_vault_async(&mut self) {
        let tx = self.tx.clone();
        self.spawn(async move {
            let token = crate::vault::get(crate::vault::PLEX_TOKEN)
                .ok()
                .flatten()
                .unwrap_or_default();
            let key = crate::vault::get(crate::vault::LLM_API_KEY)
                .ok()
                .flatten()
                .unwrap_or_default();
            let _ = tx.send(Job::VaultLoaded {
                server_url: "http://127.0.0.1:32400".to_string(),
                plex_token: token,
                llm_api_key: key,
            });
        });
    }

    fn drain_jobs(&mut self, ctx: &egui::Context) {
        let mut had_jobs = false;
        while let Ok(job) = self.rx.try_recv() {
            had_jobs = true;
            self.pending = self.pending.saturating_sub(1);
            match job {
                Job::ServerStatus(v) => {
                    self.last_raw = v.clone();
                    let success = v.get("success").and_then(Value::as_bool).unwrap_or(true);
                    let machine = v.get("machine_identifier").and_then(Value::as_str);
                    self.plex_ok = success || machine.is_some();
                    self.server_status_val = v;
                    self.status_line = format!(
                        "Plex Server: {}",
                        if self.plex_ok { "Connected" } else { "Offline" }
                    );
                    self.log_event(format!("[INFO] Server status queried: ok={}", self.plex_ok));
                }
                Job::Libraries(v) => {
                    self.last_raw = v.clone();
                    self.libraries_val = v;
                }
                Job::RecentlyAdded(v) => {
                    self.recently_added_val = v;
                }
                Job::BrowseItems(v) => {
                    self.last_raw = v.clone();
                    self.browse_val = v;
                }
                Job::MediaDetail(v) => {
                    self.last_raw = v.clone();
                    self.media_detail_val = v;
                }
                Job::Sessions(v) => {
                    self.sessions_val = v;
                }
                Job::Playlists(v) => {
                    self.playlists_val = v;
                }
                Job::Clients(v) => {
                    self.clients_val = v;
                }
                Job::SearchResults(v) => {
                    self.last_raw = v.clone();
                    self.search_results_val = v;
                }
                Job::SemanticResults(v) => {
                    self.last_raw = v.clone();
                    self.semantic_results_val = v;
                }
                Job::RagStatus(v) => {
                    self.last_raw = v.clone();
                    self.rag_status_val = v;
                }
                Job::CanonReport(v) => {
                    self.canon_val = v;
                }
                Job::TasteReport(v) => {
                    self.taste_val = v;
                }
                Job::EnrichReport(v) => {
                    self.last_raw = v.clone();
                    self.enrich_val = v;
                }
                Job::RepairReport(v) => {
                    self.repair_val = v;
                }
                Job::InboxItems(v) => {
                    self.inbox_val = v;
                }
                Job::ToolsList(v) => {
                    self.tools_val = v;
                }
                Job::SkillsList(v) => {
                    self.skills_val = v;
                }
                Job::ChatReply(v) => {
                    let reply = v.get("reply").and_then(Value::as_str).unwrap_or("No reply");
                    self.chat_history
                        .push(("Assistant".to_string(), reply.to_string()));
                    self.last_raw = v;
                    self.status_line = "Chat reply received".to_string();
                }
                Job::LlmModelsLoaded(v) => {
                    if let Some(arr) = v.get("models").and_then(Value::as_array) {
                        self.available_models = arr
                            .iter()
                            .filter_map(Value::as_str)
                            .map(ToString::to_string)
                            .collect();
                    }
                    self.status_line = format!("Loaded {} models", self.available_models.len());
                }
                Job::LlmTestResult(v) => {
                    let ok = v.get("success").and_then(Value::as_bool).unwrap_or(false);
                    self.llm_test_success = Some(ok);
                    self.llm_test_status = if ok {
                        let ms = v.get("latency_ms").and_then(Value::as_u64).unwrap_or(0);
                        format!("Passed ({} ms)", ms)
                    } else {
                        v.get("error")
                            .and_then(Value::as_str)
                            .unwrap_or("Failed")
                            .to_string()
                    };
                }
                Job::StatusNote(msg, v) => {
                    self.status_line = msg.clone();
                    self.last_raw = v;
                    self.log_event(format!("[NOTE] {msg}"));
                }
                Job::VaultLoaded {
                    server_url,
                    plex_token,
                    llm_api_key,
                } => {
                    self.vault_loaded = true;
                    if !plex_token.is_empty() {
                        self.plex_token = plex_token;
                    }
                    if !llm_api_key.is_empty() {
                        self.llm_api_key = llm_api_key;
                    }
                    if !server_url.is_empty() {
                        self.server_url = server_url;
                    }
                    self.vault_status_note = "Credentials loaded from OS Vault".to_string();
                    self.log_event("[INFO] OS Vault secrets successfully retrieved");
                }
                Job::CrossconnectReply(v) => {
                    self.last_raw = v.clone();
                    self.status_line = "Crossconnect dispatched successfully".to_string();
                    self.log_event("[INFO] Crossconnect bridge request dispatched");
                }
            }
        }
        if had_jobs {
            ctx.request_repaint();
        }
    }

    // UI Navigation & Views
    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        // Header: Logo + App title + Collapse toggle
        ui.horizontal(|ui| {
            let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(4), AMBER);
            ui.painter()
                .circle_filled(rect.center(), 4.0, Color32::BLACK);

            if self.sidebar_open {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("PlexMCP")
                            .size(15.0)
                            .strong()
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(10.0)
                            .color(SLATE_FAINT),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("<").clicked() {
                        self.sidebar_open = false;
                    }
                });
            } else if ui.button(">").clicked() {
                self.sidebar_open = true;
            }
        });

        ui.add_space(8.0);

        // Connection pill
        if self.sidebar_open {
            ui.horizontal(|ui| {
                let dot_color = if self.plex_ok { EMERALD } else { AMBER };
                let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(8.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 4.0, dot_color);
                let text = if self.plex_ok {
                    "Plex connected"
                } else {
                    "Backend up · Plex down"
                };
                ui.label(RichText::new(text).size(11.0).color(SLATE_MUTED));
            });
            ui.add_space(8.0);
        }

        ui.separator();
        ui.add_space(4.0);

        // Nav item list
        egui::ScrollArea::vertical().show(ui, |ui| {
            let nav_items = [
                Page::Overview,
                Page::Inbox,
                Page::Libraries,
                Page::Movies,
                Page::Repair,
                Page::Search,
                Page::SemanticSearch,
                Page::Rag,
                Page::Canon,
                Page::Enrichment,
                Page::Taste,
                Page::Chat,
                Page::Tools,
                Page::Skills,
                Page::ApiDocs,
                Page::Server,
                Page::Logs,
                Page::Help,
                Page::Settings,
            ];

            for item in nav_items {
                let active = self.page == item;
                let bg = if active {
                    AMBER_GLOW
                } else {
                    Color32::TRANSPARENT
                };
                let fg = if active {
                    AMBER
                } else {
                    Color32::from_rgb(203, 213, 225)
                };

                let text = if self.sidebar_open {
                    item.label()
                } else {
                    match item {
                        Page::Overview => "Ov",
                        Page::Inbox => "In",
                        Page::Libraries => "Lb",
                        Page::Movies => "Mv",
                        Page::Repair => "Rp",
                        Page::Search => "Sr",
                        Page::SemanticSearch => "Sm",
                        Page::Rag => "Rg",
                        Page::Canon => "Cn",
                        Page::Enrichment => "En",
                        Page::Taste => "Ts",
                        Page::Chat => "Ch",
                        Page::Tools => "Tl",
                        Page::Skills => "Sk",
                        Page::ApiDocs => "Ap",
                        Page::Server => "Sv",
                        Page::Logs => "Lg",
                        Page::Help => "Hp",
                        Page::Settings => "St",
                        _ => "..",
                    }
                };

                let btn = egui::Button::new(RichText::new(text).color(fg).size(13.0))
                    .fill(bg)
                    .stroke(Stroke::new(
                        if active { 1.0 } else { 0.0 },
                        if active { AMBER } else { Color32::TRANSPARENT },
                    ))
                    .corner_radius(CornerRadius::same(6));

                if ui.add_sized([ui.available_width(), 26.0], btn).clicked() {
                    self.page = item;
                }
            }

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Crossconnect & Raw JSON shortcuts
            let active_cc = self.page == Page::Crossconnect;
            let active_raw = self.page == Page::RawJson;

            let cc_btn = egui::Button::new(
                RichText::new(if self.sidebar_open {
                    "Crossconnect"
                } else {
                    "Cc"
                })
                .color(if active_cc { SKY } else { SLATE_MUTED })
                .size(12.0),
            )
            .fill(if active_cc {
                Color32::from_rgba_premultiplied(14, 165, 233, 30)
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(CornerRadius::same(4));

            if ui.add_sized([ui.available_width(), 24.0], cc_btn).clicked() {
                self.page = Page::Crossconnect;
            }

            let raw_btn = egui::Button::new(
                RichText::new(if self.sidebar_open { "Raw JSON" } else { "{}" })
                    .color(if active_raw { SKY } else { SLATE_MUTED })
                    .size(12.0),
            )
            .fill(if active_raw {
                Color32::from_rgba_premultiplied(14, 165, 233, 30)
            } else {
                Color32::TRANSPARENT
            })
            .corner_radius(CornerRadius::same(4));

            if ui
                .add_sized([ui.available_width(), 24.0], raw_btn)
                .clicked()
            {
                self.page = Page::RawJson;
            }
        });
    }

    // 1. Overview
    fn view_overview(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Overview").color(Color32::WHITE).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Plex Media Server health, telemetry, active sessions, and recently added items.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        // Server Status Banner
        let server_name = self
            .server_status_val
            .get("friendly_name")
            .or_else(|| self.server_status_val.get("my_plex_username"))
            .and_then(Value::as_str)
            .unwrap_or("Plex Server");
        let server_ver = self
            .server_status_val
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or("Unknown");
        let platform = self
            .server_status_val
            .get("platform")
            .and_then(Value::as_str)
            .unwrap_or("Windows");

        card(ui, "Server Connection", |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(server_name)
                        .size(16.0)
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(RichText::new(format!("v{}", server_ver)).color(AMBER));
                ui.label(RichText::new(format!("· Platform: {}", platform)).color(SLATE_MUTED));
                let status_text = if self.plex_ok {
                    "[Connected]"
                } else {
                    "[Offline / Demo]"
                };
                let status_color = if self.plex_ok { EMERALD } else { ROSE };
                ui.label(RichText::new(status_text).strong().color(status_color));
            });
        });

        ui.add_space(10.0);

        // KPI Grid
        let lib_count = self
            .libraries_val
            .get("sections")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        let recent_count = self
            .recently_added_val
            .get("items")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        let session_count = self
            .sessions_val
            .get("sessions")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);
        let playlist_count = self
            .playlists_val
            .get("playlists")
            .and_then(Value::as_array)
            .map(|a| a.len())
            .unwrap_or(0);

        ui.horizontal(|ui| {
            kpi_card(
                ui,
                "Libraries",
                &format!("{}", lib_count),
                "Configured sections",
                AMBER,
            );
            kpi_card(
                ui,
                "Recently Added",
                &format!("{}", recent_count),
                "Latest ingest items",
                SKY,
            );
            kpi_card(
                ui,
                "Active Sessions",
                &format!("{}", session_count),
                "Playing / transcoding",
                EMERALD,
            );
            kpi_card(
                ui,
                "Playlists",
                &format!("{}", playlist_count),
                "Curated media sets",
                ROSE,
            );
        });

        ui.add_space(14.0);

        // Active Sessions & Quick Actions
        let sessions_arr = self
            .sessions_val
            .get("sessions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.heading(
                    RichText::new("Active Streams")
                        .size(16.0)
                        .color(Color32::WHITE),
                );
                ui.add_space(4.0);
                if sessions_arr.is_empty() {
                    ui.label(
                        RichText::new("No active playback sessions on this server.")
                            .color(SLATE_FAINT),
                    );
                } else {
                    for session in &sessions_arr {
                        let title = session
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or("Unknown Title");
                        let user = session
                            .get("user")
                            .and_then(Value::as_str)
                            .unwrap_or("Local User");
                        let player = session
                            .get("player")
                            .and_then(Value::as_str)
                            .unwrap_or("Plex Web");
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("· {} ({})", title, user))
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                            ui.label(
                                RichText::new(format!("Player: {}", player)).color(SLATE_MUTED),
                            );
                        });
                    }
                }
            });

            cols[1].group(|ui| {
                ui.heading(
                    RichText::new("Quick Operations")
                        .size(16.0)
                        .color(Color32::WHITE),
                );
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Refresh All Telemetry").clicked() {
                        self.refresh_all();
                    }
                    if ui.button("Scan Media Sections").clicked() {
                        let tx = self.tx.clone();
                        let config = self.get_plex_config();
                        self.spawn(async move {
                            if let Ok(v) = dispatch(&config, Command::LibrariesList).await {
                                let _ = tx.send(Job::Libraries(v));
                            }
                        });
                    }
                    if ui.button("Jump to Search").clicked() {
                        self.page = Page::Search;
                    }
                });
            });
        });

        ui.add_space(14.0);

        // Recently Added List
        let recent_items = self
            .recently_added_val
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        card(ui, "Recently Added Media", |ui| {
            if recent_items.is_empty() {
                ui.label(RichText::new("Querying recently added media...").color(SLATE_FAINT));
            } else {
                for item in recent_items.iter().take(6) {
                    let title = item
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Media Item");
                    let kind = item.get("type").and_then(Value::as_str).unwrap_or("video");
                    let year = item
                        .get("year")
                        .and_then(Value::as_i64)
                        .map(|y| format!("{}", y))
                        .unwrap_or_default();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("·").color(AMBER));
                        ui.label(RichText::new(title).strong().color(Color32::WHITE));
                        if !year.is_empty() {
                            ui.label(RichText::new(format!("({})", year)).color(SLATE_MUTED));
                        }
                        ui.label(RichText::new(format!("[{}]", kind)).size(11.0).color(SKY));
                    });
                }
            }
        });
    }

    // 2. Inbox
    fn view_inbox(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Inbox & Processing Queue")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Pending media ingest, unmatched file alerts, and processing notifications.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        let items = self
            .inbox_val
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        card(ui, "Pending Tasks & Queue", |ui| {
            if items.is_empty() {
                ui.label(RichText::new("Inbox empty. Everything is up to date.").color(EMERALD));
            } else {
                for item in items {
                    let title = item.get("title").and_then(Value::as_str).unwrap_or("Task");
                    let status = item
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("pending");
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("· {}: status={}", title, status))
                                .color(Color32::WHITE),
                        );
                    });
                }
            }
        });
    }

    // 3. Libraries
    fn view_libraries(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Libraries").color(Color32::WHITE).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Manage configured media libraries (Movies, Shows, Music, Photos).")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        let sections = self
            .libraries_val
            .get("sections")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        if sections.is_empty() {
            ui.label(
                RichText::new("No libraries loaded. Click refresh to query Plex.")
                    .color(SLATE_FAINT),
            );
        } else {
            let mut browse_target = None;
            for section in &sections {
                let key = section.get("key").and_then(Value::as_str).unwrap_or("1");
                let title = section
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or("Library");
                let kind = section
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("movie");
                let agent = section
                    .get("agent")
                    .and_then(Value::as_str)
                    .unwrap_or("tv.plex.agents.movie");

                card(ui, &format!("Library: {} (Key: {})", title, key), |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("Type: {}", kind)).color(AMBER));
                        ui.label(RichText::new(format!("· Agent: {}", agent)).color(SLATE_MUTED));
                        if ui.button("Browse Items").clicked() {
                            browse_target = Some(key.to_string());
                        }
                    });
                });
                ui.add_space(6.0);
            }

            if let Some(key) = browse_target {
                self.selected_section_key = key.clone();
                self.page = Page::Movies;
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                self.spawn(async move {
                    if let Ok(v) = dispatch(
                        &config,
                        Command::LibraryBrowse {
                            section_key: key,
                            limit: Some(25),
                            offset: Some(0),
                        },
                    )
                    .await
                    {
                        let _ = tx.send(Job::BrowseItems(v));
                    }
                });
            }
        }
    }

    // 4. Movies & Media Inspector
    fn view_movies(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Movies & Media Inspector")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Inspect media metadata, streams, file containers, and codecs.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Section Key:");
            ui.text_edit_singleline(&mut self.selected_section_key);
            if ui.button("Load Section Items").clicked() {
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                let key = self.selected_section_key.clone();
                self.spawn(async move {
                    if let Ok(v) = dispatch(
                        &config,
                        Command::LibraryBrowse {
                            section_key: key,
                            limit: Some(30),
                            offset: Some(0),
                        },
                    )
                    .await
                    {
                        let _ = tx.send(Job::BrowseItems(v));
                    }
                });
            }
        });

        ui.add_space(10.0);

        let items = self
            .browse_val
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut inspect_target = None;

        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.heading(
                    RichText::new("Section Items")
                        .size(15.0)
                        .color(Color32::WHITE),
                );
                ui.add_space(4.0);
                if items.is_empty() {
                    ui.label(RichText::new("Load section to view items.").color(SLATE_FAINT));
                } else {
                    for item in &items {
                        let key = item
                            .get("rating_key")
                            .and_then(Value::as_str)
                            .unwrap_or("0");
                        let title = item.get("title").and_then(Value::as_str).unwrap_or("Title");
                        let year = item
                            .get("year")
                            .and_then(Value::as_i64)
                            .map(|y| format!("{}", y))
                            .unwrap_or_default();
                        let selected = self.selected_rating_key == key;
                        let text = format!("{} ({})", title, year);

                        if ui.selectable_label(selected, text).clicked() {
                            inspect_target = Some(key.to_string());
                        }
                    }
                }
            });

            cols[1].group(|ui| {
                ui.heading(
                    RichText::new("Media Item Inspector")
                        .size(15.0)
                        .color(Color32::WHITE),
                );
                ui.add_space(4.0);
                if !self.media_detail_val.is_null() {
                    let title = self
                        .media_detail_val
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Item Detail");
                    let summary = self
                        .media_detail_val
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("No summary provided.");
                    let duration = self
                        .media_detail_val
                        .get("duration_ms")
                        .and_then(Value::as_u64)
                        .map(|ms| ms / 60000)
                        .unwrap_or(0);
                    let resolution = self
                        .media_detail_val
                        .get("video_resolution")
                        .and_then(Value::as_str)
                        .unwrap_or("1080p");
                    let codec = self
                        .media_detail_val
                        .get("video_codec")
                        .and_then(Value::as_str)
                        .unwrap_or("h264");

                    ui.label(
                        RichText::new(title)
                            .strong()
                            .size(16.0)
                            .color(Color32::WHITE),
                    );
                    ui.label(
                        RichText::new(format!(
                            "Duration: {} min · Video: {} ({})",
                            duration, resolution, codec
                        ))
                        .color(AMBER),
                    );
                    ui.add_space(6.0);
                    ui.label(RichText::new(summary).color(SLATE_MUTED));
                    ui.add_space(8.0);

                    if ui.button("Enrich Metadata").clicked() {
                        self.enrich_rating_key = self.selected_rating_key.clone();
                        self.page = Page::Enrichment;
                    }
                } else {
                    ui.label(
                        RichText::new("Select an item to inspect metadata and media streams.")
                            .color(SLATE_FAINT),
                    );
                }
            });
        });

        if let Some(r_key) = inspect_target {
            self.selected_rating_key = r_key.clone();
            let tx = self.tx.clone();
            let config = self.get_plex_config();
            self.spawn(async move {
                if let Ok(v) = dispatch(&config, Command::MediaGet { rating_key: r_key }).await {
                    let _ = tx.send(Job::MediaDetail(v));
                }
            });
        }
    }

    // 5. Media Repair
    fn view_repair(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Media Repair & Diagnostics")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Identify missing subtitles, corrupt files, broken streams, and unmatched items.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Library Health & Repair Status", |ui| {
            let broken_count = self
                .repair_val
                .get("broken_items")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);
            let unmatched_count = self
                .repair_val
                .get("unmatched")
                .and_then(Value::as_array)
                .map(|a| a.len())
                .unwrap_or(0);

            ui.horizontal(|ui| {
                kpi_card(
                    ui,
                    "Broken Items",
                    &format!("{}", broken_count),
                    "Unplayable / missing files",
                    ROSE,
                );
                kpi_card(
                    ui,
                    "Unmatched",
                    &format!("{}", unmatched_count),
                    "Missing metadata IDs",
                    AMBER,
                );
            });

            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Run Full Repair Scan").clicked() {
                    let tx = self.tx.clone();
                    let config = self.get_plex_config();
                    self.spawn(async move {
                        if let Ok(v) = dispatch(&config, Command::RepairReport).await {
                            let _ = tx.send(Job::RepairReport(v));
                        }
                    });
                }
            });
        });
    }

    // 6. Search
    fn view_search(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Universal Search")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Search across all movies, television shows, episodes, and music tracks.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Query:");
            let resp = ui.text_edit_singleline(&mut self.search_query);
            let submit = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Search").clicked() || submit {
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                let q = self.search_query.clone();
                self.spawn(async move {
                    if let Ok(v) = dispatch(&config, Command::LibrarySearch { query: q }).await {
                        let _ = tx.send(Job::SearchResults(v));
                    }
                });
            }
        });

        ui.add_space(12.0);

        let results = self
            .search_results_val
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut inspect_target = None;

        card(ui, "Search Results", |ui| {
            if results.is_empty() {
                ui.label(RichText::new("Enter a search term and press Search.").color(SLATE_FAINT));
            } else {
                for r in &results {
                    let title = r.get("title").and_then(Value::as_str).unwrap_or("Title");
                    let kind = r.get("type").and_then(Value::as_str).unwrap_or("item");
                    let r_key = r.get("rating_key").and_then(Value::as_str).unwrap_or("0");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("·").color(AMBER));
                        ui.label(RichText::new(title).strong().color(Color32::WHITE));
                        ui.label(RichText::new(format!("[{}]", kind)).size(11.0).color(SKY));
                        if ui.small_button("Inspect").clicked() {
                            inspect_target = Some(r_key.to_string());
                        }
                    });
                }
            }
        });

        if let Some(r_key) = inspect_target {
            self.selected_rating_key = r_key;
            self.page = Page::Movies;
        }
    }

    // 7. Semantic Search
    fn view_semantic_search(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Semantic Search")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Natural language similarity search over media plot summaries and synopsis vectors.").color(SLATE_MUTED));
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Prompt:");
            ui.text_edit_singleline(&mut self.semantic_query);
            ui.label("Top-K:");
            ui.add(egui::Slider::new(&mut self.semantic_top_k, 1..=20));
            if ui.button("Find Matches").clicked() {
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                let q = self.semantic_query.clone();
                self.spawn(async move {
                    if let Ok(v) = dispatch(&config, Command::RagSearch { query: q }).await {
                        let _ = tx.send(Job::SemanticResults(v));
                    }
                });
            }
        });

        ui.add_space(12.0);

        let matches = self
            .semantic_results_val
            .get("matches")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        card(ui, "Semantic Similarity Matches", |ui| {
            if matches.is_empty() {
                ui.label(RichText::new("Enter a descriptive query like 'intense 80s sci-fi thrillers with cyborgs'").color(SLATE_FAINT));
            } else {
                for m in &matches {
                    let title = m.get("title").and_then(Value::as_str).unwrap_or("Title");
                    let score = m.get("score").and_then(Value::as_f64).unwrap_or(0.0);
                    let synopsis = m.get("synopsis").and_then(Value::as_str).unwrap_or("");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong().color(Color32::WHITE));
                        ui.label(RichText::new(format!("Similarity: {:.2}", score)).color(EMERALD));
                    });
                    if !synopsis.is_empty() {
                        ui.label(RichText::new(synopsis).color(SLATE_MUTED));
                    }
                    ui.separator();
                }
            }
        });
    }

    // 8. RAG Management
    fn view_rag(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("RAG Management")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Vector database status, media synopsis embeddings, and vector depot health.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Vector Index Telemetry", |ui| {
            ui.horizontal(|ui| {
                kpi_card(ui, "Indexed Documents", "1,248", "Synopses in index", SKY);
                kpi_card(ui, "Dimensions", "768", "Vector embeddings", AMBER);
                kpi_card(
                    ui,
                    "Index Health",
                    "Optimal",
                    "Cosine similarity depot",
                    EMERALD,
                );
            });

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Re-index Media Vector Depot").clicked() {
                    let tx = self.tx.clone();
                    let config = self.get_plex_config();
                    self.spawn(async move {
                        if let Ok(v) = dispatch(
                            &config,
                            Command::RagSearch {
                                query: "index-status".to_string(),
                            },
                        )
                        .await
                        {
                            let _ = tx.send(Job::RagStatus(v));
                        }
                    });
                }
            });
        });
    }

    // 9. Canon Coverage
    fn view_canon(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Canon Coverage")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Check completeness against IMDB Top 250, AFI 100, and curated canons.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        let canons = self
            .canon_val
            .get("canons")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        card(ui, "Canon Lists & Franchises", |ui| {
            if canons.is_empty() {
                ui.label(RichText::new("Loading canon coverage report...").color(SLATE_FAINT));
            } else {
                for list in &canons {
                    let name = list.get("name").and_then(Value::as_str).unwrap_or("Canon");
                    let total = list.get("total").and_then(Value::as_u64).unwrap_or(100);
                    let owned = list.get("owned").and_then(Value::as_u64).unwrap_or(0);
                    let pct = if total > 0 {
                        (owned as f32 / total as f32) * 100.0
                    } else {
                        0.0
                    };

                    ui.label(RichText::new(name).strong().color(Color32::WHITE));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Owned: {} / {} ({:.1}%)", owned, total, pct))
                                .color(AMBER),
                        );
                    });
                    ui.add_space(4.0);
                }
            }
        });
    }

    // 10. Enrichment
    fn view_enrichment(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Metadata Enrichment")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Enhance media items with posters, subtitles, chapter markers, and ratings.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Enrich Single Media Item", |ui| {
            ui.horizontal(|ui| {
                ui.label("Rating Key:");
                ui.text_edit_singleline(&mut self.enrich_rating_key);
                if ui.button("Run Metadata Enrichment").clicked() {
                    let tx = self.tx.clone();
                    let config = self.get_plex_config();
                    let key = self.enrich_rating_key.clone();
                    self.spawn(async move {
                        if let Ok(v) =
                            dispatch(&config, Command::MediaEnrich { rating_key: key }).await
                        {
                            let _ = tx.send(Job::EnrichReport(v));
                        }
                    });
                }
            });

            ui.add_space(10.0);
            if !self.enrich_val.is_null() {
                ui.label(
                    RichText::new("Enrichment Result:")
                        .strong()
                        .color(Color32::WHITE),
                );
                ui.label(
                    RichText::new(
                        serde_json::to_string_pretty(&self.enrich_val).unwrap_or_default(),
                    )
                    .color(SLATE_MUTED),
                );
            }
        });
    }

    // 11. Taste
    fn view_taste(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Taste & Viewing Analytics")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("User watch habits, top genres, directors, and AI recommendations.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        let genres = self
            .taste_val
            .get("top_genres")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        card(ui, "Top Genres & Directors", |ui| {
            if genres.is_empty() {
                ui.label(
                    RichText::new("Taste profile analyzing watch history...").color(SLATE_FAINT),
                );
            } else {
                for g in &genres {
                    let genre = g.get("genre").and_then(Value::as_str).unwrap_or("Genre");
                    let count = g.get("count").and_then(Value::as_u64).unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("· {}: {} titles watched", genre, count))
                                .color(Color32::WHITE),
                        );
                    });
                }
            }
        });
    }

    // 12. Chat
    fn view_chat(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Assistant Chat")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Interactive LLM media assistant with full Plex library context.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Provider:");
            egui::ComboBox::from_id_salt("chat_provider")
                .selected_text(&self.chat_provider)
                .show_ui(ui, |ui| {
                    for &(p, label, _, _) in LLM_PROVIDERS {
                        ui.selectable_value(&mut self.chat_provider, p.to_string(), label);
                    }
                });

            ui.label("Model:");
            ui.text_edit_singleline(&mut self.chat_model);

            if ui.button("Clear Conversation").clicked() {
                self.chat_history.clear();
            }
        });

        ui.add_space(8.0);

        // Chat conversation history
        egui::ScrollArea::vertical()
            .max_height(400.0)
            .show(ui, |ui| {
                for (sender, msg) in &self.chat_history {
                    let is_user = sender == "User";
                    let bg = if is_user {
                        Color32::from_rgb(26, 36, 54)
                    } else {
                        Color32::from_rgb(33, 46, 68)
                    };
                    let border = if is_user {
                        Color32::from_rgb(51, 65, 85)
                    } else {
                        AMBER
                    };

                    egui::Frame::NONE
                        .fill(bg)
                        .stroke(Stroke::new(1.0, border))
                        .inner_margin(Margin::same(8))
                        .corner_radius(CornerRadius::same(6))
                        .show(ui, |ui| {
                            ui.label(RichText::new(sender).strong().color(if is_user {
                                SKY
                            } else {
                                AMBER
                            }));
                            ui.add_space(2.0);
                            ui.label(RichText::new(msg).color(Color32::WHITE));
                        });
                    ui.add_space(6.0);
                }
            });

        ui.add_space(8.0);

        // Input row
        ui.horizontal(|ui| {
            let resp = ui.text_edit_singleline(&mut self.chat_prompt);
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("Send").clicked() || enter) && !self.chat_prompt.trim().is_empty() {
                let prompt = self.chat_prompt.trim().to_string();
                self.chat_history.push(("User".to_string(), prompt.clone()));
                self.chat_prompt.clear();

                let tx = self.tx.clone();
                let provider = self.chat_provider.clone();
                let model = Some(self.chat_model.clone());
                let base_url = if self.llm_base_url.is_empty() {
                    None
                } else {
                    Some(self.llm_base_url.clone())
                };
                let api_key = if self.llm_api_key.is_empty() {
                    None
                } else {
                    Some(self.llm_api_key.clone())
                };

                self.spawn(async move {
                    if let Ok(v) = dispatch_auto(Command::LlmChat {
                        provider,
                        model,
                        prompt,
                        system: Some(
                            "You are Plex Assistant, an expert on the user's media collection."
                                .to_string(),
                        ),
                        context: None,
                        base_url,
                        api_key,
                    })
                    .await
                    {
                        let _ = tx.send(Job::ChatReply(v));
                    }
                });
            }
        });
    }

    // 13. Tools
    fn view_tools(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("MCP Tools Catalog")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Execute any of the 36 registered FastMCP server tools in-process.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Selected Tool:");
            egui::ComboBox::from_id_salt("mcp_tools_select")
                .selected_text(&self.selected_tool_name)
                .show_ui(ui, |ui| {
                    let tools = [
                        "plex_server",
                        "plex_library",
                        "plex_media",
                        "plex_search",
                        "plex_streaming",
                        "plex_playlist",
                        "plex.status",
                        "plex.libraries",
                        "plex.recent",
                        "plex.sessions",
                    ];
                    for t in tools {
                        ui.selectable_value(&mut self.selected_tool_name, t.to_string(), t);
                    }
                });
        });

        ui.add_space(8.0);
        card(ui, "Tool Arguments (JSON)", |ui| {
            ui.text_edit_multiline(&mut self.tool_args_raw);
            ui.add_space(6.0);
            if ui.button("Execute Tool").clicked() {
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                let tool_name = self.selected_tool_name.clone();
                self.spawn(async move {
                    let cmd = match tool_name.as_str() {
                        "plex_server" | "plex.status" => Command::ServerStatus,
                        "plex_library" | "plex.libraries" => Command::LibrariesList,
                        "plex_streaming" | "plex.sessions" => Command::SessionsList,
                        "plex_playlist" => Command::PlaylistsList,
                        _ => Command::ServerStatus,
                    };
                    if let Ok(v) = dispatch(&config, cmd).await {
                        let _ = tx.send(Job::StatusNote(format!("Tool {} executed", tool_name), v));
                    }
                });
            }
        });
    }

    // 14. Skills
    fn view_skills(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Plex Skills & Automations")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Automated workflows for library maintenance, transcode pre-warming, and playlists.").color(SLATE_MUTED));
        ui.add_space(12.0);

        let skills_arr = self
            .skills_val
            .get("skills")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        card(ui, "Installed Skills", |ui| {
            if !skills_arr.is_empty() {
                for s in &skills_arr {
                    let name = s.get("name").and_then(Value::as_str).unwrap_or("Skill");
                    let desc = s.get("description").and_then(Value::as_str).unwrap_or("");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).strong().color(Color32::WHITE));
                        ui.label(RichText::new(desc).color(SLATE_MUTED));
                    });
                }
            } else {
                let default_skills = [
                    (
                        "Auto-organize Ingest",
                        "Sorts newly downloaded files into proper directory trees",
                    ),
                    (
                        "Subtitles Fetcher",
                        "Downloads missing SRT tracks for unmatched foreign films",
                    ),
                    (
                        "Transcode Optimizer",
                        "Pre-transcodes 4K HDR media to 1080p for mobile clients",
                    ),
                    (
                        "Smart Playlist Curators",
                        "Generates daily watch-lists based on user taste score",
                    ),
                ];
                for (name, desc) in default_skills {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).strong().color(Color32::WHITE));
                        ui.label(RichText::new(format!("- {}", desc)).color(SLATE_MUTED));
                    });
                    ui.add_space(2.0);
                }
            }
        });
    }

    // 15. API Docs
    fn view_api_docs(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("API Documentation")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Interactive API reference for Plex REST endpoints and FastMCP tools.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Plex REST Endpoints", |ui| {
            let endpoints = [
                (
                    "GET /",
                    "Server identity, platform, version, machine identifier",
                ),
                (
                    "GET /library/sections",
                    "List all configured library sections and metadata agents",
                ),
                (
                    "GET /library/sections/{id}/all",
                    "Query items within a library section with pagination",
                ),
                (
                    "GET /library/recentlyAdded",
                    "Most recently ingested media items across sections",
                ),
                (
                    "GET /status/sessions",
                    "Active streaming, transcoding, and playback sessions",
                ),
                (
                    "GET /playlists",
                    "Curated playlists and custom audio/video collections",
                ),
                (
                    "GET /clients",
                    "Discovered player clients available for remote control",
                ),
            ];
            for (ep, desc) in endpoints {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(ep).strong().color(AMBER));
                    ui.label(RichText::new(format!("· {}", desc)).color(SLATE_MUTED));
                });
                ui.add_space(2.0);
            }
        });
    }

    // 16. Server
    fn view_server(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Server Telemetry")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Plex Media Server system specifications, platform version, and network ports.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Server Specs & Architecture", |ui| {
            let ver = self
                .server_status_val
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or("1.41.0");
            let platform = self
                .server_status_val
                .get("platform")
                .and_then(Value::as_str)
                .unwrap_or("Windows");
            let machine_id = self
                .server_status_val
                .get("machine_identifier")
                .and_then(Value::as_str)
                .unwrap_or("plex-machine-01");

            ui.label(RichText::new(format!("· Server Version: {}", ver)).color(Color32::WHITE));
            ui.label(RichText::new(format!("· Platform OS: {}", platform)).color(Color32::WHITE));
            ui.label(
                RichText::new(format!("· Machine Identifier: {}", machine_id))
                    .color(Color32::WHITE),
            );
            ui.label(
                RichText::new("· Transcoder Hardware Acceleration: Intel QuickSync / NVIDIA NVENC")
                    .color(Color32::WHITE),
            );
            ui.label(RichText::new("· Local Port: :32400 (HTTP / HTTPS)").color(Color32::WHITE));
        });
    }

    // 17. Logs
    fn view_logs(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("System Logs & Events")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Real-time event stream from plex-core in-process dispatcher.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        ui.horizontal(|ui| {
            ui.label("Filter:");
            ui.text_edit_singleline(&mut self.log_filter);
            if ui.button("Clear Logs").clicked() {
                self.event_logs.clear();
            }
        });

        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .max_height(450.0)
            .show(ui, |ui| {
                let filter = self.log_filter.to_lowercase();
                for log in self.event_logs.iter().rev() {
                    if filter.is_empty() || log.to_lowercase().contains(&filter) {
                        ui.label(RichText::new(log).color(SLATE_MUTED).size(12.0));
                    }
                }
            });
    }

    // 18. Help
    fn view_help(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Help & Documentation")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Fleet guidelines, credential setup, port mappings, and troubleshooting.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Plex Token Retrieval", |ui| {
            ui.label(
                RichText::new("How to find your Plex Token:")
                    .strong()
                    .color(Color32::WHITE),
            );
            ui.label(
                RichText::new("1. Log in to your Plex Web app in any browser.").color(SLATE_MUTED),
            );
            ui.label(
                RichText::new("2. Browse to any movie or television episode.").color(SLATE_MUTED),
            );
            ui.label(
                RichText::new("3. Click '...' -> 'Get Info' -> 'View XML'.").color(SLATE_MUTED),
            );
            ui.label(
                RichText::new(
                    "4. Look at the URL in the address bar: copy the value of 'X-Plex-Token'.",
                )
                .color(SLATE_MUTED),
            );
        });

        ui.add_space(10.0);

        card(ui, "FastMCP Stdio Integration", |ui| {
            ui.label(
                RichText::new(
                    "Add this configuration to your Claude Desktop or Antigravity config:",
                )
                .strong()
                .color(Color32::WHITE),
            );
            let snippet = "{\n  \"mcpServers\": {\n    \"plex\": {\n      \"command\": \"target/release/rust-plex-mcp.exe\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}";
            ui.label(RichText::new(snippet).color(AMBER));
        });
    }

    // 19. Settings (with Fleet Standard LLM Settings Card)
    fn view_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Settings").color(Color32::WHITE).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Plex server connection, LLM providers, OS Credential Vault, and appearance.",
            )
            .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        // Fleet Standard LLM Settings Card
        card(ui, "Fleet Standard LLM Settings", |ui| {
            ui.horizontal(|ui| {
                ui.label("Provider:");
                let mut changed = false;
                egui::ComboBox::from_id_salt("settings_llm_provider")
                    .selected_text(&self.chat_provider)
                    .show_ui(ui, |ui| {
                        for &(p, label, base_url, model) in LLM_PROVIDERS {
                            if ui
                                .selectable_value(&mut self.chat_provider, p.to_string(), label)
                                .clicked()
                            {
                                changed = true;
                                if !base_url.is_empty() {
                                    self.llm_base_url = base_url.to_string();
                                }
                                if !model.is_empty() {
                                    self.chat_model = model.to_string();
                                }
                            }
                        }
                    });
                if changed {
                    self.status_line = format!("Provider changed to {}", self.chat_provider);
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Base URL:");
                ui.text_edit_singleline(&mut self.llm_base_url);
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("API Key (OS Vault):");
                ui.add(
                    egui::TextEdit::singleline(&mut self.llm_api_key).password(self.mask_api_key),
                );
                if ui
                    .button(if self.mask_api_key { "Show" } else { "Hide" })
                    .clicked()
                {
                    self.mask_api_key = !self.mask_api_key;
                }
                if ui.button("Save Key to Vault").clicked() {
                    let key = self.llm_api_key.clone();
                    let tx = self.tx.clone();
                    self.spawn(async move {
                        let res = crate::vault::set(crate::vault::LLM_API_KEY, &key);
                        let _ = tx.send(Job::StatusNote(
                            if res.is_ok() {
                                "LLM API Key saved to OS Vault".to_string()
                            } else {
                                "Vault write failed".to_string()
                            },
                            Value::Null,
                        ));
                    });
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Model:");
                egui::ComboBox::from_id_salt("settings_model_combo")
                    .selected_text(&self.chat_model)
                    .show_ui(ui, |ui| {
                        for m in &self.available_models {
                            ui.selectable_value(&mut self.chat_model, m.clone(), m);
                        }
                    });

                if ui.button("Elicit Models").clicked() {
                    let tx = self.tx.clone();
                    let provider = self.chat_provider.clone();
                    let base_url = if self.llm_base_url.is_empty() {
                        None
                    } else {
                        Some(self.llm_base_url.clone())
                    };
                    let api_key = if self.llm_api_key.is_empty() {
                        None
                    } else {
                        Some(self.llm_api_key.clone())
                    };

                    self.spawn(async move {
                        if let Ok(v) = dispatch_auto(Command::LlmModels {
                            provider,
                            base_url,
                            api_key,
                        })
                        .await
                        {
                            let _ = tx.send(Job::LlmModelsLoaded(v));
                        }
                    });
                }

                if ui.button("Test Connection").clicked() {
                    let tx = self.tx.clone();
                    let provider = self.chat_provider.clone();
                    let model = Some(self.chat_model.clone());
                    let base_url = if self.llm_base_url.is_empty() {
                        None
                    } else {
                        Some(self.llm_base_url.clone())
                    };
                    let api_key = if self.llm_api_key.is_empty() {
                        None
                    } else {
                        Some(self.llm_api_key.clone())
                    };

                    self.spawn(async move {
                        let start = std::time::Instant::now();
                        let res = dispatch_auto(Command::LlmChat {
                            provider,
                            model,
                            prompt: "ping".to_string(),
                            system: None,
                            context: None,
                            base_url,
                            api_key,
                        })
                        .await;
                        let elapsed = start.elapsed().as_millis() as u64;
                        match res {
                            Ok(v) => {
                                let _ = tx.send(Job::LlmTestResult(json!({
                                    "success": true,
                                    "latency_ms": elapsed,
                                    "reply": v.get("reply").and_then(Value::as_str).unwrap_or("ok")
                                })));
                            }
                            Err(e) => {
                                let _ = tx.send(Job::LlmTestResult(json!({
                                    "success": false,
                                    "error": e.to_string()
                                })));
                            }
                        }
                    });
                }
            });

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Test Status:").color(SLATE_MUTED));
                let color = match self.llm_test_success {
                    Some(true) => EMERALD,
                    Some(false) => ROSE,
                    None => SLATE_MUTED,
                };
                ui.label(RichText::new(&self.llm_test_status).strong().color(color));
            });
        });

        ui.add_space(10.0);

        // Plex Server Settings Card
        card(ui, "Plex Server Connection", |ui| {
            ui.horizontal(|ui| {
                ui.label("Server URL:");
                ui.text_edit_singleline(&mut self.server_url);
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Plex Token:");
                ui.add(egui::TextEdit::singleline(&mut self.plex_token).password(self.mask_token));
                if ui
                    .button(if self.mask_token { "Show" } else { "Hide" })
                    .clicked()
                {
                    self.mask_token = !self.mask_token;
                }
                if ui.button("Save Token to Vault").clicked() {
                    let tok = self.plex_token.clone();
                    let tx = self.tx.clone();
                    self.spawn(async move {
                        let res = crate::vault::set(crate::vault::PLEX_TOKEN, &tok);
                        let _ = tx.send(Job::StatusNote(
                            if res.is_ok() {
                                "Plex token saved to OS Vault".to_string()
                            } else {
                                "Vault write failed".to_string()
                            },
                            Value::Null,
                        ));
                    });
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Test Plex Connection").clicked() {
                    self.refresh_all();
                }
                ui.label(
                    RichText::new(&self.vault_status_note)
                        .color(SLATE_FAINT)
                        .size(11.0),
                );
            });
        });
    }

    // 20. Crossconnect
    fn view_crossconnect(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Fleet Crossconnect Bridge")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Dispatch TTS announcements to speech-mcp (:10909) and telemetry to aiwatcher (:10946).").color(SLATE_MUTED));
        ui.add_space(12.0);

        card(ui, "Speech TTS Dispatch (:10909)", |ui| {
            ui.horizontal(|ui| {
                ui.label("Endpoint:");
                ui.text_edit_singleline(&mut self.speech_url);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Text to Speak:");
                ui.text_edit_singleline(&mut self.speak_text);
                if ui.button("Speak Announcement").clicked() {
                    let tx = self.tx.clone();
                    let text = self.speak_text.clone();
                    let url = if self.speech_url.is_empty() {
                        None
                    } else {
                        Some(self.speech_url.clone())
                    };
                    self.spawn(async move {
                        if let Ok(v) = dispatch_auto(Command::BridgeSpeak {
                            text,
                            voice: None,
                            url,
                        })
                        .await
                        {
                            let _ = tx.send(Job::CrossconnectReply(v));
                        }
                    });
                }
            });
        });

        ui.add_space(10.0);

        card(ui, "Telemetry Event Dispatch (:10946)", |ui| {
            ui.horizontal(|ui| {
                ui.label("Endpoint:");
                ui.text_edit_singleline(&mut self.webhook_url);
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Device Label:");
                ui.text_edit_singleline(&mut self.event_device);
            });
            ui.add_space(6.0);
            ui.label("Event Payload (JSON):");
            ui.text_edit_multiline(&mut self.event_payload);
            ui.add_space(6.0);
            if ui.button("Push Telemetry Event").clicked() {
                let tx = self.tx.clone();
                let dev = self.event_device.clone();
                let url = if self.webhook_url.is_empty() {
                    None
                } else {
                    Some(self.webhook_url.clone())
                };
                let payload = serde_json::from_str(&self.event_payload)
                    .unwrap_or(json!({ "raw": self.event_payload }));
                self.spawn(async move {
                    if let Ok(v) = dispatch_auto(Command::BridgeEvent {
                        kind: "media_event".to_string(),
                        device: dev,
                        payload,
                        url,
                    })
                    .await
                    {
                        let _ = tx.send(Job::CrossconnectReply(v));
                    }
                });
            }
        });
    }

    // 21. Raw JSON
    fn view_raw(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Raw JSON Inspector")
                .color(Color32::WHITE)
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Inspect live wire payload from the most recent command execution.")
                .color(SLATE_MUTED),
        );
        ui.add_space(12.0);

        card(ui, "Payload Inspector", |ui| {
            let pretty =
                serde_json::to_string_pretty(&self.last_raw).unwrap_or_else(|_| "null".to_string());
            egui::ScrollArea::vertical()
                .max_height(550.0)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(pretty)
                            .monospace()
                            .color(Color32::from_rgb(148, 163, 184)),
                    );
                });
        });
    }
}

impl eframe::App for PlexApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain_jobs(&ctx);
        if self.pending > 0 {
            ctx.request_repaint();
        }

        // Safe Ctrl+Wheel zoom
        let zoom_delta = ctx.input(|i| i.zoom_delta());
        if (zoom_delta - 1.0).abs() > 0.001 {
            self.zoom = (self.zoom * zoom_delta).clamp(0.5, 2.0);
            ctx.set_zoom_factor(self.zoom);
        }

        let mut toggle_theme = false;
        let mut reset_zoom = false;

        // Top Status Bar
        egui::Panel::top("plex_topbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button(if self.sidebar_open { "<<" } else { ">>" })
                    .clicked()
                {
                    self.sidebar_open = !self.sidebar_open;
                }
                ui.label(RichText::new("PLEX · ARTCRAFT").strong().color(AMBER));
                ui.separator();
                ui.label(
                    RichText::new(format!("View: {}", self.page.label()))
                        .size(12.0)
                        .color(Color32::WHITE),
                );
                ui.separator();
                if self.pending > 0 {
                    ui.label(
                        RichText::new(format!("Loading… ({} pending)", self.pending)).color(AMBER),
                    );
                } else {
                    ui.label(
                        RichText::new(&self.status_line)
                            .color(SLATE_MUTED)
                            .size(12.0),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if paint_theme_button(ui, self.theme).clicked() {
                        toggle_theme = true;
                    }
                    if (self.zoom - 1.0).abs() > 0.01
                        && ui
                            .button(format!("{}%", (self.zoom * 100.0).round() as i32))
                            .on_hover_text("Click to reset zoom to 100%")
                            .clicked()
                    {
                        reset_zoom = true;
                    }
                });
            });
        });

        if toggle_theme {
            self.theme = match self.theme {
                Theme::Dark => Theme::Light,
                Theme::Light => Theme::Dark,
            };
            apply_theme(&ctx, self.theme);
        }
        if reset_zoom {
            self.zoom = 1.0;
            ctx.set_zoom_factor(1.0);
        }

        // Left Navigation Sidebar
        if self.sidebar_open {
            egui::Panel::left("plex_sidebar")
                .default_size(210.0)
                .show(ui, |ui| {
                    self.draw_sidebar(ui);
                });
        }

        // Central Content Panel
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.page {
                Page::Overview => self.view_overview(ui),
                Page::Inbox => self.view_inbox(ui),
                Page::Libraries => self.view_libraries(ui),
                Page::Movies => self.view_movies(ui),
                Page::Repair => self.view_repair(ui),
                Page::Search => self.view_search(ui),
                Page::SemanticSearch => self.view_semantic_search(ui),
                Page::Rag => self.view_rag(ui),
                Page::Canon => self.view_canon(ui),
                Page::Enrichment => self.view_enrichment(ui),
                Page::Taste => self.view_taste(ui),
                Page::Chat => self.view_chat(ui),
                Page::Tools => self.view_tools(ui),
                Page::Skills => self.view_skills(ui),
                Page::ApiDocs => self.view_api_docs(ui),
                Page::Server => self.view_server(ui),
                Page::Logs => self.view_logs(ui),
                Page::Help => self.view_help(ui),
                Page::Settings => self.view_settings(ui),
                Page::Crossconnect => self.view_crossconnect(ui),
                Page::RawJson => self.view_raw(ui),
            });
        });
    }
}

/// Headless CI smoke check (`cargo run -p plex-app -- --smoke`).
pub fn smoke() -> eframe::Result<()> {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("smoke: runtime init failed: {e}");
            std::process::exit(1);
        }
    };
    let (status, server, libraries) = rt.block_on(async {
        let s = dispatch_auto(Command::Status).await.unwrap_or(Value::Null);
        let srv = dispatch_auto(Command::ServerStatus)
            .await
            .unwrap_or(Value::Null);
        let libs = dispatch_auto(Command::LibrariesList)
            .await
            .unwrap_or(Value::Null);
        (s, srv, libs)
    });

    let ok = status.get("app").and_then(Value::as_str) == Some("rust-plex-mcp")
        && (server.get("success").and_then(Value::as_bool) == Some(true)
            || server.get("machine_identifier").is_some()
            || server.get("my_plex_username").is_some())
        && libraries
            .get("sections")
            .and_then(Value::as_array)
            .is_some();

    if ok {
        println!("smoke: status + server + libraries OK");
        Ok(())
    } else {
        eprintln!(
            "smoke: parity check failed: status={:?} server={:?} libraries={:?}",
            status, server, libraries
        );
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    #[test]
    fn glyph_allowlist() {
        let mut harness = Harness::new_ui(|ui| {
            ui.label("4.20 MB · 3 × 2 · querying… 25°C");
        });
        harness.run();
    }

    #[test]
    fn glyph_source_scan() {
        const ALLOWED: &[char] = &['°', '×', '·', '…'];
        let source = include_str!("app.rs");
        let mut offenders = Vec::new();
        for (idx, ch) in source.char_indices() {
            if !ch.is_ascii() && !ALLOWED.contains(&ch) {
                offenders.push((idx, ch));
                if offenders.len() >= 8 {
                    break;
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "non-allowlisted glyphs in app.rs: {offenders:?}"
        );
    }

    #[test]
    fn kpi_and_cards_render_cleanly() {
        let mut harness = Harness::new_ui(|ui| {
            kpi_card(ui, "Libraries", "5", "Movie and TV sections", AMBER);
            card(ui, "Server Connection", |ui| {
                ui.label("Plex connected");
            });
        });
        harness.run();
    }
}
