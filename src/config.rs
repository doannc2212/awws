use anyhow::{Context, Result};
use serde::Deserialize;
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub daemon: DaemonConfig,
    pub setter: SetterConfig,
    pub sources: SourcesConfig,
    pub cache: CacheConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DaemonConfig {
    pub interval_secs: u64,
    pub change_on_login: bool,
    pub change_on_unlock: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SetterConfig {
    pub backend: String,
    pub transition: Option<String>,
    pub transition_duration: f32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SourcesConfig {
    pub rotation: RotationStrategy,
    pub list: Vec<SourceConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationStrategy {
    RoundRobin,
    #[default]
    WeightedRandom,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SourceConfig {
    Local {
        #[serde(default = "default_weight")]
        weight: u32,
        path: String,
        #[serde(default)]
        order: LocalOrder,
    },
    Bing {
        #[serde(default = "default_weight")]
        weight: u32,
    },
    Unsplash {
        #[serde(default = "default_weight")]
        weight: u32,
        api_key: String,
        query: Option<String>,
        orientation: Option<String>,
    },
    NasaApod {
        #[serde(default = "default_weight")]
        weight: u32,
        api_key: String,
    },
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalOrder {
    #[default]
    Random,
    Sequential,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CacheConfig {
    pub max_size_mb: u64,
    pub history_size: usize,
    pub dir: String,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            interval_secs: 1800,
            change_on_login: true,
            change_on_unlock: true,
        }
    }
}

impl Default for SetterConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_string(),
            transition: Some("fade".to_string()),
            transition_duration: 1.5,
        }
    }
}

impl Default for SourcesConfig {
    fn default() -> Self {
        Self {
            rotation: RotationStrategy::WeightedRandom,
            list: vec![SourceConfig::Bing { weight: 1 }],
        }
    }
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_size_mb: 500,
            history_size: 100,
            dir: "~/.cache/awws/images".to_string(),
        }
    }
}

const DEFAULT_CONFIG: &str = include_str!("../config.toml");

pub fn load(path: &std::path::Path) -> Result<AppConfig> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create config dir {}", parent.display()))?;
        }
        fs::write(path, DEFAULT_CONFIG)
            .with_context(|| format!("failed to write default config to {}", path.display()))?;
        tracing::info!("created default config at {}", path.display());
        return Ok(AppConfig::default());
    }

    let raw =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    toml::from_str(&raw).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("awws")
        .join("config.toml")
}

pub fn expand_path(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        return dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest);
    }
    PathBuf::from(path)
}

#[derive(Debug, Clone, PartialEq)]
pub enum SourceKind {
    LocalOnly,
    RemoteOnly,
    Mixed,
}

pub fn classify_sources(sources: &[SourceConfig]) -> SourceKind {
    let has_local = sources.iter().any(|s| matches!(s, SourceConfig::Local { .. }));
    let has_remote = sources.iter().any(|s| !matches!(s, SourceConfig::Local { .. }));
    match (has_local, has_remote) {
        (true, false) => SourceKind::LocalOnly,
        (false, true) => SourceKind::RemoteOnly,
        _ => SourceKind::Mixed,
    }
}

pub fn source_weight(source: &SourceConfig) -> u32 {
    match source {
        SourceConfig::Local { weight, .. }
        | SourceConfig::Bing { weight }
        | SourceConfig::Unsplash { weight, .. }
        | SourceConfig::NasaApod { weight, .. } => *weight,
    }
}

fn default_weight() -> u32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_path_tilde_alone() {
        let expanded = expand_path("~");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home);
        }
    }

    #[test]
    fn expand_path_tilde_prefix() {
        let expanded = expand_path("~/pictures");
        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home.join("pictures"));
        }
    }

    #[test]
    fn expand_path_absolute() {
        assert_eq!(expand_path("/tmp/awws"), PathBuf::from("/tmp/awws"));
    }

    #[test]
    fn expand_path_relative() {
        assert_eq!(expand_path("relative/path"), PathBuf::from("relative/path"));
    }

    #[test]
    fn source_weight_all_variants() {
        assert_eq!(
            source_weight(&SourceConfig::Local {
                weight: 3,
                path: ".".into(),
                order: LocalOrder::Random
            }),
            3
        );
        assert_eq!(source_weight(&SourceConfig::Bing { weight: 2 }), 2);
        assert_eq!(
            source_weight(&SourceConfig::Unsplash {
                weight: 4,
                api_key: "k".into(),
                query: None,
                orientation: None
            }),
            4
        );
        assert_eq!(
            source_weight(&SourceConfig::NasaApod {
                weight: 1,
                api_key: "k".into()
            }),
            1
        );
    }

    #[test]
    fn load_missing_file_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.daemon.interval_secs, 1800);
        assert!(path.exists(), "default config should have been written");
    }

    #[test]
    fn load_valid_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[daemon]\ninterval_secs = 600\nchange_on_login = false\n",
        )
        .unwrap();
        let cfg = load(&path).unwrap();
        assert_eq!(cfg.daemon.interval_secs, 600);
        assert!(!cfg.daemon.change_on_login);
    }

    #[test]
    fn load_invalid_toml_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "not = valid [ toml ][[[").unwrap();
        assert!(load(&path).is_err());
    }

    #[test]
    fn default_daemon_config() {
        let cfg = DaemonConfig::default();
        assert_eq!(cfg.interval_secs, 1800);
        assert!(cfg.change_on_login);
        assert!(cfg.change_on_unlock);
    }

    #[test]
    fn default_setter_config() {
        let cfg = SetterConfig::default();
        assert_eq!(cfg.backend, "auto");
        assert_eq!(cfg.transition, Some("fade".to_string()));
        assert_eq!(cfg.transition_duration, 1.5);
    }

    #[test]
    fn default_cache_config() {
        let cfg = CacheConfig::default();
        assert_eq!(cfg.max_size_mb, 500);
        assert_eq!(cfg.history_size, 100);
        assert_eq!(cfg.dir, "~/.cache/awws/images");
    }
}
