use crate::protocol::{Notification, Response, ResponseData};
use std::collections::VecDeque;

use super::helpers::find_renderer_or_error;
use super::CommandContext;

pub(super) async fn subscribe(ctx: &CommandContext<'_>, device: String) -> Response {
    let em = match ctx.event_manager {
        Some(em) => em,
        None => return Response::error("Event subscriptions not enabled"),
    };

    let lip = match ctx.local_ip {
        Some(ip) => ip,
        None => return Response::error("Could not determine local IP for callbacks"),
    };

    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match em.subscribe(&renderer, lip).await {
        Ok(()) => {
            let name = renderer.friendly_name;
            tracing::info!("Subscribed to events for {}", name);
            ctx.udn_to_name.write().await.insert(renderer.udn, name);
            Response::ok()
        }
        Err(e) => Response::error(format!("Failed to subscribe: {e}")),
    }
}

pub(super) async fn unsubscribe(ctx: &CommandContext<'_>, device: String) -> Response {
    let em = match ctx.event_manager {
        Some(em) => em,
        None => return Response::error("Event subscriptions not enabled"),
    };

    let combo = ctx.combo.read().await;

    let renderer = match find_renderer_or_error(&combo, &device).await {
        Ok(r) => r,
        Err(resp) => return resp,
    };

    match em.unsubscribe(&renderer).await {
        Ok(()) => {
            tracing::info!("Unsubscribed from events for {}", renderer.friendly_name);
            Response::ok()
        }
        Err(e) => Response::error(format!("Failed to unsubscribe: {e}")),
    }
}

pub(super) async fn poll(ctx: &CommandContext<'_>, device: Option<String>) -> Response {
    let mut buffer = ctx.event_buffer.write().await;
    let events: Vec<Notification> = if let Some(ref device_filter) = device {
        let mut remaining = VecDeque::new();
        let mut matched = Vec::new();

        for event in buffer.drain(..) {
            let is_match = match &event {
                Notification::VolumeChanged { device, .. } => device == device_filter,
                Notification::MuteChanged { device, .. } => device == device_filter,
                Notification::TransportStateChanged { device, .. } => device == device_filter,
                Notification::CurrentUriChanged { device, .. } => device == device_filter,
                Notification::SubscriptionLost { device, .. } => device == device_filter,
            };

            if is_match {
                matched.push(event);
            } else {
                remaining.push_back(event);
            }
        }

        buffer.extend(remaining);
        matched
    } else {
        buffer.drain(..).collect()
    };

    Response::ok_with(ResponseData::Events { events })
}
