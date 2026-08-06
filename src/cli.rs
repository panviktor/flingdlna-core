//! CLI argument definitions using clap

use clap::{Parser, Subcommand};
use dlna_combo::socket_path;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "flingdlna")]
#[command(about = "DLNA media server and controller", long_about = None)]
pub struct Cli {
    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,

    /// Use daemon mode (connect to running daemon)
    #[arg(long)]
    pub daemon: bool,

    /// Socket path for daemon connection
    #[arg(long, default_value_os_t = socket_path())]
    pub socket: PathBuf,

    /// Config file path (default: platform-specific)
    #[arg(short, long)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Start the daemon (background process)
    #[command(name = "daemon")]
    StartDaemon {
        /// Run in foreground (don't daemonize)
        #[arg(long)]
        foreground: bool,

        /// Base port for streaming servers
        #[arg(long, default_value = "9000")]
        port: u16,

        /// Optional: also start media server
        #[arg(long)]
        serve_dir: Option<Vec<PathBuf>>,

        /// Server port (if --serve-dir is used)
        #[arg(long, default_value = "8080")]
        serve_port: u16,

        /// Server name (if --serve-dir is used). If not specified, uses hostname
        #[arg(long)]
        serve_name: Option<String>,

        /// Enable file watcher (if --serve-dir is used)
        #[arg(long, short = 'w')]
        watch: bool,
    },

    /// Stop the running daemon
    #[command(name = "kill")]
    StopDaemon,

    /// Check daemon status
    #[command(name = "ping")]
    PingDaemon,

    /// Start the media server (standalone mode)
    Serve {
        /// Directory to serve media from (can be specified multiple times)
        #[arg(short, long, required = true)]
        dir: Vec<PathBuf>,

        /// HTTP port for the server
        #[arg(short, long, default_value = "8080")]
        port: u16,

        /// Friendly name for the server. If not specified, uses "FlingDLNA ({hostname})"
        #[arg(short, long)]
        name: Option<String>,

        /// Watch directories for changes and auto-update library
        #[arg(short, long)]
        watch: bool,
    },

    /// List available renderers on the network
    List {
        /// Discovery timeout in seconds
        #[arg(short, long, default_value = "30")]
        timeout: u64,
    },

    /// Push a file to a renderer
    Push {
        /// File to play
        file: PathBuf,

        /// Device name or URL
        #[arg(short, long)]
        device: String,

        /// Port for local streaming server
        #[arg(short, long, default_value = "9000")]
        port: u16,
    },

    /// Play a URL on a renderer
    Play {
        /// URL to play (http/https)
        url: String,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Pause playback on a renderer
    Pause {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Resume playback on a renderer
    Resume {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Stop playback on a renderer
    Stop {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Wake a device using Wake-on-LAN
    Wake {
        /// Device name or URL
        #[arg(short, long)]
        device: String,

        /// MAC address (format: AA:BB:CC:DD:EE:FF), if not cached
        #[arg(short, long)]
        mac: Option<String>,
    },

    /// Seek to a position (format: HH:MM:SS or seconds)
    Seek {
        /// Position to seek to
        position: String,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Get current playback status
    Status {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Set or get volume level (0-100)
    Volume {
        /// Volume level (0-100), omit to get current
        level: Option<u8>,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Toggle or set mute
    Mute {
        /// Mute state (on/off), omit to toggle
        state: Option<String>,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    // === Queue Commands ===
    /// Add item to playback queue
    #[command(name = "queue-add")]
    QueueAdd {
        /// File path or URL to add
        source: String,

        /// Optional display title (primarily for URL sources)
        #[arg(long)]
        title: Option<String>,

        /// Optional content type for URL sources (e.g. video/mp4)
        #[arg(long)]
        content_type: Option<String>,

        /// Optional subtitle URL for URL sources
        #[arg(long)]
        subtitle_url: Option<String>,

        /// Device name or URL
        #[arg(short, long)]
        device: String,

        /// Position in queue (0-indexed, omit for end)
        #[arg(short, long)]
        position: Option<usize>,
    },

    /// Remove item from queue
    #[command(name = "queue-remove")]
    QueueRemove {
        /// Index of item to remove
        index: usize,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Clear all items from queue
    #[command(name = "queue-clear")]
    QueueClear {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// List queue items
    #[command(name = "queue")]
    QueueList {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Play next item in queue
    Next {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Play previous item in queue
    Prev {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Set shuffle mode
    Shuffle {
        /// Enable shuffle (on/off)
        state: String,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Set repeat mode
    Repeat {
        /// Repeat mode: none, one, all
        mode: String,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Interactive TUI mode (requires running daemon)
    #[command(name = "tui")]
    Tui,

    // === Resume/Position Commands ===
    /// Save current playback position (for resume later)
    #[command(name = "save-position")]
    SavePosition {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// List all saved positions
    #[command(name = "positions")]
    ListPositions,

    /// Resume playback from saved position
    #[command(name = "resume-from")]
    ResumeFrom {
        /// URL to resume
        url: String,

        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Clear saved position for a URL
    #[command(name = "clear-position")]
    ClearPosition {
        /// URL to clear position for
        url: String,
    },

    // === Event Subscription Commands ===
    /// Subscribe to UPnP events for a device
    Subscribe {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Unsubscribe from UPnP events for a device
    Unsubscribe {
        /// Device name or URL
        #[arg(short, long)]
        device: String,
    },

    /// Poll for pending UPnP events
    #[command(name = "events")]
    PollEvents {
        /// Filter by device name (optional)
        #[arg(short, long)]
        device: Option<String>,
    },

    // === Media Server Directory Commands ===
    /// List media files from server library
    #[command(name = "media")]
    ListMedia,

    /// Set media directory (clears existing files and scans new directory)
    #[command(name = "set-dir")]
    SetMediaDir {
        /// Directory path to serve
        path: PathBuf,
    },

    /// Add media directory (keeps existing files)
    #[command(name = "add-dir")]
    AddMediaDir {
        /// Directory path to add
        path: PathBuf,
    },

    /// Remove media directory
    #[command(name = "remove-dir")]
    RemoveMediaDir {
        /// Directory path to remove
        path: PathBuf,
    },

    /// Clear all media files
    #[command(name = "clear-media")]
    ClearMedia,

    /// Set auto-refresh (file watching) on/off
    #[command(name = "auto-refresh")]
    AutoRefresh {
        /// Enable or disable (on/off), omit to show status
        state: Option<String>,
    },
}
