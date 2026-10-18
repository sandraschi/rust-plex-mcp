//! Pure-Rust `egui` desktop UI for `rust-plex-mcp`.
//!
//! Enforces the 10-Point Uncrashable Binary Contract:
//! - Zero synchronous network or keyring I/O on the UI thread (`self.spawn(...)`).
//! - Zero `ctx.set_visuals()` or `ctx.set_zoom_factor()` inside `ctx.input(|i| ...)`.
//! - Strict glyph allowlist (`°`, `×`, `·`, `…`) verified at compile/test time by `glyph_source_scan`.

use crate::browser::{
    amber, faint, filter_sort_page, ink, letter_row, page_bar, present_letters, sort_combo, sub,
    trunc, SortMode, AMBER_DARK, FAINT_LIGHT, INK_LIGHT, PAGE_GRID, PAGE_LIST, SUB_LIGHT,
};
use eframe::egui::{
    self, Color32, CornerRadius, Margin, Pos2, Rect, RichText, Stroke, StrokeKind, Vec2,
};
use plex_core::{dispatch, dispatch_auto, fetch_image_bytes, Command, PlexConfig, Vendor};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
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
    PosterLoaded {
        path: String,
        bytes: Option<Vec<u8>>,
    },
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
    canon_source: String,
    taste_val: Value,
    enrich_val: Value,
    repair_val: Value,
    inbox_val: Value,
    tools_val: Value,
    skills_val: Value,
    last_raw: Value,

    // Poster textures & async cache
    poster_cache: HashMap<String, Option<egui::TextureHandle>>,
    fetching_posters: HashSet<String>,
    movies_view_grid: bool,
    movies_filter: String,
    movies_sort: SortMode,
    movies_page: usize,
    movies_letter: Option<char>,

    // Libraries browser state
    libs_query: String,
    libs_sort: SortMode,
    libs_page: usize,
    libs_letter: Option<char>,

    // Search and filter inputs
    search_query: String,
    search_mode_grid: bool,
    search_sort: SortMode,
    search_page: usize,
    search_letter: Option<char>,
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
    v.override_text_color = Some(INK_LIGHT);
    v.widgets.noninteractive.bg_fill = Color32::from_rgb(241, 245, 249);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, SUB_LIGHT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(203, 213, 225));
    v.widgets.inactive.bg_fill = Color32::WHITE;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, INK_LIGHT);
    v.widgets.inactive.bg_stroke = Stroke::new(1.5, Color32::from_rgb(148, 163, 184));
    v.widgets.hovered.bg_fill = Color32::from_rgb(226, 232, 240);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, INK_LIGHT);
    v.widgets.hovered.bg_stroke = Stroke::new(1.5, AMBER_DARK);
    v.widgets.active.bg_fill = Color32::from_rgb(226, 232, 240);
    v.widgets.active.fg_stroke = Stroke::new(1.5, AMBER_DARK);
    v.widgets.active.bg_stroke = Stroke::new(1.5, AMBER_DARK);
    v.selection.bg_fill = AMBER;
    v.selection.stroke = Stroke::new(1.0, Color32::BLACK);
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
    // 1px slate-700 on slate-900 was near-invisible; 1.5px slate-500 (dark)
    // and slate-400 (light) read as real borders in both themes.
    let stroke_color = if is_dark {
        Color32::from_rgb(100, 116, 139)
    } else {
        Color32::from_rgb(148, 163, 184)
    };
    let shadow_color = if is_dark {
        Color32::from_rgba_premultiplied(0, 0, 0, 110)
    } else {
        Color32::from_rgba_premultiplied(15, 23, 42, 45)
    };

    egui::Frame::NONE
        .fill(fill)
        .stroke(Stroke::new(1.5, stroke_color))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .outer_margin(Margin::same(2))
        .shadow(egui::Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: shadow_color,
        })
        .show(ui, add_contents);
}

pub fn card(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    card_frame(ui, |ui| {
        let title_color = ink(ui);
        ui.label(RichText::new(title).size(13.0).strong().color(title_color));
        ui.add_space(4.0);
        add_contents(ui);
    });
}

pub fn kpi_card(ui: &mut egui::Ui, label: &str, value: &str, sub_text: &str, accent: Color32) {
    card_frame(ui, |ui| {
        ui.set_min_width(140.0);
        ui.label(RichText::new(label).size(11.0).strong().color(accent));
        ui.add_space(2.0);
        let value_color = ink(ui);
        ui.label(RichText::new(value).size(22.0).strong().color(value_color));
        ui.add_space(2.0);
        let sub_color = faint(ui);
        ui.label(RichText::new(sub_text).size(10.0).color(sub_color));
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

/// Render a movie poster image or a styled film vector placeholder card.
fn render_poster_image(
    ui: &mut egui::Ui,
    tex: Option<&egui::TextureHandle>,
    size: Vec2,
    fallback_title: &str,
) {
    if let Some(t) = tex {
        ui.add(
            egui::Image::new(t)
                .fit_to_exact_size(size)
                .corner_radius(CornerRadius::same(6)),
        );
    } else {
        let is_dark = ui.visuals().dark_mode;
        let fill = if is_dark {
            Color32::from_rgb(18, 25, 38)
        } else {
            Color32::from_rgb(226, 232, 240)
        };
        let fill_deep = if is_dark {
            Color32::from_rgb(33, 46, 68)
        } else {
            Color32::from_rgb(203, 213, 225)
        };
        let edge = if is_dark {
            BORDER_DARK
        } else {
            Color32::from_rgb(148, 163, 184)
        };
        let accent = amber(ui);
        let text_color = if is_dark { SLATE_FAINT } else { FAINT_LIGHT };
        let (rect, _resp) = ui.allocate_exact_size(size, egui::Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(6), fill);
        p.rect_stroke(
            rect,
            CornerRadius::same(6),
            Stroke::new(1.5, edge),
            StrokeKind::Inside,
        );

        let c = rect.center();
        let icon_rect = Rect::from_center_size(Pos2::new(c.x, c.y - 12.0), Vec2::new(32.0, 24.0));
        p.rect_filled(icon_rect, CornerRadius::same(3), fill_deep);
        p.rect_stroke(
            icon_rect,
            CornerRadius::same(3),
            Stroke::new(1.0, accent),
            StrokeKind::Inside,
        );
        p.line_segment(
            [
                Pos2::new(icon_rect.min.x + 8.0, icon_rect.min.y),
                Pos2::new(icon_rect.min.x + 14.0, icon_rect.min.y + 8.0),
            ],
            Stroke::new(1.5, accent),
        );
        p.line_segment(
            [
                Pos2::new(icon_rect.min.x + 18.0, icon_rect.min.y),
                Pos2::new(icon_rect.min.x + 24.0, icon_rect.min.y + 8.0),
            ],
            Stroke::new(1.5, accent),
        );

        p.text(
            Pos2::new(c.x, c.y + 16.0),
            egui::Align2::CENTER_CENTER,
            trunc(fallback_title, 14),
            egui::FontId::proportional(10.5),
            text_color,
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAction {
    None,
    Clicked,
    DoubleClicked,
}

/// Render a 2:3 aspect ratio Movie Poster Card matching the original webapp.
///
/// Supports hover play icon overlay, single-click inspection, and double-click playback.
fn movie_card(
    ui: &mut egui::Ui,
    tex: Option<&egui::TextureHandle>,
    title: &str,
    year: Option<&str>,
    selected: bool,
    card_width: f32,
) -> CardAction {
    let card_height = (card_width * 1.5).round();
    let is_dark = ui.visuals().dark_mode;
    let bg = if selected {
        AMBER_GLOW
    } else if is_dark {
        CARD_DARK
    } else {
        Color32::WHITE
    };
    let border = if selected {
        AMBER
    } else if is_dark {
        BORDER_DARK
    } else {
        Color32::from_rgb(203, 213, 225)
    };

    let shadow_color = if is_dark {
        Color32::from_rgba_premultiplied(0, 0, 0, 110)
    } else {
        Color32::from_rgba_premultiplied(15, 23, 42, 45)
    };
    let frame = egui::Frame::NONE
        .fill(bg)
        .stroke(Stroke::new(if selected { 2.0 } else { 1.5 }, border))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(6))
        .outer_margin(Margin::same(2))
        .shadow(egui::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: shadow_color,
        });

    let mut action = CardAction::None;
    frame.show(ui, |ui| {
        ui.set_width(card_width);
        let resp = ui.allocate_response(Vec2::new(card_width, card_height), egui::Sense::click());
        if resp.double_clicked() {
            action = CardAction::DoubleClicked;
        } else if resp.clicked() {
            action = CardAction::Clicked;
        }

        if let Some(t) = tex {
            let painter = ui.painter_at(resp.rect);
            painter.image(
                t.id(),
                resp.rect,
                Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            let ph_fill = if is_dark {
                Color32::from_rgb(18, 25, 38)
            } else {
                Color32::from_rgb(226, 232, 240)
            };
            let ph_deep = if is_dark {
                Color32::from_rgb(33, 46, 68)
            } else {
                Color32::from_rgb(203, 213, 225)
            };
            let ph_edge = if is_dark {
                BORDER_DARK
            } else {
                Color32::from_rgb(148, 163, 184)
            };
            let ph_accent = amber(ui);
            let ph_text = if is_dark { SLATE_FAINT } else { FAINT_LIGHT };
            let p = ui.painter_at(resp.rect);
            p.rect_filled(resp.rect, CornerRadius::same(6), ph_fill);
            p.rect_stroke(
                resp.rect,
                CornerRadius::same(6),
                Stroke::new(1.5, ph_edge),
                StrokeKind::Inside,
            );

            let c = resp.rect.center();
            let icon_rect =
                Rect::from_center_size(Pos2::new(c.x, c.y - 12.0), Vec2::new(32.0, 24.0));
            p.rect_filled(icon_rect, CornerRadius::same(3), ph_deep);
            p.rect_stroke(
                icon_rect,
                CornerRadius::same(3),
                Stroke::new(1.0, ph_accent),
                StrokeKind::Inside,
            );
            p.line_segment(
                [
                    Pos2::new(icon_rect.min.x + 8.0, icon_rect.min.y),
                    Pos2::new(icon_rect.min.x + 14.0, icon_rect.min.y + 8.0),
                ],
                Stroke::new(1.5, ph_accent),
            );
            p.line_segment(
                [
                    Pos2::new(icon_rect.min.x + 18.0, icon_rect.min.y),
                    Pos2::new(icon_rect.min.x + 24.0, icon_rect.min.y + 8.0),
                ],
                Stroke::new(1.5, ph_accent),
            );

            p.text(
                Pos2::new(c.x, c.y + 16.0),
                egui::Align2::CENTER_CENTER,
                trunc(title, 14),
                egui::FontId::proportional(10.0),
                ph_text,
            );
        }

        if resp.hovered() {
            let p = ui.painter_at(resp.rect);
            p.rect_stroke(
                resp.rect,
                CornerRadius::same(6),
                Stroke::new(1.5, AMBER),
                StrokeKind::Inside,
            );

            // Translucent play icon badge matching webapp PlayIcon
            let c = resp.rect.center();
            p.circle_filled(c, 20.0, Color32::from_rgba_premultiplied(0, 0, 0, 190));
            p.circle_stroke(c, 20.0, Stroke::new(1.5, AMBER));
            let p1 = Pos2::new(c.x - 5.0, c.y - 7.0);
            let p2 = Pos2::new(c.x - 5.0, c.y + 7.0);
            let p3 = Pos2::new(c.x + 7.0, c.y);
            p.add(egui::Shape::convex_polygon(
                vec![p1, p2, p3],
                Color32::WHITE,
                Stroke::NONE,
            ));
        }

        ui.add_space(4.0);
        let title_color = ink(ui);
        let title_resp = ui.add(
            egui::Label::new(RichText::new(title).size(12.0).strong().color(title_color))
                .truncate(),
        );
        if title_resp.double_clicked() {
            action = CardAction::DoubleClicked;
        } else if title_resp.clicked() && action == CardAction::None {
            action = CardAction::Clicked;
        }
        if let Some(y) = year {
            if !y.is_empty() {
                let year_color = sub(ui);
                ui.label(RichText::new(y).size(10.5).color(year_color));
            }
        }
    });

    action
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
            canon_source: "oscars".to_string(),
            taste_val: Value::Null,
            enrich_val: Value::Null,
            repair_val: Value::Null,
            inbox_val: Value::Null,
            tools_val: Value::Null,
            skills_val: Value::Null,
            last_raw: Value::Null,

            poster_cache: HashMap::new(),
            fetching_posters: HashSet::new(),
            movies_view_grid: true,
            movies_filter: String::new(),
            movies_sort: SortMode::default(),
            movies_page: 0,
            movies_letter: None,

            libs_query: String::new(),
            libs_sort: SortMode::default(),
            libs_page: 0,
            libs_letter: None,

            search_query: String::new(),
            search_mode_grid: false,
            search_sort: SortMode::default(),
            search_page: 0,
            search_letter: None,
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

    fn prefetch_posters(&mut self, paths: &[String]) {
        for p in paths {
            if !p.is_empty()
                && !self.poster_cache.contains_key(p)
                && !self.fetching_posters.contains(p)
            {
                self.fetching_posters.insert(p.clone());
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                let path = p.clone();
                self.spawn(async move {
                    let bytes = fetch_image_bytes(&config, &path).await.ok();
                    let _ = tx.send(Job::PosterLoaded { path, bytes });
                });
            }
        }
    }

    fn play_media_item(&mut self, item: &Value) {
        let title = item
            .get("title")
            .and_then(Value::as_str)
            .unwrap_or("Untitled");
        let rating_key = item
            .get("ratingKey")
            .or_else(|| item.get("rating_key"))
            .and_then(Value::as_str)
            .unwrap_or("");

        // 1. Check for physical media file path (Media -> Part -> file;
        // serde emits lowercase, raw Plex JSON uses PascalCase - try both).
        let local_file = item
            .get("file")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                let media = item
                    .get("media")
                    .or_else(|| item.get("Media"))?
                    .as_array()?;
                for m in media {
                    if let Some(parts) = m
                        .get("part")
                        .or_else(|| m.get("Part"))
                        .and_then(Value::as_array)
                    {
                        for p in parts {
                            if let Some(f) = p.get("file").and_then(Value::as_str) {
                                return Some(f.to_string());
                            }
                        }
                    }
                }
                None
            });

        let mut launched = false;
        if let Some(file_path) = local_file {
            let p = std::path::Path::new(&file_path);
            if p.exists() {
                let target = file_path.clone();
                self.spawn(async move {
                    let _ = tokio::task::spawn_blocking(move || open::that(&target)).await;
                });
                self.status_line = format!("[Play] Playing local file: {} ({})", title, file_path);
                self.log_event(format!(
                    "[PLAY] Launched local media player for '{}' ({})",
                    title, file_path
                ));
                launched = true;
            }
        }

        if !launched {
            // 2. Open Plex Web player URL in default browser
            let server_url = if self.server_url.trim().is_empty() {
                "http://127.0.0.1:32400".to_string()
            } else {
                self.server_url.trim_end_matches('/').to_string()
            };

            let machine_id = self
                .server_status_val
                .get("machine_identifier")
                .and_then(Value::as_str)
                .unwrap_or("");

            let player_url = if !machine_id.is_empty() && !rating_key.is_empty() {
                format!(
                    "{}/web/index.html#!/server/{}/details?key=%2Flibrary%2Fmetadata%2F{}",
                    server_url, machine_id, rating_key
                )
            } else if !rating_key.is_empty() {
                format!(
                    "{}/web/index.html#!/details?key=%2Flibrary%2Fmetadata%2F{}",
                    server_url, rating_key
                )
            } else {
                format!("{}/web/index.html", server_url)
            };

            let target = player_url.clone();
            self.spawn(async move {
                let _ = tokio::task::spawn_blocking(move || open::that(&target)).await;
            });

            self.status_line = format!("[Play] Opening in Plex Web: {}", title);
            self.log_event(format!(
                "[PLAY] Opened Plex Web player for '{}' ({})",
                title, player_url
            ));
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
                dispatch(&config, Command::LibraryRecentlyAdded { limit: Some(12) }).await
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
                Job::PosterLoaded { path, bytes } => {
                    self.fetching_posters.remove(&path);
                    let tex = bytes.and_then(|b| {
                        let decoded = image::load_from_memory(&b).ok()?.to_rgba8();
                        let size = [decoded.width() as usize, decoded.height() as usize];
                        let color_img =
                            egui::ColorImage::from_rgba_unmultiplied(size, decoded.as_raw());
                        Some(ctx.load_texture(
                            format!("poster-{}", path),
                            color_img,
                            egui::TextureOptions::LINEAR,
                        ))
                    });
                    self.poster_cache.insert(path, tex);
                }
            }
        }
        if had_jobs {
            ctx.request_repaint();
        }
    }

    // UI Navigation & Views
    fn draw_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (rect, _resp) = ui.allocate_exact_size(Vec2::splat(22.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(4), AMBER);
            ui.painter()
                .circle_filled(rect.center(), 4.0, Color32::BLACK);

            if self.sidebar_open {
                ui.vertical(|ui| {
                    ui.label(RichText::new("PlexMCP").size(15.0).strong().color(ink(ui)));
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(10.0)
                            .color(faint(ui)),
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
                ui.label(RichText::new(text).size(11.0).color(sub(ui)));
            });
            ui.add_space(8.0);
        }

        ui.separator();
        ui.add_space(4.0);

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
                let fg = if active { amber(ui) } else { sub(ui) };

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
        ui.heading(RichText::new("Overview").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Plex Media Server health, telemetry, active sessions, and recently added items.",
            )
            .color(sub(ui)),
        );
        ui.add_space(12.0);

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
                        .color(ink(ui)),
                );
                ui.label(RichText::new(format!("v{}", server_ver)).color(amber(ui)));
                ui.label(RichText::new(format!("· Platform: {}", platform)).color(sub(ui)));
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

        let sessions_arr = self
            .sessions_val
            .get("sessions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        ui.columns(2, |cols| {
            cols[0].group(|ui| {
                ui.heading(RichText::new("Active Streams").size(16.0).color(ink(ui)));
                ui.add_space(4.0);
                if sessions_arr.is_empty() {
                    ui.label(
                        RichText::new("No active playback sessions on this server.")
                            .color(faint(ui)),
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
                                    .color(ink(ui)),
                            );
                            ui.label(RichText::new(format!("Player: {}", player)).color(sub(ui)));
                        });
                    }
                }
            });

            cols[1].group(|ui| {
                ui.heading(RichText::new("Quick Operations").size(16.0).color(ink(ui)));
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

        // Recently Added Poster Rail
        let recent_items = self
            .recently_added_val
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let poster_paths: Vec<String> = recent_items
            .iter()
            .filter_map(|it| {
                it.get("thumb")
                    .or_else(|| it.get("art"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();
        self.prefetch_posters(&poster_paths);

        let mut select_target = None;
        let mut play_target = None;

        card(ui, "Recently Added Media (Posters)", |ui| {
            if recent_items.is_empty() {
                ui.label(RichText::new("Querying recently added media...").color(faint(ui)));
            } else {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for item in recent_items.iter().take(10) {
                            let r_key = item
                                .get("ratingKey")
                                .or_else(|| item.get("rating_key"))
                                .and_then(Value::as_str)
                                .unwrap_or("0");
                            let title = item
                                .get("title")
                                .and_then(Value::as_str)
                                .unwrap_or("Media Item");
                            let year = item
                                .get("year")
                                .and_then(Value::as_i64)
                                .map(|y| format!("{}", y));
                            let thumb = item
                                .get("thumb")
                                .or_else(|| item.get("art"))
                                .and_then(Value::as_str);
                            let tex = thumb
                                .and_then(|p| self.poster_cache.get(p))
                                .and_then(|opt| opt.as_ref());

                            let action = movie_card(ui, tex, title, year.as_deref(), false, 115.0);
                            if action == CardAction::DoubleClicked {
                                play_target = Some(item.clone());
                            } else if action == CardAction::Clicked {
                                select_target = Some(r_key.to_string());
                            }
                        }
                    });
                });
            }
        });

        if let Some(item) = play_target {
            self.play_media_item(&item);
        }

        if let Some(r_key) = select_target {
            self.selected_rating_key = r_key.clone();
            self.page = Page::Movies;
            let tx = self.tx.clone();
            let config = self.get_plex_config();
            self.spawn(async move {
                if let Ok(v) = dispatch(&config, Command::MediaGet { rating_key: r_key }).await {
                    let _ = tx.send(Job::MediaDetail(v));
                }
            });
        }
    }

    // 2. Inbox
    fn view_inbox(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Inbox & Processing Queue")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Pending media ingest, unmatched file alerts, and processing notifications.",
            )
            .color(sub(ui)),
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
                            RichText::new(format!("· {}: status={}", title, status)).color(ink(ui)),
                        );
                    });
                }
            }
        });
    }

    // 3. Libraries
    fn view_libraries(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Libraries").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Manage configured media libraries (Movies, Shows, Music, Photos).")
                .color(sub(ui)),
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
                RichText::new("No libraries loaded. Click refresh to query Plex.").color(faint(ui)),
            );
        } else {
            ui.horizontal(|ui| {
                ui.label("Search:");
                if ui.text_edit_singleline(&mut self.libs_query).changed() {
                    self.libs_page = 0;
                }
                ui.separator();
                sort_combo(
                    ui,
                    "libs_sort_combo",
                    &mut self.libs_sort,
                    &mut self.libs_page,
                );
            });
            ui.add_space(6.0);
            letter_row(
                ui,
                &present_letters(&sections),
                &mut self.libs_letter,
                &mut self.libs_page,
            );
            ui.add_space(6.0);
            let (visible, pages, total) = filter_sort_page(
                &sections,
                &self.libs_query.clone(),
                self.libs_letter,
                self.libs_sort,
                self.libs_page,
                PAGE_LIST,
            );
            self.libs_page = self.libs_page.min(pages - 1);
            page_bar(ui, &mut self.libs_page, pages, total);
            ui.add_space(6.0);
            if visible.is_empty() {
                ui.label(RichText::new("No libraries match this filter.").color(faint(ui)));
            }
            let mut browse_target = None;
            for section in visible {
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
                        ui.label(RichText::new(format!("Type: {}", kind)).color(amber(ui)));
                        ui.label(RichText::new(format!("· Agent: {}", agent)).color(sub(ui)));
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
                            limit: Some(100),
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

    // 4. Movies & Media Inspector with Poster Cards
    fn view_movies(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Movies & Media Inspector")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Browse media collection with poster cards, metadata, and stream telemetry.",
            )
            .color(sub(ui)),
        );
        ui.add_space(10.0);

        // Section Selector Bar
        ui.horizontal(|ui| {
            ui.label("Section:");
            egui::ComboBox::from_id_salt("movie_section_combo")
                .selected_text(&self.selected_section_key)
                .show_ui(ui, |ui| {
                    if let Some(sections) =
                        self.libraries_val.get("sections").and_then(Value::as_array)
                    {
                        for s in sections {
                            let k = s.get("key").and_then(Value::as_str).unwrap_or("1");
                            let t = s.get("title").and_then(Value::as_str).unwrap_or("Section");
                            ui.selectable_value(
                                &mut self.selected_section_key,
                                k.to_string(),
                                format!("{}: {}", k, t),
                            );
                        }
                    } else {
                        ui.selectable_value(
                            &mut self.selected_section_key,
                            "1".to_string(),
                            "Section 1",
                        );
                    }
                });

            if ui.button("Load Section Items").clicked() {
                let tx = self.tx.clone();
                let config = self.get_plex_config();
                let key = self.selected_section_key.clone();
                self.spawn(async move {
                    if let Ok(v) = dispatch(
                        &config,
                        Command::LibraryBrowse {
                            section_key: key,
                            limit: Some(100),
                            offset: Some(0),
                        },
                    )
                    .await
                    {
                        let _ = tx.send(Job::BrowseItems(v));
                    }
                });
            }

            ui.separator();
            ui.label("Filter:");
            if ui.text_edit_singleline(&mut self.movies_filter).changed() {
                self.movies_page = 0;
            }
            ui.separator();
            sort_combo(
                ui,
                "movies_sort_combo",
                &mut self.movies_sort,
                &mut self.movies_page,
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mode_label = if self.movies_view_grid {
                    "View: Grid"
                } else {
                    "View: List"
                };
                if ui.button(mode_label).clicked() {
                    self.movies_view_grid = !self.movies_view_grid;
                }
            });
        });

        ui.add_space(10.0);

        let items = self
            .browse_val
            .get("items")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        ui.add_space(6.0);
        letter_row(
            ui,
            &present_letters(&items),
            &mut self.movies_letter,
            &mut self.movies_page,
        );
        ui.add_space(6.0);

        let per_page = if self.movies_view_grid {
            PAGE_GRID
        } else {
            PAGE_LIST
        };
        let (filtered_items, pages, total) = filter_sort_page(
            &items,
            &self.movies_filter.clone(),
            self.movies_letter,
            self.movies_sort,
            self.movies_page,
            per_page,
        );
        self.movies_page = self.movies_page.min(pages - 1);
        page_bar(ui, &mut self.movies_page, pages, total);
        ui.add_space(6.0);

        // Prefetch posters for the visible page only, not the whole list.
        let poster_paths: Vec<String> = filtered_items
            .iter()
            .filter_map(|it| {
                it.get("thumb")
                    .or_else(|| it.get("art"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();
        self.prefetch_posters(&poster_paths);

        let mut inspect_target = None;
        let mut play_target = None;

        ui.columns(2, |cols| {
            // Left column: Movie posters grid or list
            cols[0].group(|ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("Collection Items")
                            .size(15.0)
                            .color(ink(ui)),
                    );
                    ui.label(RichText::new(format!("({} titles)", total)).color(faint(ui)));
                });
                ui.add_space(6.0);

                if filtered_items.is_empty() {
                    ui.label(RichText::new("No items found. Click 'Load Section Items' to browse.").color(faint(ui)));
                } else if self.movies_view_grid {
                    egui::ScrollArea::vertical().max_height(600.0).show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for item in &filtered_items {
                                let key = item
                                    .get("ratingKey")
                                    .or_else(|| item.get("rating_key"))
                                    .and_then(Value::as_str)
                                    .unwrap_or("0");
                                let title = item.get("title").and_then(Value::as_str).unwrap_or("Title");
                                let year = item
                                    .get("year")
                                    .and_then(Value::as_i64)
                                    .map(|y| format!("{}", y));
                                let selected = self.selected_rating_key == key;

                                let thumb = item
                                    .get("thumb")
                                    .or_else(|| item.get("art"))
                                    .and_then(Value::as_str);
                                let tex = thumb
                                    .and_then(|p| self.poster_cache.get(p))
                                    .and_then(|opt| opt.as_ref());

                                let action = movie_card(ui, tex, title, year.as_deref(), selected, 120.0);
                                if action == CardAction::DoubleClicked {
                                    play_target = Some((**item).clone());
                                } else if action == CardAction::Clicked {
                                    inspect_target = Some(key.to_string());
                                }
                            }
                        });
                    });
                } else {
                    // List view
                    egui::ScrollArea::vertical().max_height(600.0).show(ui, |ui| {
                        for item in &filtered_items {
                            let key = item
                                .get("ratingKey")
                                .or_else(|| item.get("rating_key"))
                                .and_then(Value::as_str)
                                .unwrap_or("0");
                            let title = item.get("title").and_then(Value::as_str).unwrap_or("Title");
                            let year = item
                                .get("year")
                                .and_then(Value::as_i64)
                                .map(|y| format!("{}", y))
                                .unwrap_or_default();
                            let summary = item.get("summary").and_then(Value::as_str).unwrap_or("");
                            let selected = self.selected_rating_key == key;

                            let thumb = item
                                .get("thumb")
                                .or_else(|| item.get("art"))
                                .and_then(Value::as_str);
                            let tex = thumb
                                .and_then(|p| self.poster_cache.get(p))
                                .and_then(|opt| opt.as_ref());

                            let is_dark = ui.visuals().dark_mode;
                            let bg = if selected {
                                AMBER_GLOW
                            } else if is_dark {
                                CARD_DARK
                            } else {
                                Color32::WHITE
                            };
                            let edge = if selected {
                                amber(ui)
                            } else if is_dark {
                                BORDER_DARK
                            } else {
                                Color32::from_rgb(148, 163, 184)
                            };
                            egui::Frame::NONE
                                .fill(bg)
                                .stroke(Stroke::new(if selected { 2.0 } else { 1.5 }, edge))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::same(6))
                                .outer_margin(Margin::same(2))
                                .shadow(egui::Shadow {
                                    offset: [0, 1],
                                    blur: 6,
                                    spread: 0,
                                    color: if is_dark {
                                        Color32::from_rgba_premultiplied(0, 0, 0, 90)
                                    } else {
                                        Color32::from_rgba_premultiplied(15, 23, 42, 35)
                                    },
                                })
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        render_poster_image(ui, tex, Vec2::new(40.0, 60.0), title);
                                        let title_col = ui.vertical(|ui| {
                                            ui.label(RichText::new(format!("{} ({})", title, year)).strong().color(ink(ui)));
                                            if !summary.is_empty() {
                                                let blurb = format!("{}…", trunc(summary, 100));
                                                ui.label(RichText::new(blurb).size(11.0).color(sub(ui)));
                                            }
                                        });
                                        if title_col.response.double_clicked() {
                                            play_target = Some((**item).clone());
                                        }

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if ui.button("Inspect").clicked() {
                                                inspect_target = Some(key.to_string());
                                            }
                                            let play_btn = egui::Button::new(RichText::new("> Play").strong().color(Color32::BLACK)).fill(AMBER);
                                            if ui.add(play_btn).clicked() {
                                                play_target = Some((**item).clone());
                                            }
                                        });
                                    });
                                });
                            ui.add_space(4.0);
                        }
                    });
                }
            });

            // Right column: Detailed Media Inspector with Poster
            cols[1].group(|ui| {
                ui.heading(
                    RichText::new("Media Item Inspector")
                        .size(15.0)
                        .color(ink(ui)),
                );
                ui.add_space(6.0);
                if !self.media_detail_val.is_null() {
                    // Dispatch wraps the Plex item as {"item": {...}}; fall
                    // back to top-level for hand-built payloads.
                    let detail = self
                        .media_detail_val
                        .get("item")
                        .unwrap_or(&self.media_detail_val);
                    let title = detail
                        .get("title")
                        .and_then(Value::as_str)
                        .unwrap_or("Item Detail");
                    let summary = detail
                        .get("summary")
                        .and_then(Value::as_str)
                        .unwrap_or("No summary provided.");
                    let year = detail
                        .get("year")
                        .and_then(Value::as_i64)
                        .map(|y| format!("{y}"))
                        .unwrap_or_default();
                    // Plex reports duration in ms under "duration"; the first
                    // Media/Part carries stream + file detail.
                    let duration_ms = detail
                        .get("duration")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .max(0) as u64;
                    let duration = duration_ms / 60000;
                    let stream = detail
                        .get("Media")
                        .and_then(Value::as_array)
                        .and_then(|arr| arr.first());
                    let resolution = stream
                        .and_then(|m| m.get("videoResolution"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let codec = stream
                        .and_then(|m| m.get("videoCodec"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let audio = stream
                        .and_then(|m| m.get("audioCodec"))
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let container = stream
                        .and_then(|m| m.get("container"))
                        .and_then(Value::as_str)
                        .unwrap_or("");

                    let detail_thumb = detail
                        .get("thumb")
                        .or_else(|| detail.get("art"))
                        .and_then(Value::as_str);

                    let detail_tex = detail_thumb
                        .and_then(|p| self.poster_cache.get(p))
                        .and_then(|opt| opt.as_ref());

                    // Large Poster + Metadata Header
                    ui.horizontal(|ui| {
                        render_poster_image(ui, detail_tex, Vec2::new(140.0, 210.0), title);
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(title)
                                    .strong()
                                    .size(17.0)
                                    .color(ink(ui)),
                            );
                            if !year.is_empty() {
                                ui.label(RichText::new(format!("Year: {}", year)).color(sub(ui)));
                            }
                            if duration > 0 {
                                ui.label(RichText::new(format!("Runtime: {} min", duration)).color(sub(ui)));
                            }
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                let badge_fill = if ui.visuals().dark_mode {
                                    Color32::from_rgb(33, 46, 68)
                                } else {
                                    Color32::from_rgb(226, 232, 240)
                                };
                                for (text, accent) in [
                                    (resolution, amber(ui)),
                                    (codec, SKY),
                                    (audio, EMERALD),
                                ] {
                                    egui::Frame::NONE.fill(badge_fill).corner_radius(CornerRadius::same(4)).inner_margin(Margin::symmetric(6, 2)).show(ui, |ui| {
                                        ui.label(RichText::new(text).strong().color(accent));
                                    });
                                }
                                if !container.is_empty() {
                                    egui::Frame::NONE.fill(badge_fill).corner_radius(CornerRadius::same(4)).inner_margin(Margin::symmetric(6, 2)).show(ui, |ui| {
                                        ui.label(RichText::new(container).strong().color(sub(ui)));
                                    });
                                }
                            });
                            if let Some(kind) = detail.get("type").and_then(Value::as_str) {
                                ui.label(
                                    RichText::new(format!(
                                        "Type: {kind} · Key: {}",
                                        detail
                                            .get("ratingKey")
                                            .and_then(Value::as_str)
                                            .unwrap_or("?"),
                                    ))
                                    .color(sub(ui)),
                                );
                            }
                        });
                    });

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);

                    ui.label(RichText::new("Plot Synopsis:").strong().color(ink(ui)));
                    ui.add_space(2.0);
                    ui.label(RichText::new(summary).color(sub(ui)));
                    ui.add_space(10.0);

                    ui.horizontal(|ui| {
                        let play_btn = egui::Button::new(
                            RichText::new("> Play Movie")
                                .strong()
                                .color(Color32::BLACK),
                        )
                        .fill(AMBER);
                        if ui.add(play_btn).clicked() {
                            play_target = Some(self.media_detail_val.clone());
                        }
                        if ui.button("Enrich Metadata").clicked() {
                            self.enrich_rating_key = self.selected_rating_key.clone();
                            self.page = Page::Enrichment;
                        }
                        if ui.button("Inspect Raw JSON").clicked() {
                            self.page = Page::RawJson;
                        }
                    });
                } else {
                    ui.label(
                        RichText::new("Select a movie from the grid on the left to view poster art, stream specifications, and metadata.")
                            .color(faint(ui)),
                    );
                }
            });
        });

        if let Some(item) = play_target {
            self.play_media_item(&item);
        }

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
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Identify missing subtitles, corrupt files, broken streams, and unmatched items.",
            )
            .color(sub(ui)),
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

    // 6. Search with Poster Thumbnails
    fn view_search(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Universal Search").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Search across all movies, television shows, episodes, and music tracks.",
            )
            .color(sub(ui)),
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

        ui.horizontal(|ui| {
            let mode_label = if self.search_mode_grid {
                "View: Grid"
            } else {
                "View: List"
            };
            if ui.button(mode_label).clicked() {
                self.search_mode_grid = !self.search_mode_grid;
            }
            ui.separator();
            sort_combo(
                ui,
                "search_sort_combo",
                &mut self.search_sort,
                &mut self.search_page,
            );
        });
        ui.add_space(6.0);
        letter_row(
            ui,
            &present_letters(&results),
            &mut self.search_letter,
            &mut self.search_page,
        );
        ui.add_space(6.0);

        let per_page = if self.search_mode_grid {
            PAGE_GRID
        } else {
            PAGE_LIST
        };
        let (visible, pages, total) = filter_sort_page(
            &results,
            "",
            self.search_letter,
            self.search_sort,
            self.search_page,
            per_page,
        );
        self.search_page = self.search_page.min(pages - 1);
        page_bar(ui, &mut self.search_page, pages, total);
        ui.add_space(6.0);

        let poster_paths: Vec<String> = visible
            .iter()
            .filter_map(|it| {
                it.get("thumb")
                    .or_else(|| it.get("art"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .collect();
        self.prefetch_posters(&poster_paths);

        let mut inspect_target = None;
        let mut play_target = None;

        card(ui, "Search Results", |ui| {
            if results.is_empty() {
                ui.label(RichText::new("Enter a search term and press Search.").color(faint(ui)));
            } else if visible.is_empty() {
                ui.label(RichText::new("No results on this page.").color(faint(ui)));
            } else if self.search_mode_grid {
                ui.horizontal_wrapped(|ui| {
                    for r in visible {
                        let title = r.get("title").and_then(Value::as_str).unwrap_or("Title");
                        let year = r
                            .get("year")
                            .and_then(Value::as_i64)
                            .map(|y| format!("{y}"));
                        let r_key = r
                            .get("ratingKey")
                            .or_else(|| r.get("rating_key"))
                            .and_then(Value::as_str)
                            .unwrap_or("0");
                        let thumb = r
                            .get("thumb")
                            .or_else(|| r.get("art"))
                            .and_then(Value::as_str);
                        let tex = thumb
                            .and_then(|p| self.poster_cache.get(p))
                            .and_then(|opt| opt.as_ref());
                        let selected = self.selected_rating_key == r_key;
                        let action = movie_card(ui, tex, title, year.as_deref(), selected, 120.0);
                        if action == CardAction::DoubleClicked {
                            play_target = Some((*r).clone());
                        } else if action == CardAction::Clicked {
                            inspect_target = Some(r_key.to_string());
                        }
                    }
                });
            } else {
                for r in visible {
                    let title = r.get("title").and_then(Value::as_str).unwrap_or("Title");
                    let kind = r.get("type").and_then(Value::as_str).unwrap_or("item");
                    let r_key = r
                        .get("ratingKey")
                        .or_else(|| r.get("rating_key"))
                        .and_then(Value::as_str)
                        .unwrap_or("0");
                    let thumb = r
                        .get("thumb")
                        .or_else(|| r.get("art"))
                        .and_then(Value::as_str);
                    let tex = thumb
                        .and_then(|p| self.poster_cache.get(p))
                        .and_then(|opt| opt.as_ref());

                    let row = ui.horizontal(|ui| {
                        render_poster_image(ui, tex, Vec2::new(36.0, 54.0), title);
                        ui.label(RichText::new(title).strong().color(ink(ui)));
                        ui.label(
                            RichText::new(format!("[{kind}]"))
                                .size(11.0)
                                .color(amber(ui)),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("Inspect").clicked() {
                                inspect_target = Some(r_key.to_string());
                            }
                            let play_btn = egui::Button::new(
                                RichText::new("> Play").strong().color(Color32::BLACK),
                            )
                            .fill(AMBER);
                            if ui.add(play_btn).clicked() {
                                play_target = Some((*r).clone());
                            }
                        });
                    });
                    if row.response.double_clicked() {
                        play_target = Some((*r).clone());
                    }
                    ui.separator();
                }
            }
        });

        if let Some(item) = play_target {
            self.play_media_item(&item);
        }

        if let Some(r_key) = inspect_target {
            self.selected_rating_key = r_key.clone();
            self.page = Page::Movies;
            let tx = self.tx.clone();
            let config = self.get_plex_config();
            self.spawn(async move {
                if let Ok(v) = dispatch(&config, Command::MediaGet { rating_key: r_key }).await {
                    let _ = tx.send(Job::MediaDetail(v));
                }
            });
        }
    }

    // 7. Semantic Search
    fn view_semantic_search(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Semantic Search").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Natural language similarity search over media plot summaries and synopsis vectors.",
            )
            .color(sub(ui)),
        );
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
                ui.label(
                    RichText::new(
                        "Enter a descriptive query like 'intense 80s sci-fi thrillers with cyborgs'",
                    )
                    .color(faint(ui)),
                );
            } else {
                for m in &matches {
                    let title = m.get("title").and_then(Value::as_str).unwrap_or("Title");
                    let score = m.get("score").and_then(Value::as_f64).unwrap_or(0.0);
                    let synopsis = m.get("synopsis").and_then(Value::as_str).unwrap_or("");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong().color(ink(ui)));
                        ui.label(RichText::new(format!("Similarity: {:.2}", score)).color(EMERALD));
                    });
                    if !synopsis.is_empty() {
                        ui.label(RichText::new(synopsis).color(sub(ui)));
                    }
                    ui.separator();
                }
            }
        });
    }

    // 8. RAG Management
    fn view_rag(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("RAG Management").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Vector database status, media synopsis embeddings, and vector depot health.",
            )
            .color(sub(ui)),
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

    // 9. Canon Coverage (mirrors webapp canon tabs: Oscars / AFI 100 /
    // TMDB Top / custom list; live per-source list comparison is a core
    // follow-up - buttons switch source context and re-run the report).
    fn view_canon(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Canon Coverage").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Check completeness against Oscar winners, AFI 100, TMDB Top, and custom lists.",
            )
            .color(sub(ui)),
        );
        ui.add_space(12.0);

        let sources = [
            ("oscars", "Oscar Winners", "Every Best Picture winner"),
            ("afi", "AFI 100", "AFI 2007 top-100"),
            ("tmdb", "TMDB Top", "Top-rated by TMDB voters"),
            ("custom", "Custom List", "Your own canon slice"),
        ];
        let mut picked: Option<String> = None;
        card(ui, "Canon Sources", |ui| {
            ui.horizontal_wrapped(|ui| {
                for (id, label, hint) in sources {
                    let active = self.canon_source == id;
                    let btn = egui::Button::new(RichText::new(label).strong().color(if active {
                        amber(ui)
                    } else {
                        ink(ui)
                    }))
                    .fill(if active {
                        AMBER_GLOW
                    } else {
                        Color32::TRANSPARENT
                    })
                    .stroke(Stroke::new(
                        if active { 1.5 } else { 1.0 },
                        if active {
                            amber(ui)
                        } else if ui.visuals().dark_mode {
                            BORDER_DARK
                        } else {
                            Color32::from_rgb(148, 163, 184)
                        },
                    ))
                    .corner_radius(CornerRadius::same(6));
                    if ui
                        .add_sized([150.0, 44.0], btn)
                        .on_hover_text(hint)
                        .clicked()
                    {
                        picked = Some(id.to_string());
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "Per-source live list comparison (Wikipedia / TMDB holdings check) lands with the core canon ops follow-up.",
                )
                .size(11.0)
                .color(faint(ui)),
            );
        });

        if let Some(id) = picked {
            self.canon_source = id.clone();
            self.status_line = format!("Canon source: {id}");
            let tx = self.tx.clone();
            let config = self.get_plex_config();
            self.spawn(async move {
                if let Ok(v) = dispatch(&config, Command::CanonCoverage { library: None }).await {
                    let _ = tx.send(Job::CanonReport(v));
                }
            });
        }

        ui.add_space(10.0);

        let coverage = self
            .canon_val
            .get("coverage_pct")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        let gaps = self
            .canon_val
            .get("canon_gaps")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let sections = self.canon_val.get("total_sections").and_then(Value::as_u64);

        card(ui, "Coverage Report", |ui| {
            if self.canon_val.is_null() {
                ui.label(RichText::new("Loading canon coverage report...").color(faint(ui)));
            } else {
                ui.horizontal(|ui| {
                    kpi_card(
                        ui,
                        "Coverage",
                        &format!("{coverage:.1}%"),
                        "Canon titles held",
                        EMERALD,
                    );
                    if let Some(total) = sections {
                        kpi_card(
                            ui,
                            "Sections",
                            &format!("{total}"),
                            "Libraries scanned",
                            SKY,
                        );
                    }
                    kpi_card(
                        ui,
                        "Gaps",
                        &format!("{}", gaps.len()),
                        "Franchises incomplete",
                        AMBER,
                    );
                });
            }
        });

        ui.add_space(10.0);

        card(ui, "Missing Titles by Franchise", |ui| {
            if gaps.is_empty() {
                ui.label(
                    RichText::new("No gaps reported - run a source above to refresh.")
                        .color(faint(ui)),
                );
            } else {
                for gap in &gaps {
                    let franchise = gap
                        .get("franchise")
                        .and_then(Value::as_str)
                        .unwrap_or("Franchise");
                    ui.label(RichText::new(franchise).strong().color(ink(ui)));
                    let missing = gap
                        .get("missing")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for title in &missing {
                        let name = title
                            .get("title")
                            .and_then(Value::as_str)
                            .unwrap_or(title.as_str().unwrap_or("Unknown title"));
                        let year = title
                            .get("year")
                            .and_then(Value::as_i64)
                            .map(|y| format!(" ({y})"))
                            .unwrap_or_default();
                        ui.label(RichText::new(format!("· {name}{year}")).color(sub(ui)));
                    }
                    ui.add_space(6.0);
                }
            }
        });
    }

    // 10. Enrichment
    fn view_enrichment(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Metadata Enrichment")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Enhance media items with posters, subtitles, chapter markers, and ratings.",
            )
            .color(sub(ui)),
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
                ui.label(RichText::new("Enrichment Result:").strong().color(ink(ui)));
                ui.label(
                    RichText::new(
                        serde_json::to_string_pretty(&self.enrich_val).unwrap_or_default(),
                    )
                    .color(sub(ui)),
                );
            }
        });
    }

    // 11. Taste
    fn view_taste(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Taste & Viewing Analytics")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("User watch habits, top genres, directors, and AI recommendations.")
                .color(sub(ui)),
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
                    RichText::new("Taste profile analyzing watch history...").color(faint(ui)),
                );
            } else {
                for g in &genres {
                    let genre = g.get("genre").and_then(Value::as_str).unwrap_or("Genre");
                    let count = g.get("count").and_then(Value::as_u64).unwrap_or(0);
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("· {}: {} titles watched", genre, count))
                                .color(ink(ui)),
                        );
                    });
                }
            }
        });
    }

    // 12. Chat
    fn view_chat(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Assistant Chat").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Interactive LLM media assistant with full Plex library context.")
                .color(sub(ui)),
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

        egui::ScrollArea::vertical()
            .max_height(400.0)
            .show(ui, |ui| {
                for (sender, msg) in &self.chat_history {
                    let is_dark = ui.visuals().dark_mode;
                    let is_user = sender == "User";
                    let bg = if is_dark {
                        if is_user {
                            Color32::from_rgb(26, 36, 54)
                        } else {
                            Color32::from_rgb(33, 46, 68)
                        }
                    } else if is_user {
                        Color32::from_rgb(226, 232, 240)
                    } else {
                        Color32::WHITE
                    };
                    let border = if is_user {
                        if is_dark {
                            Color32::from_rgb(51, 65, 85)
                        } else {
                            Color32::from_rgb(148, 163, 184)
                        }
                    } else {
                        amber(ui)
                    };

                    egui::Frame::NONE
                        .fill(bg)
                        .stroke(Stroke::new(1.5, border))
                        .inner_margin(Margin::same(8))
                        .corner_radius(CornerRadius::same(6))
                        .show(ui, |ui| {
                            ui.label(RichText::new(sender).strong().color(if is_user {
                                SKY
                            } else {
                                amber(ui)
                            }));
                            ui.add_space(2.0);
                            let msg_color = ink(ui);
                            ui.label(RichText::new(msg).color(msg_color));
                        });
                    ui.add_space(6.0);
                }
            });

        ui.add_space(8.0);

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
        ui.heading(RichText::new("MCP Tools Catalog").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Execute any of the 36 registered FastMCP server tools in-process.")
                .color(sub(ui)),
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
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(RichText::new("Automated workflows for library maintenance, transcode pre-warming, and playlists.").color(sub(ui)));
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
                        ui.label(RichText::new(name).strong().color(ink(ui)));
                        ui.label(RichText::new(desc).color(sub(ui)));
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
                        ui.label(RichText::new(name).strong().color(ink(ui)));
                        ui.label(RichText::new(format!("- {}", desc)).color(sub(ui)));
                    });
                    ui.add_space(2.0);
                }
            }
        });
    }

    // 15. API Docs
    fn view_api_docs(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("API Documentation").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Interactive API reference for Plex REST endpoints and FastMCP tools.")
                .color(sub(ui)),
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
                    ui.label(RichText::new(ep).strong().color(amber(ui)));
                    ui.label(RichText::new(format!("· {}", desc)).color(sub(ui)));
                });
                ui.add_space(2.0);
            }
        });
    }

    // 16. Server
    fn view_server(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Server Telemetry").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Plex Media Server system specifications, platform version, and network ports.",
            )
            .color(sub(ui)),
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

            ui.label(RichText::new(format!("· Server Version: {}", ver)).color(ink(ui)));
            ui.label(RichText::new(format!("· Platform OS: {}", platform)).color(ink(ui)));
            ui.label(RichText::new(format!("· Machine Identifier: {}", machine_id)).color(ink(ui)));
            ui.label(
                RichText::new("· Transcoder Hardware Acceleration: Intel QuickSync / NVIDIA NVENC")
                    .color(ink(ui)),
            );
            ui.label(RichText::new("· Local Port: :32400 (HTTP / HTTPS)").color(ink(ui)));
        });
    }

    // 17. Logs
    fn view_logs(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("System Logs & Events")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Real-time event stream from plex-core in-process dispatcher.")
                .color(sub(ui)),
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
                        ui.label(RichText::new(log).color(sub(ui)).size(12.0));
                    }
                }
            });
    }

    // 18. Help
    fn view_help(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Help & Documentation")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Fleet guidelines, credential setup, port mappings, and troubleshooting.",
            )
            .color(sub(ui)),
        );
        ui.add_space(12.0);

        card(ui, "Plex Token Retrieval", |ui| {
            ui.label(
                RichText::new("How to find your Plex Token:")
                    .strong()
                    .color(ink(ui)),
            );
            ui.label(
                RichText::new("1. Log in to your Plex Web app in any browser.").color(sub(ui)),
            );
            ui.label(RichText::new("2. Browse to any movie or television episode.").color(sub(ui)));
            ui.label(RichText::new("3. Click '...' -> 'Get Info' -> 'View XML'.").color(sub(ui)));
            ui.label(
                RichText::new(
                    "4. Look at the URL in the address bar: copy the value of 'X-Plex-Token'.",
                )
                .color(sub(ui)),
            );
        });

        ui.add_space(10.0);

        card(ui, "FastMCP Stdio Integration", |ui| {
            ui.label(
                RichText::new(
                    "Add this configuration to your Claude Desktop or Antigravity config:",
                )
                .strong()
                .color(ink(ui)),
            );
            let snippet = "{\n  \"mcpServers\": {\n    \"plex\": {\n      \"command\": \"target/release/rust-plex-mcp.exe\",\n      \"args\": [\"mcp\"]\n    }\n  }\n}";
            ui.label(RichText::new(snippet).color(amber(ui)));
        });
    }

    // 19. Settings (with Fleet Standard LLM Settings Card)
    fn view_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading(RichText::new("Settings").color(ink(ui)).size(22.0));
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Plex server connection, LLM providers, OS Credential Vault, and appearance.",
            )
            .color(sub(ui)),
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
                ui.label(RichText::new("Test Status:").color(sub(ui)));
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
                        .color(faint(ui))
                        .size(11.0),
                );
            });
        });
    }

    // 20. Crossconnect
    fn view_crossconnect(&mut self, ui: &mut egui::Ui) {
        ui.heading(
            RichText::new("Fleet Crossconnect Bridge")
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Dispatch TTS announcements to speech-mcp (:10909) and telemetry to aiwatcher (:10946).",
            )
            .color(sub(ui)),
        );
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
                .color(ink(ui))
                .size(22.0),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new("Inspect live wire payload from the most recent command execution.")
                .color(sub(ui)),
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
                ui.label(RichText::new("PLEX · ARTCRAFT").strong().color(amber(ui)));
                ui.separator();
                ui.label(
                    RichText::new(format!("View: {}", self.page.label()))
                        .size(12.0)
                        .color(ink(ui)),
                );
                ui.separator();
                if self.pending > 0 {
                    ui.label(
                        RichText::new(format!("Loading… ({} pending)", self.pending))
                            .color(amber(ui)),
                    );
                } else {
                    ui.label(RichText::new(&self.status_line).color(sub(ui)).size(12.0));
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
            render_poster_image(ui, None, Vec2::new(60.0, 90.0), "Demo Film");
            let _ = movie_card(ui, None, "Blade Runner", Some("1982"), false, 110.0);
        });
        harness.run();
    }
}
