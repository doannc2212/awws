use super::{SetterOptions, WallpaperSetter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct SwwwSetter;

#[async_trait]
impl WallpaperSetter for SwwwSetter {
    async fn set(&self, path: &Path, opts: &SetterOptions) -> Result<()> {
        let mut cmd = Command::new("swww");
        cmd.arg("img").arg(path);
        if let Some(transition) = &opts.transition {
            cmd.arg("--transition-type").arg(transition);
            cmd.arg("--transition-duration")
                .arg(opts.transition_duration.to_string());
        }

        let status = cmd.status().await?;
        if status.success() {
            Ok(())
        } else {
            Err(anyhow!("swww exited with status {status}"))
        }
    }

    fn name(&self) -> &str {
        "swww"
    }
}
