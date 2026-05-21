use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{mpsc, oneshot},
};

use crate::history::HistoryEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientRequest {
    pub cmd: String,
}

impl ClientRequest {
    pub fn simple(cmd: &str) -> Self {
        Self {
            cmd: cmd.to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub paused: bool,
    pub current: Option<HistoryEntry>,
    pub next_change_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryResponse {
    pub entries: Vec<HistoryEntry>,
    pub cursor: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ClientResponse {
    Ok {
        message: String,
        status: Option<StatusResponse>,
    },
    History(HistoryResponse),
    Error {
        message: String,
    },
}

#[derive(Debug)]
pub enum DaemonCommand {
    Next,
    Prev,
    Pause,
    Resume,
    Status,
    Reload,
    History,
}

#[derive(Debug)]
pub struct CommandEnvelope {
    pub command: DaemonCommand,
    pub reply: oneshot::Sender<ClientResponse>,
}

pub fn socket_path() -> Result<PathBuf> {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .map(|dir| dir.join("awws.sock"))
        .ok_or_else(|| anyhow!("XDG_RUNTIME_DIR is not set"))
}

const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub async fn send_request(request: ClientRequest) -> Result<ClientResponse> {
    tokio::time::timeout(REQUEST_TIMEOUT, do_send_request(request))
        .await
        .context("timed out waiting for daemon response")?
}

async fn do_send_request(request: ClientRequest) -> Result<ClientResponse> {
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path)
        .await
        .with_context(|| format!("failed to connect to {}", path.display()))?;
    let raw = serde_json::to_vec(&request)?;
    stream.write_all(&raw).await?;
    stream.write_all(b"\n").await?;
    stream.shutdown().await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    serde_json::from_str(&line).context("daemon returned invalid JSON")
}

pub async fn serve(tx: mpsc::Sender<CommandEnvelope>) -> Result<()> {
    let path = socket_path()?;
    if path.exists() {
        let _ = tokio::fs::remove_file(&path).await;
    }
    let listener =
        UnixListener::bind(&path).with_context(|| format!("failed to bind {}", path.display()))?;
    tracing::info!(socket = %path.display(), "IPC server listening");

    loop {
        let (stream, _) = listener.accept().await?;
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Err(err) = handle_client(stream, tx).await {
                tracing::warn!(%err, "IPC client failed");
            }
        });
    }
}

async fn handle_client(stream: UnixStream, tx: mpsc::Sender<CommandEnvelope>) -> Result<()> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let request: ClientRequest = serde_json::from_str(&line)?;
    let command = parse_command(&request.cmd)?;
    let (reply_tx, reply_rx) = oneshot::channel();

    tx.send(CommandEnvelope {
        command,
        reply: reply_tx,
    })
    .await
    .context("daemon command loop is closed")?;

    let response = reply_rx.await.context("daemon dropped response")?;
    let raw = serde_json::to_vec(&response)?;
    let mut stream = reader.into_inner();
    stream.write_all(&raw).await?;
    stream.write_all(b"\n").await?;
    Ok(())
}

fn parse_command(cmd: &str) -> Result<DaemonCommand> {
    match cmd {
        "next" => Ok(DaemonCommand::Next),
        "prev" => Ok(DaemonCommand::Prev),
        "pause" => Ok(DaemonCommand::Pause),
        "resume" => Ok(DaemonCommand::Resume),
        "status" => Ok(DaemonCommand::Status),
        "reload" => Ok(DaemonCommand::Reload),
        "history" => Ok(DaemonCommand::History),
        other => Err(anyhow!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_all_valid_commands() {
        assert!(matches!(
            parse_command("next").unwrap(),
            DaemonCommand::Next
        ));
        assert!(matches!(
            parse_command("prev").unwrap(),
            DaemonCommand::Prev
        ));
        assert!(matches!(
            parse_command("pause").unwrap(),
            DaemonCommand::Pause
        ));
        assert!(matches!(
            parse_command("resume").unwrap(),
            DaemonCommand::Resume
        ));
        assert!(matches!(
            parse_command("status").unwrap(),
            DaemonCommand::Status
        ));
        assert!(matches!(
            parse_command("reload").unwrap(),
            DaemonCommand::Reload
        ));
        assert!(matches!(
            parse_command("history").unwrap(),
            DaemonCommand::History
        ));
    }

    #[test]
    fn parse_unknown_command_errors() {
        assert!(parse_command("unknown").is_err());
        assert!(parse_command("").is_err());
        assert!(parse_command("NEXT").is_err());
    }

    #[test]
    fn client_request_simple_roundtrip() {
        let req = ClientRequest::simple("next");
        assert_eq!(req.cmd, "next");
        let json = serde_json::to_string(&req).unwrap();
        let de: ClientRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(de.cmd, "next");
    }

    #[test]
    fn client_response_ok_has_correct_result_tag() {
        let resp = ClientResponse::Ok {
            message: "done".into(),
            status: None,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["result"], "ok");
        assert_eq!(v["message"], "done");
    }

    #[test]
    fn client_response_error_has_correct_result_tag() {
        let resp = ClientResponse::Error {
            message: "oops".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["result"], "error");
        assert_eq!(v["message"], "oops");
    }

    #[test]
    fn client_response_history_has_correct_result_tag() {
        let resp = ClientResponse::History(HistoryResponse {
            entries: vec![],
            cursor: None,
        });
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["result"], "history");
        assert_eq!(v["entries"], serde_json::json!([]));
        assert!(v["cursor"].is_null());
    }

    #[test]
    fn client_response_history_cursor_roundtrip() {
        let resp = ClientResponse::History(HistoryResponse {
            entries: vec![],
            cursor: Some(2),
        });
        let json = serde_json::to_string(&resp).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["result"], "history");
        assert_eq!(v["cursor"], 2);
    }

    #[test]
    fn socket_path_uses_xdg_runtime_dir() {
        unsafe { std::env::set_var("XDG_RUNTIME_DIR", "/run/user/1000") };
        let path = socket_path().unwrap();
        assert_eq!(path, PathBuf::from("/run/user/1000/awws.sock"));
    }
}
