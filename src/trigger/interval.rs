use super::TriggerEvent;
use tokio::{sync::mpsc, time};

pub fn spawn(interval_secs: u64, tx: mpsc::Sender<TriggerEvent>) {
    tokio::spawn(async move {
        let mut ticker = time::interval(time::Duration::from_secs(interval_secs.max(1)));
        loop {
            ticker.tick().await;
            ticker.tick().await;
            if tx.send(TriggerEvent::Interval).await.is_err() {
                break;
            }
        }
    });
}
