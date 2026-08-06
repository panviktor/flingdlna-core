use std::os::unix::io::AsRawFd;

#[cfg(target_os = "macos")]
pub fn peer_uid(stream: &tokio::net::UnixStream) -> Option<u32> {
    let fd = stream.as_raw_fd();
    let mut euid: libc::uid_t = 0;
    let mut egid: libc::gid_t = 0;
    let rc = unsafe { libc::getpeereid(fd, &mut euid, &mut egid) };
    if rc == 0 {
        Some(euid as u32)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
pub fn peer_uid(stream: &tokio::net::UnixStream) -> Option<u32> {
    let fd = stream.as_raw_fd();
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut _,
            &mut len as *mut _,
        )
    };
    if rc == 0 {
        Some(cred.uid as u32)
    } else {
        None
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn peer_uid(_stream: &tokio::net::UnixStream) -> Option<u32> {
    None
}

pub fn current_uid() -> u32 {
    unsafe { libc::geteuid() as u32 }
}

fn allow_other_uid() -> bool {
    matches!(
        std::env::var("FLINGDLNA_DAEMON_ALLOW_OTHER_UID")
            .ok()
            .as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

pub fn peer_allowed(stream: &tokio::net::UnixStream) -> bool {
    if allow_other_uid() {
        return true;
    }
    match peer_uid(stream) {
        Some(uid) => uid == current_uid(),
        None => true,
    }
}
