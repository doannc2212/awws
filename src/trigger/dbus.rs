use super::TriggerEvent;
use futures_util::StreamExt;
use tokio::sync::mpsc;
use zbus::{Connection, MatchRule, MessageStream};

pub fn spawn_unlock_listener(tx: mpsc::Sender<TriggerEvent>) {
    tokio::spawn(async move {
        if let Err(err) = listen(tx).await {
            tracing::warn!(%err, "unlock trigger disabled");
        }
    });
}

async fn listen(tx: mpsc::Sender<TriggerEvent>) -> zbus::Result<()> {
    let connection = Connection::system().await?;
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.login1.Session")?
        .member("Unlock")?
        .build();
    let mut stream = MessageStream::for_match_rule(rule, &connection, Some(8)).await?;

    while let Some(message) = stream.next().await {
        let message = message?;
        let header = message.header();
        if header
            .member()
            .map(|member| member.as_str() == "Unlock")
            .unwrap_or(false)
            && tx.send(TriggerEvent::Unlock).await.is_err()
        {
            break;
        }
    }

    Ok(())
}
