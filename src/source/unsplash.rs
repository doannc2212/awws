use super::{ImageMeta, ImageSource};
use crate::cache::ImageCache;
use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;

pub struct UnsplashSource {
    cache: ImageCache,
    api_key: String,
    query: Option<String>,
    orientation: Option<String>,
}

impl UnsplashSource {
    pub fn new(
        cache: ImageCache,
        api_key: String,
        query: Option<String>,
        orientation: Option<String>,
    ) -> Self {
        Self {
            cache,
            api_key,
            query,
            orientation,
        }
    }
}

#[derive(Debug, Deserialize)]
struct UnsplashPhoto {
    description: Option<String>,
    alt_description: Option<String>,
    user: UnsplashUser,
    urls: UnsplashUrls,
}

#[derive(Debug, Deserialize)]
struct UnsplashUser {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UnsplashUrls {
    full: String,
}

#[async_trait]
impl ImageSource for UnsplashSource {
    async fn next(&self) -> Result<ImageMeta> {
        let mut url = "https://api.unsplash.com/photos/random".to_string();
        let mut params = Vec::new();
        if let Some(query) = &self.query {
            params.push(format!("query={}", urlencoding::encode(query)));
        }
        if let Some(orientation) = &self.orientation {
            params.push(format!("orientation={}", urlencoding::encode(orientation)));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let client = reqwest::Client::new();
        let photo: UnsplashPhoto = client
            .get(url)
            .header("Authorization", format!("Client-ID {}", self.api_key))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let path = self.cache.download(&photo.urls.full, Some("jpg")).await?;

        Ok(ImageMeta {
            path,
            url: Some(photo.urls.full),
            title: photo.description.or(photo.alt_description),
            author: photo.user.name,
            source: self.name().to_string(),
        })
    }

    fn name(&self) -> &str {
        "unsplash"
    }
}
