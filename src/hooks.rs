//! Event bus and exec-hook runner for daemon lifecycle notifications.
//!
//! [`HookBus`] owns a `broadcast` channel that carries [`DaemonEvent`] values.
//! A single background listener task subscribes at startup and dispatches each
//! event to the matching shell command.  The daemon calls the typed
//! `publish_*` methods — it has no knowledge of channels, processes, or shell
//! syntax.
//!
//! ```text
//! daemon
//!   ├─ publish_change(path)  ──┐
//!   ├─ publish_start(path)   ──┤  broadcast::Sender<DaemonEvent>
//!   └─ publish_error(msg)    ──┘
//!                                   └─► listener task
//!                                          ├─ on_change → sh -c "wallust run %w"
//!                                          ├─ on_start  → sh -c "wallust run %w"
//!                                          └─ on_error  → sh -c "notify-send '%e'"
//! ```
//!
//! On `awws reload`, the caller replaces the `HookBus`.  Dropping the old one
//! closes its channel; the listener gets [`broadcast::error::RecvError::Closed`]
//! and exits cleanly before the new listener starts.

use std::{path::Path, path::PathBuf};
use tokio::sync::broadcast;

/// All events that can be published on the hook bus.
#[derive(Clone)]
enum DaemonEvent {
    /// A new wallpaper was applied successfully.  `%w` = path.
    WallpaperChanged { path: PathBuf },
    /// The daemon started.  `%w` = last wallpaper path from persisted history.
    DaemonStarted { path: PathBuf },
    /// A source or setter failed during an automatic advance.  `%e` = error.
    SourceError { message: String },
}

/// Owns the broadcast channel and manages listener lifecycle.
///
/// Call the `publish_*` methods to emit events; everything else is internal.
/// Drop the bus to shut down all listeners.
pub(crate) struct HookBus {
    tx: broadcast::Sender<DaemonEvent>,
}

impl HookBus {
    /// Build the bus.  For each non-`None` command a listener is spawned that
    /// runs the command when the matching event arrives.
    pub(crate) fn new(
        on_change: Option<&str>,
        on_start: Option<&str>,
        on_error: Option<&str>,
    ) -> Self {
        let (tx, _) = broadcast::channel(16);
        let any = on_change.is_some() || on_start.is_some() || on_error.is_some();
        if any {
            spawn_listener(
                &tx,
                on_change.map(str::to_owned),
                on_start.map(str::to_owned),
                on_error.map(str::to_owned),
            );
        }
        Self { tx }
    }

    /// Emit after every successful wallpaper change.
    pub(crate) fn publish_change(&self, path: PathBuf) {
        let _ = self.tx.send(DaemonEvent::WallpaperChanged { path });
    }

    /// Emit once at daemon start with the last wallpaper path from history.
    pub(crate) fn publish_start(&self, path: PathBuf) {
        let _ = self.tx.send(DaemonEvent::DaemonStarted { path });
    }

    /// Emit when an automatic wallpaper advance fails.
    pub(crate) fn publish_error(&self, message: String) {
        let _ = self.tx.send(DaemonEvent::SourceError { message });
    }
}

/// Subscribe to `tx` and dispatch each event to the appropriate hook command.
///
/// Lagged events are skipped with a warning rather than stalling the daemon.
/// The task exits cleanly when the sender is dropped (`RecvError::Closed`).
fn spawn_listener(
    tx: &broadcast::Sender<DaemonEvent>,
    on_change: Option<String>,
    on_start: Option<String>,
    on_error: Option<String>,
) {
    let mut rx = tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => dispatch(event, &on_change, &on_start, &on_error),
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(n, "hook listener lagged, {n} event(s) skipped");
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
        tracing::debug!("hook listener exited");
    });
}

/// Match an event to its configured command and spawn it.
fn dispatch(
    event: DaemonEvent,
    on_change: &Option<String>,
    on_start: &Option<String>,
    on_error: &Option<String>,
) {
    match event {
        DaemonEvent::WallpaperChanged { path } => {
            if let Some(cmd) = on_change {
                spawn_command(cmd.replace("%w", &path.to_string_lossy()));
            }
        }
        DaemonEvent::DaemonStarted { path } => {
            if let Some(cmd) = on_start {
                spawn_command(cmd.replace("%w", &path.to_string_lossy()));
            }
        }
        DaemonEvent::SourceError { message } => {
            if let Some(cmd) = on_error {
                spawn_command(cmd.replace("%e", &message));
            }
        }
    }
}

/// Spawn an already-expanded shell command via `sh -c` in a background task.
///
/// Exit-status errors are logged as warnings; they never propagate to the
/// caller so a broken hook cannot affect daemon operation.
fn spawn_command(cmd: String) {
    tracing::debug!(cmd = %cmd, "spawning hook");
    match tokio::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .spawn()
    {
        Ok(mut child) => {
            tokio::spawn(async move {
                match child.wait().await {
                    Ok(status) if !status.success() => {
                        tracing::warn!(cmd = %cmd, %status, "hook exited non-zero");
                    }
                    Err(err) => {
                        tracing::warn!(%err, cmd = %cmd, "hook wait failed");
                    }
                    _ => {}
                }
            });
        }
        Err(err) => {
            tracing::warn!(%err, cmd = %cmd, "failed to spawn hook");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mirror the substitution done inside `dispatch` so we can test it without
    // spawning real processes.
    fn sub_path(cmd: &str, path: &Path) -> String {
        cmd.replace("%w", &path.to_string_lossy())
    }
    fn sub_error(cmd: &str, msg: &str) -> String {
        cmd.replace("%e", msg)
    }

    #[test]
    fn on_change_substitutes_path() {
        let path = Path::new("/home/user/.cache/awws/images/abc.jpg");
        assert_eq!(
            sub_path("wallust run %w", path),
            "wallust run /home/user/.cache/awws/images/abc.jpg"
        );
    }

    #[test]
    fn on_change_substitutes_multiple_occurrences() {
        let path = Path::new("/tmp/wall.png");
        assert_eq!(
            sub_path("echo %w && cp %w /tmp/current", path),
            "echo /tmp/wall.png && cp /tmp/wall.png /tmp/current"
        );
    }

    #[test]
    fn on_change_no_placeholder_unchanged() {
        let path = Path::new("/tmp/wall.png");
        assert_eq!(
            sub_path("notify-send 'wallpaper changed'", path),
            "notify-send 'wallpaper changed'"
        );
    }

    #[test]
    fn on_start_substitutes_path() {
        let path = Path::new("/home/user/.cache/awws/images/last.jpg");
        assert_eq!(
            sub_path("wallust run %w", path),
            "wallust run /home/user/.cache/awws/images/last.jpg"
        );
    }

    #[test]
    fn on_error_substitutes_message() {
        assert_eq!(
            sub_error(
                "notify-send -u critical 'awws' '%e'",
                "connection timed out"
            ),
            "notify-send -u critical 'awws' 'connection timed out'"
        );
    }

    #[test]
    fn on_error_no_placeholder_unchanged() {
        assert_eq!(
            sub_error("notify-send awws error", "anything"),
            "notify-send awws error"
        );
    }

    #[test]
    fn hook_bus_no_hooks_publish_does_not_panic() {
        let bus = HookBus::new(None, None, None);
        bus.publish_change(PathBuf::from("/tmp/wall.png"));
        bus.publish_start(PathBuf::from("/tmp/wall.png"));
        bus.publish_error("something failed".into());
    }
}
