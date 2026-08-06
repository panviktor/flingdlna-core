use std::time::Instant;

use crate::protocol::{RendererInfo, Response, ResponseData};

use super::CommandContext;

#[cfg(feature = "chromecast")]
use super::super::chromecast::get_chromecast_renderers;

pub(super) async fn list(ctx: &CommandContext<'_>) -> Response {
    let renderers = collect_cached_renderers(ctx).await;
    Response::ok_with(ResponseData::Renderers { renderers })
}

pub(super) async fn list_cached(ctx: &CommandContext<'_>) -> Response {
    let renderers = collect_cached_renderers(ctx).await;
    Response::ok_with(ResponseData::Renderers { renderers })
}

pub(super) async fn refresh(ctx: &CommandContext<'_>, timeout_secs: u64) -> Response {
    // Force network discovery
    tracing::debug!("Forced discovery requested (timeout: {}s)", timeout_secs);
    let combo = ctx.combo.read().await;

    let timeout = std::time::Duration::from_secs(timeout_secs);

    match combo.discover_renderers_with_timeout(timeout).await {
        Ok(renderers) => {
            let infos: Vec<RendererInfo> = renderers.iter().map(|r| r.into()).collect();

            // Update cache with fresh results
            {
                let mut cache = ctx.renderer_cache.write().await;
                let now = Instant::now();
                cache.clear();
                for info in &infos {
                    cache.insert(
                        info.name.clone(),
                        super::super::cache::CachedRenderer {
                            info: info.clone(),
                            cached_at: now,
                        },
                    );
                }
            }

            Response::ok_with(ResponseData::Renderers { renderers: infos })
        }
        Err(e) => Response::error(e.to_string()),
    }
}

async fn collect_cached_renderers(ctx: &CommandContext<'_>) -> Vec<RendererInfo> {
    let cache = ctx.renderer_cache.read().await;
    #[cfg_attr(not(feature = "chromecast"), allow(unused_mut))]
    let mut renderers: Vec<RendererInfo> = cache.values().map(|c| c.info.clone()).collect();

    // Add Chromecast devices if feature is enabled
    #[cfg(feature = "chromecast")]
    {
        let cc_renderers = get_chromecast_renderers(ctx.chromecast_cache).await;
        renderers.extend(cc_renderers);
    }

    renderers
}
