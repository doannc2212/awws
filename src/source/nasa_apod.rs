use super::{ImageMeta, ImageSource};
use crate::cache::ImageCache;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;

pub struct NasaApodSource {
    cache: ImageCache,
    api_key: String,
}

impl NasaApodSource {
    pub fn new(cache: ImageCache, api_key: String) -> Self {
        Self { cache, api_key }
    }
}

#[derive(Debug, Deserialize)]
struct ApodResponse {
    title: Option<String>,
    url: String,
    media_type: String,
    copyright: Option<String>,
}

#[async_trait]
impl ImageSource for NasaApodSource {
    async fn next(&self) -> Result<ImageMeta> {
        let url = format!(
            "https://api.nasa.gov/planetary/apod?api_key={}",
            urlencoding::encode(&self.api_key)
        );
        let apod: ApodResponse = reqwest::get(url).await?.error_for_status()?.json().await?;
        if apod.media_type != "image" {
            return Err(anyhow!("NASA APOD for today is not an image"));
        }
        let path = self.cache.download(&apod.url, Some("jpg")).await?;

        Ok(ImageMeta {
            path,
            url: Some(apod.url),
            title: apod.title,
            author: apod.copyright,
            source: self.name().to_string(),
        })
    }

    fn name(&self) -> &str {
        "nasa_apod"
    }
}
