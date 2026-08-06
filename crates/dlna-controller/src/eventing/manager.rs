use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use dlna_core::{Error, Renderer, Result};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info};

use super::notify::handle_notify;
use super::types::{Subscription, UpnpEvent};

/// Create a TcpListener with SO_REUSEADDR enabled for immediate port reuse
fn create_reusable_listener(bind_addr: SocketAddr) -> Result<TcpListener> {
    let socket = Socket::new(
        Domain::for_address(bind_addr),
        Type::STREAM,
        Some(Protocol::TCP),
    )
    .map_err(Error::Io)?;
    socket.set_reuse_address(true).map_err(Error::Io)?;
    #[cfg(unix)]
    socket.set_reuse_port(true).map_err(Error::Io)?;
    socket.set_nonblocking(true).map_err(Error::Io)?;
    socket.bind(&bind_addr.into()).map_err(Error::Io)?;
    socket.listen(128).map_err(Error::Io)?;

    TcpListener::from_std(socket.into()).map_err(Error::Io)
}

/// Event subscription manager
pub struct EventManager {
    /// HTTP callback server address
    pub(crate) callback_addr: SocketAddr,
    /// Active subscriptions by SID
    pub(crate) subscriptions: Arc<RwLock<HashMap<String, Subscription>>>,
    /// Event sender
    pub(crate) event_tx: broadcast::Sender<UpnpEvent>,
    /// Shutdown signal
    shutdown_tx: Option<mpsc::Sender<()>>,
}

impl EventManager {
    /// Create a new event manager and start the callback server
    pub async fn new(callback_port: u16) -> Result<Self> {
        // Bind to all interfaces on the specified port with SO_REUSEADDR
        let bind_addr: SocketAddr = format!("0.0.0.0:{callback_port}")
            .parse()
            .map_err(|e| Error::Config(format!("Invalid bind address: {e}")))?;
        let listener = create_reusable_listener(bind_addr)?;
        let callback_addr = listener.local_addr()?;

        info!("UPnP event callback server listening on {}", callback_addr);

        let subscriptions: Arc<RwLock<HashMap<String, Subscription>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let (event_tx, _) = broadcast::channel(100);
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

        // Clone for the server task
        let subs_clone = Arc::clone(&subscriptions);
        let event_tx_clone = event_tx.clone();

        // Start the callback server
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let subs = Arc::clone(&subs_clone);
                                let tx = event_tx_clone.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_notify(stream, addr, subs, tx).await {
                                        debug!("Error handling NOTIFY: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Error accepting connection: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Event callback server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            callback_addr,
            subscriptions,
            event_tx,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Create a new event manager bound to a specific local IP address
    pub async fn new_bound(bind_ip: IpAddr, callback_port: u16) -> Result<Self> {
        let bind_addr = SocketAddr::new(bind_ip, callback_port);
        let listener = create_reusable_listener(bind_addr)?;
        let callback_addr = listener.local_addr()?;

        info!("UPnP event callback server listening on {}", callback_addr);

        let subscriptions: Arc<RwLock<HashMap<String, Subscription>>> =
            Arc::new(RwLock::new(HashMap::new()));
        let (event_tx, _) = broadcast::channel(100);
        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);

        // Clone for the server task
        let subs_clone = Arc::clone(&subscriptions);
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        match result {
                            Ok((stream, addr)) => {
                                let subs = Arc::clone(&subs_clone);
                                let tx = event_tx_clone.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handle_notify(stream, addr, subs, tx).await {
                                        debug!("Error handling NOTIFY: {}", e);
                                    }
                                });
                            }
                            Err(e) => {
                                error!("Error accepting connection: {}", e);
                            }
                        }
                    }
                    _ = shutdown_rx.recv() => {
                        info!("Event callback server shutting down");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            callback_addr,
            subscriptions,
            event_tx,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    /// Get the callback URL for subscriptions
    pub fn callback_url(&self, local_ip: &str) -> String {
        let host = if local_ip.contains(':') {
            format!("[{local_ip}]")
        } else {
            local_ip.to_string()
        };
        format!("http://{}:{}/notify", host, self.callback_addr.port())
    }

    /// Get a receiver for events
    pub fn subscribe_events(&self) -> broadcast::Receiver<UpnpEvent> {
        self.event_tx.subscribe()
    }
}

impl Drop for EventManager {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.try_send(());
        }
    }
}

// Ensure public API stays in this module.
impl EventManager {
    /// Subscribe to events from a renderer
    pub async fn subscribe(&self, renderer: &Renderer, local_ip: &str) -> Result<()> {
        super::subscribe::subscribe(self, renderer, local_ip).await
    }

    /// Unsubscribe from all events for a renderer
    pub async fn unsubscribe(&self, renderer: &Renderer) -> Result<()> {
        super::subscribe::unsubscribe(self, renderer).await
    }

    /// Renew subscriptions that are about to expire
    pub async fn renew_expiring(&self, local_ip: &str) -> Result<()> {
        super::subscribe::renew_expiring(self, local_ip).await
    }
}
