//! LanceDB-backed semantic search over the Plex library (L1).
//!
//! [`sync_library`] embeds every library item with a local ONNX model
//! ([`FastEmbedder`], MiniLM via `fastembed`, downloaded once then cached)
//! and stores title + summary + vector in a LanceDB table. [`RagIndex`]
//! queries that table by vector similarity. No API keys, no cloud, no
//! Python — the whole RAG loop runs on Goliath's CPU.
//!
//! Honest degradation ladder (never a silent zero):
//! missing table → keyword fallback over the live server; model download
//! impossible (offline first run) → keyword fallback with the reason
//! attached; Demo vendor → deterministic [`StubEmbedder`] vectors in a
//! separate `media_demo` table so demo data can never poison the live
//! index. Tests use `memory://` tables plus the stub — zero downloads.

use crate::error::DeviceError;
use crate::plex::MetadataItem;
use arrow_array::{FixedSizeListArray, Float32Array, Int32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use std::path::PathBuf;
use std::sync::Arc;

/// Live-library table name. Demo data lives in [`TABLE_MEDIA_DEMO`].
pub const TABLE_MEDIA: &str = "media";
/// Demo-vendor table: stub vectors, never mixed with the live index.
pub const TABLE_MEDIA_DEMO: &str = "media_demo";
/// fastembed default used for the live index (MiniLM, 384 dims).
pub const MODEL_NAME: &str = "AllMiniLML6V2";
/// Hostile-size fence: refuse to sync absurd libraries in one pass.
pub const MAX_SYNC_ITEMS: usize = 5000;
/// fastembed batch size per transform call.
pub const EMBED_BATCH: usize = 32;
/// Input text cap per item (titles + summaries are short; this bounds CPU).
pub const MAX_TEXT_CHARS: usize = 4000;

/// One embeddable library row.
#[derive(Debug, Clone)]
pub struct MediaRow {
    pub rating_key: String,
    pub title: String,
    pub summary: String,
    pub kind: String,
    pub year: i32,
}

impl MediaRow {
    #[must_use]
    pub fn from_item(item: &MetadataItem) -> Self {
        Self {
            rating_key: item.rating_key.clone().unwrap_or_default(),
            title: item.title.clone().unwrap_or_default(),
            summary: item.summary.clone().unwrap_or_default(),
            kind: item.r#type.clone().unwrap_or_default(),
            year: item.year.unwrap_or(0).clamp(0, 9999) as i32,
        }
    }

    /// Embedding input, char-capped. Title first so truncation keeps signal.
    #[must_use]
    pub fn embed_text(&self) -> String {
        let joined = if self.summary.trim().is_empty() {
            self.title.clone()
        } else {
            format!("{}. {}", self.title.trim(), self.summary.trim())
        };
        joined.chars().take(MAX_TEXT_CHARS).collect()
    }
}

/// One vector-search hit.
#[derive(Debug, Clone)]
pub struct RagHit {
    pub rating_key: String,
    pub title: String,
    pub summary: String,
    pub kind: String,
    pub year: i32,
    /// Raw L2 distance (lower is closer). See [`distance_to_relevance`].
    pub distance: f32,
}

/// Map L2 distance to a 0–1 relevance for display (closer → nearer 1).
#[must_use]
pub fn distance_to_relevance(distance: f32) -> f32 {
    1.0 / (1.0 + distance.max(0.0))
}

/// Embedding backend. `Send` so callers can run it on a blocking thread;
/// `&mut` because ONNX sessions mutate scratch state.
pub trait Embedder: Send {
    /// Vector width. Must match the index table's vector column.
    fn dim(&self) -> usize;
    /// Embed one batch. Empty input returns an empty vec, never an error.
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, DeviceError>;
}

/// Deterministic offline embedder (unit tests + Demo vendor). FNV-1a hash
/// seeds a xorshift stream, normalized to unit length — same text always
/// yields the same vector, which is all the mechanical tests need.
pub struct StubEmbedder {
    dim: usize,
}

impl StubEmbedder {
    #[must_use]
    pub fn new(dim: usize) -> Self {
        Self { dim: dim.max(1) }
    }
}

fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in text.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

impl Embedder for StubEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, DeviceError> {
        let mut out = Vec::with_capacity(texts.len());
        for text in texts {
            let mut state = fnv1a(text) | 1;
            let mut vec = Vec::with_capacity(self.dim);
            for _ in 0..self.dim {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                vec.push((state % 10_000) as f32 / 10_000.0);
            }
            let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-9);
            for v in &mut vec {
                *v /= norm;
            }
            out.push(vec);
        }
        Ok(out)
    }
}

/// Local ONNX embedder via `fastembed`. [`FastEmbedder::load`] downloads
/// the model on first use (honest minutes on first sync) and caches it
/// under `FASTEMBED_CACHE_DIR` / `HF_HOME` / `./.fastembed_cache`.
/// Blocking + `&mut` — always drive it from `spawn_blocking`, and share
/// one instance through the [`shared_model`] cache.
pub struct FastEmbedder {
    model: fastembed::TextEmbedding,
    dim: usize,
}

impl FastEmbedder {
    /// Load (or download-then-load) the model. Blocking: minutes on first
    /// run, seconds after. Never call on an async worker thread.
    pub fn load() -> Result<Self, DeviceError> {
        let mut model = fastembed::TextEmbedding::try_new(
            fastembed::TextInitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false),
        )
        .map_err(|e| DeviceError::Protocol(format!("rag: embedding model load failed: {e}")))?;
        let dim = model
            .embed(vec!["dimension probe"], None)
            .map_err(|e| DeviceError::Protocol(format!("rag: embedding probe failed: {e}")))?
            .first()
            .map(Vec::len)
            .unwrap_or(0);
        if dim == 0 {
            return Err(DeviceError::Protocol(
                "rag: embedding model returned an empty probe vector".to_string(),
            ));
        }
        Ok(Self { model, dim })
    }
}

impl Embedder for FastEmbedder {
    fn dim(&self) -> usize {
        self.dim
    }

    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>, DeviceError> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        self.model
            .embed(texts, Some(EMBED_BATCH))
            .map_err(|e| DeviceError::Protocol(format!("rag: embed failed: {e}")))
    }
}

/// Process-wide shared model so searches don't reload ONNX weights per
/// query. Poison-safe: a poisoned mutex surfaces as a typed error.
static SHARED_MODEL: std::sync::OnceLock<std::sync::Mutex<Option<fastembed::TextEmbedding>>> =
    std::sync::OnceLock::new();

/// Embed via the shared model, loading it on first use. Blocking — call
/// from `spawn_blocking`, never on an async worker.
pub fn embed_shared(texts: &[String]) -> Result<Vec<Vec<f32>>, DeviceError> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let cell = SHARED_MODEL.get_or_init(|| std::sync::Mutex::new(None));
    let mut guard = cell
        .lock()
        .map_err(|_| DeviceError::Protocol("rag: model mutex poisoned".to_string()))?;
    if guard.is_none() {
        let model = fastembed::TextEmbedding::try_new(
            fastembed::TextInitOptions::new(fastembed::EmbeddingModel::AllMiniLML6V2)
                .with_show_download_progress(false),
        )
        .map_err(|e| DeviceError::Protocol(format!("rag: embedding model load failed: {e}")))?;
        *guard = Some(model);
    }
    guard
        .as_mut()
        .ok_or_else(|| DeviceError::Protocol("rag: model vanished after load".to_string()))?
        .embed(texts, Some(EMBED_BATCH))
        .map_err(|e| DeviceError::Protocol(format!("rag: embed failed: {e}")))
}

/// Index location: explicit arg > `PLEX_MCP_DATA` env > OS default.
#[must_use]
pub fn index_dir(data_dir: Option<&str>) -> PathBuf {
    if let Some(dir) = data_dir
        .map(str::trim)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
    {
        return dir;
    }
    if let Ok(dir) = std::env::var("PLEX_MCP_DATA") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("rust-plex-mcp").join("lancedb");
    }
    PathBuf::from("data").join("lancedb")
}

fn media_schema(dim: usize) -> Arc<Schema> {
    Arc::new(Schema::new(vec![
        Field::new("rating_key", DataType::Utf8, false),
        Field::new("title", DataType::Utf8, false),
        Field::new("summary", DataType::Utf8, true),
        Field::new("kind", DataType::Utf8, true),
        Field::new("year", DataType::Int32, true),
        Field::new(
            "vector",
            DataType::FixedSizeList(
                Arc::new(Field::new("item", DataType::Float32, true)),
                dim as i32,
            ),
            false,
        ),
    ]))
}

fn rows_to_batch(
    schema: &Schema,
    rows: &[MediaRow],
    embeddings: &[Vec<f32>],
    dim: usize,
) -> Result<RecordBatch, DeviceError> {
    if rows.len() != embeddings.len() {
        return Err(DeviceError::InvalidInput(format!(
            "rag: {} rows but {} embeddings",
            rows.len(),
            embeddings.len()
        )));
    }
    for (idx, vec) in embeddings.iter().enumerate() {
        if vec.len() != dim {
            return Err(DeviceError::InvalidInput(format!(
                "rag: row {idx} vector width {} != index dim {dim}",
                vec.len()
            )));
        }
    }
    let flat: Vec<f32> = embeddings.iter().flatten().copied().collect();
    let vector = FixedSizeListArray::try_new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        dim as i32,
        Arc::new(Float32Array::from(flat)),
        None,
    )
    .map_err(|e| DeviceError::Protocol(format!("rag: vector column build failed: {e}")))?;
    RecordBatch::try_new(
        Arc::new(schema.clone()),
        vec![
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.rating_key.as_str())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.title.as_str())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter()
                    .map(|row| row.summary.as_str())
                    .collect::<Vec<_>>(),
            )),
            Arc::new(StringArray::from(
                rows.iter().map(|row| row.kind.as_str()).collect::<Vec<_>>(),
            )),
            Arc::new(Int32Array::from(
                rows.iter().map(|row| row.year).collect::<Vec<_>>(),
            )),
            Arc::new(vector),
        ],
    )
    .map_err(|e| DeviceError::Protocol(format!("rag: batch build failed: {e}")))
}

fn read_string(batch: &RecordBatch, column: &str, row: usize) -> String {
    batch
        .column_by_name(column)
        .and_then(|col| col.as_any().downcast_ref::<StringArray>())
        .map(|arr| arr.value(row).to_string())
        .unwrap_or_default()
}

fn read_year(batch: &RecordBatch, row: usize) -> i32 {
    batch
        .column_by_name("year")
        .and_then(|col| col.as_any().downcast_ref::<Int32Array>())
        .map(|arr| arr.value(row))
        .unwrap_or(0)
}

/// Open LanceDB handle (L1-owned; tables resolve per call).
async fn open_db(dir: &std::path::Path) -> Result<Connection, DeviceError> {
    let uri = dir.to_string_lossy().into_owned();
    connect(&uri)
        .execute()
        .await
        .map_err(|e| DeviceError::Protocol(format!("rag: lance connect failed at {uri}: {e}")))
}

/// LanceDB vector index over one table.
pub struct RagIndex {
    table: Table,
    /// Table name (live vs demo stay separated by construction).
    pub table_name: String,
    pub dim: usize,
}

impl RagIndex {
    async fn wrap(table: Table, table_name: &str, dim: usize) -> Result<Self, DeviceError> {
        Ok(Self {
            table,
            table_name: table_name.to_string(),
            dim,
        })
    }

    /// Open an existing table. Missing table is a typed `NotFound`, which
    /// dispatch turns into the keyword-fallback path — never a panic.
    pub async fn open_existing(
        dir: &std::path::Path,
        table_name: &str,
    ) -> Result<Self, DeviceError> {
        let db = open_db(dir).await?;
        let table = db.open_table(table_name).execute().await.map_err(|e| {
            DeviceError::NotFound(format!(
                "rag: table '{table_name}' not synced yet ({}): {e}",
                dir.to_string_lossy()
            ))
        })?;
        let dim = table_dim(&table).await?;
        Self::wrap(table, table_name, dim).await
    }

    /// Full rebuild: drop, recreate at `dim`, insert all rows. Libraries
    /// are small (hundreds of rows); rebuilds stay seconds-scale.
    pub async fn rebuild(
        dir: &std::path::Path,
        table_name: &str,
        rows: &[MediaRow],
        embeddings: &[Vec<f32>],
        dim: usize,
    ) -> Result<Self, DeviceError> {
        if rows.len() > MAX_SYNC_ITEMS {
            return Err(DeviceError::InvalidInput(format!(
                "rag: refusing to sync {} items (cap {MAX_SYNC_ITEMS})",
                rows.len()
            )));
        }
        if dim == 0 || dim > 4096 {
            return Err(DeviceError::InvalidInput(format!(
                "rag: implausible vector dim {dim}"
            )));
        }
        let schema = media_schema(dim);
        let batch = rows_to_batch(&schema, rows, embeddings, dim)?;
        let db = open_db(dir).await?;
        let _ = db.drop_table(table_name, &[]).await;
        let table = db
            .create_table(table_name, batch)
            .execute()
            .await
            .map_err(|e| DeviceError::Protocol(format!("rag: create table failed: {e}")))?;
        Self::wrap(table, table_name, dim).await
    }

    /// Row count (status page + sync receipts).
    pub async fn count(&self) -> Result<usize, DeviceError> {
        self.table
            .count_rows(None)
            .await
            .map_err(|e| DeviceError::Protocol(format!("rag: count failed: {e}")))
    }

    /// Vector search. `vector.len()` must equal the table dim.
    pub async fn search(&self, vector: Vec<f32>, limit: usize) -> Result<Vec<RagHit>, DeviceError> {
        if vector.len() != self.dim {
            return Err(DeviceError::InvalidInput(format!(
                "rag: query width {} != index dim {}",
                vector.len(),
                self.dim
            )));
        }
        let limit = limit.clamp(1, 50);
        let batches: Vec<RecordBatch> = self
            .table
            .query()
            .limit(limit)
            .nearest_to(vector.as_slice())
            .map_err(|e| DeviceError::Protocol(format!("rag: query build failed: {e}")))?
            .execute()
            .await
            .map_err(|e| DeviceError::Protocol(format!("rag: query failed: {e}")))?
            .try_collect()
            .await
            .map_err(|e| DeviceError::Protocol(format!("rag: result drain failed: {e}")))?;
        let mut hits = Vec::new();
        for batch in &batches {
            let distance = batch
                .column_by_name("_distance")
                .and_then(|col| col.as_any().downcast_ref::<Float32Array>());
            let rows = batch.num_rows();
            for row in 0..rows {
                hits.push(RagHit {
                    rating_key: read_string(batch, "rating_key", row),
                    title: read_string(batch, "title", row),
                    summary: read_string(batch, "summary", row),
                    kind: read_string(batch, "kind", row),
                    year: read_year(batch, row),
                    distance: distance.map(|arr| arr.value(row)).unwrap_or(f32::MAX),
                });
            }
        }
        Ok(hits)
    }
}

async fn table_dim(table: &Table) -> Result<usize, DeviceError> {
    let schema = table
        .schema()
        .await
        .map_err(|e| DeviceError::Protocol(format!("rag: schema read failed: {e}")))?;
    schema
        .field_with_name("vector")
        .map_err(|e| DeviceError::Protocol(format!("rag: no vector column: {e}")))
        .and_then(|field| match field.data_type() {
            DataType::FixedSizeList(_, size) => usize::try_from(*size)
                .map_err(|_| DeviceError::Protocol("rag: vector dim overflow".to_string())),
            other => Err(DeviceError::Protocol(format!(
                "rag: vector column has unexpected type {other:?}"
            ))),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rows() -> Vec<MediaRow> {
        vec![
            MediaRow {
                rating_key: "1".to_string(),
                title: "Blade Runner".to_string(),
                summary: "A blade runner hunts replicants in future Los Angeles.".to_string(),
                kind: "movie".to_string(),
                year: 1982,
            },
            MediaRow {
                rating_key: "2".to_string(),
                title: "Cooking Show".to_string(),
                summary: "Recipes and kitchen techniques for beginners.".to_string(),
                kind: "show".to_string(),
                year: 2020,
            },
        ]
    }

    #[test]
    fn stub_embedder_is_deterministic_and_normalized() {
        let mut stub = StubEmbedder::new(16);
        let first = stub.embed(&["hello".to_string()]);
        let second = stub.embed(&["hello".to_string()]);
        assert!(first.is_ok() && second.is_ok());
        let (first, second) = (first.unwrap_or_default(), second.unwrap_or_default());
        assert_eq!(first, second);
        assert_eq!(first.first().map(Vec::len), Some(16));
        assert!(stub.embed(&[]).unwrap_or_default().is_empty());
    }

    #[test]
    fn embed_text_prefers_title_and_caps_length() {
        let row = MediaRow {
            rating_key: "9".to_string(),
            title: "T".to_string(),
            summary: "S".to_string(),
            kind: String::new(),
            year: 0,
        };
        assert_eq!(row.embed_text(), "T. S");
        let long = MediaRow {
            summary: "x".repeat(9000),
            ..row
        };
        assert_eq!(long.embed_text().chars().count(), MAX_TEXT_CHARS);
    }

    #[test]
    fn batch_rejects_shape_mismatches() {
        let schema = media_schema(4);
        let rows = test_rows();
        let bad_dim = vec![vec![0.0; 4], vec![0.0; 3]];
        assert!(rows_to_batch(&schema, &rows, &bad_dim, 4).is_err());
        let short = vec![vec![0.0; 4]];
        assert!(rows_to_batch(&schema, &rows, &short, 4).is_err());
    }

    /// Full mechanical round-trip on an in-memory table with stub vectors:
    /// rebuild → count → search → identical text ranks first.
    #[tokio::test]
    async fn memory_round_trip_ranks_identical_text_first() -> Result<(), DeviceError> {
        let dir = std::path::Path::new("memory://rag-test");
        let rows = test_rows();
        let mut stub = StubEmbedder::new(32);
        let texts: Vec<String> = rows.iter().map(MediaRow::embed_text).collect();
        let embeddings = stub.embed(&texts)?;
        let index = RagIndex::rebuild(dir, "media", &rows, &embeddings, 32).await?;
        assert_eq!(index.count().await?, 2);
        let probe = stub.embed(&[rows[0].embed_text()])?;
        let hits = index
            .search(probe.into_iter().next().unwrap_or_default(), 5)
            .await?;
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].rating_key, "1");
        assert_eq!(hits[0].title, "Blade Runner");
        assert!(hits[0].distance <= hits[1].distance);
        Ok(())
    }

    #[tokio::test]
    async fn missing_table_is_typed_not_found() -> Result<(), DeviceError> {
        let dir = std::path::Path::new("memory://rag-missing");
        match RagIndex::open_existing(dir, "media").await {
            Ok(_) => Err(DeviceError::Protocol(
                "expected NotFound for a missing table".to_string(),
            )),
            Err(e) => {
                assert!(matches!(e, DeviceError::NotFound(_)));
                Ok(())
            }
        }
    }

    #[tokio::test]
    async fn wrong_width_query_is_rejected() -> Result<(), DeviceError> {
        let dir = std::path::Path::new("memory://rag-width");
        let rows = test_rows();
        let mut stub = StubEmbedder::new(32);
        let texts: Vec<String> = rows.iter().map(MediaRow::embed_text).collect();
        let embeddings = stub.embed(&texts)?;
        let index = RagIndex::rebuild(dir, "media", &rows, &embeddings, 32).await?;
        let err = match index.search(vec![0.0; 7], 5).await {
            Ok(_) => {
                return Err(DeviceError::Protocol(
                    "expected InvalidInput for a wrong-width query".to_string(),
                ));
            }
            Err(e) => e,
        };
        assert!(matches!(err, DeviceError::InvalidInput(_)));
        Ok(())
    }
}
