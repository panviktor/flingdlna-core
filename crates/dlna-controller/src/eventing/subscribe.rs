use std::time::{Duration, Instant};

use dlna_core::{Error, Renderer, Result};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};
use url::Url;

use super::manager::EventManager;
use super::types::{Subscription, UpnpEvent};

const SUBSCRIBE_CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const SUBSCRIBE_READ_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SUBSCRIBE_HEADERS_BYTES: usize = 16 * 1024;

pub(super) async fn subscribe(
    manager: &EventManager,
    renderer: &Renderer,
    local_ip: &str,
) -> Result<()> {
    let callback = manager.callback_url(local_ip);

    // Subscribe to RenderingControl (volume, mute)
    if let Some(ref event_url) = renderer.rendering_control_event_url {
        match subscribe_to_service(
            manager,
            event_url,
            &callback,
            &renderer.udn,
            "RenderingControl",
        )
        .await
        {
            Ok(()) => info!(
                "Subscribed to RenderingControl events for {}",
                renderer.friendly_name
            ),
            Err(e) => warn!("Failed to subscribe to RenderingControl: {}", e),
        }
    }

    // Subscribe to AVTransport (transport state, current URI)
    if let Some(ref event_url) = renderer.av_transport_event_url {
        match subscribe_to_service(manager, event_url, &callback, &renderer.udn, "AVTransport")
            .await
        {
            Ok(()) => info!(
                "Subscribed to AVTransport events for {}",
                renderer.friendly_name
            ),
            Err(e) => warn!("Failed to subscribe to AVTransport: {}", e),
        }
    }

    Ok(())
}

pub(super) async fn unsubscribe(manager: &EventManager, renderer: &Renderer) -> Result<()> {
    let subs = manager.subscriptions.read().await;
    let to_unsub: Vec<_> = subs
        .values()
        .filter(|s| s.device_udn == renderer.udn)
        .cloned()
        .collect();
    drop(subs);

    for sub in to_unsub {
        if let Err(e) = unsubscribe_service(sub.event_url.clone(), &sub.sid).await {
            warn!("Failed to unsubscribe {}: {}", sub.service, e);
        }
        manager.subscriptions.write().await.remove(&sub.sid);
    }

    Ok(())
}

pub(super) async fn renew_expiring(manager: &EventManager, local_ip: &str) -> Result<()> {
    let now = Instant::now();
    let callback = manager.callback_url(local_ip);

    let subs = manager.subscriptions.read().await;
    let to_renew: Vec<_> = subs
        .values()
        .filter(|s| s.expires_at <= now)
        .cloned()
        .collect();
    drop(subs);

    for sub in to_renew {
        match renew_subscription(manager, &sub, &callback).await {
            Ok(()) => debug!(
                "Renewed subscription for {} {}",
                sub.device_udn, sub.service
            ),
            Err(e) => {
                warn!("Failed to renew subscription: {}", e);
                let _ = manager.event_tx.send(UpnpEvent::SubscriptionLost {
                    device_udn: sub.device_udn.clone(),
                    service: sub.service.clone(),
                });
                manager.subscriptions.write().await.remove(&sub.sid);
            }
        }
    }

    Ok(())
}

async fn subscribe_to_service(
    manager: &EventManager,
    event_url: &Url,
    callback: &str,
    device_udn: &str,
    service: &str,
) -> Result<()> {
    let host = event_url
        .host_str()
        .ok_or_else(|| Error::Network("No host in URL".into()))?;
    let port = event_url.port().unwrap_or(80);
    let path = event_url.path();

    let addr = format!("{host}:{port}");
    let mut stream = tokio::time::timeout(SUBSCRIBE_CONNECT_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| Error::Timeout)??;

    let request = format!(
        "SUBSCRIBE {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         CALLBACK: <{callback}>\r\n\
         NT: upnp:event\r\n\
         TIMEOUT: Second-1800\r\n\
         \r\n"
    );

    stream.write_all(request.as_bytes()).await?;

    let response = read_subscribe_response(stream, "SUBSCRIBE response headers too large").await?;

    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Err(Error::Network(format!(
            "SUBSCRIBE failed: {}",
            response.lines().next().unwrap_or("Unknown error")
        )));
    }

    let sid = extract_header(&response, "SID")
        .ok_or_else(|| Error::Network("No SID in SUBSCRIBE response".into()))?;

    let timeout_secs = extract_header(&response, "TIMEOUT")
        .and_then(|t| t.strip_prefix("Second-").map(|s| s.to_string()))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1800);

    let timeout = Duration::from_secs(timeout_secs);
    let expires_at = Instant::now() + timeout - Duration::from_secs(60);

    let subscription = Subscription {
        sid: sid.clone(),
        device_udn: device_udn.to_string(),
        service: service.to_string(),
        event_url: event_url.clone(),
        expires_at,
        timeout,
    };

    manager
        .subscriptions
        .write()
        .await
        .insert(sid, subscription);

    debug!(
        "Subscription created: {} for {} (timeout: {}s)",
        service, device_udn, timeout_secs
    );

    Ok(())
}

async fn unsubscribe_service(event_url: Url, sid: &str) -> Result<()> {
    let host = event_url
        .host_str()
        .ok_or_else(|| Error::Network("No host".into()))?;
    let port = event_url.port().unwrap_or(80);
    let path = event_url.path();

    let addr = format!("{host}:{port}");
    let mut stream = tokio::time::timeout(SUBSCRIBE_CONNECT_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| Error::Timeout)??;

    let request = format!(
        "UNSUBSCRIBE {path} HTTP/1.1\r\n\
         Host: {host}:{port}\r\n\
         SID: {sid}\r\n\
         \r\n"
    );

    stream.write_all(request.as_bytes()).await?;

    debug!("Unsubscribed from {}", sid);

    Ok(())
}

async fn renew_subscription(
    manager: &EventManager,
    sub: &Subscription,
    _callback: &str,
) -> Result<()> {
    let host = sub
        .event_url
        .host_str()
        .ok_or_else(|| Error::Network("No host".into()))?;
    let port = sub.event_url.port().unwrap_or(80);
    let path = sub.event_url.path();

    let addr = format!("{host}:{port}");
    let mut stream = tokio::time::timeout(SUBSCRIBE_CONNECT_TIMEOUT, TcpStream::connect(&addr))
        .await
        .map_err(|_| Error::Timeout)??;

    let request = format!(
        "SUBSCRIBE {} HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         SID: {}\r\n\
         TIMEOUT: Second-1800\r\n\
         \r\n",
        path, host, port, sub.sid
    );

    stream.write_all(request.as_bytes()).await?;

    let response =
        read_subscribe_response(stream, "SUBSCRIBE renewal response headers too large").await?;

    if !response.starts_with("HTTP/1.1 200") && !response.starts_with("HTTP/1.0 200") {
        return Err(Error::Network(format!(
            "Renewal failed: {}",
            response.lines().next().unwrap_or("")
        )));
    }

    let timeout_secs = extract_header(&response, "TIMEOUT")
        .and_then(|t| t.strip_prefix("Second-").map(|s| s.to_string()))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(1800);

    let mut subs = manager.subscriptions.write().await;
    if let Some(s) = subs.get_mut(&sub.sid) {
        s.expires_at = Instant::now() + Duration::from_secs(timeout_secs) - Duration::from_secs(60);
    }

    Ok(())
}

async fn read_subscribe_response(stream: TcpStream, too_large_msg: &str) -> Result<String> {
    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    let mut read_bytes = 0usize;
    loop {
        let mut line = String::new();
        let n = tokio::time::timeout(SUBSCRIBE_READ_TIMEOUT, reader.read_line(&mut line))
            .await
            .map_err(|_| Error::Timeout)??;
        if n == 0 || line == "\r\n" {
            break;
        }
        read_bytes = read_bytes.saturating_add(n);
        if read_bytes > MAX_SUBSCRIBE_HEADERS_BYTES {
            return Err(Error::InvalidResponse(too_large_msg.into()));
        }
        response.push_str(&line);
    }

    Ok(response)
}

/// Extract header value from HTTP response
fn extract_header(response: &str, header: &str) -> Option<String> {
    for line in response.lines() {
        let lower = line.to_lowercase();
        let header_lower = format!("{}:", header.to_lowercase());
        if lower.starts_with(&header_lower) {
            return Some(line[header.len() + 1..].trim().to_string());
        }
    }
    None
}
