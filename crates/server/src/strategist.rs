//! Lets a local language model steer the computer players. Every so often
//! each computer's report goes to the model, which answers with a
//! `Directive`: a posture, a target and a few preferences. The computer's own
//! rules still do all the playing, and with no model, a slow one or a
//! useless answer, they carry on as before.
//!
//! Switched on by `--llm` (or `SNOWBALLZ_LLM_MODEL`). In a build with the
//! `llm` feature the model runs inside the server (`engine.rs`), fetched from
//! Hugging Face the first time. Otherwise, or when the name is an Ollama
//! one (`qwen3.5:4b`), the server looks for a model server by itself (Ollama,
//! llama-server and LM Studio on their usual ports) and, if there is none,
//! starts Ollama (`SNOWBALLZ_LLM_START` says how to start another).
//! `SNOWBALLZ_LLM_URL` pins an OpenAI-style base such as
//! `http://127.0.0.1:8080/v1`. All of it runs on a thread, so the game never
//! waits for the model.

use sim::ai::{Directive, CHAT_GUIDE, DIRECTIVE_GUIDE, TALK_GUIDE};
use sim::game::Game;
use sim::json::{quote, Json};
use sim::TICK_RATE;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// Game time before the first question, and between rounds of them.
const FIRST_AFTER: u32 = 15 * TICK_RATE;
const EVERY: u32 = 30 * TICK_RATE;
const CONNECT: Duration = Duration::from_secs(5);
const REPLY: Duration = Duration::from_secs(120);
const PROBE: Duration = Duration::from_secs(2);
/// How long a model server we started gets to come up.
const START_WAIT: Duration = Duration::from_secs(60);
/// A model download that takes longer than this is given up on.
const PULL_WAIT: Duration = Duration::from_secs(45 * 60);
/// The model `--llm auto` asks for: small, quick and fine at this job.
pub const DEFAULT_MODEL: &str = "qwen3.5:4b";
/// Where Ollama, llama-server and LM Studio listen by default.
const CANDIDATES: [&str; 4] = ["http://127.0.0.1:11434/v1", "http://127.0.0.1:8080/v1", "http://127.0.0.1:1234/v1", "http://127.0.0.1:8090/v1"];

/// What the model thread is asked.
enum Job {
    /// Advise computer player `.0` given its report.
    Advise(u8, String),
    /// Put a reply into the computer's own words.
    Speak(Line),
}

/// A computer's reply to a chat message, and what is needed to reword it.
struct Line {
    player: u8,
    to: Option<u8>,
    /// What the other player said.
    heard: String,
    /// What the game decided the answer is (for small talk, the game's own
    /// line, said if the model has nothing better).
    fact: String,
    /// A deal or a fact about the game: the model may only reword it.
    binding: bool,
    /// The last few lines of chat, oldest first.
    recent: String,
    report: String,
}

enum Answer {
    Advice(u8, Result<Advice, String>),
    /// The reworded reply, and how it came about (for the log); on failure
    /// a binding fact goes out as it was. `aloud` is false for small talk the
    /// model didn't answer: the game's stock line only goes in the log.
    Said(u8, Option<u8>, String, &'static str, bool),
}

pub struct Strategist {
    jobs: Sender<Job>,
    /// Chat goes on its own line, ahead of any advice: people are waiting.
    talk: Sender<Job>,
    replies: Receiver<Answer>,
    next_round: u32,
    /// Questions asked and not yet answered: no new round until it is zero.
    outstanding: usize,
    complained: bool,
    /// The model thread has found, started or downloaded its model (or
    /// given up). Until then nobody waits on it.
    ready: Arc<AtomicBool>,
    /// The game's recent chat, "Name: text", so a reply follows the talk.
    log: Vec<String>,
}

impl Strategist {
    /// Set up from the environment, or `None` if no model is configured.
    pub fn from_env() -> Option<Strategist> {
        let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let (url, model, start) = (var("SNOWBALLZ_LLM_URL"), var("SNOWBALLZ_LLM_MODEL"), var("SNOWBALLZ_LLM_START"));
        if url.is_none() && model.is_none() {
            return None;
        }
        Some(Strategist::start_with(url.unwrap_or_default(), model.unwrap_or_default(), start.unwrap_or_default()))
    }

    /// `url` empty means look for a server; `start` is a shell command that
    /// starts one (empty: Ollama, if a model is named and Ollama is there).
    pub fn start_with(url: String, model: String, start: String) -> Strategist {
        let (jobs, job_rx) = mpsc::channel();
        let (talk, talk_rx) = mpsc::channel();
        let (reply_tx, replies) = mpsc::channel();
        let ready = Arc::new(AtomicBool::new(false));
        let flag = ready.clone();
        std::thread::spawn(move || work(Runner::new(url, model, start), talk_rx, job_rx, reply_tx, flag));
        Strategist { jobs, talk, replies, next_round: FIRST_AFTER, outstanding: 0, complained: false, ready, log: Vec::new() }
    }

    /// Once per server tick: apply any answers, and start a new round of
    /// questions when it is time.
    pub fn step(&mut self, g: &mut Game) {
        while let Ok(answer) = self.replies.try_recv() {
            match answer {
                Answer::Said(player, to, text, how, aloud) => {
                    g.ai_log(player, format!("Reply wording: {how}"));
                    if aloud {
                        g.ai_say(player, to, text);
                    } else {
                        g.ai_think(player, to, text);
                    }
                }
                Answer::Advice(player, reply) => {
                    self.outstanding = self.outstanding.saturating_sub(1);
                    match reply {
                        Ok(Advice { directive: d, reason: why, say }) => {
                            g.set_ai_directive(player, d);
                            // The model's own words for a new plan, if it gave any.
                            if let (Some(plan), Some(say)) = (d.plan, say) {
                                g.set_ai_plan(player, plan, Some(say));
                            }
                            if let Some(why) = why {
                                g.ai_log(player, format!("  advisor's reason: {why}"));
                            }
                            self.complained = false;
                        }
                        Err(why) if !self.complained => {
                            eprintln!("The strategy model did not help player {player} ({why}); the computer plays on alone.");
                            self.complained = true;
                        }
                        Err(_) => {}
                    }
                }
            }
        }
        if self.ready.load(Ordering::Relaxed) && self.outstanding == 0 && g.tick >= self.next_round {
            self.next_round = g.tick + EVERY;
            for (player, report) in g.ai_briefings() {
                if self.jobs.send(Job::Advise(player, report)).is_ok() {
                    self.outstanding += 1;
                }
            }
        }
    }

    /// A computer's reply to `heard` has been decided; have the model put it
    /// in the computer's own words. It is said when the model answers, or
    /// as it stands if the model can't.
    #[cfg(test)]
    fn reword(&mut self, g: &mut Game, player: u8, to: Option<u8>, heard: &str, fact: String) {
        self.answer(g, player, to, heard, fact, true);
    }

    /// Remember a line of chat, so replies can follow the conversation.
    pub fn heard(&mut self, who: &str, text: &str) {
        self.log.push(format!("{who}: {}", text.chars().take(160).collect::<String>()));
        if self.log.len() > 8 {
            self.log.remove(0);
        }
    }

    /// A computer answers chat: binding replies are only reworded, small
    /// talk is the model's own answer to what was said.
    pub fn answer(&mut self, g: &mut Game, player: u8, to: Option<u8>, heard: &str, fact: String, binding: bool) {
        if !self.ready.load(Ordering::Relaxed) {
            // (Still downloading or loading: a deal is answered as it stands,
            // small talk waits for the model.)
            return if binding { g.ai_say(player, to, fact) } else { g.ai_think(player, to, fact) };
        }
        let report = g.ai_briefings().into_iter().find(|(p, _)| *p == player).map(|(_, r)| r).unwrap_or_default();
        let line = Line { player, to, heard: heard.to_string(), fact: fact.clone(), binding, recent: self.log.join("\n"), report };
        if self.talk.send(Job::Speak(line)).is_err() {
            if binding {
                g.ai_say(player, to, fact);
            } else {
                g.ai_think(player, to, fact);
            }
        }
    }
}

/// Where the model is, and how to get it going.
struct Server {
    url: String,
    model: String,
    start: String,
    /// The base URL of the server once found.
    base: Option<String>,
    tried_start: bool,
}

impl Server {
    /// The base URL of a running model server and its `/models` reply,
    /// starting one first if there is none and a start is possible.
    fn find(&mut self) -> Result<String, String> {
        if let Some(base) = &self.base {
            return Ok(base.clone());
        }
        let candidates: Vec<&str> = if self.url.is_empty() { CANDIDATES.to_vec() } else { vec![self.url.as_str()] };
        let mut found = probe(&candidates);
        if found.is_none() && !self.tried_start {
            self.tried_start = true;
            let cmd = if !self.start.is_empty() {
                Some(self.start.clone())
            } else if !self.model.is_empty() && self.url.is_empty() && on_path("ollama") {
                Some("ollama serve".to_string())
            } else {
                None
            };
            if let Some(cmd) = cmd {
                eprintln!("Starting the model server for the computer players: {cmd}");
                spawn(&cmd).map_err(|e| format!("could not start `{cmd}`: {e}"))?;
                let began = Instant::now();
                while found.is_none() && began.elapsed() < START_WAIT {
                    std::thread::sleep(Duration::from_millis(500));
                    found = probe(&candidates);
                }
            }
        }
        let (base, mut models) = found.ok_or_else(|| {
            if self.model.is_empty() || !self.url.is_empty() || !self.start.is_empty() {
                "no model server is running".to_string()
            } else {
                "no model server is running, and Ollama is not installed (https://ollama.com/download)".to_string()
            }
        })?;
        if self.model.is_empty() {
            self.model = first_model(&models).ok_or("the server lists no models")?;
        } else if !lists(&models, &self.model) {
            // Fetch what is missing, once, so a first run needs no set-up.
            pull(&base, &self.model)?;
            models = http("GET", &format!("{base}/models"), None, PROBE).map(|(_, b)| b).unwrap_or_default();
            if !lists(&models, &self.model) {
                return Err(format!("the server still lacks the model {:?}", self.model));
            }
        }
        self.base = Some(base.clone());
        Ok(base)
    }

    fn ask(&mut self, system: &str, user: &str, max_tokens: u32, json: bool) -> Result<String, String> {
        let base = self.find()?;
        ask(&base, &mut self.model, system, user, max_tokens, json).inspect_err(|e| {
            // A model server that has gone away is looked for again next time.
            if !e.starts_with("HTTP") {
                self.base = None;
            }
        })
    }
}

/// Where answers come from.
enum Runner {
    Http(Server),
    #[cfg(feature = "llm")]
    Embedded {
        source: crate::engine::Source,
        /// Loaded on first use; a failure is remembered for the log and tried again next time.
        engine: Option<crate::engine::Engine>,
    },
}

impl Runner {
    fn new(url: String, model: String, start: String) -> Runner {
        #[cfg(feature = "llm")]
        if url.is_empty() && start.is_empty() {
            if let Some(source) = crate::engine::Source::parse(&model) {
                return Runner::Embedded { source, engine: None };
            }
        }
        // `auto` without the built-in engine: the same small model, through Ollama.
        let model = if model == "auto" { DEFAULT_MODEL.to_string() } else { model };
        Runner::Http(Server { url: url.trim_end_matches('/').to_string(), model, start, base: None, tried_start: false })
    }

    /// Find, start or download whatever is needed, before the first question.
    fn warm_up(&mut self) {
        if let Err(why) = self.ask("Reply with OK.", "OK?", 4, false) {
            eprintln!("The model for the computer players is not ready ({why}); they play on alone for now.");
        }
    }

    fn ask(&mut self, system: &str, user: &str, max_tokens: u32, json: bool) -> Result<String, String> {
        match self {
            Runner::Http(server) => server.ask(system, user, max_tokens, json),
            #[cfg(feature = "llm")]
            Runner::Embedded { source, engine } => {
                if engine.is_none() {
                    *engine = Some(crate::engine::Engine::load(source)?);
                }
                engine.as_ref().map_or(Err("no model".to_string()), |e| e.ask(system, user, max_tokens, json))
            }
        }
    }
}

/// The model thread: one job at a time, chat first, then advice in the
/// order asked.
fn work(mut runner: Runner, talk: Receiver<Job>, jobs: Receiver<Job>, replies: Sender<Answer>, ready: Arc<AtomicBool>) {
    runner.warm_up();
    ready.store(true, Ordering::Relaxed);
    loop {
        let job = match talk.try_recv() {
            Ok(j) => j,
            Err(mpsc::TryRecvError::Disconnected) => return,
            Err(mpsc::TryRecvError::Empty) => match jobs.try_recv() {
                Ok(j) => j,
                Err(mpsc::TryRecvError::Disconnected) => return,
                Err(mpsc::TryRecvError::Empty) => match talk.recv_timeout(Duration::from_millis(50)) {
                    Ok(j) => j,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                },
            },
        };
        let answer = match job {
            Job::Advise(player, report) => Answer::Advice(player, advise(&mut runner, &report)),
            Job::Speak(line) if !line.binding => {
                let prompt = format!("{}\nRecent chat:\n{}\nAnswer this line: {}", line.report, line.recent, line.heard);
                let (said, how, aloud) = match runner.ask(TALK_GUIDE, &prompt, 120, false).ok().and_then(|t| clean_line(&t)) {
                    Some(l) if !makes_deals(&l) => (l, "the model's own answer", true),
                    Some(l) => {
                        eprintln!("Model small talk thrown out, it made a deal: {l:?}");
                        (line.fact, "kept quiet, because the model's answer made a deal", false)
                    }
                    None => (line.fact, "kept quiet, because the model gave nothing", false),
                };
                Answer::Said(line.player, line.to, said, how, aloud)
            }
            Job::Speak(line) => {
                let prompt = format!("{}\nThey said: {}\nFACT to carry: {}", line.report, line.heard, line.fact);
                let (said, how) = match runner.ask(CHAT_GUIDE, &prompt, 120, false).ok().and_then(|t| clean_line(&t)) {
                    Some(l) if faithful(&line.fact, &l) => (l, "the model's"),
                    Some(l) => {
                        let how = "the game's own, because the model's version changed the facts";
                        eprintln!("Model wording thrown out ({:?}) for {:?}", l, line.fact);
                        (line.fact, how)
                    }
                    None => (line.fact, "the game's own, because the model gave nothing"),
                };
                Answer::Said(line.player, line.to, said, how, true)
            }
        };
        if replies.send(answer).is_err() {
            return;
        }
    }
}

/// A model's answer: the directive, why, and what to say about a new plan.
pub struct Advice {
    pub directive: Directive,
    pub reason: Option<String>,
    pub say: Option<String>,
}

fn advise(runner: &mut Runner, report: &str) -> Result<Advice, String> {
    advise_with(runner, DIRECTIVE_GUIDE, report)
}

fn advise_with(runner: &mut Runner, guide: &str, report: &str) -> Result<Advice, String> {
    let text = runner.ask(guide, report, 400, true)?;
    Ok(Advice {
        directive: Directive::from_reply(&text)?,
        reason: Directive::reason_from_reply(&text),
        say: Directive::say_from_reply(&text).and_then(|l| clean_line(&l)),
    })
}

/// The model asked one question at a time, waiting for each answer: for
/// tools that judge its advice (`snowballz-eval`), not for live games.
pub struct Advisor {
    runner: Runner,
    /// What the model is told about the job: `DIRECTIVE_GUIDE` unless a
    /// tool is trying another wording.
    pub guide: String,
}

impl Advisor {
    /// The same model `--llm MODEL` would use (`SNOWBALLZ_LLM_URL` pins a server).
    pub fn new(model: &str) -> Advisor {
        let url = std::env::var("SNOWBALLZ_LLM_URL").unwrap_or_default();
        let mut runner = Runner::new(url, model.to_string(), String::new());
        runner.warm_up();
        Advisor { runner, guide: DIRECTIVE_GUIDE.to_string() }
    }

    /// A directive for the computer whose report this is, and the model's reason.
    pub fn advise(&mut self, report: &str) -> Result<Advice, String> {
        advise_with(&mut self.runner, &self.guide, report)
    }
}

/// With `--llm`, fetch the model's weights in the background as the server
/// starts, so that the first game finds them ready.
pub fn prefetch() {
    #[cfg(feature = "llm")]
    if let Ok(model) = std::env::var("SNOWBALLZ_LLM_MODEL") {
        if std::env::var_os("SNOWBALLZ_LLM_URL").is_none() {
            if let Some(source) = crate::engine::Source::parse(&model) {
                std::thread::spawn(move || {
                    if let Err(why) = crate::engine::ensure(&source) {
                        eprintln!("The model for the computer players could not be fetched: {why}");
                    }
                });
            }
        }
    }
}

/// Download and load the model now, and try it once, so that a later
/// `--llm` finds everything ready: `snowballz-server --llm-setup [MODEL]`.
pub fn setup(model: &str) -> Result<String, String> {
    #[cfg(feature = "llm")]
    {
        let source = crate::engine::Source::parse(model).ok_or_else(|| format!("{model:?} is not a model this build runs itself; give a path to a .gguf file, hf:owner/repo/file.gguf, or auto"))?;
        let engine = crate::engine::Engine::load(&source)?;
        let began = Instant::now();
        let said = engine.ask("Reply with one short sentence.", "Say hello to a penguin.", 30, false)?;
        Ok(format!(
            "Ready: the model runs on the {} and answered in {:.1}s: {}",
            if engine.on_gpu { "graphics card" } else { "CPU" },
            began.elapsed().as_secs_f32(),
            said.trim()
        ))
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = model;
        Err("this server was built without the `llm` feature (cargo build --release -p server --features vulkan). Without it the model comes from Ollama: install it from https://ollama.com/download, then `snowballz-server --llm qwen3.5:4b` downloads the model".to_string())
    }
}

/// Does the model's wording still say what the game decided? A cheap check,
/// not a proof: it may not add numbers, a truce keeps its minutes, and a
/// truce stays one.
fn faithful(fact: &str, line: &str) -> bool {
    let numbers = |s: &str| s.split(|c: char| !c.is_ascii_digit()).filter(|n| !n.is_empty()).map(String::from).collect::<Vec<_>>();
    let (said, given) = (numbers(line), numbers(fact));
    let low = line.to_lowercase();
    let minutes = fact.split(" minute").next().and_then(|before| numbers(before).pop()).filter(|_| fact.contains(" minute"));
    let truce_ok = !fact.to_lowercase().contains("truce for") || ["truce", "peace", "ceasefire"].iter().any(|w| low.contains(w));
    said.iter().all(|n| given.contains(n)) && minutes.is_none_or(|m| said.contains(&m)) && truce_ok
}

/// Does small talk agree to something only the game can agree to?
fn makes_deals(line: &str) -> bool {
    let low = line.to_lowercase();
    let words: Vec<&str> = low.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let yes = ["deal", "agreed", "accept", "truce", "ally", "allies", "alliance", "peace", "ceasefire", "promise", "team"];
    words.iter().any(|w| yes.contains(w)) && !words.iter().any(|w| ["no", "not", "never", "nope"].contains(w))
}

/// Does a `/models` reply list this model? (Ollama adds `:latest` to bare names.)
fn lists(models: &str, model: &str) -> bool {
    let Ok(j) = Json::parse(models) else { return false };
    j.get("data").map(Json::as_arr).unwrap_or(&[]).iter().filter_map(|m| m.get("id")?.as_str()).any(|id| id == model || id.strip_suffix(":latest") == Some(model))
}

/// Ask an Ollama server to download a model, and wait for it. Only Ollama
/// has this; any other server just won't know the route.
fn pull(base: &str, model: &str) -> Result<(), String> {
    let root = base.trim_end_matches('/').trim_end_matches("/v1");
    eprintln!("Downloading the model {model} for the computer players (once; this can take a few minutes)...");
    let body = format!("{{\"model\":{},\"stream\":false}}", quote(model));
    match http("POST", &format!("{root}/api/pull"), Some(&body), PULL_WAIT) {
        Ok((200, reply)) if reply.contains("\"success\"") => {
            eprintln!("Downloaded {model}.");
            Ok(())
        }
        Ok((200, reply)) => Err(format!("could not download {model}: {}", reply.chars().take(160).collect::<String>())),
        Ok((404, _)) => Err(format!("{model} is not on the model server, and it cannot download models itself")),
        Ok((code, reply)) => Err(format!("could not download {model} (HTTP {code}): {}", reply.chars().take(160).collect::<String>())),
        Err(e) => Err(format!("could not download {model}: {e}")),
    }
}

fn probe(candidates: &[&str]) -> Option<(String, String)> {
    candidates.iter().find_map(|base| match http("GET", &format!("{base}/models"), None, PROBE) {
        Ok((200, body)) => Some((base.to_string(), body)),
        _ => None,
    })
}

fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).is_file()))
}

/// Runs a start command detached from our own output. Ollama is told to
/// keep the model loaded, since a game asks every half minute.
fn spawn(cmd: &str) -> std::io::Result<()> {
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .env("OLLAMA_KEEP_ALIVE", "24h")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

/// One line of chat out of whatever the model wrote.
fn clean_line(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let line = line.trim_matches(|c| c == '"' || c == '\'' || c == '`').trim();
    (!line.is_empty()).then(|| line.chars().take(200).collect())
}

/// Ask the model something; the text of its reply.
fn ask(base: &str, model: &mut String, system: &str, user: &str, max_tokens: u32, json: bool) -> Result<String, String> {
    let format = if json { "\"response_format\":{\"type\":\"json_object\"}," } else { "" };
    // Thinking is off (Ollama and llama-server each have their own switch):
    // a model that thinks first spends its whole answer on it.
    let body = format!(
        "{{\"model\":{},\"messages\":[{{\"role\":\"system\",\"content\":{}}},{{\"role\":\"user\",\"content\":{}}}],\
         \"temperature\":0.7,\"max_tokens\":{max_tokens},{format}\
         \"chat_template_kwargs\":{{\"enable_thinking\":false}},\"reasoning_effort\":\"none\",\"stream\":false}}",
        quote(model),
        quote(system),
        quote(user)
    );
    match http("POST", &format!("{base}/chat/completions"), Some(&body), REPLY)? {
        (200, reply) => content_of(&reply),
        (404, _) => Err(format!("HTTP 404: the server has no model {model:?} (for Ollama: ollama pull {model})")),
        (code, _) => Err(format!("HTTP {code}")),
    }
}

/// The text inside a chat-completions reply.
fn content_of(reply: &str) -> Result<String, String> {
    let j = Json::parse(reply).map_err(|_| "the reply was not JSON".to_string())?;
    let message = j.get("choices").map(Json::as_arr).and_then(|c| c.first()).and_then(|c| c.get("message")).ok_or("the reply had no message")?;
    let text = |k: &str| message.get(k).and_then(Json::as_str).filter(|s| !s.trim().is_empty());
    text("content").or_else(|| text("reasoning_content")).map(String::from).ok_or_else(|| "the reply was empty".to_string())
}

fn first_model(models: &str) -> Option<String> {
    Json::parse(models).ok()?.get("data").map(Json::as_arr)?.first()?.get("id")?.as_str().map(String::from)
}

/// One request, one reply: the status code and the body.
fn http(method: &str, url: &str, body: Option<&str>, timeout: Duration) -> Result<(u16, String), String> {
    let rest = url.strip_prefix("http://").ok_or("only http:// URLs are supported")?;
    let (hostport, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let addr = if hostport.contains(':') { hostport.to_string() } else { format!("{hostport}:80") };
    let sock = addr.to_socket_addrs().map_err(|e| e.to_string())?.next().ok_or_else(|| format!("could not find {hostport}"))?;
    let mut s = TcpStream::connect_timeout(&sock, timeout.min(CONNECT)).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(timeout)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(CONNECT)).map_err(|e| e.to_string())?;
    let body = body.unwrap_or("");
    let req = format!(
        "{method} {path} HTTP/1.1\r\nHost: {hostport}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
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
    use sim::ai::{AiLevel, Posture};
    use sim::game::PlayerSetup;
    use sim::map::Map;
    use std::net::TcpListener;
    use std::time::Instant;

    /// A server that answers /models and /chat/completions with `content`,
    /// the chat reply chunked as llama-server sends it.
    fn fake_server(content: String) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        serve(listener, content);
        url
    }

    fn serve(listener: TcpListener, content: String) {
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let get = buf[..n].starts_with(b"GET ");
                let reply = if get {
                    let body = r#"{"data":[{"id":"fake-model"}]}"#;
                    format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
                } else {
                    let body = format!(r#"{{"choices":[{{"message":{{"content":{}}}}}]}}"#, quote(&content));
                    let (a, b) = body.split_at(body.len() / 2);
                    format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{a}\r\n{:x}\r\n{b}\r\n0\r\n\r\n", a.len(), b.len())
                };
                let _ = s.write_all(reply.as_bytes());
            }
        });
    }

    fn game() -> Game {
        let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
        let map = Map::load_file(std::path::Path::new(&path)).unwrap();
        let setups = (0..2).map(|i| PlayerSetup { name: format!("AI {i}"), color: i as u8, team: i as u8, ai: Some(AiLevel::Hard) }).collect();
        Game::new(map, setups, 1)
    }

    /// Ticks the game, stepping the strategist, until `done` or ten seconds pass.
    fn run_until(s: &mut Strategist, g: &mut Game, done: impl Fn(&Strategist, &Game) -> bool) -> bool {
        run_until_for(s, g, 10, done)
    }

    fn run_until_for(s: &mut Strategist, g: &mut Game, seconds: u64, done: impl Fn(&Strategist, &Game) -> bool) -> bool {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(seconds) {
            g.tick();
            g.events.clear();
            s.step(g);
            if done(s, g) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        false
    }

    /// A round of questions went out and every answer (or failure) is back.
    fn round_done(s: &Strategist, _: &Game) -> bool {
        s.next_round > FIRST_AFTER && s.outstanding == 0
    }

    #[test]
    fn chunked_bodies_are_joined() {
        assert_eq!(dechunk(b"4\r\nWiki\r\n5;ext=1\r\npedia\r\n0\r\n\r\n").unwrap(), b"Wikipedia");
        assert!(dechunk(b"9\r\nshort\r\n").is_err());
    }

    #[test]
    fn the_model_steers_the_computer() {
        let url = fake_server("Plan:\n```json\n{\"posture\":\"rush\",\"target\":1,\"favour\":\"slinger\",\"research\":null}\n```".into());
        let mut s = Strategist::start_with(url, String::new(), String::new());
        let mut g = game();
        let steered = |_: &Strategist, g: &Game| (0..2).all(|p| g.ai_directive(p).is_some_and(|d| d.posture == Posture::Rush));
        assert!(run_until(&mut s, &mut g, steered), "{:?}", g.ai_briefings());
        // Player 0 may pick on player 1; player 1 cannot aim at itself.
        assert_eq!(g.ai_directive(0).unwrap().target, Some(1));
        assert_eq!(g.ai_directive(1).unwrap().target, None);
    }

    /// Ticks until the game has something new to say; returns what was said.
    fn chat_after(s: &mut Strategist, g: &mut Game) -> Option<String> {
        let began = Instant::now();
        while began.elapsed() < Duration::from_secs(10) {
            g.tick();
            s.step(g);
            let said = g.events.iter().find_map(|e| match e {
                sim::game::Event::Chat { from: 0, text, .. } => Some(text.clone()),
                _ => None,
            });
            g.events.clear();
            if said.is_some() {
                return said;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    #[test]
    fn the_model_puts_a_reply_in_the_computers_voice() {
        let mut s = Strategist::start_with(fake_server("\"Hah! Peace, you say? Fine, 3 minutes, but keep your axes off my trees.\"".into()), String::new(), String::new());
        let mut g = game();
        for (ai, to, fact) in g.ai_replies(1, Some(0), "truce?") {
            s.reword(&mut g, ai, to, "truce?", fact);
        }
        assert_eq!(chat_after(&mut s, &mut g).as_deref(), Some("Hah! Peace, you say? Fine, 3 minutes, but keep your axes off my trees."));
    }

    /// Ordinary chat gets the model's own answer to what was said, not a
    /// stock line reworded.
    #[test]
    fn small_talk_is_the_models_own_answer() {
        let mut s = Strategist::start_with(fake_server("Nice hat? Wait till you see my snowballers.".into()), String::new(), String::new());
        let mut g = game();
        s.heard("P1", "nice hat, penguin");
        let replies = g.ai_replies_full(1, Some(0), "nice hat, penguin");
        assert!(!replies.is_empty() && replies.iter().all(|(_, r)| !r.binding), "{replies:?}");
        for (ai, r) in replies {
            s.answer(&mut g, ai, r.to, "nice hat, penguin", r.text, r.binding);
        }
        assert_eq!(chat_after(&mut s, &mut g).as_deref(), Some("Nice hat? Wait till you see my snowballers."));
    }

    /// Small talk can't strike a deal the game never made.
    #[test]
    fn small_talk_that_makes_a_deal_is_thrown_out() {
        assert!(makes_deals("Fine, truce it is, friend."));
        assert!(makes_deals("Deal! Allies till the end."));
        assert!(!makes_deals("No truce for you, featherbrain."));
        assert!(!makes_deals("Nice try. Snowballs at dawn."));
        let mut s = Strategist::start_with(fake_server("Sure, we're allies now!".into()), String::new(), String::new());
        let mut g = game();
        for (ai, r) in g.ai_replies_full(1, Some(0), "you look cold") {
            s.answer(&mut g, ai, r.to, "you look cold", r.text.clone(), r.binding);
        }
        // The computer keeps quiet rather than say its stock line.
        let said = chat_after(&mut s, &mut g);
        assert!(said.is_none(), "{said:?}");
        let log = g.ai_notes()[0].1.join("\n");
        assert!(log.contains("kept quiet, because the model's answer made a deal"), "{log}");
    }

    #[test]
    fn with_no_model_the_plain_reply_is_said() {
        let mut s = Strategist::start_with("http://127.0.0.1:9/v1".into(), String::new(), String::new());
        let mut g = game();
        for (ai, to, fact) in g.ai_replies(1, Some(0), "truce?") {
            s.reword(&mut g, ai, to, "truce?", fact);
        }
        let said = chat_after(&mut s, &mut g).expect("nothing was said");
        assert!(said.contains("truce for"), "{said}");
    }

    #[test]
    fn the_server_waits_for_a_model_server_it_started() {
        // The start command does nothing; the "model server" appears a moment later.
        let probe_port = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = probe_port.local_addr().unwrap();
        drop(probe_port);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            serve(TcpListener::bind(addr).unwrap(), "{\"posture\":\"turtle\"}".into());
        });
        let mut s = Strategist::start_with(format!("http://{addr}/v1"), String::new(), "true".into());
        let mut g = game();
        let steered = |_: &Strategist, g: &Game| (0..2).all(|p| g.ai_directive(p).is_some_and(|d| d.posture == Posture::Turtle));
        assert!(run_until(&mut s, &mut g, steered), "the game never heard from the late model server");
    }

    /// An Ollama look-alike: lists no model until it has been asked to pull
    /// one. Returns its base URL and the paths it was asked for.
    fn fake_ollama() -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        std::thread::spawn(move || {
            let mut pulled = false;
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 8192];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = head.lines().next().unwrap_or("").to_string();
                log.lock().unwrap().push(line.clone());
                let body = if line.contains("/api/pull") {
                    pulled = true;
                    r#"{"status":"success"}"#.to_string()
                } else if line.contains("/models") {
                    if pulled { r#"{"data":[{"id":"tiny:latest"}]}"#.to_string() } else { r#"{"data":[]}"#.to_string() }
                } else {
                    format!(r#"{{"choices":[{{"message":{{"content":{}}}}}]}}"#, quote("{\"posture\":\"rush\"}"))
                };
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
            }
        });
        (url, seen)
    }

    #[test]
    fn a_missing_model_is_downloaded_once() {
        let (url, seen) = fake_ollama();
        let mut s = Strategist::start_with(url, "tiny".into(), String::new());
        let mut g = game();
        let steered = |_: &Strategist, g: &Game| (0..2).all(|p| g.ai_directive(p).is_some_and(|d| d.posture == Posture::Rush));
        assert!(run_until(&mut s, &mut g, steered), "the model never advised");
        let pulls = seen.lock().unwrap().iter().filter(|l| l.contains("/api/pull")).count();
        assert_eq!(pulls, 1, "{:?}", seen.lock().unwrap());
    }

    #[test]
    fn model_names_match_the_way_ollama_lists_them() {
        let listed = r#"{"data":[{"id":"qwen3.5:4b"},{"id":"gemma4:e4b"},{"id":"tiny:latest"}]}"#;
        assert!(lists(listed, "qwen3.5:4b") && lists(listed, "tiny") && lists(listed, "tiny:latest"));
        assert!(!lists(listed, "qwen3.5:9b") && !lists(listed, "gemma") && !lists("{}", "tiny"));
    }

    #[test]
    fn a_rewording_must_keep_the_numbers_and_the_truce() {
        let fact = "Very well, Ann. A truce for 5 minutes.";
        assert!(faithful(fact, "Fine, Ann. Peace for 5 minutes, no more."));
        assert!(!faithful(fact, "Fine, Ann. A truce for 50 minutes."));
        assert!(!faithful(fact, "Fine, Ann. Five minutes of quiet."), "the minutes were dropped");
        assert!(faithful("Very well, AI 1. A truce for 3 minutes.", "Hah, 3 minutes of peace, AI 1."));
        assert!(faithful("Your people are in my forest.", "Get your people out of my forest!"));
        assert!(!faithful("No.", "Fine, a truce for 3 minutes."));
    }

    #[test]
    fn a_reply_is_one_clean_line() {
        assert_eq!(clean_line("\n  \"Get out!\"  \nand more").as_deref(), Some("Get out!"));
        assert_eq!(clean_line("   \n "), None);
    }

    #[test]
    fn a_useless_model_changes_nothing() {
        let mut s = Strategist::start_with(fake_server("I refuse to play.".into()), String::new(), String::new());
        let mut g = game();
        assert!(run_until(&mut s, &mut g, round_done), "no round was asked and answered");
        assert!((0..2).all(|p| g.ai_directive(p) == Some(Directive::default())));
    }

    #[test]
    fn a_missing_model_changes_nothing() {
        let mut s = Strategist::start_with("http://127.0.0.1:9/v1".into(), String::new(), String::new());
        let mut g = game();
        assert!(run_until(&mut s, &mut g, round_done), "no round was asked and answered");
        assert!((0..2).all(|p| g.ai_directive(p) == Some(Directive::default())));
    }

    /// Needs a real model, e.g.
    /// `SNOWBALLZ_LLM_URL=http://127.0.0.1:11434/v1 SNOWBALLZ_LLM_MODEL=qwen3.5:4b cargo test --release -p server real_model -- --ignored --nocapture`
    /// Times a directive and a chat line, from a game three minutes in.
    #[test]
    #[ignore]
    fn real_model() {
        let url = std::env::var("SNOWBALLZ_LLM_URL").expect("set SNOWBALLZ_LLM_URL");
        let url = url.trim_end_matches('/').to_string();
        let mut model = std::env::var("SNOWBALLZ_LLM_MODEL").expect("set SNOWBALLZ_LLM_MODEL");
        let mut g = game();
        for _ in 0..3 * 60 * TICK_RATE {
            g.tick();
            g.events.clear();
        }
        let report = g.ai_briefings().remove(0).1;
        let facts = ["I grant a truce for 5 minutes.", "Your people are in my forest. Get them out and we can talk.", "I have nothing against you. Yet."];
        let (mut ok, mut slow) = (0, Vec::new());
        for round in 0..4 {
            let began = Instant::now();
            let reply = ask(&url, &mut model, DIRECTIVE_GUIDE, &report, 400, true);
            let secs = began.elapsed().as_secs_f32();
            let parsed = reply.as_ref().map_err(|e| e.clone()).and_then(|t| Directive::from_reply(t));
            println!("directive {round}: {secs:.1}s {:?} <- {:?}", parsed, reply.map(|t| t.chars().take(90).collect::<String>()));
            // (The first round also loads the model.)
            if round > 0 {
                slow.push(secs);
            }
            ok += parsed.is_ok() as u32;
        }
        let mut talk = Vec::new();
        for (i, fact) in facts.iter().enumerate() {
            let prompt = format!("{report}\nThey said: truce?\nFACT to carry: {fact}");
            let began = Instant::now();
            let reply = ask(&url, &mut model, CHAT_GUIDE, &prompt, 120, false);
            let secs = began.elapsed().as_secs_f32();
            println!("chat {i}: {secs:.1}s {:?}", reply.as_ref().ok().and_then(|t| clean_line(t)));
            talk.push(secs);
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        println!("RESULT {model}: valid directives {ok}/4, directive {:.1}s, chat {:.1}s", mean(&slow), mean(&talk));
    }

    /// With nothing running on the usual ports, the strategist starts Ollama
    /// itself: `SNOWBALLZ_LLM_MODEL=qwen3.5:4b cargo test --release -p server starts_ollama -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn starts_ollama_by_itself() {
        let model = std::env::var("SNOWBALLZ_LLM_MODEL").expect("set SNOWBALLZ_LLM_MODEL");
        assert!(probe(&CANDIDATES).is_none(), "a model server is already running");
        let began = Instant::now();
        let mut s = Strategist::start_with(String::new(), model, String::new());
        let mut g = game();
        // Game time runs far faster than the model starts, so tick slowly.
        while began.elapsed() < Duration::from_secs(20 * 60) && !(s.next_round > FIRST_AFTER && s.outstanding == 0) {
            g.tick();
            g.events.clear();
            s.step(&mut g);
            std::thread::sleep(Duration::from_millis(20));
        }
        println!("{:.1}s: {}", began.elapsed().as_secs_f32(), g.ai_briefings()[0].1.lines().last().unwrap());
        assert!(s.next_round > FIRST_AFTER && s.outstanding == 0, "no answer came");
        assert!((0..2).any(|p| g.ai_directive(p) != Some(Directive::default())), "the model's advice never arrived");
    }

    /// The built-in engine, end to end: `cargo test --release -p server --features vulkan embedded_engine -- --ignored --nocapture`
    #[cfg(feature = "llm")]
    #[test]
    #[ignore]
    fn embedded_engine_steers_a_game() {
        let mut s = Strategist::start_with(String::new(), "auto".into(), String::new());
        let mut g = game();
        // (The game runs far faster than real time here, so rounds follow each other at once.)
        let advised = |_: &Strategist, g: &Game| g.ai_directive(0) != Some(Directive::default());
        assert!(run_until_for(&mut s, &mut g, 120, advised), "no answer came");
        println!("{:?}", g.ai_directive(0));
        for (ai, to, fact) in g.ai_replies(1, Some(0), "truce?") {
            s.reword(&mut g, ai, to, "truce?", fact);
        }
        let said = chat_after(&mut s, &mut g);
        println!("chat: {said:?}");
        assert!(said.is_some_and(|t| !t.is_empty()));
    }

    /// The built-in engine making small talk; prints what it says to each
    /// line: `cargo test --release -p server --features llm embedded_small_talk -- --ignored --nocapture`
    #[cfg(feature = "llm")]
    #[test]
    #[ignore]
    fn embedded_small_talk() {
        let mut s = Strategist::start_with(String::new(), "auto".into(), String::new());
        let mut g = game();
        let ready = |s: &Strategist, _: &Game| s.ready.load(Ordering::Relaxed);
        assert!(run_until_for(&mut s, &mut g, 300, ready), "the model never loaded");
        g.players[0].name = "Tux the Careless".into();
        g.players[1].name = "Sam".into();
        for line in ["hi!", "how's it going over there?", "your igloo looks tiny lol", "what's your favourite fish?", "gg, you played well", "can we be allies?"] {
            s.heard("Sam", line);
            for (ai, r) in g.ai_replies_full(1, Some(0), line) {
                s.answer(&mut g, ai, r.to, line, r.text, r.binding);
            }
            let began = Instant::now();
            let mut said = None;
            while said.is_none() && began.elapsed() < Duration::from_secs(60) {
                g.tick();
                s.step(&mut g);
                said = g.events.iter().find_map(|e| match e {
                    sim::game::Event::Chat { from: 0, to: Some(1), text } => Some(text.clone()),
                    _ => None,
                });
                g.events.clear();
                std::thread::sleep(Duration::from_millis(5));
            }
            println!("{line:?} -> {said:?} ({:.1}s)", began.elapsed().as_secs_f32());
            if let Some(t) = said {
                s.heard("Tux the Careless", &t);
            }
        }
    }
}
