use crate::source::ImageMeta;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::VecDeque, path::PathBuf};
use tokio::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub path: PathBuf,
    pub url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub source: String,
    pub set_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct History {
    entries: VecDeque<HistoryEntry>,
    cursor: Option<usize>,
    max_entries: usize,
}

impl History {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            cursor: None,
            max_entries,
        }
    }

    pub async fn load(path: &std::path::Path, max_entries: usize) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new(max_entries));
        }

        let raw = fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read history {}", path.display()))?;
        let mut history: History = serde_json::from_str(&raw)
            .with_context(|| format!("failed to parse history {}", path.display()))?;
        history.max_entries = max_entries;
        history.trim();
        Ok(history)
    }

    pub async fn save(&self, path: &std::path::Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(path, raw).await?;
        Ok(())
    }

    pub fn push(&mut self, meta: ImageMeta) -> HistoryEntry {
        if let Some(cursor) = self.cursor {
            if cursor + 1 < self.entries.len() {
                self.entries.truncate(cursor + 1);
            }
        }

        let entry = HistoryEntry {
            path: meta.path,
            url: meta.url,
            title: meta.title,
            author: meta.author,
            source: meta.source,
            set_at: chrono::Utc::now(),
        };

        self.entries.push_back(entry.clone());
        self.trim();
        self.cursor = self.entries.len().checked_sub(1);
        entry
    }

    pub fn previous(&mut self) -> Option<HistoryEntry> {
        let cursor = self.cursor?;
        if cursor == 0 {
            return None;
        }
        self.cursor = Some(cursor - 1);
        self.current()
    }

    pub fn next_existing(&mut self) -> Option<HistoryEntry> {
        let cursor = self.cursor?;
        if cursor + 1 >= self.entries.len() {
            return None;
        }
        self.cursor = Some(cursor + 1);
        self.current()
    }

    pub fn current(&self) -> Option<HistoryEntry> {
        self.cursor.and_then(|idx| self.entries.get(idx).cloned())
    }

    pub fn wrap_to_first(&mut self) -> Option<HistoryEntry> {
        if self.entries.is_empty() {
            return None;
        }
        self.cursor = Some(0);
        self.current()
    }

    pub fn entries(&self) -> impl Iterator<Item = &HistoryEntry> {
        self.entries.iter()
    }

    fn trim(&mut self) {
        while self.entries.len() > self.max_entries {
            self.entries.pop_front();
            self.cursor = self.cursor.and_then(|idx| idx.checked_sub(1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::ImageMeta;
    use std::path::PathBuf;

    fn meta(path: &str) -> ImageMeta {
        ImageMeta {
            path: PathBuf::from(path),
            url: None,
            title: None,
            author: None,
            source: "test".into(),
        }
    }

    #[test]
    fn new_is_empty() {
        let h = History::new(10);
        assert!(h.current().is_none());
        assert_eq!(h.entries().count(), 0);
    }

    #[test]
    fn push_sets_cursor_to_new_entry() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        assert_eq!(h.current().unwrap().path, PathBuf::from("/tmp/a.jpg"));
    }

    #[test]
    fn push_multiple_cursor_tracks_latest() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        h.push(meta("/tmp/b.jpg"));
        assert_eq!(h.current().unwrap().path, PathBuf::from("/tmp/b.jpg"));
        assert_eq!(h.entries().count(), 2);
    }

    #[test]
    fn previous_moves_cursor_back() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        h.push(meta("/tmp/b.jpg"));
        let prev = h.previous().unwrap();
        assert_eq!(prev.path, PathBuf::from("/tmp/a.jpg"));
        assert_eq!(h.current().unwrap().path, PathBuf::from("/tmp/a.jpg"));
    }

    #[test]
    fn previous_at_beginning_returns_none() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        assert!(h.previous().is_none());
    }

    #[test]
    fn previous_on_empty_returns_none() {
        let mut h = History::new(10);
        assert!(h.previous().is_none());
    }

    #[test]
    fn next_existing_moves_cursor_forward() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        h.push(meta("/tmp/b.jpg"));
        h.previous();
        let next = h.next_existing().unwrap();
        assert_eq!(next.path, PathBuf::from("/tmp/b.jpg"));
    }

    #[test]
    fn next_existing_at_end_returns_none() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        assert!(h.next_existing().is_none());
    }

    #[test]
    fn push_truncates_future_entries() {
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        h.push(meta("/tmp/b.jpg"));
        h.push(meta("/tmp/c.jpg"));
        h.previous(); // at b
        h.previous(); // at a
        h.push(meta("/tmp/d.jpg")); // truncates b, c; appends d
        assert!(h.next_existing().is_none());
        assert_eq!(h.entries().count(), 2);
        assert_eq!(h.current().unwrap().path, PathBuf::from("/tmp/d.jpg"));
    }

    #[test]
    fn trim_enforces_max_entries() {
        let mut h = History::new(3);
        for i in 0..5u8 {
            h.push(meta(&format!("/tmp/{i}.jpg")));
        }
        assert_eq!(h.entries().count(), 3);
    }

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let mut h = History::new(10);
        h.push(meta("/tmp/a.jpg"));
        h.push(meta("/tmp/b.jpg"));
        h.save(&path).await.unwrap();

        let loaded = History::load(&path, 10).await.unwrap();
        assert_eq!(loaded.entries().count(), 2);
        assert_eq!(loaded.current().unwrap().path, PathBuf::from("/tmp/b.jpg"));
    }

    #[tokio::test]
    async fn load_missing_file_returns_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");
        let h = History::load(&path, 10).await.unwrap();
        assert!(h.current().is_none());
        assert_eq!(h.entries().count(), 0);
    }

    #[tokio::test]
    async fn load_respects_max_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.json");
        let mut h = History::new(10);
        for i in 0..8u8 {
            h.push(meta(&format!("/tmp/{i}.jpg")));
        }
        h.save(&path).await.unwrap();

        let loaded = History::load(&path, 3).await.unwrap();
        assert_eq!(loaded.entries().count(), 3);
    }
}
