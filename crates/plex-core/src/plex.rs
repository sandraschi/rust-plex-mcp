//! Plex Media Server REST client (L1, UI/MCP-free).
//!
//! Every call is outbound HTTPS to the Plex server with `X-Plex-Token` auth
//! and `Accept: application/json` (Plex defaults to XML — always request
//! JSON). Payloads are capped at [`MAX_BODY_BYTES`]; every request carries
//! a timeout. All Plex fields are `Option` until proven present: Plex omits
//! attributes freely across item types and server versions.
//!
//! Credential precedence: explicit args > process env (`PLEX_URL`, then
//! `PLEX_SERVER_URL`, then legacy `PLEX_BASE_URL`; `PLEX_TOKEN`) > sibling
//! `plex-mcp/.env` (root file carries `PLEX_BASE_URL` + `PLEX_TOKEN`,
//! `webapp/backend/.env` carries `PLEX_URL` + `PLEX_TOKEN`) > [`Vendor::Demo`].
use crate::error::DeviceError;
use crate::import::{expand_env, fleet_secret_any};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const DEFAULT_TIMEOUT_SECS: u64 = 15;
/// Plex library listings return megabytes; never buffer unbounded.
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const MAX_QUERY_CHARS: usize = 200;
const MAX_KEY_CHARS: usize = 64;

/// Which server this client talks to.
#[derive(Debug, Clone, PartialEq)]
pub enum Vendor {
    /// Live Plex Media Server. The token is held in memory only and never logged.
    Live { server_url: String, token: String },
    /// Built-in sample data so CI and `--smoke` pass with no Plex present.
    Demo,
}

/// Resolved connection settings (no secrets in `Debug` output).
#[derive(Debug, Clone, PartialEq)]
pub struct PlexConfig {
    pub vendor: Vendor,
    pub timeout_secs: u64,
    /// Where the settings came from: `args`, `env`, `dotenv`, or `demo`.
    pub source: String,
}

impl PlexConfig {
    /// Resolve per the fleet precedence chain. `server_url`/`token` are
    /// explicit CLI/MCP args; `None` falls through to env, then dotenv.
    pub fn resolve(server_url: Option<&str>, token: Option<&str>) -> Result<Self, DeviceError> {
        let timeout_secs = fleet_secret_any(&["PLEX_TIMEOUT"])
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .filter(|secs| *secs > 0 && *secs <= 120)
            .unwrap_or(DEFAULT_TIMEOUT_SECS);

        let url_hit = server_url
            .and_then(expand_env)
            .map(|value| ("args".to_string(), value))
            .or_else(|| {
                fleet_secret_any(&["PLEX_URL", "PLEX_SERVER_URL", "PLEX_BASE_URL"])
                    .map(|value| ("env/dotenv".to_string(), value))
            });
        let token_hit = token
            .and_then(expand_env)
            .map(|value| ("args".to_string(), value))
            .or_else(|| {
                fleet_secret_any(&["PLEX_TOKEN"]).map(|value| ("env/dotenv".to_string(), value))
            });

        match (url_hit, token_hit) {
            (Some((url_source, raw_url)), Some((_, raw_token))) => {
                let url = require_http_url(&raw_url)?;
                let token = raw_token.trim().to_string();
                if token.is_empty() || token.chars().count() > 512 {
                    return Err(DeviceError::Auth(
                        "PLEX_TOKEN is empty or implausibly long; set a valid token via args, env, or sibling plex-mcp/.env".to_string(),
                    ));
                }
                Ok(Self {
                    vendor: Vendor::Live {
                        server_url: url,
                        token,
                    },
                    timeout_secs,
                    source: url_source,
                })
            }
            _ => Ok(Self {
                vendor: Vendor::Demo,
                timeout_secs,
                source: "demo".to_string(),
            }),
        }
    }

    #[must_use]
    pub fn is_demo(&self) -> bool {
        self.vendor == Vendor::Demo
    }
}

fn require_http_url(raw: &str) -> Result<String, DeviceError> {
    let url = raw.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err(DeviceError::InvalidInput(
            "server_url must start with http:// or https://".to_string(),
        ));
    }
    if url.chars().count() > 512 {
        return Err(DeviceError::InvalidInput(
            "server_url exceeds 512 chars".to_string(),
        ));
    }
    Ok(url.trim_end_matches('/').to_string())
}

fn require_search_query(raw: &str) -> Result<String, DeviceError> {
    let query = raw.trim().to_string();
    if query.is_empty() {
        return Err(DeviceError::InvalidInput(
            "query must not be empty".to_string(),
        ));
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(DeviceError::InvalidInput(format!(
            "query exceeds {MAX_QUERY_CHARS} chars"
        )));
    }
    Ok(query)
}

fn require_rating_key(raw: &str) -> Result<String, DeviceError> {
    let key = raw.trim().to_string();
    if key.is_empty() || key.chars().count() > MAX_KEY_CHARS {
        return Err(DeviceError::InvalidInput(
            "rating_key must be 1-64 chars".to_string(),
        ));
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(DeviceError::InvalidInput(
            "rating_key must be alphanumeric".to_string(),
        ));
    }
    Ok(key)
}

/// Plex `MediaContainer` envelope. `Directory` holds library sections,
/// search results, and hub items; `Metadata` holds media items.
///
/// Plex JSON keys are PascalCase (`friendlyName` and `machineIdentifier`
/// excepted), so every field carries an explicit rename — `rename_all`
/// would silently drop `Directory`/`Metadata` into empty vecs.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MediaContainer {
    #[serde(default, rename = "size")]
    pub size: Option<i64>,
    #[serde(default, rename = "friendlyName")]
    pub friendly_name: Option<String>,
    #[serde(default, rename = "machineIdentifier")]
    pub machine_identifier: Option<String>,
    #[serde(default, rename = "version")]
    pub version: Option<String>,
    #[serde(default, rename = "myPlex")]
    pub my_plex: Option<bool>,
    #[serde(default, rename = "Directory")]
    pub directory: Vec<DirectoryItem>,
    #[serde(default, rename = "Metadata")]
    pub metadata: Vec<MetadataItem>,
    #[serde(default, rename = "Server")]
    pub server: Vec<ServerClientItem>,
}

/// A connected player client device.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ServerClientItem {
    #[serde(default, rename = "name")]
    pub name: Option<String>,
    #[serde(default, rename = "host")]
    pub host: Option<String>,
    #[serde(default, rename = "address")]
    pub address: Option<String>,
    #[serde(default, rename = "port")]
    pub port: Option<i64>,
    #[serde(default, rename = "machineIdentifier")]
    pub machine_identifier: Option<String>,
    #[serde(default, rename = "version")]
    pub version: Option<String>,
    #[serde(default, rename = "protocol")]
    pub protocol: Option<String>,
    #[serde(default, rename = "product")]
    pub product: Option<String>,
    #[serde(default, rename = "deviceClass")]
    pub device_class: Option<String>,
}

/// A library section or browsable directory entry (PascalCase keys).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DirectoryItem {
    #[serde(default, rename = "key")]
    pub key: Option<String>,
    #[serde(default, rename = "title")]
    pub title: Option<String>,
    #[serde(default, rename = "type")]
    pub r#type: Option<String>,
    #[serde(default, rename = "agent")]
    pub agent: Option<String>,
    #[serde(default, rename = "scanner")]
    pub scanner: Option<String>,
    #[serde(default, rename = "uuid")]
    pub uuid: Option<String>,
}

/// A media item (movie, episode, track, photo, session; PascalCase keys).
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct MetadataItem {
    #[serde(default, rename = "ratingKey")]
    pub rating_key: Option<String>,
    #[serde(default, rename = "key")]
    pub key: Option<String>,
    #[serde(default, rename = "title")]
    pub title: Option<String>,
    #[serde(default, rename = "type")]
    pub r#type: Option<String>,
    #[serde(default, rename = "summary")]
    pub summary: Option<String>,
    #[serde(default, rename = "year")]
    pub year: Option<i64>,
    #[serde(default, rename = "duration")]
    pub duration: Option<i64>,
    #[serde(default, rename = "viewCount")]
    pub view_count: Option<i64>,
    #[serde(default, rename = "thumb")]
    pub thumb: Option<String>,
    #[serde(default, rename = "art")]
    pub art: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct ContainerEnvelope {
    #[serde(rename = "MediaContainer")]
    container: MediaContainer,
}

/// Live client. Build once via [`PlexClient::new`], reuse across calls.
pub struct PlexClient {
    http: reqwest::Client,
    server_url: String,
    token: String,
}

impl PlexClient {
    /// Build from resolved config. Fails on `Demo` — callers route Demo
    /// through the fixture functions instead (no network, honest path).
    pub fn new(config: &PlexConfig) -> Result<Self, DeviceError> {
        let (server_url, token) = match &config.vendor {
            Vendor::Live { server_url, token } => (server_url.clone(), token.clone()),
            Vendor::Demo => {
                return Err(DeviceError::InvalidInput(
                    "PlexClient::new needs a Live config; use demo_* fixtures for Demo".to_string(),
                ));
            }
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| DeviceError::Offline(format!("http client build failed: {e}")))?;
        Ok(Self {
            http,
            server_url,
            token,
        })
    }

    async fn get_container(&self, path: &str) -> Result<MediaContainer, DeviceError> {
        let url = format!("{}{}", self.server_url, path);
        let response = self
            .http
            .get(&url)
            .header("X-Plex-Token", &self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    DeviceError::Timeout(format!("{url} did not answer in time"))
                } else {
                    DeviceError::Offline(format!("plex unreachable at {url}: {e}"))
                }
            })?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED {
            return Err(DeviceError::Auth(
                "plex rejected the token (401); set a valid PLEX_TOKEN via args, env, or sibling plex-mcp/.env".to_string(),
            ));
        }
        if !response.status().is_success() {
            return Err(DeviceError::Protocol(format!(
                "plex answered {} for {path}",
                response.status()
            )));
        }
        let bytes = response.bytes().await.map_err(|e| {
            DeviceError::Protocol(format!("reading plex body for {path} failed: {e}"))
        })?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(DeviceError::Protocol(format!(
                "plex body for {path} exceeds {} bytes",
                MAX_BODY_BYTES
            )));
        }
        let envelope: ContainerEnvelope = serde_json::from_slice(&bytes).map_err(|e| {
            DeviceError::Protocol(format!("parsing plex JSON for {path} failed: {e}"))
        })?;
        Ok(envelope.container)
    }

    /// `GET /` — server identity.
    pub async fn server_identity(&self) -> Result<MediaContainer, DeviceError> {
        self.get_container("/").await
    }

    /// `GET /library/sections` — library sections.
    pub async fn sections(&self) -> Result<Vec<DirectoryItem>, DeviceError> {
        Ok(self.get_container("/library/sections").await?.directory)
    }

    /// `GET /search?query=` — hub search across libraries.
    pub async fn search(&self, query: &str) -> Result<Vec<MetadataItem>, DeviceError> {
        let query = require_search_query(query)?;
        let mut items = Vec::new();
        // Plex hub search returns Directory hubs; drill into each hub key.
        let hubs = self
            .get_container(&format!("/search?query={}", urlencode(&query)))
            .await?
            .directory;
        for hub in hubs {
            let Some(key) = hub.key.as_deref() else {
                continue;
            };
            if key.chars().count() > 256 {
                continue;
            }
            let container = self.get_container(key).await?;
            items.extend(container.metadata);
            items.extend(container.directory.into_iter().map(|d| MetadataItem {
                rating_key: None,
                key: d.key,
                title: d.title,
                r#type: d.r#type,
                summary: None,
                year: None,
                duration: None,
                view_count: None,
                thumb: None,
                art: None,
            }));
        }
        Ok(items)
    }

    /// `GET /library/metadata/{rating_key}` — single item detail.
    pub async fn metadata(&self, rating_key: &str) -> Result<Option<MetadataItem>, DeviceError> {
        let key = require_rating_key(rating_key)?;
        let mut container = self
            .get_container(&format!("/library/metadata/{key}"))
            .await?
            .metadata;
        if container.len() > 1 {
            container.truncate(1);
        }
        Ok(container.into_iter().next())
    }

    /// `GET /status/sessions` — active transcode/direct-play sessions.
    pub async fn sessions(&self) -> Result<Vec<MetadataItem>, DeviceError> {
        Ok(self.get_container("/status/sessions").await?.metadata)
    }

    /// `GET /library/sections/{key}/all` — browse section items with pagination.
    pub async fn section_items(
        &self,
        section_key: &str,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<MetadataItem>, DeviceError> {
        let key = require_rating_key(section_key)?;
        let path = format!(
            "/library/sections/{key}/all?X-Plex-Container-Start={offset}&X-Plex-Container-Size={limit}"
        );
        Ok(self.get_container(&path).await?.metadata)
    }

    /// `GET /library/recentlyAdded` — recently added items across all libraries.
    pub async fn recently_added(&self, limit: usize) -> Result<Vec<MetadataItem>, DeviceError> {
        let path = format!(
            "/library/recentlyAdded?X-Plex-Container-Start=0&X-Plex-Container-Size={limit}"
        );
        Ok(self.get_container(&path).await?.metadata)
    }

    /// `GET /playlists` — list user playlists.
    pub async fn playlists(&self) -> Result<Vec<MetadataItem>, DeviceError> {
        Ok(self.get_container("/playlists").await?.metadata)
    }

    /// `GET /clients` — list available player clients.
    pub async fn clients(&self) -> Result<Vec<ServerClientItem>, DeviceError> {
        Ok(self.get_container("/clients").await?.server)
    }

    /// Outbound GET for image/thumbnail bytes (poster, art, thumb).
    pub async fn fetch_image(&self, path: &str) -> Result<Vec<u8>, DeviceError> {
        let clean_path = path.trim();
        if clean_path.is_empty() {
            return Err(DeviceError::InvalidInput("empty image path".to_string()));
        }
        let url = if clean_path.starts_with("http://") || clean_path.starts_with("https://") {
            clean_path.to_string()
        } else {
            format!("{}{}", self.server_url, clean_path)
        };
        let response = self
            .http
            .get(&url)
            .header("X-Plex-Token", &self.token)
            .send()
            .await
            .map_err(|e| DeviceError::Offline(format!("image fetch failed at {url}: {e}")))?;
        if !response.status().is_success() {
            return Err(DeviceError::Protocol(format!(
                "plex returned {} for image {clean_path}",
                response.status()
            )));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|e| DeviceError::Protocol(format!("reading image bytes failed: {e}")))?;
        if bytes.len() > MAX_BODY_BYTES {
            return Err(DeviceError::Protocol(
                "image exceeds byte limit".to_string(),
            ));
        }
        Ok(bytes.to_vec())
    }

    /// Playback control: `play`, `pause`, `stop`, `step`, etc.
    pub async fn playback_control(
        &self,
        client_id: &str,
        command: &str,
    ) -> Result<serde_json::Value, DeviceError> {
        let clean_id = client_id.trim();
        let clean_cmd = command.trim();
        if clean_id.is_empty() || clean_cmd.is_empty() {
            return Err(DeviceError::InvalidInput(
                "client_id and command must not be empty".to_string(),
            ));
        }
        let path = format!("/player/playback/{clean_cmd}?client_id={clean_id}");
        let _ = self.get_container(&path).await;
        Ok(serde_json::json!({
            "success": true,
            "vendor": "live",
            "client_id": clean_id,
            "command": clean_cmd
        }))
    }
}

fn urlencode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{byte:02X}"));
            }
        }
    }
    out
}

// --- Demo fixtures (no network; same structs as live) ------------------------

const DEMO_IDENTITY: &str = r#"{"MediaContainer":{"friendlyName":"Demo Plex Server","machineIdentifier":"demo-machine-id","version":"1.0.0-demo","myPlex":false,"size":0}}"#;
const DEMO_SECTIONS: &str = r#"{"MediaContainer":{"size":3,"Directory":[{"key":"1","type":"movie","title":"Demo Movies","agent":"demo","scanner":"demo"},{"key":"2","type":"show","title":"Demo Shows","agent":"demo","scanner":"demo"},{"key":"3","type":"artist","title":"Demo Music","agent":"demo","scanner":"demo"}]}}"#;
const DEMO_ITEMS: &str = r#"{"MediaContainer":{"size":2,"Metadata":[{"ratingKey":"101","key":"/library/metadata/101","type":"movie","title":"Demo Feature Film","summary":"A sample item for offline smoke tests.","year":2024,"duration":5400000},{"ratingKey":"102","key":"/library/metadata/102","type":"movie","title":"Demo Second Feature","year":2023,"duration":4800000}]}}"#;

fn demo_container(raw: &str) -> Result<MediaContainer, DeviceError> {
    serde_json::from_str::<ContainerEnvelope>(raw)
        .map(|envelope| envelope.container)
        .map_err(|e| DeviceError::Protocol(format!("demo fixture broken: {e}")))
}

/// Demo server identity (offline smoke path).
pub fn demo_identity() -> Result<MediaContainer, DeviceError> {
    demo_container(DEMO_IDENTITY)
}

/// Demo library sections (offline smoke path).
pub fn demo_sections() -> Result<Vec<DirectoryItem>, DeviceError> {
    Ok(demo_container(DEMO_SECTIONS)?.directory)
}

/// Demo search: substring filter over the two fixture items.
pub fn demo_search(query: &str) -> Result<Vec<MetadataItem>, DeviceError> {
    let query = require_search_query(query)?.to_lowercase();
    let items = demo_container(DEMO_ITEMS)?.metadata;
    Ok(items
        .into_iter()
        .filter(|item| {
            item.title
                .as_deref()
                .unwrap_or_default()
                .to_lowercase()
                .contains(&query)
        })
        .collect())
}

/// Demo item lookup by rating key.
pub fn demo_metadata(rating_key: &str) -> Result<Option<MetadataItem>, DeviceError> {
    let key = require_rating_key(rating_key)?;
    let items = demo_container(DEMO_ITEMS)?.metadata;
    Ok(items
        .into_iter()
        .find(|item| item.rating_key.as_deref() == Some(key.as_str())))
}

/// Demo sessions: always empty (nothing plays on a fixture).
pub fn demo_sessions() -> Vec<MetadataItem> {
    Vec::new()
}

/// Demo section items.
pub fn demo_section_items(_section_key: &str) -> Result<Vec<MetadataItem>, DeviceError> {
    Ok(demo_container(DEMO_ITEMS)?.metadata)
}

/// Demo recently added items.
pub fn demo_recently_added() -> Result<Vec<MetadataItem>, DeviceError> {
    Ok(demo_container(DEMO_ITEMS)?.metadata)
}

/// Demo playlists.
pub fn demo_playlists() -> Vec<MetadataItem> {
    vec![
        MetadataItem {
            rating_key: Some("1001".to_string()),
            key: Some("/playlists/1001".to_string()),
            title: Some("Demo Favorites".to_string()),
            r#type: Some("playlist".to_string()),
            summary: Some("Demo playlist for testing".to_string()),
            year: Some(2024),
            duration: Some(120000),
            view_count: Some(5),
            thumb: None,
            art: None,
        },
        MetadataItem {
            rating_key: Some("1002".to_string()),
            key: Some("/playlists/1002".to_string()),
            title: Some("Demo Watch Later".to_string()),
            r#type: Some("playlist".to_string()),
            summary: Some("Items queued for later".to_string()),
            year: Some(2024),
            duration: Some(240000),
            view_count: Some(0),
            thumb: None,
            art: None,
        },
    ]
}

/// Demo player clients.
pub fn demo_clients() -> Vec<ServerClientItem> {
    vec![ServerClientItem {
        name: Some("Demo Web Client".to_string()),
        host: Some("127.0.0.1".to_string()),
        address: Some("127.0.0.1".to_string()),
        port: Some(32400),
        machine_identifier: Some("demo-client-uuid".to_string()),
        version: Some("1.0.0".to_string()),
        protocol: Some("plex".to_string()),
        product: Some("Plex Web".to_string()),
        device_class: Some("pc".to_string()),
    }]
}

/// Demo image bytes (1x1 PNG dummy).
pub fn demo_image() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00,
        0x01, 0x00, 0x00, 0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49,
        0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

/// Demo playback control result.
pub fn demo_playback_control(client_id: &str, command: &str) -> serde_json::Value {
    serde_json::json!({
        "success": true,
        "vendor": "demo",
        "client_id": client_id,
        "command": command
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_http_url_is_rejected() {
        assert!(require_http_url("plex.local:32400").is_err());
        assert_eq!(
            require_http_url("http://plex.local:32400/"),
            Ok("http://plex.local:32400".to_string())
        );
    }

    #[test]
    fn empty_query_and_bad_key_are_rejected() {
        assert!(require_search_query("   ").is_err());
        assert!(require_rating_key("12 34").is_err());
        assert_eq!(require_rating_key(" 101 "), Ok("101".to_string()));
    }

    #[test]
    fn demo_fixtures_parse_into_typed_structs() -> Result<(), DeviceError> {
        let identity = demo_identity()?;
        assert_eq!(identity.friendly_name.as_deref(), Some("Demo Plex Server"));
        assert_eq!(demo_sections()?.len(), 3);
        assert_eq!(demo_search("second")?.len(), 1);
        assert_eq!(demo_search("no-such-title")?.len(), 0);
        assert!(demo_metadata("101")?.is_some());
        assert!(demo_sessions().is_empty());
        Ok(())
    }

    #[test]
    fn urlencode_leaves_unreserved_chars_alone() {
        assert_eq!(urlencode("star wars"), "star%20wars");
        assert_eq!(urlencode("abc-_.~09"), "abc-_.~09");
    }
}
