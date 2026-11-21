//! What a browser does: fetch the web build over HTTP, then play over a
//! WebSocket on the same port.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use sim::proto::*;

fn start(web_dir: Option<std::path::PathBuf>) -> server::ServerHandle {
    server::spawn(server::ServerConfig {
        name: "test".into(),
        bind: "127.0.0.1:0".parse().unwrap(),
        maps_dir: server::maps_dir(&server::find_assets()),
        user_maps_dir: None,
        announce: false,
        stop_with_host: false,
        web_dir,
        ai_notes_for_players: false,
    })
    .unwrap()
}

fn connect(addr: SocketAddr) -> TcpStream {
    let s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s
}

/// Send a request and read until the server hangs up.
fn http(addr: SocketAddr, req: &str) -> Vec<u8> {
    let mut s = connect(addr);
    s.write_all(req.as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    out
}

fn body(resp: &[u8]) -> &[u8] {
    let at = resp.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    &resp[at..]
}

#[test]
fn serves_the_web_build_and_nothing_else() {
    let dir = std::env::temp_dir().join(format!("snowballz-web-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), "<html>hi</html>").unwrap();
    std::fs::write(dir.join("assets/a.txt"), "plain").unwrap();
    std::fs::write(dir.join("assets/a.txt.gz"), "zipped").unwrap();
    std::fs::write(dir.join(".secret"), "no").unwrap();
    let h = start(Some(dir.clone()));

    let r = http(h.addr, "GET / HTTP/1.1\r\nHost: x\r\n\r\n");
    assert!(r.starts_with(b"HTTP/1.1 200 OK"));
    assert_eq!(body(&r), b"<html>hi</html>");
    assert!(String::from_utf8_lossy(&r).contains("text/html"));

    let r = http(h.addr, "GET /assets/a.txt HTTP/1.1\r\nHost: x\r\n\r\n");
    assert_eq!(body(&r), b"plain");
    let r = http(h.addr, "GET /assets/a.txt HTTP/1.1\r\nAccept-Encoding: gzip, br\r\n\r\n");
    assert_eq!(body(&r), b"zipped");
    assert!(String::from_utf8_lossy(&r).contains("Content-Encoding: gzip"));

    assert!(http(h.addr, "GET /nope HTTP/1.1\r\n\r\n").starts_with(b"HTTP/1.1 404"));
    assert!(http(h.addr, "GET /../Cargo.toml HTTP/1.1\r\n\r\n").starts_with(b"HTTP/1.1 404"));
    assert!(http(h.addr, "GET /.secret HTTP/1.1\r\n\r\n").starts_with(b"HTTP/1.1 404"));

    // The game still works on the same port while it serves files.
    let mut s = connect(h.addr);
    let mut f = Vec::new();
    write_frame(&mut f, &ClientMsg::Hello { version: PROTOCOL_VERSION, name: "native".into(), token: 0 }.encode()).unwrap();
    s.write_all(&f).unwrap();
    let mut r = FrameReader::default();
    s.set_nonblocking(true).unwrap();
    let mut got_welcome = false;
    for _ in 0..200 {
        r.fill(&mut s).unwrap();
        while let Some(frame) = r.next().unwrap() {
            got_welcome |= matches!(ServerMsg::decode(&frame), Ok(ServerMsg::Welcome { .. }));
        }
        if got_welcome {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(got_welcome);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn no_web_dir_means_a_polite_404() {
    let h = start(None);
    assert!(http(h.addr, "GET / HTTP/1.1\r\n\r\n").starts_with(b"HTTP/1.1 404"));
}

/// A masked client frame, as a browser sends.
fn client_frame(payload: &[u8]) -> Vec<u8> {
    let mask = [9, 8, 7, 6];
    let mut f = vec![0x82];
    assert!(payload.len() < 126);
    f.push(0x80 | payload.len() as u8);
    f.extend_from_slice(&mask);
    f.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    f
}

/// Read one unmasked server frame: (opcode, payload).
fn read_server_frame(s: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut head = [0u8; 2];
    s.read_exact(&mut head).unwrap();
    assert_eq!(head[1] & 0x80, 0, "server frames are not masked");
    let len = match head[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            s.read_exact(&mut b).unwrap();
            u16::from_be_bytes(b) as usize
        }
        127 => {
            let mut b = [0u8; 8];
            s.read_exact(&mut b).unwrap();
            u64::from_be_bytes(b) as usize
        }
        n => n as usize,
    };
    let mut payload = vec![0; len];
    s.read_exact(&mut payload).unwrap();
    (head[0] & 0x0F, payload)
}

#[test]
fn websocket_carries_the_game_protocol() {
    let h = start(None);
    let mut s = connect(h.addr);
    s.write_all(
        b"GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
          Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n",
    )
    .unwrap();
    // Read the response head byte by byte so no frame bytes are swallowed.
    let mut head = Vec::new();
    while !head.ends_with(b"\r\n\r\n") {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).unwrap();
        head.push(b[0]);
    }
    let head = String::from_utf8(head).unwrap();
    assert!(head.starts_with("HTTP/1.1 101"));
    assert!(head.contains("Sec-WebSocket-Accept: s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));

    let mut f = Vec::new();
    write_frame(&mut f, &ClientMsg::Hello { version: PROTOCOL_VERSION, name: "browser".into(), token: 0 }.encode()).unwrap();
    s.write_all(&client_frame(&f)).unwrap();

    // Each server message arrives as one binary WebSocket message holding a
    // normal length-prefixed frame.
    let mut got = (false, false);
    let mut chat_sent = false;
    for _ in 0..10 {
        let (op, payload) = read_server_frame(&mut s);
        assert_eq!(op, 2);
        let mut r = FrameReader::default();
        r.push(&payload);
        let frame = r.next().unwrap().expect("one whole frame per message");
        match ServerMsg::decode(&frame).unwrap() {
            ServerMsg::Welcome { .. } => got.0 = true,
            ServerMsg::Lobby(l) => {
                got.1 = l.slots.iter().any(|s| s.name == "browser");
                if !chat_sent {
                    chat_sent = true;
                    // A ping is answered with a pong.
                    let mask = [1, 2, 3, 4];
                    let mut ping = vec![0x89, 0x80 | 2];
                    ping.extend_from_slice(&mask);
                    ping.extend([b'h' ^ mask[0], b'i' ^ mask[1]]);
                    s.write_all(&ping).unwrap();
                }
            }
            _ => {}
        }
        if got.0 && got.1 {
            break;
        }
    }
    assert!(got.0 && got.1, "welcome and a lobby with the browser seated");
    // The pong comes back through the same stream.
    loop {
        let (op, payload) = read_server_frame(&mut s);
        if op == 0xA {
            assert_eq!(payload, b"hi");
            break;
        }
    }
}
