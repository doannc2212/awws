use super::{SetterOptions, WallpaperSetter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct HyprpaperSetter;

#[async_trait]
impl WallpaperSetter for HyprpaperSetter {
    async fn set(&self, path: &Path, _opts: &SetterOptions) -> Result<()> {
        let running = Command::new("pgrep")
            .args(["-x", "hyprpaper"])
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !running {
            return Err(anyhow!("hyprpaper is not running"));
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("wallpaper path is not valid UTF-8"))?;

        let out = Command::new("hyprctl")
            .args(["hyprpaper", "wallpaper", &format!(", {path_str}")])
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        if !out.status.success() || stdout.to_lowercase().contains("err") {
            return Err(anyhow!("hyprpaper wallpaper failed: {stdout}"));
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "hyprpaper"
    }
}
