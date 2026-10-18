//! L3 desktop app: egui immediate-mode GUI over `plex-core`, in-process.
//!
//! ArtCraft native stack -- no WebView2, no JavaScript, one `cargo` toolchain.
//! `--smoke` runs the headless parity check (for CI) without opening a window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

use eframe::egui;
use std::io::Write;
use std::path::PathBuf;

mod app;
mod registry;
mod vault;

fn install_forensic_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("[PANIC in plex-app]: {info}\n");
        let _ = std::io::stderr().write_all(msg.as_bytes());
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

fn main() -> eframe::Result<()> {
    install_forensic_panic_hook();
    if std::env::args().any(|arg| arg == "--smoke") {
        return app::smoke();
    }
    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => std::sync::Arc::new(rt),
        Err(e) => {
            eprintln!("failed to start async runtime: {e}");
            std::process::exit(1);
        }
    };
    let viewport = egui::ViewportBuilder::default()
        .with_title("Plex (Rust) v0.4")
        .with_inner_size([1360.0, 880.0])
        .with_min_inner_size([960.0, 620.0]);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Plex (Rust)",
        options,
        Box::new(move |ctx| Ok(Box::new(app::PlexApp::new(ctx, runtime)))),
    )
}
