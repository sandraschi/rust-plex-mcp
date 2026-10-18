//! L3 desktop shell (Phase 1 scaffold).
//!
//! Phase 1 proves the `eframe` dependency resolves and links; the dashboard
//! (`Libraries`, `Books`, `Analytics` + kernel tabs) lands in Phase 4 per
//! `docs/CONVERSION_DOSSIER.md` §6. Until then this binary reports identity
//! and exits — no fake windows, no mock UI.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

fn main() {
    println!(
        "{}",
        serde_json::json!({
            "app": "plex-app",
            "version": env!("CARGO_PKG_VERSION"),
            "phase": "1-scaffold",
            "next": "Phase 4 egui dashboard (Libraries, Books, Analytics)",
        })
    );
}
