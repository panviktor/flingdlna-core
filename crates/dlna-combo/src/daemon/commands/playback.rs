use std::path::PathBuf;
use std::time::Duration;

use crate::protocol::{PlayRequestPayload, Response, ResponseData};

use super::helpers::find_renderer_or_error;
use super::CommandContext;

pub(super) async fn push(
    ctx: &CommandContext<'_>,
    file: PathBuf,
    device: String,
    port: u16,
) -> Response {
    if !file.exists() {
        return Response::error(format!("File not found: {file:?}"));
    }

    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let stream_port = if port != 9000 {
        port
    } else {
        ctx.allocated_port
    };

    match combo
        .controller()
        .play_file_with_port(&renderer, &file, stream_port)
        .await
    {
        Ok((video_url, _subtitle_url)) => {
            let file_name = file
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            let name = renderer.friendly_name;
            let url = video_url;
            let stream_info = super::super::cache::ActiveStream {
                device_name: name.clone(),
                file_path: file,
                port: stream_port,
                url: url.clone(),
                started_at: std::time::Instant::now(),
            };

            ctx.streams.write().await.insert(name.clone(), stream_info);

            Response::ok_with(ResponseData::Streaming {
                device: name,
                file: file_name,
                url,
            })
        }
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn play_url(ctx: &CommandContext<'_>, url: String, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match combo.play_url(&url, &renderer).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn play_request(
    ctx: &CommandContext<'_>,
    request: PlayRequestPayload,
    device: String,
) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let play_request = crate::PlayRequest::new(request.url)
        .with_content_type(request.content_type.as_deref())
        .with_subtitle_url(request.subtitle_url.as_deref())
        .with_title(request.title.as_deref());

    match combo.play_request(&play_request, &renderer).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn pause(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match combo.pause(&renderer).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn resume(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match combo.controller().resume(&renderer).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn stop(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    ctx.streams.write().await.remove(&renderer.friendly_name);

    match combo.stop(&renderer).await {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn seek(ctx: &CommandContext<'_>, device: String, position_secs: u64) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match combo
        .seek(&renderer, Duration::from_secs(position_secs))
        .await
    {
        Ok(()) => Response::ok(),
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn status(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match combo.get_playback_info(&renderer).await {
        Ok(info) => Response::ok_with(ResponseData::Playback {
            state: info.state.to_string(),
            position_secs: info.position.as_secs(),
            duration_secs: info.duration.as_secs(),
            current_uri: info.current_uri,
        }),
        Err(e) => Response::error(e.to_string()),
    }
}
