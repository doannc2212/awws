mod awww;
mod hyprpaper;
mod swaybg;
mod swww;

use crate::config::SetterConfig;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use std::{path::Path, sync::Arc};

#[derive(Debug, Clone)]
pub struct SetterOptions {
    pub transition: Option<String>,
    pub transition_duration: f32,
}

impl From<&SetterConfig> for SetterOptions {
    fn from(value: &SetterConfig) -> Self {
        Self {
            transition: value.transition.clone(),
            transition_duration: value.transition_duration,
        }
    }
}

#[async_trait]
pub trait WallpaperSetter: Send + Sync {
    async fn set(&self, path: &Path, opts: &SetterOptions) -> Result<()>;
    fn name(&self) -> &str;
}

pub fn from_config(cfg: &SetterConfig) -> Result<Arc<dyn WallpaperSetter>> {
    match cfg.backend.as_str() {
        "awww" => Ok(Arc::new(awww::AwwwSetter)),
        "swww" => Ok(Arc::new(swww::SwwwSetter)),
        "hyprpaper" => Ok(Arc::new(hyprpaper::HyprpaperSetter)),
        "swaybg" => Ok(Arc::new(swaybg::SwaybgSetter)),
        "auto" => Ok(Arc::new(FallbackSetter(vec![
            Arc::new(awww::AwwwSetter),
            Arc::new(swww::SwwwSetter),
            Arc::new(hyprpaper::HyprpaperSetter),
            Arc::new(swaybg::SwaybgSetter),
        ]))),
        other => Err(anyhow!("unknown setter backend: {other}")),
    }
}

struct FallbackSetter(Vec<Arc<dyn WallpaperSetter>>);

#[async_trait]
impl WallpaperSetter for FallbackSetter {
    async fn set(&self, path: &Path, opts: &SetterOptions) -> Result<()> {
        let mut last_err = anyhow!("no setters configured");
        for setter in &self.0 {
            match setter.set(path, opts).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!(setter = setter.name(), err = %e, "setter failed, trying next");
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    fn name(&self) -> &str {
        "auto"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SetterConfig;

    #[test]
    fn from_config_all_known_backends() {
        for backend in &["awww", "swww", "hyprpaper", "swaybg", "auto"] {
            let cfg = SetterConfig {
                backend: backend.to_string(),
                ..SetterConfig::default()
            };
            let setter = from_config(&cfg).unwrap();
            assert_eq!(setter.name(), *backend);
        }
    }

    #[test]
    fn from_config_unknown_backend_errors() {
        let cfg = SetterConfig {
            backend: "sway-bg".into(),
            ..SetterConfig::default()
        };
        assert!(from_config(&cfg).is_err());
    }

    #[test]
    fn setter_options_from_config() {
        let cfg = SetterConfig {
            backend: "swww".into(),
            transition: Some("wipe".into()),
            transition_duration: 2.5,
        };
        let opts = SetterOptions::from(&cfg);
        assert_eq!(opts.transition, Some("wipe".into()));
        assert_eq!(opts.transition_duration, 2.5);
    }

    #[test]
    fn setter_options_none_transition() {
        let cfg = SetterConfig {
            backend: "hyprpaper".into(),
            transition: None,
            transition_duration: 1.0,
        };
        let opts = SetterOptions::from(&cfg);
        assert!(opts.transition.is_none());
    }
}
