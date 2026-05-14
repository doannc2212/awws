use super::{ImageMeta, ImageSource};
use crate::cache::ImageCache;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rand::seq::SliceRandom;
use serde::Deserialize;

pub struct BingSource {
    cache: ImageCache,
}

impl BingSource {
    pub fn new(cache: ImageCache) -> Self {
        Self { cache }
    }
}

#[derive(Debug, Deserialize)]
struct BingResponse {
    images: Vec<BingImage>,
}

#[derive(Debug, Clone, Deserialize)]
struct BingImage {
    url: String,
    title: Option<String>,
    copyright: Option<String>,
}

#[async_trait]
impl ImageSource for BingSource {
    async fn next(&self) -> Result<ImageMeta> {
        let endpoint = "https://www.bing.com/HPImageArchive.aspx?format=js&idx=0&n=8&mkt=en-US";
        let response: BingResponse = reqwest::get(endpoint)
            .await?
            .error_for_status()?
            .json()
            .await?;
        let image = response
            .images
            .into_iter()
            .collect::<Vec<_>>()
            .choose(&mut rand::thread_rng())
            .cloned()
            .ok_or_else(|| anyhow!("Bing response did not include an image"))?;
        let url = if image.url.starts_with("http") {
            image.url
        } else {
            format!("https://www.bing.com{}", image.url)
        };
        let path = self.cache.download(&url, Some("jpg")).await?;
        Ok(ImageMeta {
            path,
            url: Some(url),
            title: image.title,
            author: image.copyright,
            source: self.name().to_string(),
        })
    }

    fn name(&self) -> &str {
        "bing"
    }
}
