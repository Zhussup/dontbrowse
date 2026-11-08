pub mod halfblock;

pub use halfblock::HalfblockImage;

use std::path::PathBuf;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::util::paths::Paths;

const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

pub fn raw_cache_path(paths: &Paths, url: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let name = hex::encode(&hasher.finalize()[..16]);
    paths.raw_cache_dir().join(format!("{name}.img"))
}

pub fn raw_cache_path_for(paths: &Paths, url: &str) -> PathBuf {
    raw_cache_path(paths, url)
}

/// Fetch image bytes, using the on-disk raw cache when available.
pub async fn fetch_bytes(
    client: &reqwest::Client,
    paths: &Paths,
    url: &str,
) -> anyhow::Result<Vec<u8>> {
    let path = raw_cache_path(paths, url);
    if let Ok(bytes) = std::fs::read(&path)
        && !bytes.is_empty()
        && bytes.len() <= MAX_IMAGE_BYTES
    {
        return Ok(bytes);
    }

    let response = client
        .get(url)
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if !response.status().is_success() {
        anyhow::bail!("image HTTP {}", response.status());
    }
    let bytes = response.bytes().await?;
    if bytes.len() > MAX_IMAGE_BYTES {
        anyhow::bail!("image is larger than 8 MiB");
    }

    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, &bytes);
    Ok(bytes.to_vec())
}
