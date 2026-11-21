//! Just enough HTTP and WebSocket (RFC 6455) for browsers to play.
//!
//! A browser can't open a raw TCP socket, so the web build talks to the same
//! port through a WebSocket, which carries the ordinary framed protocol as
//! binary messages. The same port also serves the web build's files, so a
//! friend only needs one URL. No dependencies: SHA-1 and base64 are only
//! needed for the handshake.

use std::path::{Path, PathBuf};

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&(data.len() as u64 * 8).to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, c) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, &wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

fn base64(data: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The `Sec-WebSocket-Accept` value that answers a client's key.
pub fn accept_key(key: &str) -> String {
    base64(&sha1(format!("{}258EAFA5-E914-47DA-95CA-C5AB0DC85B11", key.trim()).as_bytes()))
}

// -------------------------------------------------------------------------
// Frames

const OP_CONT: u8 = 0x0;
const OP_BINARY: u8 = 0x2;
const OP_CLOSE: u8 = 0x8;
const OP_PING: u8 = 0x9;
const OP_PONG: u8 = 0xA;

/// Largest message we accept: the biggest game frame a client may send
/// (a map from the host) plus its length prefix.
const MAX_MESSAGE: usize = crate::HOST_FRAME + 16;

/// A server-to-client frame (never masked).
pub fn frame(op: u8, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(payload.len() + 10);
    f.push(0x80 | op);
    match payload.len() {
        n if n < 126 => f.push(n as u8),
        n if n < 65536 => {
            f.push(126);
            f.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            f.push(127);
            f.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    f.extend_from_slice(payload);
    f
}

pub fn binary(payload: &[u8]) -> Vec<u8> {
    frame(OP_BINARY, payload)
}

/// What the other end sent, once a whole frame has arrived.
#[derive(Debug, PartialEq)]
pub enum Event {
    /// Application bytes (binary or text message, fragments joined).
    Data(Vec<u8>),
    /// Answer this ping with a pong of the same bytes.
    Ping(Vec<u8>),
    Close,
}

#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
    /// Bytes at the front of `buf` already decoded; dropped when more
    /// arrives so a run of tiny frames costs one pass, not a shift each.
    start: usize,
    message: Vec<u8>,
    /// Between the first fragment of a message and its last.
    fragmenting: bool,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) {
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
        self.buf.extend_from_slice(bytes);
    }

    /// Bytes waiting to be decoded.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start + self.message.len()
    }

    /// The next complete event, `Ok(None)` if more bytes are needed.
    pub fn next(&mut self) -> Result<Option<Event>, &'static str> {
        loop {
            let b = &self.buf[self.start..];
            if b.len() < 2 {
                return Ok(None);
            }
            let fin = b[0] & 0x80 != 0;
            let op = b[0] & 0x0F;
            if b[0] & 0x70 != 0 {
                return Err("unsupported extension bits");
            }
            if b[1] & 0x80 == 0 {
                return Err("client frames must be masked");
            }
            let (len, mut at) = match b[1] & 0x7F {
                126 if b.len() >= 4 => (u16::from_be_bytes([b[2], b[3]]) as usize, 4),
                127 if b.len() >= 10 => (u64::from_be_bytes(b[2..10].try_into().unwrap()) as usize, 10),
                126 | 127 => return Ok(None),
                n => (n as usize, 2),
            };
            if len > MAX_MESSAGE {
                return Err("message too large");
            }
            // Control frames are short and never fragmented (RFC 6455 5.5).
            if op >= OP_CLOSE && (len > 125 || !fin) {
                return Err("bad control frame");
            }
            if b.len() < at + 4 + len {
                return Ok(None);
            }
            let mask = [b[at], b[at + 1], b[at + 2], b[at + 3]];
            at += 4;
            let payload: Vec<u8> = b[at..at + len].iter().enumerate().map(|(i, x)| x ^ mask[i % 4]).collect();
            self.start += at + len;
            match op {
                OP_PING => return Ok(Some(Event::Ping(payload))),
                OP_PONG => {}
                OP_CLOSE => return Ok(Some(Event::Close)),
                OP_CONT | 0x1 | OP_BINARY => {
                    // A continuation needs a message to continue, and a new
                    // message can't start in the middle of another.
                    if (op == OP_CONT) != self.fragmenting {
                        return Err("fragments out of order");
                    }
                    self.fragmenting = !fin;
                    self.message.extend_from_slice(&payload);
                    if self.message.len() > MAX_MESSAGE {
                        return Err("message too large");
                    }
                    if fin {
                        return Ok(Some(Event::Data(std::mem::take(&mut self.message))));
                    }
                }
                _ => return Err("unknown opcode"),
            }
        }
    }
}

pub fn pong(payload: &[u8]) -> Vec<u8> {
    frame(OP_PONG, payload)
}

pub fn close() -> Vec<u8> {
    frame(OP_CLOSE, &[])
}

// -------------------------------------------------------------------------
// HTTP

pub struct Request {
    pub path: String,
    headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Does this ask to become a WebSocket? Returns the client's key.
    pub fn websocket_key(&self) -> Option<&str> {
        let up = self.header("upgrade")?;
        let conn = self.header("connection")?;
        let version_ok = self.header("sec-websocket-version").is_none_or(|v| v == "13");
        (up.eq_ignore_ascii_case("websocket") && conn.to_ascii_lowercase().contains("upgrade") && version_ok)
            .then(|| self.header("sec-websocket-key"))
            .flatten()
    }
}

/// Parse a request head; `None` until the blank line has arrived. Returns
/// the request and how many bytes it used.
pub fn parse_request(buf: &[u8]) -> Result<Option<(Request, usize)>, &'static str> {
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") else {
        return Ok(None);
    };
    let head = std::str::from_utf8(&buf[..end]).map_err(|_| "bad request")?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    first.next(); // the method: only GET gets this far
    let path = first.next().ok_or("bad request line")?.to_string();
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string()))
        .collect();
    Ok(Some((Request { path, headers }, end + 4)))
}

pub fn upgrade_response(key: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n",
        accept_key(key)
    )
    .into_bytes()
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "txt" | "json" => "text/plain; charset=utf-8",
        "ogg" => "audio/ogg",
        _ => "application/octet-stream",
    }
}

pub(crate) fn respond(status: &str, ctype: &str, extra: &str, body: &[u8]) -> Vec<u8> {
    let mut r = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    r.extend_from_slice(body);
    r
}

/// Does an Accept-Encoding header allow gzip (and not `gzip;q=0`)?
fn accepts_gzip(header: &str) -> bool {
    header.split(',').any(|part| {
        let mut bits = part.split(';').map(str::trim);
        let coding = bits.next().unwrap_or("");
        let refused = bits.any(|b| b.strip_prefix("q=").is_some_and(|q| q.parse::<f32>().is_ok_and(|q| q <= 0.0)));
        coding.eq_ignore_ascii_case("gzip") && !refused
    })
}

/// Map a URL path to a file under `root`, refusing anything that could
/// climb out of it.
fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let path = url_path.split(['?', '#']).next().unwrap_or("");
    let mut out = root.to_path_buf();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if part.starts_with('.') || part.contains(['\\', '\0', ':']) {
            return None;
        }
        out.push(part);
    }
    if out == root {
        out.push("index.html");
    }
    Some(out)
}

/// The response to a plain GET: a file from the web build, or an error page.
pub fn serve(web_dir: Option<&Path>, req: &Request) -> Vec<u8> {
    let Some(root) = web_dir else {
        return respond("404 Not Found", "text/plain", "", b"this server has no web build\n");
    };
    let Some(path) = resolve(root, &req.path) else {
        return respond("404 Not Found", "text/plain", "", b"not found\n");
    };
    let gzip_ok = req.header("accept-encoding").is_some_and(accepts_gzip);
    let gz = PathBuf::from(format!("{}.gz", path.display()));
    let (file, data, encoding) = match (gzip_ok.then(|| std::fs::read(&gz).ok().map(|d| (gz.clone(), d))).flatten(), std::fs::read(&path)) {
        (Some((f, zipped)), _) => (f, zipped, "Content-Encoding: gzip\r\n"),
        (None, Ok(plain)) => (path.clone(), plain, ""),
        (None, Err(_)) => return respond("404 Not Found", "text/plain", "", b"not found\n"),
    };
    // Browsers revalidate every visit, but a file that hasn't changed costs
    // only a 304, not the whole 600 KB wasm again.
    let modified = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
    let secs = modified.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let etag = format!("\"{:x}-{:x}{}\"", data.len(), secs, if encoding.is_empty() { "" } else { "-gz" });
    let extra = format!("{encoding}ETag: {etag}\r\nCache-Control: no-cache\r\nVary: Accept-Encoding\r\n");
    if req.header("if-none-match") == Some(etag.as_str()) {
        return respond("304 Not Modified", content_type(&path), &extra, b"");
    }
    respond("200 OK", content_type(&path), &extra, &data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_matches_known_vectors() {
        assert_eq!(hex(&sha1(b"abc")), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(hex(&sha1(b"")), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        // Two blocks, and padding that spills into a second one.
        assert_eq!(
            hex(&sha1(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
    }

    #[test]
    fn handshake_matches_the_rfc_example() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn base64_pads() {
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
    }

    fn masked(op: u8, fin: bool, payload: &[u8]) -> Vec<u8> {
        let mask = [0x12, 0x34, 0x56, 0x78];
        let mut f = vec![if fin { 0x80 } else { 0 } | op];
        match payload.len() {
            n if n < 126 => f.push(0x80 | n as u8),
            n => {
                f.push(0x80 | 126);
                f.extend_from_slice(&(n as u16).to_be_bytes());
            }
        }
        f.extend_from_slice(&mask);
        f.extend(payload.iter().enumerate().map(|(i, x)| x ^ mask[i % 4]));
        f
    }

    #[test]
    fn decodes_masked_frames_split_across_reads() {
        let long = vec![7u8; 300];
        let mut wire = masked(OP_BINARY, true, b"hello");
        wire.extend(masked(OP_BINARY, true, &long));
        let mut d = Decoder::default();
        for chunk in wire.chunks(7) {
            d.push(chunk);
        }
        assert_eq!(d.next().unwrap(), Some(Event::Data(b"hello".to_vec())));
        assert_eq!(d.next().unwrap(), Some(Event::Data(long)));
        assert_eq!(d.next().unwrap(), None);
    }

    #[test]
    fn joins_fragments_and_answers_pings_between_them() {
        let mut d = Decoder::default();
        d.push(&masked(OP_BINARY, false, b"ab"));
        d.push(&masked(OP_PING, true, b"p"));
        d.push(&masked(OP_CONT, true, b"cd"));
        assert_eq!(d.next().unwrap(), Some(Event::Ping(b"p".to_vec())));
        assert_eq!(d.next().unwrap(), Some(Event::Data(b"abcd".to_vec())));
    }

    #[test]
    fn control_frames_must_be_short_and_whole() {
        let mut d = Decoder::default();
        d.push(&masked(OP_PING, true, &[0; 126]));
        assert!(d.next().is_err(), "ping over 125 bytes");
        let mut d = Decoder::default();
        d.push(&masked(OP_PING, false, b"x"));
        assert!(d.next().is_err(), "fragmented ping");
    }

    #[test]
    fn fragments_must_be_in_order() {
        let mut d = Decoder::default();
        d.push(&masked(OP_CONT, true, b"x"));
        assert!(d.next().is_err(), "continuation with nothing to continue");
        let mut d = Decoder::default();
        d.push(&masked(OP_BINARY, false, b"a"));
        d.push(&masked(OP_BINARY, true, b"b"));
        assert!(d.next().is_err(), "new message inside an unfinished one");
    }

    #[test]
    fn a_pile_of_tiny_frames_decodes_quickly() {
        let mut d = Decoder::default();
        let one = masked(OP_BINARY, true, b"hi");
        d.push(&one.repeat(200_000));
        let t = std::time::Instant::now();
        let mut n = 0;
        while let Some(Event::Data(_)) = d.next().unwrap() {
            n += 1;
        }
        assert_eq!(n, 200_000);
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn gzip_is_only_used_when_allowed() {
        assert!(accepts_gzip("gzip, deflate, br"));
        assert!(accepts_gzip("deflate, GZIP;q=0.5"));
        assert!(!accepts_gzip("gzip;q=0"));
        assert!(!accepts_gzip("identity"));
    }

    #[test]
    fn unchanged_files_get_a_304() {
        let dir = std::env::temp_dir().join(format!("snowballz-etag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.html"), "<html>hi</html>").unwrap();
        let get = |extra: &str| {
            let raw = format!("GET / HTTP/1.1\r\nHost: x\r\n{extra}\r\n");
            let (req, _) = parse_request(raw.as_bytes()).unwrap().unwrap();
            String::from_utf8_lossy(&serve(Some(&dir), &req)).into_owned()
        };
        let first = get("");
        assert!(first.starts_with("HTTP/1.1 200"));
        let etag = first.lines().find_map(|l| l.strip_prefix("ETag: ")).unwrap().to_string();
        assert!(first.contains("Cache-Control: no-cache"));
        let again = get(&format!("If-None-Match: {etag}\r\n"));
        assert!(again.starts_with("HTTP/1.1 304"), "{again}");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unmasked_client_frames() {
        let mut d = Decoder::default();
        d.push(&binary(b"hi"));
        assert!(d.next().is_err());
    }

    #[test]
    fn server_frames_have_the_right_length_forms() {
        assert_eq!(binary(&[1; 5])[..2], [0x82, 5]);
        assert_eq!(binary(&[1; 300])[..4], [0x82, 126, 1, 44]);
        assert_eq!(binary(&vec![1; 70000])[..2], [0x82, 127]);
    }

    #[test]
    fn parses_an_upgrade_request() {
        let raw = b"GET /ws HTTP/1.1\r\nHost: x\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Key: abc==\r\n\r\nEXTRA";
        let (req, used) = parse_request(raw).unwrap().unwrap();
        assert_eq!(&raw[used..], b"EXTRA");
        assert_eq!(req.websocket_key(), Some("abc=="));
        assert!(parse_request(b"GET / HTTP/1.1\r\nHost").unwrap().is_none());
    }

    #[test]
    fn static_paths_cannot_escape_the_web_dir() {
        let root = Path::new("/srv/web");
        assert_eq!(resolve(root, "/").unwrap(), root.join("index.html"));
        assert_eq!(resolve(root, "/assets/maps/a.sbmap?x=1").unwrap(), root.join("assets/maps/a.sbmap"));
        assert!(resolve(root, "/../etc/passwd").is_none());
        assert!(resolve(root, "/a/..%2f").is_none());
        assert!(resolve(root, "/.git/config").is_none());
        assert!(resolve(root, "/a\\b").is_none());
    }
}
