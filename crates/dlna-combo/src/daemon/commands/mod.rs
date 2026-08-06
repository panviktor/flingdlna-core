//! Command execution for daemon IPC

use super::cache::{ActiveStream, CachedRenderer, ChromecastCache};
use crate::database::Database;
use crate::protocol::{Command, Notification, Response};
use crate::queue::Queue;
use crate::DlnaCombo;
use dlna_controller::EventManager;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{broadcast, RwLock};

mod discovery;
mod events;
mod helpers;
mod media;
mod misc;
mod playback;
mod positions;
mod queue;
mod volume;

pub(crate) struct CommandContext<'a> {
    pub combo: &'a Arc<RwLock<DlnaCombo>>,
    pub streams: &'a Arc<RwLock<HashMap<String, ActiveStream>>>,
    pub queues: &'a Arc<RwLock<HashMap<String, Queue>>>,
    pub renderer_cache: &'a Arc<RwLock<HashMap<String, CachedRenderer>>>,
    pub shutdown_tx: &'a broadcast::Sender<()>,
    pub started_at: Instant,
    pub allocated_port: u16,
    pub event_manager: &'a Option<Arc<EventManager>>,
    pub local_ip: &'a Option<String>,
    pub event_buffer: &'a Arc<RwLock<VecDeque<Notification>>>,
    pub udn_to_name: &'a Arc<RwLock<HashMap<String, String>>>,
    pub chromecast_cache: &'a ChromecastCache,
    pub db: &'a Arc<Database>,
}

/// Execute a daemon command and return the response
#[allow(clippy::too_many_arguments)]
pub async fn execute_command(
    cmd: Command,
    combo: &Arc<RwLock<DlnaCombo>>,
    streams: &Arc<RwLock<HashMap<String, ActiveStream>>>,
    queues: &Arc<RwLock<HashMap<String, Queue>>>,
    renderer_cache: &Arc<RwLock<HashMap<String, CachedRenderer>>>,
    shutdown_tx: &broadcast::Sender<()>,
    started_at: Instant,
    allocated_port: u16,
    event_manager: &Option<Arc<EventManager>>,
    local_ip: &Option<String>,
    event_buffer: &Arc<RwLock<VecDeque<Notification>>>,
    udn_to_name: &Arc<RwLock<HashMap<String, String>>>,
    #[allow(unused_variables)] chromecast_cache: &ChromecastCache,
    db: &Arc<Database>,
) -> Response {
    let ctx = CommandContext {
        combo,
        streams,
        queues,
        renderer_cache,
        shutdown_tx,
        started_at,
        allocated_port,
        event_manager,
        local_ip,
        event_buffer,
        udn_to_name,
        chromecast_cache,
        db,
    };

    match cmd {
        Command::Ping => misc::ping(),
        Command::Info => misc::info(&ctx).await,
        Command::Shutdown => misc::shutdown(&ctx),

        Command::List { timeout_secs: _ } => discovery::list(&ctx).await,
        Command::ListCached => discovery::list_cached(&ctx).await,
        Command::Refresh { timeout_secs } => discovery::refresh(&ctx, timeout_secs).await,

        Command::ListMedia => media::list_media(&ctx).await,

        Command::Push { file, device, port } => playback::push(&ctx, file, device, port).await,
        Command::PlayUrl { url, device } => playback::play_url(&ctx, url, device).await,
        Command::PlayRequest { request, device } => {
            playback::play_request(&ctx, request, device).await
        }
        Command::Pause { device } => playback::pause(&ctx, device).await,
        Command::Resume { device } => playback::resume(&ctx, device).await,
        Command::Stop { device } => playback::stop(&ctx, device).await,
        Command::Seek {
            device,
            position_secs,
        } => playback::seek(&ctx, device, position_secs).await,
        Command::Status { device } => playback::status(&ctx, device).await,

        Command::GetVolume { device } => volume::get_volume(&ctx, device).await,
        Command::SetVolume { device, volume } => volume::set_volume(&ctx, device, volume).await,
        Command::AdjustVolume { device, delta } => volume::adjust_volume(&ctx, device, delta).await,
        Command::Mute { device, mute } => volume::mute(&ctx, device, mute).await,

        Command::QueueAdd {
            source,
            title,
            content_type,
            subtitle_url,
            device,
            position,
        } => {
            queue::add(
                &ctx,
                source,
                title,
                content_type,
                subtitle_url,
                device,
                position,
            )
            .await
        }
        Command::QueueRemove { device, index } => queue::remove(&ctx, device, index).await,
        Command::QueueClear { device } => queue::clear(&ctx, device).await,
        Command::QueueList { device } => queue::list(&ctx, device).await,
        Command::Next { device } => queue::next(&ctx, device).await,
        Command::Prev { device } => queue::prev(&ctx, device).await,
        Command::Shuffle { device, enabled } => queue::shuffle(&ctx, device, enabled).await,
        Command::Repeat { device, mode } => queue::repeat(&ctx, device, mode).await,

        Command::SavePosition { device } => positions::save(&ctx, device).await,
        Command::GetPosition { uri } => positions::get(&ctx, uri),
        Command::ListPositions => positions::list(&ctx),
        Command::ClearPosition { uri } => positions::clear(&ctx, uri),
        Command::ResumeFrom { device, uri } => positions::resume_from(&ctx, device, uri).await,

        Command::Subscribe { device } => events::subscribe(&ctx, device).await,
        Command::Unsubscribe { device } => events::unsubscribe(&ctx, device).await,
        Command::PollEvents { device } => events::poll(&ctx, device).await,

        Command::Wake { device, mac } => misc::wake(&ctx, device, mac).await,

        Command::SetMediaDir { path } => media::set_media_dir(&ctx, path).await,
        Command::AddMediaDir { path } => media::add_media_dir(&ctx, path).await,
        Command::RemoveMediaDir { path } => media::remove_media_dir(&ctx, path).await,
        Command::ClearMedia => media::clear_media(&ctx).await,
        Command::RescanMedia => media::rescan_media(&ctx).await,
        Command::SetAutoRefresh { enabled } => media::set_auto_refresh(&ctx, enabled).await,
        Command::GetAutoRefresh => media::get_auto_refresh(&ctx).await,
    }
}
