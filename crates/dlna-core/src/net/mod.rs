mod diagnostics;
mod selection;

#[cfg(unix)]
mod unix;

pub use diagnostics::{log_network_diagnostics, log_route_diagnostics};
pub use selection::{
    best_local_ipv4, ifindex_for_ipv4, local_ipv4_bind_candidates, local_ipv4_broadcasts,
    local_ipv4_candidates,
};
