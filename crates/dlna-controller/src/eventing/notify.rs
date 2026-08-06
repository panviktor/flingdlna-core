use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use dlna_core::{Error, Result};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{broadcast, RwLock};
use tracing::debug;

use super::parser::parse_and_emit_events;
use super::types::{Subscription, UpnpEvent};

const MAX_NOTIFY_HEADERS_BYTES: usize = 16 * 1024;
const MAX_NOTIFY_BODY_BYTES: usize = 256 * 1024;
const NOTIFY_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Handle incoming NOTIFY request
pub(super) async fn handle_notify(
    mut stream: TcpStream,
    _addr: SocketAddr,
    subscriptions: Arc<RwLock<HashMap<String, Subscription>>>,
    event_tx: broadcast::Sender<UpnpEvent>,
) -> Result<()> {
    let mut reader = BufReader::new(&mut stream);
    let mut content_length = 0usize;
    let mut sid = String::new();
    let mut headers_bytes = 0usize;

    // Read headers
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).await?;
        if n == 0 || line == "\r\n" {
            break;
        }

        headers_bytes = headers_bytes.saturating_add(n);
        if headers_bytes > MAX_NOTIFY_HEADERS_BYTES {
            drop(reader);
            let response =
                "HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\n\r\n";
            let _ = stream.write_all(response.as_bytes()).await;
            return Ok(());
        }

        if let Some(len) = line
            .strip_prefix("Content-Length:")
            .or_else(|| line.strip_prefix("CONTENT-LENGTH:"))
        {
            content_length = len.trim().parse().unwrap_or(0);
        }
        if let Some(s) = line
            .strip_prefix("SID:")
            .or_else(|| line.strip_prefix("sid:"))
        {
            sid = s.trim().to_string();
        }
    }

    if content_length > MAX_NOTIFY_BODY_BYTES {
        drop(reader);
        let response = "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\n\r\n";
        let _ = stream.write_all(response.as_bytes()).await;
        return Ok(());
    }

    let mut body = vec![0u8; content_length];
    tokio::time::timeout(NOTIFY_READ_TIMEOUT, reader.read_exact(&mut body))
        .await
        .map_err(|_| Error::Timeout)??;
    let body = String::from_utf8_lossy(&body);

    drop(reader);
    let response = "HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
    stream.write_all(response.as_bytes()).await?;

    debug!("Received NOTIFY for SID {} ({} bytes)", sid, content_length);

    let device_udn = {
        let subs = subscriptions.read().await;
        subs.get(&sid).map(|s| s.device_udn.clone())
    };

    let device_udn = match device_udn {
        Some(udn) => udn,
        None => {
            debug!("Unknown SID: {}", sid);
            return Ok(());
        }
    };

    parse_and_emit_events(&body, &device_udn, &event_tx);

    Ok(())
}
