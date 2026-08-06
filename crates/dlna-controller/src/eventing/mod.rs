//! UPnP Event Subscription support
//!
//! This module provides functionality for subscribing to UPnP events
//! from DLNA Media Renderers. It allows receiving real-time notifications
//! about volume changes, mute state, and transport state.

mod manager;
mod notify;
mod parser;
mod subscribe;
mod types;

pub use manager::EventManager;
pub use types::UpnpEvent;
