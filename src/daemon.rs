use crate::{
    cache::ImageCache,
    config,
    history::{History, HistoryEntry},
    hooks::HookBus,
    ipc::{self, ClientResponse, CommandEnvelope, DaemonCommand, StatusResponse},
    setter::{self, SetterOptions, WallpaperSetter},
    source::SourceRegistry,
    trigger::{self, TriggerEvent},
};
use anyhow::{Context, Result};
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{signal, sync::mpsc};

pub async fn run(config_path: PathBuf) -> Result<()> {
    let cfg = config::load(&config_path)?;
    let mut state = DaemonState::build(config_path, cfg).await?;

    if let Some(current) = state.history.current() {
        state.hooks.publish_start(current.path);
    }

    let (command_tx, mut command_rx) = mpsc::channel::<CommandEnvelope>(32);
    let ipc_task = tokio::spawn(ipc::serve(command_tx));

    tracing::info!("daemon started");
    loop {
        tokio::select! {
            Some(event) = state.triggers.recv() => {
                if !state.paused {
                    if let Err(err) = state.advance(format!("{event:?}")).await {
                        tracing::warn!(%err, "failed to advance wallpaper");
                    }
                }
            }
            Some(envelope) = command_rx.recv() => {
                let response = state.handle_command(envelope.command).await;
                let _ = envelope.reply.send(response);
            }
            result = signal::ctrl_c() => {
                result?;
                break;
            }
        }
    }

    ipc_task.abort();
    if let Ok(path) = ipc::socket_path() {
        let _ = tokio::fs::remove_file(path).await;
    }
    Ok(())
}

struct DaemonState {
    config_path: PathBuf,
    cfg: config::AppConfig,
    cache: ImageCache,
    history: History,
    sources: SourceRegistry,
    setter: Arc<dyn WallpaperSetter>,
    setter_opts: SetterOptions,
    triggers: mpsc::Receiver<TriggerEvent>,
    hooks: HookBus,
    paused: bool,
    next_change_at: Option<chrono::DateTime<chrono::Utc>>,
    source_kind: config::SourceKind,
}

impl DaemonState {
    async fn build(config_path: PathBuf, cfg: config::AppConfig) -> Result<Self> {
        let source_kind = config::classify_sources(&cfg.sources.list);
        if source_kind == config::SourceKind::Mixed {
            return Err(anyhow::anyhow!(
                "mixed local and remote sources are not supported; use only local or only remote sources"
            ));
        }

        let cache = ImageCache::new(&cfg.cache).await?;
        let history = cache.load_history(cfg.cache.history_size).await?;
        let sources = SourceRegistry::from_config(&cfg, cache.clone())?;
        let setter = setter::from_config(&cfg.setter)?;
        let setter_opts = SetterOptions::from(&cfg.setter);
        let triggers = trigger::spawn_all(&cfg.daemon);
        let next_change_at = next_change_at(cfg.daemon.interval_secs);
        let hooks = HookBus::new(
            cfg.daemon.on_change.as_deref(),
            cfg.daemon.on_start.as_deref(),
            cfg.daemon.on_error.as_deref(),
        );

        Ok(Self {
            config_path,
            cfg,
            cache,
            history,
            sources,
            setter,
            setter_opts,
            triggers,
            hooks,
            paused: false,
            next_change_at,
            source_kind,
        })
    }

    async fn reload(&mut self) -> Result<()> {
        let cfg = config::load(&self.config_path)?;
        let source_kind = config::classify_sources(&cfg.sources.list);
        if source_kind == config::SourceKind::Mixed {
            return Err(anyhow::anyhow!(
                "mixed local and remote sources are not supported; use only local or only remote sources"
            ));
        }

        let cache = ImageCache::new(&cfg.cache).await?;
        let history = cache.load_history(cfg.cache.history_size).await?;
        let sources = SourceRegistry::from_config(&cfg, cache.clone())?;
        let setter = setter::from_config(&cfg.setter)?;
        let setter_opts = SetterOptions::from(&cfg.setter);
        let triggers = trigger::spawn_all(&cfg.daemon);
        let hooks = HookBus::new(
            cfg.daemon.on_change.as_deref(),
            cfg.daemon.on_start.as_deref(),
            cfg.daemon.on_error.as_deref(),
        );

        self.cfg = cfg;
        self.cache = cache;
        self.history = history;
        self.sources = sources;
        self.setter = setter;
        self.setter_opts = setter_opts;
        self.triggers = triggers;
        self.hooks = hooks;
        self.next_change_at = next_change_at(self.cfg.daemon.interval_secs);
        self.source_kind = source_kind;
        Ok(())
    }

    async fn handle_command(&mut self, command: DaemonCommand) -> ClientResponse {
        let result = match command {
            DaemonCommand::Next => self.apply_next().await.map(|_| "advanced"),
            DaemonCommand::Prev => self.apply_previous().await.map(|_| "moved to previous"),
            DaemonCommand::Pause => {
                self.paused = true;
                Ok("paused")
            }
            DaemonCommand::Resume => {
                self.paused = false;
                Ok("resumed")
            }
            DaemonCommand::Status => Ok("ok"),
            DaemonCommand::Reload => self.reload().await.map(|_| "reloaded"),
            DaemonCommand::History => {
                return ClientResponse::History(ipc::HistoryResponse {
                    entries: self.history.entries().cloned().collect(),
                    cursor: self.history.cursor(),
                });
            }
            DaemonCommand::Clean => {
                return match self.apply_clean().await {
                    Ok(removed) => ClientResponse::Ok {
                        message: format!("cleaned {removed} cached image(s)"),
                        status: Some(self.status()),
                    },
                    Err(err) => ClientResponse::Error {
                        message: err.to_string(),
                    },
                };
            }
        };

        match result {
            Ok(message) => ClientResponse::Ok {
                message: message.to_string(),
                status: Some(self.status()),
            },
            Err(err) => ClientResponse::Error {
                message: err.to_string(),
            },
        }
    }

    async fn advance(&mut self, reason: String) -> Result<HistoryEntry> {
        tracing::info!(%reason, "advancing wallpaper");

        let meta = match self.sources.next().await {
            Ok(meta) => meta,
            Err(err) => {
                self.hooks.publish_error(err.to_string());
                return Err(err);
            }
        };

        if let Err(err) = self.setter.set(&meta.path, &self.setter_opts).await {
            let err = err.context(format!("setter {} failed", self.setter.name()));
            self.hooks.publish_error(err.to_string());
            return Err(err);
        }

        self.hooks.publish_change(meta.path.clone());
        let entry = self.history.push(meta);
        self.cache.save_history(&self.history).await?;
        self.cache.evict(&self.history).await?;
        self.next_change_at = next_change_at(self.cfg.daemon.interval_secs);
        Ok(entry)
    }

    async fn apply_previous(&mut self) -> Result<HistoryEntry> {
        let entry = self
            .history
            .previous()
            .context("already at oldest history entry")?;
        self.setter.set(&entry.path, &self.setter_opts).await?;
        self.hooks.publish_change(entry.path.clone());
        self.cache.save_history(&self.history).await?;
        Ok(entry)
    }

    async fn apply_next(&mut self) -> Result<HistoryEntry> {
        if let Some(entry) = self.history.next_existing() {
            self.setter.set(&entry.path, &self.setter_opts).await?;
            self.hooks.publish_change(entry.path.clone());
            self.cache.save_history(&self.history).await?;
            return Ok(entry);
        }

        match self.source_kind {
            config::SourceKind::LocalOnly => {
                if let Some(entry) = self.history.wrap_to_first() {
                    self.setter.set(&entry.path, &self.setter_opts).await?;
                    self.hooks.publish_change(entry.path.clone());
                    self.cache.save_history(&self.history).await?;
                    return Ok(entry);
                }
                self.advance("ipc next (initial)".to_string()).await
            }
            _ => self.advance("ipc next".to_string()).await,
        }
    }

    async fn apply_clean(&mut self) -> Result<u64> {
        self.history.keep_only_current();
        let removed = self.cache.clean(&self.history).await?;
        self.cache.save_history(&self.history).await?;
        Ok(removed)
    }

    fn status(&self) -> StatusResponse {
        StatusResponse {
            paused: self.paused,
            current: self.history.current(),
            next_change_at: self.next_change_at,
        }
    }
}

fn next_change_at(interval_secs: u64) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::Duration::from_std(Duration::from_secs(interval_secs))
        .ok()
        .map(|duration| chrono::Utc::now() + duration)
}
