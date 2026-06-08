use crate::{config, history::History};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use tokio::{fs, io::AsyncWriteExt};

#[derive(Debug, Clone)]
pub struct ImageCache {
    pub image_dir: PathBuf,
    history_path: PathBuf,
    max_size_bytes: u64,
}

impl ImageCache {
    pub async fn new(cfg: &crate::config::CacheConfig) -> Result<Self> {
        let image_dir = config::expand_path(&cfg.dir);
        fs::create_dir_all(&image_dir).await?;
        let history_path = image_dir
            .parent()
            .unwrap_or(Path::new("."))
            .join("history.json");
        Ok(Self {
            image_dir,
            history_path,
            max_size_bytes: cfg.max_size_mb.saturating_mul(1024 * 1024),
        })
    }

    pub async fn download(&self, url: &str, suggested_ext: Option<&str>) -> Result<PathBuf> {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let ext = suggested_ext.unwrap_or("jpg").trim_start_matches('.');
        let path = self
            .image_dir
            .join(format!("{:x}.{ext}", hasher.finalize()));

        if path.exists() {
            return Ok(path);
        }

        let bytes = reqwest::get(url).await?.error_for_status()?.bytes().await?;
        let tmp_path = path.with_extension(format!("{ext}.part"));
        let mut file = fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        fs::rename(&tmp_path, &path).await?;
        Ok(path)
    }

    pub async fn load_history(&self, max_entries: usize) -> Result<History> {
        History::load(&self.history_path, max_entries).await
    }

    pub async fn save_history(&self, history: &History) -> Result<()> {
        history.save(&self.history_path).await
    }

    pub async fn clean(&self, history: &History) -> Result<u64> {
        let protected: std::collections::HashSet<PathBuf> =
            history.entries().map(|entry| entry.path.clone()).collect();
        let mut files = fs::read_dir(&self.image_dir).await?;
        let mut removed = 0u64;

        while let Some(entry) = files.next_entry().await? {
            let meta = entry.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            let path = entry.path();
            if protected.contains(&path) {
                continue;
            }
            fs::remove_file(&path)
                .await
                .with_context(|| format!("failed to remove {}", path.display()))?;
            removed += 1;
        }

        Ok(removed)
    }

    pub async fn evict(&self, history: &History) -> Result<()> {
        let mut files = fs::read_dir(&self.image_dir).await?;
        let protected: std::collections::HashSet<PathBuf> =
            history.entries().map(|entry| entry.path.clone()).collect();
        let mut candidates = Vec::new();
        let mut total = 0u64;

        while let Some(entry) = files.next_entry().await? {
            let meta = entry.metadata().await?;
            if !meta.is_file() {
                continue;
            }
            total = total.saturating_add(meta.len());
            let path = entry.path();
            candidates.push((meta.modified().ok(), meta.len(), path));
        }

        if total <= self.max_size_bytes {
            return Ok(());
        }

        candidates.sort_by_key(|(modified, _, _)| *modified);
        for (_, size, path) in candidates {
            if total <= self.max_size_bytes {
                break;
            }
            if protected.contains(&path) {
                continue;
            }
            fs::remove_file(&path)
                .await
                .with_context(|| format!("failed to evict {}", path.display()))?;
            total = total.saturating_sub(size);
        }

        Ok(())
    }
}
