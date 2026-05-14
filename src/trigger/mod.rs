mod dbus;
mod interval;

use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum TriggerEvent {
    Interval,
    Login,
    Unlock,
}

pub fn spawn_all(cfg: &crate::config::DaemonConfig) -> mpsc::Receiver<TriggerEvent> {
    let (tx, rx) = mpsc::channel(32);

    interval::spawn(cfg.interval_secs, tx.clone());

    if cfg.change_on_login {
        let tx = tx.clone();
        tokio::spawn(async move {
            let _ = tx.send(TriggerEvent::Login).await;
        });
    }

    if cfg.change_on_unlock {
        dbus::spawn_unlock_listener(tx);
    }

    rx
}
