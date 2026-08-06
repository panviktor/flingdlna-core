use std::time::Duration;

use crate::protocol::{PositionInfo, Response, ResponseData};

use super::helpers::find_renderer_or_error;
use super::CommandContext;

pub(super) async fn save(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let info = match combo.get_playback_info(&renderer).await {
        Ok(info) => info,
        Err(e) => return Response::error(e.to_string()),
    };

    let uri = match info.current_uri {
        Some(uri) => uri,
        None => return Response::error("No media currently playing"),
    };

    if let Err(e) =
        ctx.db
            .save_position(&uri, info.position, info.duration, &renderer.friendly_name)
    {
        return Response::error(format!("Failed to save position: {e}"));
    }

    Response::ok_with(ResponseData::Position {
        uri,
        position_secs: info.position.as_secs(),
        duration_secs: info.duration.as_secs(),
        device: renderer.friendly_name,
        saved_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    })
}

pub(super) fn get(ctx: &CommandContext<'_>, uri: String) -> Response {
    match ctx.db.get_position(&uri) {
        Ok(Some(pos)) => Response::ok_with(ResponseData::Position {
            uri: pos.uri,
            position_secs: pos.position_secs,
            duration_secs: pos.duration_secs,
            device: pos.device,
            saved_at: pos.saved_at,
        }),
        Ok(None) => Response::error(format!("No saved position for: {uri}")),
        Err(e) => Response::error(format!("Database error: {e}")),
    }
}

pub(super) fn list(ctx: &CommandContext<'_>) -> Response {
    match ctx.db.all_positions() {
        Ok(positions) => {
            let infos: Vec<PositionInfo> = positions
                .into_iter()
                .map(|p| PositionInfo {
                    uri: p.uri,
                    position_secs: p.position_secs,
                    duration_secs: p.duration_secs,
                    device: p.device,
                    saved_at: p.saved_at,
                })
                .collect();
            Response::ok_with(ResponseData::Positions { positions: infos })
        }
        Err(e) => Response::error(format!("Database error: {e}")),
    }
}

pub(super) fn clear(ctx: &CommandContext<'_>, uri: String) -> Response {
    if let Err(e) = ctx.db.remove_position(&uri) {
        return Response::error(format!("Failed to remove: {e}"));
    }
    Response::ok()
}

pub(super) async fn resume_from(ctx: &CommandContext<'_>, device: String, uri: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let position = match ctx.db.get_position(&uri) {
        Ok(Some(pos)) => Duration::from_secs(pos.position_secs),
        Ok(None) => Duration::ZERO,
        Err(_) => Duration::ZERO,
    };

    if let Err(e) = combo.play_url(&uri, &renderer).await {
        return Response::error(e.to_string());
    }

    tokio::time::sleep(Duration::from_millis(500)).await;

    if position > Duration::ZERO {
        if let Err(e) = combo.seek(&renderer, position).await {
            tracing::debug!("Failed to seek to saved position: {}", e);
        }
    }

    Response::ok()
}
