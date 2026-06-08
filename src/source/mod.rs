mod bing;
mod local;
mod nasa_apod;
mod unsplash;
mod wallhaven;

use crate::{cache::ImageCache, config};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rand::{distributions::WeightedIndex, prelude::*};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

pub use bing::BingSource;
pub use local::LocalSource;
pub use nasa_apod::NasaApodSource;
pub use unsplash::UnsplashSource;
pub use wallhaven::WallhavenSource;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageMeta {
    pub path: PathBuf,
    pub url: Option<String>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub source: String,
}

#[async_trait]
pub trait ImageSource: Send + Sync {
    async fn next(&self) -> Result<ImageMeta>;
    fn name(&self) -> &str;
}

#[derive(Clone)]
pub struct WeightedSource {
    pub weight: u32,
    pub source: Arc<dyn ImageSource>,
}

pub struct SourceRegistry {
    sources: Vec<WeightedSource>,
    rotation: config::RotationStrategy,
    cursor: usize,
}

impl SourceRegistry {
    pub fn from_config(cfg: &config::AppConfig, cache: ImageCache) -> Result<Self> {
        let mut sources = Vec::new();
        for source_cfg in &cfg.sources.list {
            let source: Arc<dyn ImageSource> = match source_cfg {
                config::SourceConfig::Local { path, order, .. } => {
                    match LocalSource::new(config::expand_path(path), order.clone()) {
                        Ok(s) => Arc::new(s),
                        Err(err) => {
                            tracing::warn!(%err, "skipping local source");
                            continue;
                        }
                    }
                }
                config::SourceConfig::Bing { .. } => Arc::new(BingSource::new(cache.clone())),
                config::SourceConfig::Unsplash {
                    api_key,
                    query,
                    orientation,
                    ..
                } => Arc::new(UnsplashSource::new(
                    cache.clone(),
                    api_key.clone(),
                    query.clone(),
                    orientation.clone(),
                )),
                config::SourceConfig::NasaApod { api_key, .. } => {
                    Arc::new(NasaApodSource::new(cache.clone(), api_key.clone()))
                }
                config::SourceConfig::Wallhaven {
                    api_key,
                    query,
                    categories,
                    purity,
                    ..
                } => Arc::new(WallhavenSource::new(
                    cache.clone(),
                    api_key.clone(),
                    query.clone(),
                    categories.clone(),
                    purity.clone(),
                )),
            };
            sources.push(WeightedSource {
                weight: config::source_weight(source_cfg).max(1),
                source,
            });
        }

        if sources.is_empty() {
            return Err(anyhow!("no sources configured"));
        }

        Ok(Self {
            sources,
            rotation: cfg.sources.rotation.clone(),
            cursor: 0,
        })
    }

    pub async fn next(&mut self) -> Result<ImageMeta> {
        let start = self.cursor;
        let mut errors = Vec::new();

        for _ in 0..self.sources.len() {
            let idx = self.pick_index()?;
            let source = self.sources[idx].source.clone();
            match source.next().await {
                Ok(meta) => return Ok(meta),
                Err(err) => {
                    tracing::warn!(source = source.name(), %err, "source failed");
                    errors.push(format!("{}: {err}", source.name()));
                    if matches!(self.rotation, config::RotationStrategy::RoundRobin) {
                        self.cursor = (idx + 1) % self.sources.len();
                    }
                }
            }
        }

        self.cursor = start;
        Err(anyhow!("all sources failed: {}", errors.join("; ")))
    }

    fn pick_index(&mut self) -> Result<usize> {
        match self.rotation {
            config::RotationStrategy::RoundRobin => {
                let idx = self.cursor;
                self.cursor = (self.cursor + 1) % self.sources.len();
                Ok(idx)
            }
            config::RotationStrategy::WeightedRandom => {
                let weights = self.sources.iter().map(|source| source.weight);
                let dist = WeightedIndex::new(weights)?;
                Ok(dist.sample(&mut thread_rng()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config::RotationStrategy;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Stub {
        name: String,
        succeed: bool,
        calls: Arc<AtomicUsize>,
    }

    impl Stub {
        fn new(name: &str, succeed: bool) -> (Arc<Self>, Arc<AtomicUsize>) {
            let calls = Arc::new(AtomicUsize::new(0));
            let stub = Arc::new(Self {
                name: name.into(),
                succeed,
                calls: calls.clone(),
            });
            (stub, calls)
        }
    }

    #[async_trait]
    impl ImageSource for Stub {
        async fn next(&self) -> Result<ImageMeta> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.succeed {
                Ok(ImageMeta {
                    path: PathBuf::from(format!("/{}.jpg", self.name)),
                    url: None,
                    title: None,
                    author: None,
                    source: self.name.clone(),
                })
            } else {
                Err(anyhow!("{} failed", self.name))
            }
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    fn registry(sources: Vec<WeightedSource>, rotation: RotationStrategy) -> SourceRegistry {
        SourceRegistry {
            sources,
            rotation,
            cursor: 0,
        }
    }

    fn weighted(source: Arc<dyn ImageSource>) -> WeightedSource {
        WeightedSource { weight: 1, source }
    }

    #[test]
    fn round_robin_pick_index_cycles() {
        let (a, _) = Stub::new("a", true);
        let (b, _) = Stub::new("b", true);
        let mut reg = registry(vec![weighted(a), weighted(b)], RotationStrategy::RoundRobin);

        assert_eq!(reg.pick_index().unwrap(), 0);
        assert_eq!(reg.pick_index().unwrap(), 1);
        assert_eq!(reg.pick_index().unwrap(), 0);
    }

    #[tokio::test]
    async fn round_robin_next_advances_across_sources() {
        let (a, _) = Stub::new("a", true);
        let (b, _) = Stub::new("b", true);
        let mut reg = registry(vec![weighted(a), weighted(b)], RotationStrategy::RoundRobin);

        assert_eq!(reg.next().await.unwrap().source, "a");
        assert_eq!(reg.next().await.unwrap().source, "b");
        assert_eq!(reg.next().await.unwrap().source, "a");
    }

    #[tokio::test]
    async fn next_fails_over_to_working_source() {
        let (a, a_calls) = Stub::new("a", false);
        let (b, b_calls) = Stub::new("b", true);
        let mut reg = registry(vec![weighted(a), weighted(b)], RotationStrategy::RoundRobin);

        let meta = reg.next().await.unwrap();
        assert_eq!(meta.source, "b");
        assert_eq!(
            a_calls.load(Ordering::SeqCst),
            1,
            "failed source must be tried"
        );
        assert_eq!(b_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn next_errors_when_all_sources_fail() {
        let (a, _) = Stub::new("a", false);
        let (b, _) = Stub::new("b", false);
        let mut reg = registry(vec![weighted(a), weighted(b)], RotationStrategy::RoundRobin);

        let err = reg.next().await.unwrap_err().to_string();
        assert!(err.contains("all sources failed"));
        assert!(err.contains("a failed"));
        assert!(err.contains("b failed"));
    }

    #[tokio::test]
    async fn next_tries_each_source_only_once_per_call() {
        let (a, a_calls) = Stub::new("a", false);
        let (b, b_calls) = Stub::new("b", false);
        let mut reg = registry(vec![weighted(a), weighted(b)], RotationStrategy::RoundRobin);

        let _ = reg.next().await;
        assert_eq!(a_calls.load(Ordering::SeqCst), 1);
        assert_eq!(b_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn weighted_random_single_source_is_returned() {
        let (a, a_calls) = Stub::new("a", true);
        let mut reg = registry(vec![weighted(a)], RotationStrategy::WeightedRandom);

        assert_eq!(reg.next().await.unwrap().source, "a");
        assert_eq!(a_calls.load(Ordering::SeqCst), 1);
    }
}
