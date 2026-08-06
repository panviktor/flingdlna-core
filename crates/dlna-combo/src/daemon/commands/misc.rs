use crate::protocol::{Response, ResponseData};

use super::CommandContext;

pub(super) fn ping() -> Response {
    Response::ok_with(ResponseData::Pong)
}

pub(super) async fn info(ctx: &CommandContext<'_>) -> Response {
    let streams_count = ctx.streams.read().await.len();
    Response::ok_with(ResponseData::DaemonInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        uptime_secs: ctx.started_at.elapsed().as_secs(),
        active_streams: streams_count,
    })
}

pub(super) fn shutdown(ctx: &CommandContext<'_>) -> Response {
    let _ = ctx.shutdown_tx.send(());
    Response::ok()
}

pub(super) async fn wake(
    ctx: &CommandContext<'_>,
    device: String,
    mac: Option<String>,
) -> Response {
    use dlna_core::wol;

    if let Some(mac) = mac {
        return match wol::wake(&mac, None) {
            Ok(()) => {
                tracing::info!("Sent Wake-on-LAN to {} ({})", device, mac);
                Response::ok()
            }
            Err(e) => Response::error(format!("Failed to send WoL: {e}")),
        };
    }

    let cache = ctx.renderer_cache.read().await;
    let mac_address = cache
        .get(&device)
        .and_then(|r| r.info.mac_address.as_deref());

    match mac_address {
        Some(mac) => match wol::wake(mac, None) {
            Ok(()) => {
                tracing::info!("Sent Wake-on-LAN to {} ({})", device, mac);
                Response::ok()
            }
            Err(e) => Response::error(format!("Failed to send WoL: {e}")),
        },
        None => Response::error(format!(
            "No MAC address known for device '{device}'. Use --mac to specify one."
        )),
    }
}
