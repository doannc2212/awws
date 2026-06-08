use super::{ImageMeta, ImageSource};
use crate::cache::ImageCache;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;

pub struct WallhavenSource {
    cache: ImageCache,
    api_key: Option<String>,
    query: Option<String>,
    categories: Option<String>,
    purity: Option<String>,
}

impl WallhavenSource {
    pub fn new(
        cache: ImageCache,
        api_key: Option<String>,
        query: Option<String>,
        categories: Option<String>,
        purity: Option<String>,
    ) -> Self {
        Self {
            cache,
            api_key,
            query,
            categories,
            purity,
        }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<Wallpaper>,
}

#[derive(Debug, Deserialize)]
struct Wallpaper {
    url: String,
    path: String,
    file_type: String,
}

#[async_trait]
impl ImageSource for WallhavenSource {
    async fn next(&self) -> Result<ImageMeta> {
        let mut url = "https://wallhaven.cc/api/v1/search".to_string();
        let mut params = vec!["sorting=random".to_string()];
        if let Some(query) = &self.query {
            params.push(format!("q={}", urlencoding::encode(query)));
        }
        if let Some(categories) = &self.categories {
            params.push(format!("categories={}", urlencoding::encode(categories)));
        }
        if let Some(purity) = &self.purity {
            params.push(format!("purity={}", urlencoding::encode(purity)));
        }
        if let Some(api_key) = &self.api_key {
            params.push(format!("apikey={}", urlencoding::encode(api_key)));
        }
        url.push('?');
        url.push_str(&params.join("&"));

        let response: SearchResponse = reqwest::get(url).await?.error_for_status()?.json().await?;
        let wallpaper = response
            .data
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("Wallhaven search returned no results"))?;
        let ext = wallpaper.file_type.rsplit('/').next().map(|t| match t {
            "jpeg" => "jpg",
            other => other,
        });
        let path = self.cache.download(&wallpaper.path, ext).await?;

        Ok(ImageMeta {
            path,
            url: Some(wallpaper.url),
            title: None,
            author: None,
            source: self.name().to_string(),
        })
    }

    fn name(&self) -> &str {
        "wallhaven"
    }
}
