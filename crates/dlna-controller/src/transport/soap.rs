use std::time::Duration;

use dlna_core::{Error, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, trace, warn};
use url::Url;

use super::config::get_soap_config;
use super::xml::extract_xml_element;

/// Send a SOAP action to a control URL with retry logic and timeout
pub(crate) async fn send_soap_action(
    url: &Url,
    action: &str,
    body: &str,
    service_type: &str,
) -> Result<String> {
    let config = get_soap_config();
    send_soap_with_retry(url, action, body, service_type, config).await
}

/// Send a SOAP action with configurable retry and timeout
async fn send_soap_with_retry(
    url: &Url,
    action: &str,
    body: &str,
    service: &str,
    config: &super::config::SoapConfig,
) -> Result<String> {
    let mut last_error: Option<Error> = None;

    for attempt in 0..=config.max_retries {
        let result = tokio::time::timeout(
            config.timeout,
            send_soap_action_raw(url, action, body, service),
        )
        .await;

        let err = match result {
            Ok(Ok(response)) => return Ok(response),
            Ok(Err(e)) => e,
            Err(_) => Error::Timeout,
        };

        let retryable = soap_error_retryable(&err);
        last_error = Some(err);

        if attempt < config.max_retries && retryable {
            // Calculate exponential backoff with jitter
            let exp_delay = config.base_delay * (2_u32.pow(attempt));
            let jitter_ms = fastrand_u64(0..exp_delay.as_millis() as u64 / 4 + 1);
            let delay = exp_delay + Duration::from_millis(jitter_ms);

            warn!(
                "SOAP action '{}' failed (attempt {}/{}), retrying in {:?}: {:?}",
                action,
                attempt + 1,
                config.max_retries + 1,
                delay,
                last_error
            );

            tokio::time::sleep(delay).await;
        } else {
            if retryable {
                debug!(
                    "SOAP action '{}' failed (attempt {}/{}), no retries left: {:?}",
                    action,
                    attempt + 1,
                    config.max_retries + 1,
                    last_error
                );
            } else {
                debug!(
                    "SOAP action '{}' failed (attempt {}/{}), not retryable: {:?}",
                    action,
                    attempt + 1,
                    config.max_retries + 1,
                    last_error
                );
            }
            break;
        }
    }

    Err(last_error.unwrap_or(Error::Timeout))
}

fn soap_error_retryable(err: &Error) -> bool {
    match err {
        Error::Timeout => true,
        Error::Network(_) | Error::Ssdp(_) => true,
        Error::Io(e) => matches!(
            e.kind(),
            std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::ConnectionRefused
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::BrokenPipe
        ),
        Error::Http(msg) => {
            // Check UPnP error codes for retryable errors
            if let Some(upnp_code) = parse_upnp_error_code(err) {
                // 701: Transition not available (device is transitioning states)
                // 718: Invalid InstanceID (device not ready)
                return matches!(upnp_code, 701 | 718);
            }

            // Check HTTP status codes
            // Most SOAP faults come back as 500 and are not transient.
            match parse_http_status_from_error_message(msg) {
                Some(408 | 429 | 502 | 503 | 504) => true,
                Some(_) => false,
                None => false,
            }
        }
        _ => false,
    }
}

fn parse_http_status_from_error_message(msg: &str) -> Option<u16> {
    let msg = msg.strip_prefix("HTTP ")?;
    let code = msg.split(|c: char| c == ':' || c.is_whitespace()).next()?;
    code.parse().ok()
}

fn parse_upnp_error_code(err: &Error) -> Option<u16> {
    if let Error::Http(msg) = err {
        // Format: "HTTP 500: ... UPnP error 701 ..." (see format_soap_http_error line 241)
        if msg.contains("UPnP error ") {
            return msg
                .split("UPnP error ")
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|code| code.parse().ok());
        }
    }
    None
}

/// Internal: Send SOAP action without retry (raw implementation)
async fn send_soap_action_raw(
    url: &Url,
    action: &str,
    body: &str,
    service: &str,
) -> Result<String> {
    let host = url
        .host_str()
        .ok_or_else(|| Error::Network("No host in URL".into()))?;
    let port = url.port().unwrap_or(80);
    let path = match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_string(),
    };

    let host_header = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };

    let addr = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };

    let mut stream = TcpStream::connect(&addr).await?;

    let soap_action = format!("\"{service}#{action}\"");

    let request = format!(
        "POST {} HTTP/1.1\r\n\
         Host: {}:{}\r\n\
         Content-Type: text/xml; charset=\"utf-8\"\r\n\
         Content-Length: {}\r\n\
         SOAPACTION: {}\r\n\
         Connection: close\r\n\
         \r\n\
         {}",
        path,
        host_header,
        port,
        body.len(),
        soap_action,
        body
    );

    trace!(
        "Sending SOAP request to {}:{}{} ({} bytes)",
        host_header,
        port,
        path,
        body.len()
    );

    stream.write_all(request.as_bytes()).await?;

    const MAX_RESPONSE_BYTES: usize = 512 * 1024;
    let mut response: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        if response.len().saturating_add(n) > MAX_RESPONSE_BYTES {
            return Err(Error::InvalidResponse(format!(
                "SOAP response exceeded {MAX_RESPONSE_BYTES} bytes"
            )));
        }
        response.extend_from_slice(&buf[..n]);
    }

    let response_str = String::from_utf8_lossy(&response);
    let status_line = response_str.lines().next().unwrap_or("");
    trace!("Received SOAP response: {}", status_line);

    let body_str = if let Some(body_start) = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|idx| idx + 4)
    {
        String::from_utf8_lossy(&response[body_start..]).to_string()
    } else {
        response_str.to_string()
    };

    if status_line.starts_with("HTTP/1.1 ") || status_line.starts_with("HTTP/1.0 ") {
        if let Some(code) = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
        {
            if code >= 400 {
                let msg = format_soap_http_error(code, status_line, &body_str, service, action);
                return Err(Error::Http(msg));
            }
        }
    }

    Ok(body_str)
}

fn format_soap_http_error(
    http_code: u16,
    status_line: &str,
    body: &str,
    service: &str,
    action: &str,
) -> String {
    let body = body.trim();

    let upnp_code = extract_xml_element(body, "errorCode");
    let upnp_desc = extract_xml_element(body, "errorDescription");
    let soap_fault = extract_xml_element(body, "faultstring");

    let details = if let Some(code) = upnp_code {
        match upnp_desc {
            Some(desc) if !desc.is_empty() => format!("UPnP error {code} ({desc})"),
            _ => format!("UPnP error {code}"),
        }
    } else if let Some(fault) = soap_fault.filter(|s| !s.is_empty()) {
        format!("SOAP fault: {fault}")
    } else if !body.is_empty() {
        format!("body: {}", truncate_utf8(body, 600))
    } else {
        "no response body".to_string()
    };

    format!("HTTP {http_code}: {status_line} ({service}#{action}) {details}")
}

fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

/// Fast random number generator (simple xorshift)
fn fastrand_u64(range: std::ops::Range<u64>) -> u64 {
    use std::cell::Cell;
    thread_local! {
        static RNG: Cell<u64> = Cell::new(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        );
    }

    if range.is_empty() {
        return 0;
    }

    RNG.with(|rng| {
        let mut state = rng.get();
        state ^= state >> 12;
        state ^= state << 25;
        state ^= state >> 27;
        rng.set(state);
        let value = state.wrapping_mul(0x2545F4914F6CDD1D);
        range.start + (value % (range.end - range.start))
    })
}
