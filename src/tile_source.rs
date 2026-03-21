use crate::config::MapConfig;
use crate::styler::Styler;
use crate::tile::{self, ParsedTile};

use anyhow::Result;
use lru::LruCache;
use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Source for vector tiles. Supports HTTP tile servers and TileJSON endpoints.
///
/// If the source URL ends with `/`, it is treated as a direct tile URL prefix
/// (e.g., `http://example.com/tiles/` fetches `http://example.com/tiles/{z}/{x}/{y}.pbf`).
///
/// Otherwise, the source is treated as a TileJSON endpoint. The TileJSON is
/// fetched once on first tile request, and the tile URL template is extracted
/// from the `tiles` array.
pub struct TileSource {
    source: String,
    cache: Arc<Mutex<LruCache<String, ParsedTile>>>,
    persist_path: Option<PathBuf>,
    styler: Arc<Styler>,
    language: String,
    client: reqwest::Client,
    tile_url_template: Arc<Mutex<Option<String>>>,
}

fn source_hash(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

impl TileSource {
    pub fn new(config: &MapConfig, styler: Arc<Styler>) -> Self {
        let persist_path = if config.persist_downloaded_tiles {
            dirs::cache_dir().map(|d| d.join("terminalmap").join(source_hash(&config.source)))
        } else {
            None
        };

        if let Some(ref p) = persist_path {
            let _ = std::fs::create_dir_all(p);
        }

        Self {
            source: config.source.clone(),
            cache: Arc::new(Mutex::new(LruCache::new(
                NonZeroUsize::new(32).unwrap(),
            ))),
            persist_path,
            styler,
            language: config.language.clone(),
            client: reqwest::Client::new(),
            tile_url_template: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn get_tile(&self, z: u32, x: i32, y: i32) -> Result<ParsedTile> {
        let key = format!("{}-{}-{}", z, x, y);

        // Check in-memory cache
        {
            let mut cache = self.cache.lock().await;
            if let Some(cached) = cache.get(&key) {
                return Ok(cached.clone());
            }
        }

        // Try embedded tiles (zoom 0-1 bundled for offline use)
        let buffer = if let Some(embedded) = crate::embedded_tiles::get(z, x, y) {
            embedded.to_vec()
        } else if let Some(persisted) = self.get_persisted(z, x, y) {
            // Try persistent cache
            persisted
        } else {
            let buffer = self.fetch_http(z, x, y).await?;
            self.persist_tile(z, x, y, &buffer);
            buffer
        };

        let parsed = tile::load_tile(&buffer, &self.styler, &self.language)?;

        // Store in cache
        {
            let mut cache = self.cache.lock().await;
            cache.put(key, parsed.clone());
        }

        Ok(parsed)
    }

    /// Resolve the tile URL for the given coordinates.
    /// For direct prefix sources (ending with `/`), constructs `{source}{z}/{x}/{y}.pbf`.
    /// For TileJSON sources, resolves the template on first call.
    async fn resolve_tile_url(&self, z: u32, x: i32, y: i32) -> Result<String> {
        // Direct prefix mode (backward compatible with mapscii-style URLs)
        if self.source.ends_with('/') {
            return Ok(format!("{}{}/{}/{}.pbf", self.source, z, x, y));
        }

        // Check if we already have a resolved template
        {
            let template = self.tile_url_template.lock().await;
            if let Some(ref t) = *template {
                return Ok(t
                    .replace("{z}", &z.to_string())
                    .replace("{x}", &x.to_string())
                    .replace("{y}", &y.to_string()));
            }
        }

        // Fetch TileJSON to discover tile URL template
        let resp = self.client.get(&self.source).send().await?;
        let body = resp.text().await?;
        let json: Value = serde_json::from_str(&body)
            .map_err(|e| anyhow::anyhow!("Failed to parse TileJSON from {}: {}", self.source, e))?;
        let tiles_url = json
            .get("tiles")
            .and_then(|t| t.as_array())
            .and_then(|arr| arr.first())
            .and_then(|s| s.as_str())
            .ok_or_else(|| anyhow::anyhow!("No tiles URL found in TileJSON response from {}", self.source))?
            .to_string();

        let url = tiles_url
            .replace("{z}", &z.to_string())
            .replace("{x}", &x.to_string())
            .replace("{y}", &y.to_string());

        // Cache the template for future requests
        {
            let mut template = self.tile_url_template.lock().await;
            *template = Some(tiles_url);
        }

        Ok(url)
    }

    async fn fetch_http(&self, z: u32, x: i32, y: i32) -> Result<Vec<u8>> {
        let url = self.resolve_tile_url(z, x, y).await?;
        let resp = self.client.get(&url).send().await?;
        let bytes = resp.bytes().await?;
        Ok(bytes.to_vec())
    }

    fn get_persisted(&self, z: u32, x: i32, y: i32) -> Option<Vec<u8>> {
        let base = self.persist_path.as_ref()?;
        let path = base.join(z.to_string()).join(format!("{}-{}.pbf", x, y));
        std::fs::read(path).ok()
    }

    fn persist_tile(&self, z: u32, x: i32, y: i32, buffer: &[u8]) {
        if let Some(ref base) = self.persist_path {
            let dir = base.join(z.to_string());
            let _ = std::fs::create_dir_all(&dir);
            let path = dir.join(format!("{}-{}.pbf", x, y));
            let _ = std::fs::write(path, buffer);
        }
    }
}
