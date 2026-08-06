use std::path::PathBuf;

use crate::protocol::{MediaFileInfo, Response, ResponseData};

use super::CommandContext;

pub(super) async fn list_media(ctx: &CommandContext<'_>) -> Response {
    let combo = ctx.combo.read().await;

    if let Some(server) = combo.server() {
        let files = server.files();
        let infos: Vec<MediaFileInfo> = files.iter().map(|f| f.into()).collect();
        Response::ok_with(ResponseData::MediaFiles { files: infos })
    } else {
        Response::error("Media server not running".to_string())
    }
}

pub(super) async fn set_media_dir(ctx: &CommandContext<'_>, path: PathBuf) -> Response {
    if !path.exists() {
        return Response::error(format!("Directory not found: {path:?}"));
    }
    if !path.is_dir() {
        return Response::error(format!("Path is not a directory: {path:?}"));
    }

    let mut combo = ctx.combo.write().await;
    match combo.set_media_directory(path.clone()) {
        Ok(count) => {
            tracing::info!("Set media directory to {:?}, found {} files", path, count);
            Response::ok_with(ResponseData::FileCount { count })
        }
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn add_media_dir(ctx: &CommandContext<'_>, path: PathBuf) -> Response {
    if !path.exists() {
        return Response::error(format!("Directory not found: {path:?}"));
    }
    if !path.is_dir() {
        return Response::error(format!("Path is not a directory: {path:?}"));
    }

    let mut combo = ctx.combo.write().await;
    match combo.add_media_directory(path.clone()) {
        Ok(count) => {
            tracing::info!("Added media directory {:?}, found {} files", path, count);
            Response::ok_with(ResponseData::FileCount { count })
        }
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn remove_media_dir(ctx: &CommandContext<'_>, path: PathBuf) -> Response {
    let mut combo = ctx.combo.write().await;
    match combo.remove_media_directory(path.clone()) {
        Ok(()) => {
            tracing::info!("Removed media directory {:?}", path);
            Response::ok()
        }
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn clear_media(ctx: &CommandContext<'_>) -> Response {
    let combo = ctx.combo.read().await;
    combo.clear_media_files();
    tracing::info!("Cleared all media files");
    Response::ok()
}

pub(super) async fn rescan_media(ctx: &CommandContext<'_>) -> Response {
    let combo = ctx.combo.read().await;
    if let Some(server) = combo.server() {
        let count = server.files().len();
        Response::ok_with(ResponseData::FileCount { count })
    } else {
        Response::error("Media server not running")
    }
}

pub(super) async fn set_auto_refresh(ctx: &CommandContext<'_>, enabled: bool) -> Response {
    let mut combo = ctx.combo.write().await;
    match combo.set_file_watch_enabled(enabled) {
        Ok(()) => {
            tracing::info!(
                "Auto-refresh {}",
                if enabled { "enabled" } else { "disabled" }
            );
            Response::ok_with(ResponseData::AutoRefresh { enabled })
        }
        Err(e) => Response::error(e.to_string()),
    }
}

pub(super) async fn get_auto_refresh(ctx: &CommandContext<'_>) -> Response {
    let combo = ctx.combo.read().await;
    let enabled = combo.is_file_watch_enabled();
    Response::ok_with(ResponseData::AutoRefresh { enabled })
}
