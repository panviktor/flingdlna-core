use std::path::PathBuf;

use crate::protocol::{QueueItemInfo, Response, ResponseData};
use crate::queue::{Queue, QueueItem, QueueSource, RepeatMode};

use super::helpers::find_renderer_or_error;
use super::CommandContext;

pub(super) async fn add(
    ctx: &CommandContext<'_>,
    source: String,
    title: Option<String>,
    content_type: Option<String>,
    subtitle_url: Option<String>,
    device: String,
    position: Option<usize>,
) -> Response {
    let mut item = if source.starts_with("http://") || source.starts_with("https://") {
        if content_type.is_some() || subtitle_url.is_some() {
            QueueItem::from_url_request(source, content_type, subtitle_url)
        } else {
            QueueItem::from_url(source)
        }
    } else {
        let path = PathBuf::from(&source);
        if !path.exists() {
            return Response::error(format!("File not found: {source}"));
        }

        let duration = {
            let combo = ctx.combo.read().await;
            combo
                .server()
                .and_then(|s| s.files().into_iter().find(|f| f.path == path))
                .and_then(|f| f.duration)
        };

        QueueItem::from_file(path).with_duration(duration)
    };

    if let Some(title) = title {
        if matches!(
            &item.source,
            QueueSource::Url(_) | QueueSource::UrlRequest { .. }
        ) {
            item.title = title;
        }
    }

    let mut queues = ctx.queues.write().await;
    let queue = queues.entry(device).or_insert_with(Queue::new);

    match position {
        Some(pos) => queue.insert(pos, item),
        None => queue.add(item),
    }

    Response::ok()
}

pub(super) async fn remove(ctx: &CommandContext<'_>, device: String, index: usize) -> Response {
    let mut queues = ctx.queues.write().await;

    let queue = match queues.get_mut(&device) {
        Some(q) => q,
        None => return Response::error(format!("No queue for device: {device}")),
    };

    match queue.remove(index) {
        Some(_) => Response::ok(),
        None => Response::error(format!("Invalid queue index: {index}")),
    }
}

pub(super) async fn clear(ctx: &CommandContext<'_>, device: String) -> Response {
    let mut queues = ctx.queues.write().await;

    if let Some(queue) = queues.get_mut(&device) {
        queue.clear();
    }

    Response::ok()
}

pub(super) async fn list(ctx: &CommandContext<'_>, device: String) -> Response {
    let queues = ctx.queues.read().await;

    let queue = match queues.get(&device) {
        Some(q) => q,
        None => {
            return Response::ok_with(ResponseData::Queue {
                items: vec![],
                current_index: None,
                shuffle: false,
                repeat: "none".to_string(),
            });
        }
    };

    let items: Vec<QueueItemInfo> = queue
        .items()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let is_url = matches!(
                &item.source,
                QueueSource::Url(_) | QueueSource::UrlRequest { .. }
            );
            QueueItemInfo {
                index,
                title: item.title.clone(),
                source: match &item.source {
                    QueueSource::File(p) => p.to_string_lossy().to_string(),
                    QueueSource::Url(u) => u.clone(),
                    QueueSource::UrlRequest { url, .. } => url.clone(),
                },
                is_url,
                duration_secs: item.duration.map(|d| d.as_secs()),
            }
        })
        .collect();

    let repeat = match queue.repeat() {
        RepeatMode::None => "none",
        RepeatMode::One => "one",
        RepeatMode::All => "all",
    };

    Response::ok_with(ResponseData::Queue {
        items,
        current_index: queue.current_index(),
        shuffle: queue.shuffle(),
        repeat: repeat.to_string(),
    })
}

pub(super) async fn next(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let mut queues = ctx.queues.write().await;
    let queue = match queues.get_mut(&device) {
        Some(q) => q,
        None => return Response::error(format!("No queue for device: {device}")),
    };

    let next_item = match queue.next() {
        Some(item) => item.clone(),
        None => return Response::error("No next item in queue"),
    };

    match &next_item.source {
        QueueSource::File(path) => match combo.push(path, &renderer).await {
            Ok(()) => Response::ok(),
            Err(e) => Response::error(e.to_string()),
        },
        QueueSource::Url(url) => {
            let request = crate::PlayRequest::new(url).with_title(Some(next_item.title.as_str()));
            match combo.play_request(&request, &renderer).await {
                Ok(()) => Response::ok(),
                Err(e) => Response::error(e.to_string()),
            }
        }
        QueueSource::UrlRequest {
            url,
            content_type,
            subtitle_url,
        } => {
            let request = crate::PlayRequest::new(url)
                .with_content_type(content_type.as_deref())
                .with_subtitle_url(subtitle_url.as_deref())
                .with_title(Some(next_item.title.as_str()));
            match combo.play_request(&request, &renderer).await {
                Ok(()) => Response::ok(),
                Err(e) => Response::error(e.to_string()),
            }
        }
    }
}

pub(super) async fn prev(ctx: &CommandContext<'_>, device: String) -> Response {
    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    let mut queues = ctx.queues.write().await;
    let queue = match queues.get_mut(&device) {
        Some(q) => q,
        None => return Response::error(format!("No queue for device: {device}")),
    };

    let prev_item = match queue.prev() {
        Some(item) => item.clone(),
        None => return Response::error("No previous item in queue"),
    };

    match &prev_item.source {
        QueueSource::File(path) => match combo.push(path, &renderer).await {
            Ok(()) => Response::ok(),
            Err(e) => Response::error(e.to_string()),
        },
        QueueSource::Url(url) => {
            let request = crate::PlayRequest::new(url).with_title(Some(prev_item.title.as_str()));
            match combo.play_request(&request, &renderer).await {
                Ok(()) => Response::ok(),
                Err(e) => Response::error(e.to_string()),
            }
        }
        QueueSource::UrlRequest {
            url,
            content_type,
            subtitle_url,
        } => {
            let request = crate::PlayRequest::new(url)
                .with_content_type(content_type.as_deref())
                .with_subtitle_url(subtitle_url.as_deref())
                .with_title(Some(prev_item.title.as_str()));
            match combo.play_request(&request, &renderer).await {
                Ok(()) => Response::ok(),
                Err(e) => Response::error(e.to_string()),
            }
        }
    }
}

pub(super) async fn shuffle(ctx: &CommandContext<'_>, device: String, enabled: bool) -> Response {
    let mut queues = ctx.queues.write().await;
    let queue = queues.entry(device).or_insert_with(Queue::new);
    queue.set_shuffle(enabled);
    Response::ok()
}

pub(super) async fn repeat(ctx: &CommandContext<'_>, device: String, mode: String) -> Response {
    let repeat_mode = match mode.to_lowercase().as_str() {
        "none" | "off" => RepeatMode::None,
        "one" | "single" | "track" => RepeatMode::One,
        "all" | "queue" | "playlist" => RepeatMode::All,
        _ => return Response::error(format!("Invalid repeat mode: {mode}. Use: none, one, all")),
    };

    let mut queues = ctx.queues.write().await;
    let queue = queues.entry(device).or_insert_with(Queue::new);
    queue.set_repeat(repeat_mode);
    Response::ok()
}
