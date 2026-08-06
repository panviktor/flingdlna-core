//! HTTP server for DLNA Media Server

use crate::content_directory::{
    get_current_connection_ids_response, get_protocol_info_response,
    get_search_capabilities_response, get_sort_capabilities_response,
    get_system_update_id_response, handle_browse, parse_browse_request,
};
use crate::description::{
    generate_connection_manager_scpd, generate_content_directory_scpd, generate_device_description,
};
use crate::state::ServerState;
use axum::{
    body::Body,
    body::Bytes,
    extract::{ConnectInfo, Path, State},
    http::{header, HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use dlna_core::{Result, SsdpService};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::sync::broadcast;
use tokio_util::io::ReaderStream;
use tower::limit::ConcurrencyLimitLayer;
use tower::timeout::TimeoutLayer;
use tower::{BoxError, ServiceBuilder};
use tower_http::limit::RequestBodyLimitLayer;
use tracing::{debug, error, info, warn};

const MAX_HTTP_BODY_BYTES: usize = 256 * 1024;
const MAX_HTTP_HEADER_BYTES: usize = 32 * 1024;
const MAX_HTTP_CONCURRENCY: usize = 64;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(15);

async fn limit_headers(req: axum::http::Request<Body>, next: Next) -> Response {
    let mut total = 0usize;
    for (name, value) in req.headers().iter() {
        total = total.saturating_add(name.as_str().len());
        total = total.saturating_add(value.as_bytes().len());
        if total > MAX_HTTP_HEADER_BYTES {
            return (
                StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
                "Request headers too large",
            )
                .into_response();
        }
    }
    next.run(req).await
}

/// Shared state for HTTP handlers
#[derive(Clone)]
struct AppState {
    server_state: Arc<ServerState>,
    ssdp: Arc<SsdpService>,
}

/// Run the HTTP server
pub async fn run_server(
    state: Arc<ServerState>,
    ssdp: Arc<SsdpService>,
    port: u16,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    let bind_ip = server_bind_ip(ssdp.local_ip());

    let app_state = AppState {
        server_state: state,
        ssdp,
    };

    let control = Router::new()
        .route("/ContentDirectory/control", post(content_directory_control))
        .route(
            "/ConnectionManager/control",
            post(connection_manager_control),
        )
        .layer(
            ServiceBuilder::new()
                .layer(axum::error_handling::HandleErrorLayer::new(
                    |err: BoxError| async move {
                        error!("Control request failed: {}", err);
                        (StatusCode::REQUEST_TIMEOUT, "Request timed out").into_response()
                    },
                ))
                .layer(TimeoutLayer::new(CONTROL_TIMEOUT)),
        );

    let app = Router::new()
        // Device description
        .route("/description.xml", get(description_handler))
        // ContentDirectory service
        .route(
            "/ContentDirectory/scpd.xml",
            get(content_directory_scpd_handler),
        )
        .route("/ContentDirectory/event", get(event_subscribe_handler))
        // ConnectionManager service
        .route(
            "/ConnectionManager/scpd.xml",
            get(connection_manager_scpd_handler),
        )
        .route("/ConnectionManager/event", get(event_subscribe_handler))
        .merge(control)
        // Media streaming
        .route("/media/{id}", get(serve_media))
        .layer(middleware::from_fn(limit_headers))
        .layer(RequestBodyLimitLayer::new(MAX_HTTP_BODY_BYTES))
        .layer(ConcurrencyLimitLayer::new(MAX_HTTP_CONCURRENCY))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind(SocketAddr::new(bind_ip, port)).await?;
    info!("HTTP server listening on {}", listener.local_addr()?);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = shutdown.recv().await;
        info!("HTTP server shutting down");
    })
    .await?;

    Ok(())
}

/// Run the HTTP server with a pre-bound listener
/// This is used when the port is auto-selected (0) to get the actual port first
pub async fn run_server_with_listener(
    state: Arc<ServerState>,
    ssdp: Arc<SsdpService>,
    listener: tokio::net::TcpListener,
    mut shutdown: broadcast::Receiver<()>,
) -> Result<()> {
    let app_state = AppState {
        server_state: state,
        ssdp,
    };

    let control = Router::new()
        .route("/ContentDirectory/control", post(content_directory_control))
        .route(
            "/ConnectionManager/control",
            post(connection_manager_control),
        )
        .layer(
            ServiceBuilder::new()
                .layer(axum::error_handling::HandleErrorLayer::new(
                    |err: BoxError| async move {
                        error!("Control request failed: {}", err);
                        (StatusCode::REQUEST_TIMEOUT, "Request timed out").into_response()
                    },
                ))
                .layer(TimeoutLayer::new(CONTROL_TIMEOUT)),
        );

    let app = Router::new()
        // Device description
        .route("/description.xml", get(description_handler))
        // ContentDirectory service
        .route(
            "/ContentDirectory/scpd.xml",
            get(content_directory_scpd_handler),
        )
        .route("/ContentDirectory/event", get(event_subscribe_handler))
        // ConnectionManager service
        .route(
            "/ConnectionManager/scpd.xml",
            get(connection_manager_scpd_handler),
        )
        .route("/ConnectionManager/event", get(event_subscribe_handler))
        .merge(control)
        // Media streaming
        .route("/media/{id}", get(serve_media))
        .layer(middleware::from_fn(limit_headers))
        .layer(RequestBodyLimitLayer::new(MAX_HTTP_BODY_BYTES))
        .layer(ConcurrencyLimitLayer::new(MAX_HTTP_CONCURRENCY))
        .with_state(app_state);

    let addr = listener.local_addr()?;
    info!("HTTP server listening on {}", addr);

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let _ = shutdown.recv().await;
        info!("HTTP server shutting down");
    })
    .await?;

    Ok(())
}

fn server_bind_ip(default_ip: IpAddr) -> IpAddr {
    match std::env::var("FLINGDLNA_SERVER_BIND").ok().as_deref() {
        Some(v) if !v.trim().is_empty() => v.trim().parse().unwrap_or(default_ip),
        _ => default_ip,
    }
}

/// Device description XML handler
async fn description_handler(State(state): State<AppState>) -> impl IntoResponse {
    let info = state.server_state.server_info();
    let base_url = state.ssdp.base_url();
    let xml = generate_device_description(&info, &base_url);

    ([(header::CONTENT_TYPE, "text/xml; charset=utf-8")], xml)
}

/// ContentDirectory SCPD handler
async fn content_directory_scpd_handler() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        generate_content_directory_scpd(),
    )
}

/// ConnectionManager SCPD handler
async fn connection_manager_scpd_handler() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        generate_connection_manager_scpd(),
    )
}

/// ContentDirectory control handler
async fn content_directory_control(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if body.len() > MAX_HTTP_BODY_BYTES {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
            "Request too large",
        )
            .into_response();
    }

    let soap_action = headers
        .get("SOAPACTION")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // Extract client IP for content filtering
    let client_ip = addr.ip().to_string();
    debug!(
        "ContentDirectory SOAP action: {}, client: {}",
        soap_action, client_ip
    );

    let body_str = match std::str::from_utf8(&body) {
        Ok(s) => s,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                "Invalid request body encoding",
            )
                .into_response();
        }
    };

    let response = if soap_action.contains("Browse") {
        match parse_browse_request(body_str) {
            Ok(request) => {
                let base_url = state.ssdp.base_url();
                match handle_browse(&state.server_state, &request, &base_url, Some(&client_ip)) {
                    Ok(response) => response.to_soap_response(),
                    Err(e) => {
                        error!("Browse error: {}", e);
                        soap_error_response(501, "Action Failed")
                    }
                }
            }
            Err(e) => {
                error!("Failed to parse browse request: {}", e);
                soap_error_response(402, "Invalid Args")
            }
        }
    } else if soap_action.contains("GetSystemUpdateID") {
        get_system_update_id_response(state.server_state.content_update_id())
    } else if soap_action.contains("GetSearchCapabilities") {
        get_search_capabilities_response().to_string()
    } else if soap_action.contains("GetSortCapabilities") {
        get_sort_capabilities_response().to_string()
    } else {
        warn!("Unknown SOAP action: {}", soap_action);
        soap_error_response(401, "Invalid Action")
    };

    (
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        response,
    )
        .into_response()
}

/// ConnectionManager control handler
async fn connection_manager_control(headers: HeaderMap) -> impl IntoResponse {
    let soap_action = headers
        .get("SOAPACTION")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    debug!("ConnectionManager SOAP action: {}", soap_action);

    let response = if soap_action.contains("GetProtocolInfo") {
        get_protocol_info_response().to_string()
    } else if soap_action.contains("GetCurrentConnectionIDs") {
        get_current_connection_ids_response().to_string()
    } else {
        soap_error_response(401, "Invalid Action")
    };

    (
        [(header::CONTENT_TYPE, "text/xml; charset=utf-8")],
        response,
    )
}

/// Event subscription handler (stub)
async fn event_subscribe_handler() -> impl IntoResponse {
    StatusCode::NOT_IMPLEMENTED
}

/// Serve media file with Range request support
async fn serve_media(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let file = match state.server_state.get_file(&id) {
        Some(f) => f,
        None => {
            return (StatusCode::NOT_FOUND, "File not found").into_response();
        }
    };

    let path = &file.path;
    let size = file.size;
    let mime = &file.mime_type;

    // Check for Range header
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range_header(s, size));

    match range {
        Some((start, end)) => {
            // Partial content response
            serve_partial_file(path, mime, start, end, size).await
        }
        None => {
            // Full file response
            serve_full_file(path, mime, size).await
        }
    }
}

/// Parse Range header (format: bytes=start-end or bytes=start- or bytes=-suffix)
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

    // Validate range
    if start > end || start >= file_size {
        return None;
    }

    let end = end.min(file_size - 1);
    Some((start, end))
}

/// Serve full file
async fn serve_full_file(path: &std::path::Path, mime: &str, size: u64) -> Response {
    let file = match File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open file {:?}: {}", path, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
        }
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let resp = Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, size)
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

/// Serve partial file (Range request)
async fn serve_partial_file(
    path: &std::path::Path,
    mime: &str,
    start: u64,
    end: u64,
    total_size: u64,
) -> Response {
    let mut file = match File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to open file {:?}: {}", path, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to open file").into_response();
        }
    };

    // Seek to start position
    if let Err(e) = file.seek(std::io::SeekFrom::Start(start)).await {
        error!("Failed to seek in file: {}", e);
        return (StatusCode::INTERNAL_SERVER_ERROR, "Seek failed").into_response();
    }

    let length = end - start + 1;
    let file = file.take(length);
    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let content_range = format!("bytes {start}-{end}/{total_size}");

    let resp = Response::builder()
        .status(StatusCode::PARTIAL_CONTENT)
        .header(header::CONTENT_TYPE, mime)
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

/// Generate SOAP error response
fn soap_error_response(code: u32, description: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
  <s:Body>
    <s:Fault>
      <faultcode>s:Client</faultcode>
      <faultstring>UPnPError</faultstring>
      <detail>
        <UPnPError xmlns="urn:schemas-upnp-org:control-1-0">
          <errorCode>{code}</errorCode>
          <errorDescription>{description}</errorDescription>
        </UPnPError>
      </detail>
    </s:Fault>
  </s:Body>
</s:Envelope>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_range_header() {
        let size = 1000;

        // Standard range
        assert_eq!(parse_range_header("bytes=0-499", size), Some((0, 499)));

        // Open-ended range
        assert_eq!(parse_range_header("bytes=500-", size), Some((500, 999)));

        // Suffix range
        assert_eq!(parse_range_header("bytes=-100", size), Some((900, 999)));

        // Invalid
        assert_eq!(parse_range_header("bytes=1000-", size), None);
        assert_eq!(parse_range_header("invalid", size), None);
    }
}
