use super::{SetterOptions, WallpaperSetter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct SwaybgSetter;

#[async_trait]
impl WallpaperSetter for SwaybgSetter {
    async fn set(&self, path: &Path, _opts: &SetterOptions) -> Result<()> {
        let status = Command::new("swaybg")
            .arg("-i")
            .arg(path)
            .arg("-m")
            .arg("fill")
            .status()
            .await?;

        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("swaybg exited with status {status}"))
        }
    }

    fn name(&self) -> &str {
        "swaybg"
    }
}
