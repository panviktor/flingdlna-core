//! Local file streaming server for pushing files to renderers

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use dlna_core::{best_local_ipv4, Error, Result};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{error, info};

use crate::subtitles::{find_subtitle_for_video, subtitle_mime_type};

const MAX_STREAM_BODY_BYTES: usize = 8 * 1024;
const MAX_STREAM_HEADER_BYTES: usize = 16 * 1024;
const MAX_STREAM_CONCURRENCY: usize = 32;

async fn limit_headers(req: axum::http::Request<Body>, next: Next) -> Response {
    let mut total = 0usize;
    for (name, value) in req.headers().iter() {
        total = total.saturating_add(name.as_str().len());
        total = total.saturating_add(value.as_bytes().len());
        if total > MAX_STREAM_HEADER_BYTES {
            return (
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                "Request headers too large",
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Simple HTTP server for streaming a local file to a renderer
pub struct StreamingServer {
    port: u16,
    _file_path: PathBuf,
    shutdown_tx: broadcast::Sender<()>,
}

impl StreamingServer {
    /// Start the streaming server and return the URL to the file
    /// Returns (Self, video_url, optional_subtitle_url)
    pub async fn start(file_path: PathBuf, port: u16) -> Result<(Self, String, Option<String>)> {
        if !file_path.exists() {
            return Err(Error::FileNotFound(file_path));
        }

        let (shutdown_tx, _) = broadcast::channel::<()>(1);
        let shutdown_rx = shutdown_tx.subscribe();

        // Get local IP for the URL
        let local_ip = IpAddr::V4(best_local_ipv4()?);

        let _filename = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("media")
            .to_string();

        let mime = mime_guess::from_path(&file_path)
            .first()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let file_size = tokio::fs::metadata(&file_path).await?.len();

        // Find subtitle file
        let subtitle_path = find_subtitle_for_video(&file_path);
        let subtitle_mime = subtitle_path
            .as_ref()
            .map(|p| subtitle_mime_type(p).to_string());

        if let Some(ref sub_path) = subtitle_path {
            info!("Found subtitle: {}", sub_path.display());
        }

        // Shared state for the handler
        let state = Arc::new(StreamingState {
            file_path: file_path.clone(),
            mime_type: mime,
            file_size,
            subtitle_path: subtitle_path.clone(),
            subtitle_mime,
        });

        let mut app = Router::new().route("/media", get(serve_file).head(serve_head));

        // Add subtitle route if subtitle exists
        if subtitle_path.is_some() {
            app = app.route("/subtitle", get(serve_subtitle));
        }

        let app = app.with_state(state);
        let app = app
            .layer(middleware::from_fn(limit_headers))
            .layer(RequestBodyLimitLayer::new(MAX_STREAM_BODY_BYTES))
            .layer(ConcurrencyLimitLayer::new(MAX_STREAM_CONCURRENCY));

        let bind_addr = SocketAddr::new(local_ip, port);

        // Create socket with SO_REUSEADDR to allow immediate port reuse
        let socket = Socket::new(
            Domain::for_address(bind_addr),
            Type::STREAM,
            Some(Protocol::TCP),
        )
        .map_err(Error::Io)?;
        socket.set_reuse_address(true).map_err(Error::Io)?;
        #[cfg(unix)]
        socket.set_reuse_port(true).map_err(Error::Io)?;
        socket.set_nonblocking(true).map_err(Error::Io)?;
        socket.bind(&bind_addr.into()).map_err(Error::Io)?;
        socket.listen(128).map_err(Error::Io)?;

        let listener = tokio::net::TcpListener::from_std(socket.into()).map_err(Error::Io)?;
        let actual_port = listener.local_addr()?.port();
        info!("Streaming server started on {}", listener.local_addr()?);

        // Spawn the server
        tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async move {
                let mut rx = shutdown_rx;
                let _ = rx.recv().await;
                info!("Streaming server shutting down");
            })
            .await
            .ok();
        });

        let url_host = match local_ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => format!("[{ip}]"),
        };
        let video_url = format!("http://{url_host}:{actual_port}/media");
        let subtitle_url =
            subtitle_path.map(|_| format!("http://{url_host}:{actual_port}/subtitle"));

        Ok((
            Self {
                port: actual_port,
                _file_path: file_path,
                shutdown_tx,
            },
            video_url,
            subtitle_url,
        ))
    }

    /// Stop the streaming server
    pub fn stop(&self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Get the port
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for StreamingServer {
    fn drop(&mut self) {
        self.stop();
    }
}

#[derive(Clone)]
struct StreamingState {
    file_path: PathBuf,
    mime_type: String,
    file_size: u64,
    subtitle_path: Option<PathBuf>,
    subtitle_mime: Option<String>,
}

/// Serve the file with Range request support
async fn serve_file(
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    axum::extract::State(state): axum::extract::State<Arc<StreamingState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    info!(
        "Streaming request from {} range={} ua={}",
        remote, range_header, user_agent
    );

    // Check for Range header
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range_header(s, state.file_size));

    match range {
        Some((start, end)) => serve_partial(&state, start, end).await,
        None => serve_full(&state).await,
    }
}

async fn serve_head(
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    axum::extract::State(state): axum::extract::State<Arc<StreamingState>>,
    headers: axum::http::HeaderMap,
) -> Response {
    let range_header = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    info!(
        "Streaming HEAD from {} range={} ua={}",
        remote, range_header, user_agent
    );

    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range_header(s, state.file_size));

    match range {
        Some((start, end)) => head_partial(&state, start, end),
        None => head_full(&state),
    }
}

fn head_full(state: &StreamingState) -> Response {
    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &state.mime_type)
        .header(header::CONTENT_LENGTH, state.file_size)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            "contentFeatures.dlna.org",
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        )
        .header("transferMode.dlna.org", "Streaming")
        .body(Body::empty());

    match resp {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to build response: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            )
                .into_response()
        }
    }
}

fn head_partial(state: &StreamingState, start: u64, end: u64) -> Response {
    let length = end - start + 1;
    let content_range = format!("bytes {}-{}/{}", start, end, state.file_size);

    let resp = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, &state.mime_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::CONTENT_RANGE, content_range)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            "contentFeatures.dlna.org",
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        )
        .header("transferMode.dlna.org", "Streaming")
        .body(Body::empty());

    match resp {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to build response: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            )
                .into_response()
        }
    }
}

async fn serve_full(state: &StreamingState) -> Response {
    let file = match File::open(&state.file_path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open file: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
        }
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, &state.mime_type)
        .header(header::CONTENT_LENGTH, state.file_size)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            "contentFeatures.dlna.org",
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        )
        .header("transferMode.dlna.org", "Streaming")
        .body(body);

    match resp {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to build response: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            )
                .into_response()
        }
    }
}

async fn serve_partial(state: &StreamingState, start: u64, end: u64) -> Response {
    let mut file = match File::open(&state.file_path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open file: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
        }
    };

    if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
        error!("Failed to seek: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Seek failed").into_response();
    }

    let length = end - start + 1;
    let file = file.take(length);
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let content_range = format!("bytes {}-{}/{}", start, end, state.file_size);

    let resp = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, &state.mime_type)
        .header(header::CONTENT_LENGTH, length)
        .header(header::CONTENT_RANGE, content_range)
        .header(header::ACCEPT_RANGES, "bytes")
        .header(
            "contentFeatures.dlna.org",
            "DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000",
        )
        .header("transferMode.dlna.org", "Streaming")
        .body(body);

    match resp {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to build response: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            )
                .into_response()
        }
    }
}

/// Serve subtitle file
async fn serve_subtitle(
    axum::extract::State(state): axum::extract::State<Arc<StreamingState>>,
) -> Response {
    let subtitle_path = match &state.subtitle_path {
        Some(p) => p,
        None => {
            return (StatusCode::NOT_FOUND, "No subtitle available").into_response();
        }
    };

    let (file, size) = match (
        File::open(subtitle_path).await,
        tokio::fs::metadata(subtitle_path).await,
    ) {
        (Ok(f), Ok(m)) => (f, m.len()),
        (Err(e), _) | (_, Err(e)) => {
            error!("Failed to open subtitle: {}", e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open subtitle").into_response();
        }
    };

    let mime = state.subtitle_mime.as_deref().unwrap_or("text/plain");

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, size)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .body(body);

    match resp {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to build response: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to build response",
            )
                .into_response()
        }
    }
}

fn parse_range_header(header: &str, file_size: u64) -> Option<(u64, u64)> {
    if !header.starts_with("bytes=") {
        return None;
    }
    if file_size == 0 {
        return None;
    }

    let range_spec = &header[6..];
    let parts: Vec<&str> = range_spec.split('-').collect();

    if parts.len() != 2 {
        return None;
    }

    let is_suffix_range = parts[0].is_empty();

    let start: u64 = if is_suffix_range {
        // Suffix range: -500 means last 500 bytes
        let suffix: u64 = parts[1].parse().ok()?;
        file_size.saturating_sub(suffix)
    } else {
        parts[0].parse().ok()?
    };

    let end: u64 = if is_suffix_range || parts[1].is_empty() {
        // For suffix range (-N) or open-ended range (start-), end is file_size - 1
        file_size - 1
    } else {
        parts[1].parse().ok()?
    };

    if start > end || start >= file_size {
        return None;
    }

    let end = end.min(file_size - 1);
    Some((start, end))
}
