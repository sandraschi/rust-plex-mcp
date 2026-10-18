//! Fleet import: legacy `plex-mcp` checkout discovery + secret resolution.
//!
//! Precedence everywhere: explicit args > process env > legacy `.env`.
//! Base directory resolves from `PLEX_MCP_DIR`, else by walking up from the
//! running exe looking for a `plex-mcp` sibling. Secrets live in memory
//! only: never logged, never committed (this repo has no `.env` of its own).
//! Phase 2 extends this with `config.json`/library-path parsing per
//! `docs/CONVERSION_DOSSIER.md` §4.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Locate the legacy `plex-mcp` checkout, if this machine has one.
pub fn fleet_base_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("PLEX_MCP_DIR") {
        let path = PathBuf::from(dir.trim());
        if path.is_dir() {
            return Some(path);
        }
    }
    let exe = std::env::current_exe().ok()?;
    let mut cursor = exe.parent();
    for _ in 0..8 {
        let dir = cursor?;
        let candidate = dir.join("plex-mcp");
        if candidate.is_dir() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

fn dotenv_map(base: &Path) -> HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut map = HashMap::new();
            let text = std::fs::read_to_string(base.join(".env")).unwrap_or_default();
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let line = line.strip_prefix("export ").unwrap_or(line);
                let Some((key, value)) = line.split_once('=') else {
                    continue;
                };
                let key = key.trim().to_string();
                let mut value = value.trim().to_string();
                if value.len() >= 2 && value.starts_with('"') && value.ends_with('"')
                    || value.len() >= 2 && value.starts_with('\'') && value.ends_with('\'')
                {
                    value = value[1..value.len() - 1].to_string();
                }
                if let Some(commented) = value.split(" #").next() {
                    value = commented.trim().to_string();
                }
                if !key.is_empty() && !value.is_empty() {
                    map.insert(key, value);
                }
            }
            map
        })
        .clone()
}

/// Secret lookup: process env first, legacy `.env` second.
pub fn fleet_secret(name: &str) -> Option<String> {
    if let Ok(value) = std::env::var(name) {
        let trimmed = value.trim().to_string();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    let base = fleet_base_dir()?;
    dotenv_map(&base).get(name).cloned()
}

/// First hit across aliases.
pub fn fleet_secret_any(names: &[&str]) -> Option<String> {
    names.iter().filter_map(|name| fleet_secret(name)).next()
}

/// Expand `${VAR}`, `${VAR:-default}`, `${VAR-default}` and `$VAR`
/// references; plain values pass through untouched.
pub fn expand_env(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(inner) = trimmed
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
    {
        let (name, default) = if let Some((name, fallback)) = inner.split_once(":-") {
            (name, Some(fallback))
        } else if let Some((name, fallback)) = inner.split_once('-') {
            (name, Some(fallback))
        } else {
            (inner, None)
        };
        if let Some(hit) = fleet_secret(name.trim()) {
            return Some(hit);
        }
        return default
            .map(|fallback| fallback.trim().to_string())
            .filter(|fallback| !fallback.is_empty());
    }
    if let Some(name) = trimmed.strip_prefix('$') {
        if !name.is_empty() && !name.contains(char::is_whitespace) {
            return fleet_secret(name);
        }
    }
    Some(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_env_passes_plain_values_and_defaults() {
        assert_eq!(
            expand_env(" http://127.0.0.1:32400 "),
            Some("http://127.0.0.1:32400".to_string())
        );
        assert_eq!(expand_env("   "), None);
        assert_eq!(
            expand_env("${RUST_PLEX_MCP_DEFINITELY_UNSET:-fallback}"),
            Some("fallback".to_string())
        );
        assert_eq!(expand_env("${RUST_PLEX_MCP_DEFINITELY_UNSET}"), None);
    }

    #[test]
    fn missing_checkout_is_none_not_error() {
        let _ = fleet_base_dir();
    }
}
