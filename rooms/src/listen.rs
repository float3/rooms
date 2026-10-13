//! Where to listen.
//!
//! Under systemd, the port is held by a socket unit and the descriptor is
//! passed in, which lets the service itself run with no network at all: it
//! cannot open a connection to the internet, to another service on
//! loopback, or to anything else. Outside systemd, the address is bound
//! the ordinary way.

use tokio::net::TcpListener;

/// The socket systemd passed, if it passed one to this process.
#[cfg(unix)]
fn inherited() -> Option<std::net::TcpListener> {
    use std::os::fd::FromRawFd;
    let pid: u32 = std::env::var("LISTEN_PID").ok()?.parse().ok()?;
    if pid != std::process::id() || std::env::var("LISTEN_FDS").ok()? != "1" {
        return None;
    }
    // SAFETY: systemd passes exactly one descriptor, at 3, and nothing
    // else in the process owns it.
    Some(unsafe { std::net::TcpListener::from_raw_fd(3) })
}

#[cfg(not(unix))]
fn inherited() -> Option<std::net::TcpListener> {
    None
}

/// The socket to serve on: the one systemd passed, or `addr` bound here.
///
/// Panics if neither works, because a server that is listening nowhere
/// has nothing to do.
pub async fn listen(addr: &str) -> TcpListener {
    match inherited() {
        Some(passed) => {
            passed
                .set_nonblocking(true)
                .expect("cannot use the socket systemd passed");
            TcpListener::from_std(passed).expect("cannot use the socket systemd passed")
        }
        None => TcpListener::bind(addr)
            .await
            .unwrap_or_else(|e| panic!("cannot bind {addr}: {e}")),
    }
}

/// What to print at startup, which is the address actually being served
/// rather than the one that was asked for.
pub fn where_it_listens(listener: &TcpListener) -> String {
    match listener.local_addr() {
        Ok(local) => format!("listening on http://{local}"),
        Err(_) => "listening on the socket systemd passed".into(),
    }
}
