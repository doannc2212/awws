use super::{ImageMeta, ImageSource};
use crate::config::LocalOrder;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rand::seq::SliceRandom;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct LocalSource {
    dir: PathBuf,
    order: LocalOrder,
    cursor: AtomicUsize,
}

impl LocalSource {
    pub fn new(dir: PathBuf, order: LocalOrder) -> Result<Self> {
        if !dir.exists() {
            return Err(anyhow!(
                "local source directory does not exist: {}",
                dir.display()
            ));
        }
        Ok(Self {
            dir,
            order,
            cursor: AtomicUsize::new(0),
        })
    }
}

#[async_trait]
impl ImageSource for LocalSource {
    async fn next(&self) -> Result<ImageMeta> {
        let mut entries = tokio::fs::read_dir(&self.dir).await?;
        let mut images = Vec::new();

        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if is_image(&path) {
                images.push(path);
            }
        }

        images.sort();
        if images.is_empty() {
            return Err(anyhow!("no images found in {}", self.dir.display()));
        }

        let path = match self.order {
            LocalOrder::Random => images
                .choose(&mut rand::thread_rng())
                .cloned()
                .expect("images is non-empty"),
            LocalOrder::Sequential => {
                let idx = self.cursor.fetch_add(1, Ordering::Relaxed) % images.len();
                images[idx].clone()
            }
        };

        Ok(ImageMeta {
            path,
            url: None,
            title: None,
            author: None,
            source: self.name().to_string(),
        })
    }

    fn name(&self) -> &str {
        "local"
    }
}

fn is_image(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "jpg" | "jpeg" | "png" | "webp"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_image_accepts_valid_extensions() {
        for ext in &["jpg", "jpeg", "png", "webp"] {
            assert!(
                is_image(std::path::Path::new(&format!("photo.{ext}"))),
                "expected {ext} to be accepted"
            );
        }
    }

    #[test]
    fn is_image_is_case_insensitive() {
        assert!(is_image(std::path::Path::new("photo.JPG")));
        assert!(is_image(std::path::Path::new("photo.PNG")));
        assert!(is_image(std::path::Path::new("photo.WebP")));
    }

    #[test]
    fn is_image_rejects_non_image_extensions() {
        assert!(!is_image(std::path::Path::new("file.txt")));
        assert!(!is_image(std::path::Path::new("file.mp4")));
        assert!(!is_image(std::path::Path::new("file.gif")));
        assert!(!is_image(std::path::Path::new("archive.tar.gz")));
    }

    #[test]
    fn is_image_rejects_no_extension() {
        assert!(!is_image(std::path::Path::new("README")));
        assert!(!is_image(std::path::Path::new("")));
    }

    #[test]
    fn new_fails_for_missing_directory() {
        let result = LocalSource::new(
            PathBuf::from("/nonexistent/awws/wallpapers"),
            LocalOrder::Random,
        );
        assert!(result.is_err());
    }

    #[test]
    fn new_succeeds_for_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let result = LocalSource::new(dir.path().to_path_buf(), LocalOrder::Random);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn next_errors_on_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let source = LocalSource::new(dir.path().to_path_buf(), LocalOrder::Sequential).unwrap();
        assert!(source.next().await.is_err());
    }

    #[tokio::test]
    async fn next_skips_non_image_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.txt"), b"").unwrap();
        std::fs::write(dir.path().join("photo.jpg"), b"").unwrap();
        let source = LocalSource::new(dir.path().to_path_buf(), LocalOrder::Sequential).unwrap();
        let meta = source.next().await.unwrap();
        assert!(meta.path.extension().unwrap() == "jpg");
        assert_eq!(meta.source, "local");
    }

    #[tokio::test]
    async fn next_sequential_iterates_in_sorted_order() {
        let dir = tempfile::tempdir().unwrap();
        for name in &["c.jpg", "a.jpg", "b.jpg"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let source = LocalSource::new(dir.path().to_path_buf(), LocalOrder::Sequential).unwrap();
        let first = source.next().await.unwrap().path;
        let second = source.next().await.unwrap().path;
        let third = source.next().await.unwrap().path;
        assert!(first.ends_with("a.jpg"));
        assert!(second.ends_with("b.jpg"));
        assert!(third.ends_with("c.jpg"));
    }

    #[tokio::test]
    async fn next_sequential_wraps_around() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"").unwrap();
        let source = LocalSource::new(dir.path().to_path_buf(), LocalOrder::Sequential).unwrap();
        let first = source.next().await.unwrap().path;
        let second = source.next().await.unwrap().path;
        assert_eq!(first, second);
    }

    #[tokio::test]
    async fn next_random_returns_a_valid_image() {
        let dir = tempfile::tempdir().unwrap();
        for name in &["a.jpg", "b.png"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let source = LocalSource::new(dir.path().to_path_buf(), LocalOrder::Random).unwrap();
        let meta = source.next().await.unwrap();
        assert!(is_image(&meta.path));
        assert!(meta.path.starts_with(dir.path()));
    }
}
