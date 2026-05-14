mod bing;
mod local;
mod nasa_apod;
mod unsplash;

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
