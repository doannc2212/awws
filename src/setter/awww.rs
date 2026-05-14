use super::{SetterOptions, WallpaperSetter};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::path::Path;
use tokio::process::Command;

pub struct AwwwSetter;

#[async_trait]
impl WallpaperSetter for AwwwSetter {
    async fn set(&self, path: &Path, opts: &SetterOptions) -> Result<()> {
        let mut cmd = Command::new("awww");
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
            Err(anyhow!("awww exited with status {status}"))
        }
    }

    fn name(&self) -> &str {
        "awww"
    }
}
