//! Client side of the connection, plus LAN game discovery.
//!
//! Natively the connection is a TCP socket. In a browser it's a WebSocket to
//! the same server port, which carries the same length-prefixed frames.

use crate::platform as date;
use sim::proto::{write_frame, ClientMsg, FrameReader, ServerMsg};

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Connecting, Discovery};
#[cfg(target_arch = "wasm32")]
pub use browser::{Connecting, Discovery};

pub struct Net {
    link: Link,
    reader: FrameReader,
    pub ping_ms: f32,
    last_ping: f64,
    epoch: f64,
}

impl Net {
    fn new(link: Link) -> Net {
        let now = date::now();
        Net { link, reader: FrameReader::default(), ping_ms: 0.0, last_ping: now - 10.0, epoch: now }
    }

    pub fn send(&mut self, msg: ClientMsg) {
        let mut frame = Vec::new();
        let _ = write_frame(&mut frame, &msg.encode());
        self.link.write(&frame);
    }

    /// Drain everything the server sent. Err means the connection is gone.
    pub fn poll(&mut self) -> Result<Vec<ServerMsg>, String> {
        self.link.flush();
        if date::now() - self.last_ping > 2.0 {
            self.last_ping = date::now();
            let t = ((self.last_ping - self.epoch) * 1000.0) as u32;
            self.send(ClientMsg::Ping(t));
        }
        self.link.fill(&mut self.reader)?;
        let mut out = Vec::new();
        while let Some(f) = self.reader.next().map_err(|e| e.to_string())? {
            match ServerMsg::decode(&f) {
                Ok(ServerMsg::Pong(t)) => {
                    let now = ((date::now() - self.epoch) * 1000.0) as u32;
                    let rtt = now.wrapping_sub(t) as f32;
                    self.ping_ms = if self.ping_ms == 0.0 { rtt } else { self.ping_ms * 0.7 + rtt * 0.3 };
                }
                Ok(m) => out.push(m),
                Err(e) => return Err(format!("bad message from server: {e}")),
            }
        }
        Ok(out)
    }
}

#[cfg(not(target_arch = "wasm32"))]
use native::Link;
#[cfg(target_arch = "wasm32")]
use browser::Link;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use std::io::{self, Write};
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs, UdpSocket};
    use std::sync::mpsc::{channel, Receiver, TryRecvError};
    use std::time::{Duration, Instant};

    use sim::proto::{parse_beacon, Beacon, FrameReader, DEFAULT_PORT, DISCOVERY_PORT};

    use super::Net;

    pub struct Link {
        stream: TcpStream,
        outbox: Vec<u8>,
    }

    impl Link {
        pub fn write(&mut self, frame: &[u8]) {
            self.outbox.extend_from_slice(frame);
            self.flush();
        }

        pub fn flush(&mut self) {
            while !self.outbox.is_empty() {
                match self.stream.write(&self.outbox) {
                    Ok(0) => return,
                    Ok(n) => {
                        self.outbox.drain(..n);
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(_) => return,
                }
            }
        }

        pub fn fill(&mut self, reader: &mut FrameReader) -> Result<(), String> {
            reader.fill(&mut self.stream).map_err(|e| format!("disconnected: {e}"))
        }
    }

    /// A connection attempt running on a helper thread so the UI stays responsive.
    pub struct Connecting {
        rx: Receiver<Result<(TcpStream, SocketAddr), String>>,
    }

    impl Connecting {
        pub fn start(target: &str) -> Connecting {
            let (tx, rx) = channel();
            let t = target.trim().to_string();
            std::thread::spawn(move || {
                let with_port = if t.contains(':') && !t.ends_with(']') { t.clone() } else { format!("{t}:{DEFAULT_PORT}") };
                let res = (|| {
                    let addrs: Vec<SocketAddr> = with_port
                        .to_socket_addrs()
                        .map_err(|e| format!("can't resolve {t}: {e}"))?
                        .collect();
                    let mut last = String::from("no address");
                    for a in addrs {
                        match TcpStream::connect_timeout(&a, Duration::from_secs(4)) {
                            Ok(s) => return Ok((s, a)),
                            Err(e) => last = format!("{a}: {e}"),
                        }
                    }
                    Err(last)
                })();
                let _ = tx.send(res);
            });
            Connecting { rx }
        }

        pub fn poll(&self) -> Option<Result<Net, String>> {
            match self.rx.try_recv() {
                Ok(Ok((stream, _))) => Some(open(stream).map_err(|e| e.to_string())),
                Ok(Err(e)) => Some(Err(e)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err("connection thread died".into())),
            }
        }
    }

    fn open(stream: TcpStream) -> io::Result<Net> {
        stream.set_nodelay(true)?;
        stream.set_nonblocking(true)?;
        Ok(Net::new(Link { stream, outbox: Vec::new() }))
    }

    /// Listens for servers announcing themselves on the LAN.
    pub struct Discovery {
        sock: Option<UdpSocket>,
        pub found: Vec<(SocketAddr, Beacon, Instant)>,
    }

    impl Discovery {
        pub fn new() -> Discovery {
            let sock = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).ok();
            if let Some(s) = &sock {
                let _ = s.set_nonblocking(true);
            }
            Discovery { sock, found: Vec::new() }
        }

        pub fn available(&self) -> bool {
            self.sock.is_some()
        }

        pub fn poll(&mut self) {
            let Some(s) = &self.sock else { return };
            let mut buf = [0u8; 512];
            while let Ok((n, from)) = s.recv_from(&mut buf) {
                if let Some(b) = parse_beacon(&buf[..n]) {
                    let addr = SocketAddr::new(from.ip(), b.port);
                    self.found.retain(|(a, _, _)| *a != addr);
                    self.found.push((addr, b, Instant::now()));
                }
            }
            self.found.retain(|(_, _, t)| t.elapsed() < Duration::from_secs(4));
            self.found.sort_by_key(|(a, _, _)| *a);
        }
    }
}

#[cfg(target_arch = "wasm32")]
mod browser {
    use std::cell::Cell;

    use crate::platform as date;
    use sim::proto::{Beacon, FrameReader};

    use super::Net;
    use crate::web::{self, Socket, SocketState};

    pub struct Link {
        sock: Socket,
    }

    impl Link {
        /// Each frame travels as one binary WebSocket message.
        pub fn write(&mut self, frame: &[u8]) {
            self.sock.send(frame);
        }

        pub fn flush(&mut self) {}

        pub fn fill(&mut self, reader: &mut FrameReader) -> Result<(), String> {
            while let Some(m) = self.sock.recv() {
                reader.push(&m);
            }
            match self.sock.state() {
                SocketState::Closed => Err("disconnected: the server closed the connection".into()),
                _ => Ok(()),
            }
        }
    }

    pub struct Connecting {
        /// Handed over to the `Net` once the socket opens.
        sock: Cell<Option<Socket>>,
        url: String,
        began: f64,
    }

    impl Connecting {
        /// `target` is a `ws://` or `wss://` URL, or a bare `host[:port]`.
        pub fn start(target: &str) -> Connecting {
            let t = target.trim();
            let url = if t.starts_with("ws://") || t.starts_with("wss://") {
                t.to_string()
            } else {
                let scheme = if web::page_is_https() { "wss" } else { "ws" };
                format!("{scheme}://{t}")
            };
            Connecting { sock: Cell::new(Some(Socket::open(&url))), url, began: date::now() }
        }

        pub fn poll(&self) -> Option<Result<Net, String>> {
            let sock = self.sock.take()?;
            match sock.state() {
                SocketState::Open => Some(Ok(Net::new(Link { sock }))),
                SocketState::Closed => Some(Err(format!("can't reach {}", self.url))),
                SocketState::Connecting if date::now() - self.began > 10.0 => Some(Err(format!("{} didn't answer", self.url))),
                SocketState::Connecting => {
                    self.sock.set(Some(sock));
                    None
                }
            }
        }
    }

    /// The browser has no LAN discovery.
    pub struct Discovery {
        pub found: Vec<(std::net::SocketAddr, Beacon, f64)>,
    }

    impl Discovery {
        pub fn new() -> Discovery {
            Discovery { found: Vec::new() }
        }
        pub fn available(&self) -> bool {
            false
        }
        pub fn poll(&mut self) {}
    }
}
