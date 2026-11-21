//! Asks a local language model to design a map. The model answers with a
//! JSON map spec (see `sim::mapgen`). With no model, or a useless answer, the
//! spec is made from the words in the request instead.
//!
//! Plain HTTP over `std::net`, so no crates. Only `http://` URLs, which is
//! what local servers speak. The work runs on a thread; the UI polls for it.

use crate::settings::Settings;
use sim::json::{quote, Json};
use sim::mapgen::{MapSpec, SPEC_GUIDE};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// Where llama-server, Ollama and LM Studio listen by default.
const CANDIDATES: [&str; 3] = ["http://127.0.0.1:8080/v1", "http://127.0.0.1:11434/v1", "http://127.0.0.1:1234/v1"];
const NONE_FOUND: &str = "No local model found,";
const PROBE: Duration = Duration::from_secs(2);
const LOAD_WAIT: Duration = Duration::from_secs(240);
const LOAD_POLL: Duration = Duration::from_secs(3);
const REPLY: Duration = Duration::from_secs(300);

pub struct LlmConfig {
    pub url: String,
    pub model: String,
    pub start: String,
}

impl LlmConfig {
    /// `SNOWBALLZ_LLM_URL`, if set, wins over the saved URL.
    pub fn from_settings(s: &Settings) -> LlmConfig {
        let url = std::env::var("SNOWBALLZ_LLM_URL").ok().filter(|u| !u.trim().is_empty()).unwrap_or_else(|| s.llm_url.clone());
        LlmConfig { url: url.trim().trim_end_matches('/').into(), model: s.llm_model.trim().into(), start: s.llm_start.clone() }
    }
}

pub enum MapJob {
    /// Still going; text for the player, e.g. "Starting the local model…"
    Working(String),
    /// `from_model` is false when we fell back to MapSpec::from_words; `note` says why (plain English, may be empty).
    Done { spec: MapSpec, from_model: bool, note: String },
}

pub struct MapRequest {
    rx: Receiver<MapJob>,
}

/// Starts a background thread; never blocks the caller.
pub fn request_map(cfg: LlmConfig, prompt: String, seed: u64) -> MapRequest {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let done = run(&cfg, &prompt, seed, &tx);
        let _ = tx.send(done);
    });
    MapRequest { rx }
}

impl MapRequest {
    /// Newest message since the last call, if any.
    pub fn poll(&mut self) -> Option<MapJob> {
        let mut newest = None;
        while let Ok(job) = self.rx.try_recv() {
            newest = Some(job);
        }
        newest
    }
}

fn run(cfg: &LlmConfig, prompt: &str, seed: u64, tx: &Sender<MapJob>) -> MapJob {
    let say = |s: &str| {
        let _ = tx.send(MapJob::Working(s.into()));
    };
    let (base, models) = match find_server(cfg, &say) {
        Ok(found) => found,
        Err(why) => return fallback(prompt, seed, &why),
    };
    let model = if cfg.model.is_empty() { first_model(&models) } else { cfg.model.clone() };
    say("The model is designing your map…");
    let body = format!(
        "{{\"model\":{},\"messages\":[{{\"role\":\"system\",\"content\":{}}},{{\"role\":\"user\",\"content\":{}}}],\
         \"temperature\":0.8,\"max_tokens\":2500,\"response_format\":{{\"type\":\"json_object\"}},\
         \"chat_template_kwargs\":{{\"enable_thinking\":false}},\"reasoning_effort\":\"low\",\"stream\":false}}",
        quote(&model),
        quote(SPEC_GUIDE),
        quote(prompt)
    );
    let reply = match http("POST", &format!("{base}/chat/completions"), Some(&body), REPLY) {
        Ok((200, text)) => text,
        Ok((code, _)) => return fallback(prompt, seed, &format!("The local model answered with HTTP {code},")),
        Err(e) => return fallback(prompt, seed, &format!("The local model did not answer ({e}),")),
    };
    match spec_from_reply(&reply, seed) {
        Ok(spec) => MapJob::Done { spec, from_model: true, note: String::new() },
        Err(why) => fallback(prompt, seed, &format!("{why},")),
    }
}

fn fallback(prompt: &str, seed: u64, why: &str) -> MapJob {
    MapJob::Done {
        spec: MapSpec::from_words(prompt, seed),
        from_model: false,
        note: format!("{why} so the map was made from your words."),
    }
}

/// The base URL of a running server and its `/models` reply.
fn find_server(cfg: &LlmConfig, say: &dyn Fn(&str)) -> Result<(String, String), String> {
    let candidates: Vec<&str> = if cfg.url.is_empty() { CANDIDATES.to_vec() } else { vec![cfg.url.as_str()] };
    if let Some(found) = probe(&candidates) {
        return Ok(found);
    }
    let start = cfg.start.trim();
    if start.is_empty() {
        return Err(NONE_FOUND.into());
    }
    say("Starting the local model…");
    spawn(start).map_err(|e| format!("Could not start the local model ({e}),"))?;
    let began = Instant::now();
    while began.elapsed() < LOAD_WAIT {
        std::thread::sleep(LOAD_POLL);
        if let Some(found) = probe(&candidates) {
            return Ok(found);
        }
        say(&format!("Waiting for the local model to load… {}s", began.elapsed().as_secs()));
    }
    Err(NONE_FOUND.into())
}

fn probe(candidates: &[&str]) -> Option<(String, String)> {
    candidates.iter().find_map(|base| match http("GET", &format!("{base}/models"), None, PROBE) {
        Ok((200, body)) => Some((base.to_string(), body)),
        _ => None,
    })
}

/// Runs a start command detached: it keeps going after we return.
fn spawn(cmd: &str) -> std::io::Result<()> {
    let mut c = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C");
        c
    } else {
        let mut c = Command::new("sh");
        c.arg("-c");
        c
    };
    c.arg(cmd).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().map(drop)
}

/// The id of the first model in a `/models` reply.
fn first_model(models: &str) -> String {
    Json::parse(models)
        .ok()
        .and_then(|j| j.get("data").map(Json::as_arr)?.first()?.get("id")?.as_str().map(String::from))
        .unwrap_or_default()
}

fn spec_from_reply(reply: &str, seed: u64) -> Result<MapSpec, String> {
    let j = Json::parse(reply).map_err(|_| "The local model's reply was not JSON".to_string())?;
    let message = j
        .get("choices")
        .map(Json::as_arr)
        .and_then(|c| c.first())
        .and_then(|c| c.get("message"))
        .ok_or("The local model's reply had no message")?;
    let text = |k: &str| message.get(k).and_then(Json::as_str).filter(|s| !s.trim().is_empty());
    let content = text("content").or_else(|| text("reasoning_content")).ok_or("The local model's reply was empty")?;
    let obj = Json::find_object(content).map_err(|_| "The local model's reply had no map in it".to_string())?;
    Ok(MapSpec::from_json(&obj, seed))
}

/// `http://host:port/path` into `host:port` and `/path`.
fn split_url(url: &str) -> Result<(&str, &str), String> {
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs are supported")?;
    Ok(match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    })
}

/// One request, one reply. Returns the status code and the body.
fn http(method: &str, url: &str, body: Option<&str>, timeout: Duration) -> Result<(u16, String), String> {
    let (hostport, path) = split_url(url)?;
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{hostport}:80") };
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or_else(|| format!("could not find {hostport}"))?;
    let mut s = TcpStream::connect_timeout(&sock, timeout).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\n\r\n{body}",
        body.len()
    );
    s.write_all(req.as_bytes()).map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).map_err(|e| e.to_string())?;

    let split = find(&raw, b"\r\n\r\n").ok_or("the reply had no headers")?;
    let head = String::from_utf8_lossy(&raw[..split]).to_ascii_lowercase();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .ok_or("the reply had no status code")?;
    let rest = &raw[split + 4..];
    let body = if head.contains("transfer-encoding: chunked") {
        dechunk(rest)?
    } else {
        match head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse::<usize>().ok()) {
            Some(n) if n <= rest.len() => rest[..n].to_vec(),
            _ => rest.to_vec(),
        }
    };
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// Joins the pieces of a `Transfer-Encoding: chunked` body.
fn dechunk(mut b: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let line = find(b, b"\r\n").ok_or("bad chunk")?;
        let size = std::str::from_utf8(&b[..line])
            .ok()
            .and_then(|l| usize::from_str_radix(l.split(';').next()?.trim(), 16).ok())
            .ok_or("bad chunk size")?;
        b = &b[line + 2..];
        if size == 0 {
            return Ok(out);
        }
        if b.len() < size {
            return Err("the reply was cut short".into());
        }
        out.extend_from_slice(&b[..size]);
        b = b.get(size + 2..).unwrap_or(&[]);
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Waits for the map request to finish.
    fn wait(mut req: MapRequest) -> MapJob {
        let began = Instant::now();
        loop {
            match req.poll() {
                Some(MapJob::Working(_)) => {}
                Some(done) => return done,
                None => std::thread::sleep(Duration::from_millis(10)),
            }
            assert!(began.elapsed() < Duration::from_secs(60), "map request never finished");
        }
    }

    /// Reads one request: the headers and the body that follows them.
    fn read_request(s: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match s.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = find(&buf, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_ascii_lowercase();
                let len: usize =
                    head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    /// A server that answers /models and /chat/completions. The chat reply
    /// comes back chunked, as llama-server sends it.
    fn fake_server(content: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let req = read_request(&mut s);
                let reply = if req.starts_with("GET ") {
                    let body = r#"{"object":"list","data":[{"id":"fake-model"}]}"#;
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                } else {
                    let body = format!(r#"{{"choices":[{{"message":{{"content":{}}}}}]}}"#, quote(&content));
                    let (a, b) = body.split_at(body.len() / 2);
                    format!(
                        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n",
                        a.len(),
                        b.len()
                    )
                };
                let _ = s.write_all(reply.as_bytes());
            }
        });
        url
    }

    #[test]
    fn chunked_bodies_are_joined() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
        assert!(dechunk(b"9\r\nshort\r\n").is_err());
    }

    #[test]
    fn urls_are_split_into_host_and_path() {
        assert_eq!(split_url("http://127.0.0.1:8090/v1/models").unwrap(), ("127.0.0.1:8090", "/v1/models"));
        assert_eq!(split_url("http://localhost").unwrap(), ("localhost", "/"));
        assert!(split_url("https://example.com/v1").is_err());
    }

    #[test]
    fn asks_the_server_and_reads_its_map() {
        let content = "Here is your map:\n```json\n{\"name\":\"Crystal Isles\",\"size\":96,\"igloos\":5,\
                       \"layout\":\"scattered\",\"victory\":\"gather\",\"gather\":40}\n```\nEnjoy!";
        let cfg = LlmConfig { url: fake_server(content.into()), model: String::new(), start: String::new() };
        match wait(request_map(cfg, "crystal isles".into(), 7)) {
            MapJob::Done { spec, from_model, note } => {
                assert!(from_model, "{note}");
                assert!(note.is_empty());
                assert_eq!(spec.name, "Crystal Isles");
                assert_eq!(spec.igloos, 5);
            }
            MapJob::Working(_) => panic!("no map"),
        }
    }

    #[test]
    fn unreachable_server_falls_back_to_words() {
        let cfg = LlmConfig { url: "http://127.0.0.1:9/v1".into(), model: String::new(), start: String::new() };
        let began = Instant::now();
        match wait(request_map(cfg, "frozen islands".into(), 3)) {
            MapJob::Done { spec, from_model, note } => {
                assert!(!from_model);
                assert_eq!(note, "No local model found, so the map was made from your words.");
                assert_eq!(spec, MapSpec::from_words("frozen islands", 3));
            }
            MapJob::Working(_) => panic!("no map"),
        }
        assert!(began.elapsed() < Duration::from_secs(10));
    }

    /// Needs a real model: `SNOWBALLZ_LLM_URL=http://127.0.0.1:8090/v1 cargo test --release -p client real_local_model -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_local_model() {
        let cfg = LlmConfig::from_settings(&Settings::default());
        let prompt = "a frozen archipelago where five tribes fight over crystal on a central island";
        let began = Instant::now();
        let mut req = request_map(cfg, prompt.into(), 1);
        loop {
            match req.poll() {
                Some(MapJob::Working(s)) => println!("{s}"),
                Some(MapJob::Done { spec, from_model, note }) => {
                    println!("{}", spec.to_json());
                    println!("from_model={from_model} note={note:?} took {:.1}s", began.elapsed().as_secs_f32());
                    assert!(from_model, "{note}");
                    break;
                }
                None => std::thread::sleep(Duration::from_millis(200)),
            }
        }
    }
}
