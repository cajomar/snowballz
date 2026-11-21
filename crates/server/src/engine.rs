//! A language model run inside the server (llama.cpp), with its weights
//! fetched from Hugging Face the first time. Built with `--features llm`.
//!
//! The model thread owns the `Engine`; nothing here is shared.

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{LlamaChatMessage, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use std::fs::File;
use std::io::{Read, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Small, quick and plenty for this job.
pub const DEFAULT_REPO: &str = "unsloth/Qwen3.5-4B-GGUF";
pub const DEFAULT_FILE: &str = "Qwen3.5-4B-Q4_K_M.gguf";
/// Prompt and answer must fit in this many tokens.
const CONTEXT: u32 = 4096;
/// Silence between progress lines of a download, in percent.
const PROGRESS_STEP: u64 = 10;

/// Where a model comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A GGUF file on disk.
    Path(PathBuf),
    /// A GGUF file in a Hugging Face repository.
    Hub { repo: String, file: String },
}

impl Source {
    /// `auto` (the default model), `hf:owner/repo/file.gguf` or a path to a
    /// `.gguf` file. Anything else (an Ollama name, say) is not ours.
    pub fn parse(spec: &str) -> Option<Source> {
        let spec = spec.trim();
        if spec.is_empty() || spec == "auto" {
            return Some(Source::Hub { repo: DEFAULT_REPO.into(), file: DEFAULT_FILE.into() });
        }
        if let Some(rest) = spec.strip_prefix("hf:") {
            let (repo, file) = rest.rsplit_once('/')?;
            return (repo.contains('/') && file.ends_with(".gguf")).then(|| Source::Hub { repo: repo.into(), file: file.into() });
        }
        spec.ends_with(".gguf").then(|| Source::Path(PathBuf::from(spec)))
    }
}

/// Where downloaded models are kept.
pub fn cache_dir() -> PathBuf {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("SNOWBALLZ_CACHE") {
        return dir;
    }
    let base = if cfg!(target_os = "windows") {
        var("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        var("HOME").map(|h| h.join("Library/Caches"))
    } else {
        var("XDG_CACHE_HOME").or_else(|| var("HOME").map(|h| h.join(".cache")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("snowballz")
}

/// The file for a model on disk, downloading it first if it isn't there.
pub fn ensure(source: &Source) -> Result<PathBuf, String> {
    match source {
        Source::Path(p) => {
            if is_gguf(p) {
                Ok(p.clone())
            } else {
                Err(format!("{} is not a GGUF model file", p.display()))
            }
        }
        Source::Hub { repo, file } => {
            let dest = cache_dir().join("models").join(repo.replace('/', "--")).join(file);
            if is_gguf(&dest) {
                return Ok(dest);
            }
            download(&format!("https://huggingface.co/{repo}/resolve/main/{file}"), &dest)?;
            Ok(dest)
        }
    }
}

fn is_gguf(path: &Path) -> bool {
    let mut magic = [0u8; 4];
    File::open(path).and_then(|mut f| f.read_exact(&mut magic)).is_ok() && &magic == b"GGUF"
}

/// Fetch `url` into `dest`. A half-finished download is picked up where it
/// stopped, and the file only appears under its real name once whole.
pub fn download(url: &str, dest: &Path) -> Result<(), String> {
    let dir = dest.parent().ok_or("no directory to download into")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    let part = dest.with_extension("part");
    let have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(15))).build().into();
    let mut request = agent.get(url);
    if have > 0 {
        request = request.header("Range", format!("bytes={have}-"));
    }
    let response = request.call().map_err(|e| format!("could not download {url}: {e}"))?;
    let resumed = response.status().as_u16() == 206;
    let sent: u64 = response.headers().get("content-length").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(0);
    let start = if resumed { have } else { 0 };
    let total = start + sent;
    eprintln!("Downloading {} ({:.1} GB) to {} ...", dest.file_name().unwrap_or_default().to_string_lossy(), total as f64 / 1e9, dir.display());
    let mut out = if resumed {
        std::fs::OpenOptions::new().append(true).open(&part)
    } else {
        File::create(&part)
    }
    .map_err(|e| format!("could not write {}: {e}", part.display()))?;
    let mut body = response.into_body().into_reader();
    let mut buf = vec![0u8; 1 << 20];
    let (mut done, mut told) = (start, 0u64);
    loop {
        let n = body.read(&mut buf).map_err(|e| format!("the download broke off at {:.1} GB (run again to resume): {e}", done as f64 / 1e9))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| format!("could not write {}: {e}", part.display()))?;
        done += n as u64;
        if total > 0 && done * 100 / total >= told + PROGRESS_STEP {
            told = done * 100 / total / PROGRESS_STEP * PROGRESS_STEP;
            eprintln!("  {told}%");
        }
    }
    drop(out);
    if total > 0 && done != total {
        return Err(format!("the download stopped at {done} of {total} bytes (run again to resume)"));
    }
    if !is_gguf(&part) {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{url} did not give a GGUF model file"));
    }
    std::fs::rename(&part, dest).map_err(|e| format!("could not finish {}: {e}", dest.display()))?;
    eprintln!("Downloaded {}.", dest.display());
    Ok(())
}

/// A JSON object, nothing else: the sampler can only choose tokens that keep
/// the text valid, so a small model can't answer in prose.
const JSON_GRAMMAR: &str = r#"
root ::= object
object ::= "{" ws ( pair ( "," ws pair )* )? "}" ws
pair ::= string ":" ws value
value ::= object | array | string | number | "true" | "false" | "null"
array ::= "[" ws ( value ( "," ws value )* )? "]" ws
string ::= "\"" ( [^"\\\x7F\x00-\x1F] | "\\" ["\\bfnrt] )* "\"" ws
number ::= "-"? [0-9]+ ( "." [0-9]+ )? ws
ws ::= [ \t\n]*
"#;

pub struct Engine {
    backend: LlamaBackend,
    model: LlamaModel,
    /// Whether the weights went to a graphics card.
    pub on_gpu: bool,
}

impl Engine {
    /// Load the model, fetching it first if need be.
    pub fn load(source: &Source) -> Result<Engine, String> {
        let path = ensure(source)?;
        let mut backend = LlamaBackend::init().map_err(|e| format!("could not start llama.cpp: {e}"))?;
        backend.void_logs();
        // Every layer on the card if there is one. SNOWBALLZ_LLM_GPU_LAYERS=0
        // keeps it off (the card may be busy with something else).
        let layers: u32 = std::env::var("SNOWBALLZ_LLM_GPU_LAYERS").ok().and_then(|v| v.parse().ok()).unwrap_or(if backend.supports_gpu_offload() { 999 } else { 0 });
        let model = LlamaModel::load_from_file(&backend, &path, &LlamaModelParams::default().with_n_gpu_layers(layers))
            .map_err(|e| format!("could not load {}: {e}", path.display()))?;
        Ok(Engine { backend, model, on_gpu: layers > 0 })
    }

    /// One answer to one question. `json` forces a JSON object.
    pub fn ask(&self, system: &str, user: &str, max_tokens: u32, json: bool) -> Result<String, String> {
        let model = &self.model;
        let template = model.chat_template(None).ok();
        let messages = [("system", system), ("user", user)]
            .map(|(role, text)| LlamaChatMessage::new(role.into(), text.into()).map_err(|e| e.to_string()));
        let messages = messages.into_iter().collect::<Result<Vec<_>, _>>()?;
        let mut prompt = template
            .as_ref()
            .and_then(|t| model.apply_chat_template(t, &messages, true).ok())
            // A model whose template llama.cpp can't apply: ChatML, which most speak.
            .unwrap_or_else(|| format!("<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"));
        // Thinking off: a model that thinks first spends its answer on it.
        if template.as_ref().and_then(|t| t.to_string().ok()).is_some_and(|t| t.contains("<think>")) {
            prompt.push_str("<think>\n\n</think>\n\n");
        }
        let vocab = model.vocab();
        let tokens = vocab.tokenize(prompt.as_bytes(), vocab.should_add_bos(), true);
        if tokens.len() as u32 + max_tokens > CONTEXT {
            return Err(format!("the question is too long ({} tokens)", tokens.len()));
        }
        let params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(CONTEXT)).with_n_batch(CONTEXT).with_n_ubatch(512);
        let mut ctx = model.new_context(&self.backend, params).map_err(|e| format!("could not make a context: {e}"))?;
        let mut batch = LlamaBatch::new(CONTEXT as usize, 1);
        for (i, token) in tokens.iter().enumerate() {
            batch.add(*token, i as i32, &[0], i == tokens.len() - 1).map_err(|e| e.to_string())?;
        }
        ctx.decode(&mut batch).map_err(|e| format!("the model failed on the question: {e}"))?;

        let mut samplers = Vec::new();
        if json {
            samplers.push(LlamaSampler::grammar(model, JSON_GRAMMAR, "root").map_err(|e| format!("bad grammar: {e}"))?);
        }
        samplers.push(LlamaSampler::temp(0.7));
        samplers.push(LlamaSampler::dist(seed()));
        let mut sampler = LlamaSampler::chain_simple(samplers);
        let mut out = Vec::new();
        let mut at = tokens.len() as i32;
        for _ in 0..max_tokens {
            // (`sample` also tells the sampler, grammar included, what was chosen.)
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            if vocab.is_eog(token) {
                break;
            }
            out.extend(vocab.token_to_piece(token, false, None));
            batch.clear();
            batch.add(token, at, &[0], true).map_err(|e| e.to_string())?;
            at += 1;
            ctx.decode(&mut batch).map_err(|e| format!("the model failed while answering: {e}"))?;
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }
}

fn seed() -> u32 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn model_names_say_where_they_come_from() {
        assert_eq!(Source::parse("auto"), Some(Source::Hub { repo: DEFAULT_REPO.into(), file: DEFAULT_FILE.into() }));
        assert_eq!(Source::parse("hf:acme/tiny-GGUF/tiny-Q4.gguf"), Some(Source::Hub { repo: "acme/tiny-GGUF".into(), file: "tiny-Q4.gguf".into() }));
        assert_eq!(Source::parse("/models/x.gguf"), Some(Source::Path("/models/x.gguf".into())));
        assert_eq!(Source::parse("qwen3.5:4b"), None, "an Ollama name is not ours");
        assert_eq!(Source::parse("hf:tiny.gguf"), None);
    }

    /// Serves `body` for any GET, honouring a Range header like a CDN does.
    fn cdn(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/model.gguf", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                let from: usize = head.split("range: bytes=").nth(1).and_then(|r| r.split('-').next()).and_then(|r| r.trim().parse().ok()).unwrap_or(0);
                let (status, extra) = if from > 0 { ("206 Partial Content", format!("Content-Range: bytes {from}-{}/{}\r\n", body.len() - 1, body.len())) } else { ("200 OK", String::new()) };
                let rest = &body[from.min(body.len())..];
                let _ = s.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n", rest.len()).as_bytes());
                let _ = s.write_all(rest);
            }
        });
        url
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("snowballz-engine-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn a_download_lands_whole_under_its_real_name() {
        let mut model = b"GGUF".to_vec();
        model.extend((0..300_000u32).map(|i| i as u8));
        let dir = scratch("whole");
        let dest = dir.join("m.gguf");
        download(&cdn(model.clone()), &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), model);
        assert!(!dest.with_extension("part").exists());
        assert!(is_gguf(&dest));
    }

    #[test]
    fn a_broken_download_resumes_where_it_stopped() {
        let mut model = b"GGUF".to_vec();
        model.extend((0..300_000u32).map(|i| (i * 7) as u8));
        let dir = scratch("resume");
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("m.gguf");
        std::fs::write(dest.with_extension("part"), &model[..100_000]).unwrap();
        download(&cdn(model.clone()), &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), model, "the stitched file differs");
    }

    #[test]
    fn something_that_is_not_a_model_is_refused() {
        let dir = scratch("refuse");
        let dest = dir.join("m.gguf");
        let err = download(&cdn(b"<html>sign in</html>".to_vec()), &dest).unwrap_err();
        assert!(err.contains("GGUF"), "{err}");
        assert!(!dest.exists() && !dest.with_extension("part").exists());
    }

    /// Needs the weights: `SNOWBALLZ_LLM_MODEL=auto cargo test --release -p server --features vulkan real_engine -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_engine() {
        let source = Source::parse(&std::env::var("SNOWBALLZ_LLM_MODEL").unwrap_or_else(|_| "auto".into())).expect("a model the engine can load");
        let engine = Engine::load(&source).unwrap();
        println!("on the GPU: {}", engine.on_gpu);
        for _ in 0..3 {
            let began = std::time::Instant::now();
            let d = engine
                .ask(sim::ai::DIRECTIVE_GUIDE, "Minute 3. You are player 0 (AI 0), Hard level. Workers 13, fighters 10. Player 1 (AI 1): enemy, 0 of its igloos found.", 200, true)
                .unwrap();
            println!("{:.2}s directive {d:?} -> {:?}", began.elapsed().as_secs_f32(), sim::ai::Directive::from_reply(&d));
            assert!(sim::ai::Directive::from_reply(&d).is_ok());
        }
        let line = engine.ask(sim::ai::CHAT_GUIDE, "They said: truce?\nFACT to carry: Very well, Ann. A truce for 5 minutes.", 80, false).unwrap();
        println!("chat: {line:?}");
        assert!(!line.trim().is_empty());
    }

    /// How the computers answer small talk: `SNOWBALLZ_LLM_MODEL=auto cargo test --release -p server --features vulkan small_talk_sample -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn small_talk_sample() {
        let source = Source::parse(&std::env::var("SNOWBALLZ_LLM_MODEL").unwrap_or_else(|_| "auto".into())).expect("a model the engine can load");
        let engine = Engine::load(&source).unwrap();
        let report = "Minute 4. You are player 1 (Tux), Hard level. Workers 11, fighters 8.\nYour character: Tux: attacks early with small groups, talks a lot.";
        for said in ["hi!", "how's it going?", "your igloo looks tiny lol", "what's your favourite fish?", "are you a bot?", "gg, you played well", "can we be allies?", "lol", "I'm new to this game, any tips?"] {
            let prompt = format!("{report}\nRecent chat:\nSam: {said}\nAnswer this line: {said}");
            let line = engine.ask(sim::ai::TALK_GUIDE, &prompt, 120, false).unwrap();
            println!("{said:>32} -> {}", line.trim());
        }
    }
}
