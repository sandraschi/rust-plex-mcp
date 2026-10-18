//! Custom Plex registry: user-pinned servers and media items.
//!
//! Stored as plain JSON next to `ui.json`. Corrupt file -> empty registry, never a crash.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegServer {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegPinnedMedia {
    pub id: String,
    pub title: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Registry {
    pub servers: Vec<RegServer>,
    pub pinned: Vec<RegPinnedMedia>,
}

pub fn path() -> Option<std::path::PathBuf> {
    std::env::var("LOCALAPPDATA").ok().map(|base| {
        std::path::PathBuf::from(base)
            .join("rust-plex-mcp")
            .join("registry.json")
    })
}

pub fn load() -> Registry {
    let Some(path) = path() else {
        return Registry::default();
    };
    let text = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

pub fn save(registry: &Registry) {
    let Some(path) = path() else { return };
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(parent);
        }
    }
    if let Ok(text) = serde_json::to_string_pretty(registry) {
        let _ = std::fs::write(path, text);
    }
}

pub fn pin_server(name: &str, url: &str) -> Registry {
    let mut reg = load();
    if !reg.servers.iter().any(|s| s.url == url) {
        reg.servers.push(RegServer {
            name: name.to_string(),
            url: url.to_string(),
        });
        save(&reg);
    }
    reg
}

pub fn unpin_server(url: &str) -> Registry {
    let mut reg = load();
    reg.servers.retain(|s| s.url != url);
    save(&reg);
    reg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_file_is_empty_registry() {
        let registry: Registry = serde_json::from_str("not json{{").unwrap_or_default();
        assert!(registry.servers.is_empty());
        assert!(registry.pinned.is_empty());
    }

    #[test]
    fn registry_manipulations() {
        let mut reg = Registry::default();
        reg.servers.push(RegServer {
            name: "TestServer".to_string(),
            url: "http://127.0.0.1:32400".to_string(),
        });
        assert_eq!(reg.servers.len(), 1);
        reg.servers.retain(|s| s.name != "TestServer");
        assert!(reg.servers.is_empty());
    }
}
