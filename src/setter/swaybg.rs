use super::{SetterOptions, WallpaperSetter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct SwaybgSetter;

#[async_trait]
impl WallpaperSetter for SwaybgSetter {
    async fn set(&self, path: &Path, _opts: &SetterOptions) -> Result<()> {
        // Kill any existing swaybg instance before spawning a new one.
        let _ = Command::new("pkill").args(["-x", "swaybg"]).status().await;

        let mut child = Command::new("swaybg")
            .arg("-i")
            .arg(path)
            .arg("-m")
            .arg("fill")
            .spawn()?;

        // Give swaybg a moment to fail fast on bad input.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        match child.try_wait()? {
            Some(status) if !status.success() => {
                Err(anyhow!("swaybg exited early with status {status}"))
            }
            _ => Ok(()),
        }
    }

    fn name(&self) -> &str {
        "swaybg"
    }
}
