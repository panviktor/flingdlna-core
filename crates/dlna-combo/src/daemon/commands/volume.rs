use crate::protocol::{Response, ResponseData};

use super::helpers::find_renderer_or_error;
use super::CommandContext;

pub(super) async fn get_volume(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let volume = match combo.controller().get_volume(&renderer).await {
        Ok(v) => v,
        Err(e) => return Response::error(e.to_string()),
    };

    let muted = combo
        .controller()
        .get_mute(&renderer)
        .await
        .unwrap_or(false);

    Response::ok_with(ResponseData::Volume { volume, muted })
}

pub(super) async fn set_volume(ctx: &CommandContext<'_>, device: String, volume: u8) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match combo.controller().set_volume(&renderer, volume).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn adjust_volume(ctx: &CommandContext<'_>, device: String, delta: i8) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let current = match combo.controller().get_volume(&renderer).await {
        Ok(v) => v as i16,
        Err(e) => return Response::error(format!("Failed to get volume: {e}")),
    };

    let new_volume = (current + delta as i16).clamp(0, 100) as u8;

    match combo.controller().set_volume(&renderer, new_volume).await {
        Ok(()) => Response::ok_with(ResponseData::Volume {
            volume: new_volume,
            muted: false,
        }),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn mute(ctx: &CommandContext<'_>, device: String, mute: Option<bool>) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let new_mute = match mute {
        Some(m) => m,
        None => !combo
            .controller()
            .get_mute(&renderer)
            .await
            .unwrap_or(false),
    };

    match combo.controller().set_mute(&renderer, new_mute).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}
