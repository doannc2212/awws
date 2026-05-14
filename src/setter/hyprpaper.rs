use super::{SetterOptions, WallpaperSetter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct HyprpaperSetter;

impl HyprpaperSetter {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl WallpaperSetter for HyprpaperSetter {
    async fn set(&self, path: &Path, _opts: &SetterOptions) -> Result<()> {
        let preload = Command::new("hyprctl")
            .arg("hyprpaper")
            .arg("preload")
            .arg(path)
            .status()
            .await?;
        if !preload.success() {
            return Err(anyhow!(
                "hyprctl hyprpaper preload exited with status {preload}"
            ));
        }

        let wallpaper = format!(",{}", path.display());
        let status = Command::new("hyprctl")
            .arg("hyprpaper")
            .arg("wallpaper")
            .arg(wallpaper)
            .status()
            .await?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow!(
                "hyprctl hyprpaper wallpaper exited with status {status}"
            ))
        }
    }

    fn name(&self) -> &str {
        "hyprpaper"
    }
}
