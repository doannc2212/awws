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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ImageMeta;
    use sha2::{Digest, Sha256};
    use tokio::io::AsyncWriteExt;

    async fn cache_with(dir: &Path, max_size_mb: u64) -> ImageCache {
        let cfg = crate::config::CacheConfig {
            max_size_mb,
            history_size: 10,
            dir: dir.to_string_lossy().into_owned(),
        };
        ImageCache::new(&cfg).await.unwrap()
    }

    async fn write_file(path: &Path, size: usize) {
        let mut file = fs::File::create(path).await.unwrap();
        file.write_all(&vec![0u8; size]).await.unwrap();
        file.flush().await.unwrap();
    }

    fn history_protecting(paths: &[PathBuf]) -> History {
        let mut history = History::new(100);
        for path in paths {
            history.push(ImageMeta {
                path: path.clone(),
                url: None,
                title: None,
                author: None,
                source: "test".into(),
            });
        }
        history
    }

    #[tokio::test]
    async fn evict_noop_when_under_limit() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_with(dir.path(), 10).await;
        let a = cache.image_dir.join("a.jpg");
        let b = cache.image_dir.join("b.jpg");
        write_file(&a, 1024).await;
        write_file(&b, 1024).await;

        cache.evict(&History::new(10)).await.unwrap();

        assert!(a.exists());
        assert!(b.exists());
    }

    #[tokio::test]
    async fn evict_removes_unprotected_when_over_limit() {
        let dir = tempfile::tempdir().unwrap();
        // max_size_mb = 0 means any non-empty cache is over the limit.
        let cache = cache_with(dir.path(), 0).await;
        let a = cache.image_dir.join("a.jpg");
        let b = cache.image_dir.join("b.jpg");
        write_file(&a, 1024).await;
        write_file(&b, 1024).await;

        cache.evict(&History::new(10)).await.unwrap();

        assert!(!a.exists());
        assert!(!b.exists());
    }

    #[tokio::test]
    async fn evict_keeps_protected_history_entries() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_with(dir.path(), 0).await;
        let protected = cache.image_dir.join("keep.jpg");
        let stale = cache.image_dir.join("drop.jpg");
        write_file(&protected, 1024).await;
        write_file(&stale, 1024).await;

        let history = history_protecting(&[protected.clone()]);
        cache.evict(&history).await.unwrap();

        assert!(protected.exists(), "current history entry must survive eviction");
        assert!(!stale.exists());
    }

    #[tokio::test]
    async fn evict_stops_once_under_limit() {
        let dir = tempfile::tempdir().unwrap();
        // 1 MiB limit; three ~512 KiB files total ~1.5 MiB, so eviction must
        // remove at least one but stop before clearing everything.
        let cache = cache_with(dir.path(), 1).await;
        let half_mb = 512 * 1024;
        for name in ["a.jpg", "b.jpg", "c.jpg"] {
            write_file(&cache.image_dir.join(name), half_mb).await;
        }

        cache.evict(&History::new(10)).await.unwrap();

        let mut total = 0u64;
        let mut files = fs::read_dir(&cache.image_dir).await.unwrap();
        while let Some(entry) = files.next_entry().await.unwrap() {
            total += entry.metadata().await.unwrap().len();
        }
        assert!(total <= 1024 * 1024, "remaining cache must be under limit");
        assert!(total > 0, "eviction should stop once under the limit");
    }

    #[tokio::test]
    async fn clean_removes_everything_not_in_history() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_with(dir.path(), 100).await;
        let protected = cache.image_dir.join("keep.jpg");
        let orphan_a = cache.image_dir.join("a.jpg");
        let orphan_b = cache.image_dir.join("b.jpg");
        write_file(&protected, 1024).await;
        write_file(&orphan_a, 1024).await;
        write_file(&orphan_b, 1024).await;

        let history = history_protecting(&[protected.clone()]);
        let removed = cache.clean(&history).await.unwrap();

        assert_eq!(removed, 2);
        assert!(protected.exists());
        assert!(!orphan_a.exists());
        assert!(!orphan_b.exists());
    }

    #[tokio::test]
    async fn download_returns_cached_path_without_fetch() {
        let dir = tempfile::tempdir().unwrap();
        let cache = cache_with(dir.path(), 100).await;
        let url = "https://example.com/image.png";

        // Pre-create the file at the SHA256-derived path; download must
        // short-circuit and never touch the network.
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let expected = cache.image_dir.join(format!("{:x}.png", hasher.finalize()));
        write_file(&expected, 1024).await;

        let path = cache.download(url, Some("png")).await.unwrap();

        assert_eq!(path, expected);
    }
}
