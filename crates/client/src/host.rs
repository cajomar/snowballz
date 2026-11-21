//! Hosting a game from the client: an embedded server on a background
//! thread, plus whatever helps friends reach it. A browser can't listen on a
//! port, so there `start` just says no (browser players join a dedicated
//! server instead).

use crate::gfx::Color;

#[cfg(not(target_arch = "wasm32"))]
pub use native::*;
#[cfg(target_arch = "wasm32")]
pub use browser::*;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::net::SocketAddr;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use sim::proto::DEFAULT_PORT;

    use super::Color;

    pub enum UpnpState {
        Pending,
        Mapped(server::upnp::Mapping),
        Failed,
    }

    pub struct Hosted {
        /// Dropping it stops the server.
        _server: server::ServerHandle,
        /// Router port forwarding for internet games, set up in the background.
        upnp: Option<Arc<Mutex<UpnpState>>>,
        port: u16,
        /// How friends on the same network can reach us.
        addresses: Vec<String>,
    }

    impl Hosted {
        pub fn port(&self) -> u16 {
            self.port
        }

        /// Release the router mapping without stalling the UI: removing it can take a moment.
        pub fn close_in_background(mut self) {
            if let Some(u) = self.upnp.take() {
                std::thread::spawn(move || drop(u));
            }
        }

        /// Lines telling the host how friends can reach the game.
        pub fn join_info(&self) -> Vec<(String, Color)> {
            use crate::ui::theme;
            let lan = &self.addresses;
            let mut out = Vec::new();
            if lan.is_empty() {
                return out;
            }
            out.push((format!("Same network: {}", lan.join("  or  ")), theme::TEXT_DIM));
            let port = lan[0].rsplit(':').next().unwrap_or("7777").to_string();
            if let Some(ts) = tailscale_ip() {
                out.push((format!("Tailscale: {ts}:{port}"), theme::TEXT_DIM));
            }
            let Some(u) = &self.upnp else { return out };
            let line = match &*u.lock().unwrap() {
                UpnpState::Pending => ("Internet: asking your router to open the port...".to_string(), theme::TEXT_DIM),
                UpnpState::Mapped(m) => match m.external_ip {
                    Some(ip) if !server::upnp::is_private(ip) => (format!("Internet: friends join at {ip}:{port}"), theme::GOOD),
                    _ => ("Internet: your router is behind another router/ISP NAT, so use Tailscale or a server".to_string(), theme::ACCENT),
                },
                UpnpState::Failed => (format!("Internet: forward TCP port {port} on your router to this computer (see README)"), theme::ACCENT),
            };
            out.push(line);
            out
        }
    }

    /// Start a server. `single` is a private practice game on a random local port.
    pub fn start(player: &str, single: bool, assets: &Path) -> Result<Hosted, String> {
        let cfg = |bind| server::ServerConfig {
            name: if single { "Practice".into() } else { format!("{player}'s game") },
            bind,
            maps_dir: server::maps_dir(assets),
            user_maps_dir: server::user_maps_dir(),
            announce: !single,
            stop_with_host: true,
            web_dir: None,
            ai_notes_for_players: server::ai_notes_for_players(),
        };
        let bind: SocketAddr = if single { "127.0.0.1:0".parse().unwrap() } else { SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)) };
        let handle = server::spawn(cfg(bind)).or_else(|e| {
            if single {
                Err(e)
            } else {
                // Default port busy: take any port.
                server::spawn(cfg(SocketAddr::from(([0, 0, 0, 0], 0))))
            }
        });
        let h = handle.map_err(|e| format!("Couldn't start server: {e}"))?;
        let port = h.addr.port();
        Ok(Hosted {
            _server: h,
            upnp: (!single).then(|| start_upnp(port)),
            port,
            addresses: if single { Vec::new() } else { local_addresses(port) },
        })
    }

    fn start_upnp(port: u16) -> Arc<Mutex<UpnpState>> {
        let st = Arc::new(Mutex::new(UpnpState::Pending));
        let st2 = st.clone();
        std::thread::spawn(move || {
            let r = match server::upnp::forward(port, "Snowballz") {
                Ok(m) => UpnpState::Mapped(m),
                Err(_) => UpnpState::Failed,
            };
            if let Ok(mut s) = st2.lock() {
                *s = r;
            }
        });
        st
    }

    /// This machine's Tailscale address, if it's on a tailnet. Connecting a UDP
    /// socket sends nothing; it only picks the interface that would be used.
    fn tailscale_ip() -> Option<std::net::Ipv4Addr> {
        let s = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
        s.connect("100.100.100.100:53").ok()?;
        match s.local_addr().ok()?.ip() {
            std::net::IpAddr::V4(ip) if ip.octets()[0] == 100 && (64..128).contains(&ip.octets()[1]) => Some(ip),
            _ => None,
        }
    }

    fn local_addresses(port: u16) -> Vec<String> {
        // Connecting a UDP socket sends nothing; it just picks the outgoing interface.
        let mut out = Vec::new();
        if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if s.connect("192.168.255.255:9").is_ok() || s.connect("10.255.255.255:9").is_ok() {
                if let Ok(a) = s.local_addr() {
                    if !a.ip().is_unspecified() {
                        out.push(format!("{}:{port}", a.ip()));
                    }
                }
            }
        }
        if out.is_empty() {
            out.push(format!("<your ip>:{port}"));
        }
        out
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::path::Path;

    use super::Color;

    /// Never exists in a browser.
    pub enum Hosted {}

    impl Hosted {
        pub fn port(&self) -> u16 {
            match *self {}
        }

        pub fn close_in_background(self) {
            match self {}
        }

        pub fn join_info(&self) -> Vec<(String, Color)> {
            match *self {}
        }
    }

    pub fn start(_player: &str, _single: bool, _assets: &Path) -> Result<Hosted, String> {
        Err("Hosting needs the desktop game. In the browser, join a server instead.".into())
    }
}
