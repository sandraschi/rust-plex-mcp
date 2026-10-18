//! Credential resolution and shared I/O guards.
//!
//! Precedence everywhere: explicit args > process env > legacy `.env`
//! (via `import::fleet_secret`). Secrets live in memory only: never logged,
//! never committed. Plex auth material (server URL + token or account) is
//! resolved by the caller from these primitives; see `import.rs`.
use crate::error::DeviceError;
use std::future::Future;
use std::time::Duration;

/// Single-call I/O budget. Multi-step handshakes use explicit multiples.
pub const DEVICE_TIMEOUT: Duration = Duration::from_secs(5);

/// Account material. `None` means "not provided anywhere" — callers turn
/// that into a typed `Auth` error naming the exact env var.
#[derive(Debug, Clone, Default)]
pub struct Credentials {
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Credentials {
    /// Explicit args win; environment is the fallback (process env, then
    /// legacy `.env` via the fleet import).
    pub fn resolve(
        username: Option<String>,
        password: Option<String>,
    ) -> Result<Self, DeviceError> {
        let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        Ok(Self {
            username: clean(username).or_else(|| env_non_empty("PLEX_USERNAME")),
            password: clean(password).or_else(|| env_non_empty("PLEX_PASSWORD")),
        })
    }

    /// Require both halves (server login flows) or fail typed.
    /// Callers hold no secrets in logs: this returns owned clones for the
    /// handshake call only.
    pub fn require_account(&self) -> Result<(String, String), DeviceError> {
        match (&self.username, &self.password) {
            (Some(u), Some(p)) => Ok((u.clone(), p.clone())),
            _ => Err(DeviceError::Auth(
                "Plex account required: pass username/password or set PLEX_USERNAME + PLEX_PASSWORD".to_string(),
            )),
        }
    }
}

fn env_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .or_else(|| crate::import::fleet_secret(name))
}

/// Shared vendor-error classifier: timeout/auth/unsupported/protocol from
/// message text. Every driver maps its foreign error type through here so
/// agents see one stable taxonomy.
pub fn classify_vendor_error(context: String) -> DeviceError {
    let lower = context.to_ascii_lowercase();
    if lower.contains("timed out") || lower.contains("timeout") || lower.contains("deadline") {
        DeviceError::Timeout(context)
    } else if lower.contains("auth")
        || lower.contains("login")
        || lower.contains("password")
        || lower.contains("credential")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("401")
        || lower.contains("403")
    {
        DeviceError::Auth(context)
    } else if lower.contains("unsupported") || lower.contains("not support") {
        DeviceError::Unsupported(context)
    } else {
        DeviceError::Protocol(context)
    }
}

/// Bound any single device op. Timeout carries the op name, never a secret.
pub async fn with_timeout<T>(op: &str, fut: impl Future<Output = T>) -> Result<T, DeviceError> {
    with_timeout_secs(op, DEVICE_TIMEOUT.as_secs(), fut).await
}

/// Multi-step handshakes get an explicit budget.
pub async fn with_timeout_secs<T>(
    op: &str,
    secs: u64,
    fut: impl Future<Output = T>,
) -> Result<T, DeviceError> {
    match tokio::time::timeout(Duration::from_secs(secs), fut).await {
        Ok(value) => Ok(value),
        Err(_) => Err(DeviceError::Timeout(format!("{op} exceeded {secs}s"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_args_win_over_env() {
        let creds = Credentials::resolve(Some("u".to_string()), None).unwrap_or_default();
        assert_eq!(creds.username.as_deref(), Some("u"));
    }

    #[test]
    fn missing_account_is_typed_auth_error() {
        let creds = Credentials::default();
        assert!(matches!(creds.require_account(), Err(DeviceError::Auth(_))));
    }
}
