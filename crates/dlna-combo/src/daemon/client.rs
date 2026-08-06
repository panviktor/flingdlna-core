//! Daemon client for IPC communication

use super::config::default_socket_path;
use crate::protocol::{Command, RendererInfo, Response, ResponseData};
use std::path::PathBuf;

/// Client for communicating with the daemon
pub struct DaemonClient {
    socket_path: PathBuf,
}

impl DaemonClient {
    fn token() -> Option<String> {
        std::env::var("FLINGDLNA_DAEMON_TOKEN")
            .ok()
            .filter(|s| !s.is_empty())
    }

    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn default_path() -> Self {
        Self::new(default_socket_path())
    }

    /// Check if daemon is running
    pub fn is_daemon_running(&self) -> bool {
        self.socket_path.exists()
    }

    /// Send a command and get response
    pub async fn send(&self, cmd: &Command) -> dlna_core::Result<Response> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::UnixStream;

        let stream = UnixStream::connect(&self.socket_path).await.map_err(|e| {
            dlna_core::Error::Network(format!(
                "Cannot connect to daemon at {:?}: {}",
                self.socket_path, e
            ))
        })?;

        let (reader, mut writer) = stream.into_split();

        // Send command
        let mut value = serde_json::to_value(cmd)
            .map_err(|e| dlna_core::Error::Network(format!("Serialization error: {e}")))?;
        if let (Some(token), Some(obj)) = (Self::token(), value.as_object_mut()) {
            obj.insert("token".to_string(), serde_json::Value::String(token));
        }
        let cmd_json = serde_json::to_string(&value)
            .map_err(|e| dlna_core::Error::Network(format!("Serialization error: {e}")))?;

        writer.write_all(cmd_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: Response = serde_json::from_str(&line)
            .map_err(|e| dlna_core::Error::Network(format!("Invalid response: {e}")))?;

        Ok(response)
    }

    /// Convenience methods
    pub async fn ping(&self) -> dlna_core::Result<bool> {
        match self.send(&Command::Ping).await? {
            Response::Ok {
                data: Some(ResponseData::Pong),
            } => Ok(true),
            _ => Ok(false),
        }
    }

    pub async fn list(&self, timeout_secs: u64) -> dlna_core::Result<Vec<RendererInfo>> {
        match self.send(&Command::List { timeout_secs }).await? {
            Response::Ok {
                data: Some(ResponseData::Renderers { renderers }),
            } => Ok(renderers),
            Response::Error { message, .. } => Err(dlna_core::Error::Network(message)),
            _ => Err(dlna_core::Error::Network("Unexpected response".into())),
        }
    }

    pub async fn shutdown(&self) -> dlna_core::Result<()> {
        self.send(&Command::Shutdown).await?;
        Ok(())
    }
}

/// Check if a daemon is already running at the given path
pub fn is_daemon_running(socket_path: &std::path::Path) -> bool {
    if !socket_path.exists() {
        return false;
    }

    // Try to connect to verify it's actually running
    std::os::unix::net::UnixStream::connect(socket_path).is_ok()
}
