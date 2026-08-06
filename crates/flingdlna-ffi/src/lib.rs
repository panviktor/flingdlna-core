#![allow(clippy::not_unsafe_ptr_arg_deref)]
//! FFI bindings for flingdlna
//!
//! This crate provides C-compatible functions for integrating flingdlna
//! into iOS/macOS applications via static library linking.

mod device;
mod device_actions;
mod device_info;
mod discovery;
mod errors;
mod helpers;
mod lifecycle;
mod logging;
mod media;
mod playability;
mod playback;
mod server;
mod session_events;
mod state;
mod types;
mod volume;

pub use device_actions::FFIPlayModeCallback;
pub use device_info::*;
pub use discovery::*;
pub use errors::*;
pub use lifecycle::*;
pub use media::*;
pub use playability::*;
pub use playback::*;
pub use server::*;
pub use session_events::*;
pub use types::*;
pub use volume::*;
