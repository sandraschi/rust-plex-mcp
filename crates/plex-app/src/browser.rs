//! Fleet kernel: shared list-browser controls for `egui` list pages.
//!
//! Drop-in reuse: copy this file **verbatim** into any repo's app `src/`
//! next to `app.rs`, add `mod browser;`, and import what the page needs.
//! Only dependencies are `eframe::egui` and `serde_json` — zero app state,
//! zero network, zero theme globals. (Repconv SOP Phase 1 kernel list.)
//!
//! Every list page gets the same five controls with zero re-invention:
//! Card/List-agnostic [`filter_sort_page`] pipeline (text search + first-
//! letter bucket + sort + page slice), [`letter_row`] first-letter jump row
//! for very large lists, [`page_bar`] First/Prev/Next/Last pagination, and
//! [`sort_combo`] sort dropdown. Text colors go through [`ink`], [`sub`],
//! [`faint`], and [`amber`] so pages stay legible in both themes.
//!
//! Item shape contract: list items are `serde_json::Value` objects with
//! `title`, `type`, and `year` fields (Plex `ratingKey`/`type`/`year`,
//! Calibre `title`/`author`, file rows `name`/`size` — adapt [`title_of`],
//! [`type_of`], [`year_of`] per domain when the keys differ).

use eframe::egui::{self, Color32, CornerRadius, RichText};

const AMBER: Color32 = Color32::from_rgb(245, 158, 11);
const AMBER_GLOW: Color32 = Color32::from_rgba_premultiplied(245, 158, 11, 35);
const SLATE_MUTED: Color32 = Color32::from_rgb(148, 163, 184);
const SLATE_FAINT: Color32 = Color32::from_rgb(100, 116, 139);
/// Amber darkened for text on light backgrounds (amber-700; raw AMBER
/// fails contrast on white).
pub const AMBER_DARK: Color32 = Color32::from_rgb(180, 83, 9);
pub const INK_LIGHT: Color32 = Color32::from_rgb(15, 23, 42);
pub const SUB_LIGHT: Color32 = Color32::from_rgb(51, 65, 85);
pub const FAINT_LIGHT: Color32 = Color32::from_rgb(71, 85, 105);

/// Primary text: near-white in dark mode, slate-900 in light mode.
/// Use for every heading / title / body label instead of `Color32::WHITE`.
#[must_use]
pub fn ink(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        Color32::from_rgb(241, 245, 249)
    } else {
        INK_LIGHT
    }
}

/// Secondary text (hardcoded slate-400 is unreadable on white).
#[must_use]
pub fn sub(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        SLATE_MUTED
    } else {
        SUB_LIGHT
    }
}

/// Faint text (hardcoded slate-500 is invisible on white).
#[must_use]
pub fn faint(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        SLATE_FAINT
    } else {
        FAINT_LIGHT
    }
}

/// Accent text/border that stays readable on both themes.
#[must_use]
pub fn amber(ui: &egui::Ui) -> Color32 {
    if ui.visuals().dark_mode {
        AMBER
    } else {
        AMBER_DARK
    }
}

/// Char-boundary-safe truncation. `&title[..n]` byte slices panic on
/// multibyte titles (e.g. Japanese library names).
#[must_use]
pub fn trunc(text: &str, max_chars: usize) -> &str {
    if text.chars().count() <= max_chars {
        return text;
    }
    let end = text
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(text.len());
    &text[..end]
}

/// Sort orders offered by every list page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortMode {
    #[default]
    TitleAsc,
    TitleDesc,
    YearNew,
    YearOld,
    TypeAsc,
}

impl SortMode {
    /// All options in dropdown order.
    pub const ALL: &'static [(Self, &'static str)] = &[
        (Self::TitleAsc, "Title A-Z"),
        (Self::TitleDesc, "Title Z-A"),
        (Self::YearNew, "Newest first"),
        (Self::YearOld, "Oldest first"),
        (Self::TypeAsc, "Type A-Z"),
    ];

    #[must_use]
    pub fn label(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(mode, _)| *mode == self)
            .map(|(_, label)| *label)
            .unwrap_or("Title A-Z")
    }
}

/// Items per list page. Grids show 48 posters; rows show 25 lines.
pub const PAGE_GRID: usize = 48;
pub const PAGE_LIST: usize = 25;

/// Title accessor. Override per domain when the key differs.
#[must_use]
pub fn title_of(item: &serde_json::Value) -> &str {
    item.get("title")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// Year accessor. Missing years sort last in both directions.
#[must_use]
pub fn year_of(item: &serde_json::Value) -> i64 {
    item.get("year")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(i64::MIN)
}

/// Type/kind accessor for the Type sort and query matching.
#[must_use]
pub fn type_of(item: &serde_json::Value) -> &str {
    item.get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
}

/// First-letter bucket for the jump row: A-Z, else `#`.
#[must_use]
pub fn letter_of(title: &str) -> char {
    title
        .chars()
        .next()
        .map(|c| {
            let upper = c.to_uppercase().next().unwrap_or('#');
            if upper.is_ascii_alphabetic() {
                upper
            } else {
                '#'
            }
        })
        .unwrap_or('#')
}

/// Shared pipeline: text query + first-letter bucket + sort + page slice.
/// Returns the visible items, page count, and pre-page total.
#[must_use]
pub fn filter_sort_page<'a>(
    items: &'a [serde_json::Value],
    query: &str,
    letter: Option<char>,
    sort: SortMode,
    page: usize,
    per_page: usize,
) -> (Vec<&'a serde_json::Value>, usize, usize) {
    let query = query.to_lowercase();
    let mut picked: Vec<&serde_json::Value> = items
        .iter()
        .filter(|item| {
            let title = title_of(item);
            if let Some(want) = letter {
                if letter_of(title) != want {
                    return false;
                }
            }
            if query.is_empty() {
                return true;
            }
            title.to_lowercase().contains(&query) || type_of(item).to_lowercase().contains(&query)
        })
        .collect();
    match sort {
        SortMode::TitleAsc => {
            picked.sort_by_key(|item| title_of(item).to_lowercase());
        }
        SortMode::TitleDesc => {
            picked.sort_by_key(|item| std::cmp::Reverse(title_of(item).to_lowercase()));
        }
        SortMode::YearNew => {
            picked.sort_by_key(|item| std::cmp::Reverse(year_of(item)));
        }
        SortMode::YearOld => {
            picked.sort_by_key(|item| year_of(item));
        }
        SortMode::TypeAsc => {
            picked.sort_by(|a, b| {
                type_of(a)
                    .cmp(type_of(b))
                    .then(title_of(a).to_lowercase().cmp(&title_of(b).to_lowercase()))
            });
        }
    }
    let total = picked.len();
    let per_page = per_page.max(1);
    let pages = total.div_ceil(per_page).max(1);
    let page = page.min(pages - 1);
    let start = page.saturating_mul(per_page);
    let visible = picked.into_iter().skip(start).take(per_page).collect();
    (visible, pages, total)
}

/// Distinct first letters present in a slice, `#` last.
#[must_use]
pub fn present_letters(items: &[serde_json::Value]) -> Vec<char> {
    let mut letters: Vec<char> = items.iter().map(|item| letter_of(title_of(item))).collect();
    letters.sort_unstable();
    letters.dedup();
    letters.sort_by_key(|c| (*c == '#', *c));
    letters
}

/// First-letter jump row for very large lists. Selecting a letter resets
/// to page 0; `All` clears the bucket.
pub fn letter_row(
    ui: &mut egui::Ui,
    letters: &[char],
    selected: &mut Option<char>,
    page: &mut usize,
) {
    ui.horizontal_wrapped(|ui| {
        let all_active = selected.is_none();
        let all_btn = egui::Button::new(RichText::new("All").strong().color(if all_active {
            amber(ui)
        } else {
            sub(ui)
        }))
        .corner_radius(CornerRadius::same(4));
        if ui.add_sized([38.0, 24.0], all_btn).clicked() {
            *selected = None;
            *page = 0;
        }
        for letter in letters {
            let active = *selected == Some(*letter);
            let btn =
                egui::Button::new(RichText::new(letter.to_string()).strong().color(if active {
                    amber(ui)
                } else {
                    ink(ui)
                }))
                .fill(if active {
                    AMBER_GLOW
                } else {
                    Color32::TRANSPARENT
                })
                .corner_radius(CornerRadius::same(4));
            if ui.add_sized([30.0, 24.0], btn).clicked() {
                *selected = if active { None } else { Some(*letter) };
                *page = 0;
            }
        }
    });
}

/// First / Prev / page indicator / Next / Last bar. Clamps `page`.
pub fn page_bar(ui: &mut egui::Ui, page: &mut usize, pages: usize, total: usize) {
    let pages = pages.max(1);
    *page = (*page).min(pages - 1);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(*page > 0, egui::Button::new("First"))
            .clicked()
        {
            *page = 0;
        }
        if ui
            .add_enabled(*page > 0, egui::Button::new("Prev"))
            .clicked()
        {
            *page = page.saturating_sub(1);
        }
        ui.label(
            RichText::new(format!("Page {} of {} ({total} items)", *page + 1, pages))
                .color(sub(ui)),
        );
        if ui
            .add_enabled(*page + 1 < pages, egui::Button::new("Next"))
            .clicked()
        {
            *page = page.saturating_add(1).min(pages - 1);
        }
        if ui
            .add_enabled(*page + 1 < pages, egui::Button::new("Last"))
            .clicked()
        {
            *page = pages - 1;
        }
    });
}

/// Sort dropdown shared by list pages. Resets to page 0 on change.
pub fn sort_combo(ui: &mut egui::Ui, id: &str, sort: &mut SortMode, page: &mut usize) {
    ui.label("Sort:");
    egui::ComboBox::from_id_salt(id)
        .selected_text(sort.label())
        .show_ui(ui, |ui| {
            for (mode, label) in SortMode::ALL {
                if ui.selectable_value(sort, *mode, *label).clicked() {
                    *page = 0;
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn items() -> Vec<serde_json::Value> {
        vec![
            json!({"title": "Blade Runner", "type": "movie", "year": 1982}),
            json!({"title": "Convention Tales", "type": "show", "year": 2020}),
            json!({"title": "日本語タイトル", "type": "movie", "year": 2024}),
            json!({"title": "2001: A Space Odyssey", "type": "movie", "year": 1968}),
        ]
    }

    #[test]
    fn trunc_never_splits_multibyte_titles() {
        assert_eq!(trunc("日本語タイトル", 3), "日本語");
        assert_eq!(trunc("short", 14), "short");
    }

    #[test]
    fn letter_buckets_cover_alpha_and_hash() {
        assert_eq!(letter_of("Blade Runner"), 'B');
        assert_eq!(letter_of("日本語"), '#');
        assert_eq!(letter_of("2001: A Space Odyssey"), '#');
        assert_eq!(letter_of(""), '#');
    }

    #[test]
    fn pipeline_filters_sorts_and_pages() {
        let all = items();
        let (visible, pages, total) = filter_sort_page(&all, "", None, SortMode::TitleAsc, 0, 2);
        assert_eq!(total, 4);
        assert_eq!(pages, 2);
        assert_eq!(visible.len(), 2);
        let (second, _, _) = filter_sort_page(&all, "", None, SortMode::TitleAsc, 1, 2);
        assert_eq!(second.len(), 2);
        let (queried, _, queried_total) =
            filter_sort_page(&all, "blade", None, SortMode::TitleAsc, 0, 48);
        assert_eq!(queried_total, 1);
        assert_eq!(title_of(queried[0]), "Blade Runner");
        let (_, _, letter_total) = filter_sort_page(&all, "", Some('#'), SortMode::TitleAsc, 0, 48);
        assert_eq!(letter_total, 2);
    }

    #[test]
    fn year_sorts_put_missing_years_last() {
        let mut all = items();
        all.push(json!({"title": "No Year", "type": "movie"}));
        let (visible, _, _) = filter_sort_page(&all, "", None, SortMode::YearNew, 0, 48);
        assert_eq!(title_of(visible[0]), "日本語タイトル");
        assert_eq!(title_of(visible[visible.len() - 1]), "No Year");
    }
}
