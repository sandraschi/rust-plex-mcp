//! OS secret vault (Windows Credential Manager via `keyring`).
//!
//! Passwords must survive restarts WITHOUT living in plain files.
//! `ui.json` holds config; this vault holds the secrets. Reads
//! fail soft (session-only mode + a note) when the vault is unavailable
//! (headless CI, locked keychains) -- never a crash, never a log line.
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]
#![allow(dead_code)]

pub const SERVICE: &str = "rust-plex-mcp";

/// Secret slots. Names are stable -- renaming orphans stored credentials.
pub const PLEX_TOKEN: &str = "plex-token";
pub const LLM_API_KEY: &str = "llm-api-key";
pub const WEBHOOK_URL: &str = "webhook-url";
pub const SPEECH_URL: &str = "speech-url";

pub fn get(slot: &str) -> Result<Option<String>, String> {
    let entry = keyring::Entry::new(SERVICE, slot).map_err(|e| format!("vault open: {e}"))?;
    match entry.get_password() {
        Ok(secret) if !secret.trim().is_empty() => Ok(Some(secret)),
        Ok(_) => Ok(None),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("vault read: {e}")),
    }
}

pub fn set(slot: &str, secret: &str) -> Result<(), String> {
    if secret.trim().is_empty() {
        return delete(slot);
    }
    let entry = keyring::Entry::new(SERVICE, slot).map_err(|e| format!("vault open: {e}"))?;
    entry
        .set_password(secret)
        .map_err(|e| format!("vault write: {e}"))
}

pub fn delete(slot: &str) -> Result<(), String> {
    let entry = keyring::Entry::new(SERVICE, slot).map_err(|e| format!("vault open: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("vault delete: {e}")),
    }
}
