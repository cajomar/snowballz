//! Dedicated Snowballz server.
//!
//!     snowballz-server [--port 7777] [--name "My Server"] [--maps DIR] [--user-maps DIR] [--web DIR] [--no-announce] [--upnp] [--llm [MODEL]] [--llm-setup [MODEL]]

use std::net::SocketAddr;

fn main() {
    let mut port = sim::proto::DEFAULT_PORT;
    let mut name = "Snowballz Server".to_string();
    let mut maps = server::maps_dir(&server::find_assets());
    let mut user_maps = None;
    let mut web = None;
    let mut announce = true;
    let mut upnp = false;
    let mut args = std::env::args().skip(1);
    // An option read ahead by `--llm`, to be handled next.
    let mut pending: Option<String> = None;
    while let Some(a) = pending.take().or_else(|| args.next()) {
        match a.as_str() {
            "--port" => port = args.next().and_then(|p| p.parse().ok()).expect("--port needs a number"),
            "--name" => name = args.next().expect("--name needs a value"),
            "--maps" => maps = args.next().expect("--maps needs a directory").into(),
            "--user-maps" => user_maps = Some(args.next().expect("--user-maps needs a directory").into()),
            "--web" => web = Some(args.next().expect("--web needs a directory").into()),
            "--no-announce" => announce = false,
            "--upnp" => upnp = true,
            "--llm" => {
                // A bare `--llm` (or `--llm auto`) takes the default model.
                let model = match args.next() {
                    Some(m) if !m.starts_with("--") => m,
                    Some(other) => {
                        pending = Some(other);
                        "auto".to_string()
                    }
                    None => "auto".to_string(),
                };
                std::env::set_var("SNOWBALLZ_LLM_MODEL", model)
            }
            "--llm-setup" => {
                let model = args.next().unwrap_or_else(|| "auto".to_string());
                match server::setup_llm(&model) {
                    Ok(report) => println!("{report}"),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(1);
                    }
                }
                return;
            }
            "-h" | "--help" => {
                println!("usage: snowballz-server [--port N] [--name NAME] [--maps DIR] [--user-maps DIR] [--web DIR] [--no-announce] [--upnp] [--llm [MODEL]] [--llm-setup [MODEL]]");
                println!("  --maps DIR       built-in maps to offer (default: assets/maps)");
                println!("  --user-maps DIR  maps added in the lobby are saved here, and listed on later runs");
                println!("  --web DIR        also serve the browser build (see tools/web.sh) from this directory, on the same port");
                println!("  --llm [MODEL]    let a language model advise the computer players. With no name: Qwen3.5 4B, run inside the server (builds with --features llm; downloaded the first time) or through Ollama. Also takes hf:owner/repo/file.gguf, a path to a .gguf, or an Ollama name");
                println!("  --llm-setup [MODEL]  download and try the model now, then exit");
                println!("  --upnp  ask the router to forward the port (for home servers)");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }
    server::prefetch_llm();
    let cfg = server::ServerConfig {
        name,
        bind: SocketAddr::from(([0, 0, 0, 0], port)),
        maps_dir: maps,
        user_maps_dir: user_maps,
        announce,
        stop_with_host: false,
        web_dir: web,
        ai_notes_for_players: server::ai_notes_for_players(),
    };
    // Kept alive until the server stops; dropping it closes the port.
    let _mapping = if upnp {
        match server::upnp::forward(port, "Snowballz server") {
            Ok(m) => {
                match m.external_ip {
                    Some(ip) if !server::upnp::is_private(ip) => println!("router forwarded port {port}: join at {ip}:{port}"),
                    _ => println!("router forwarded port {port}, but it's behind another NAT: not reachable from the internet"),
                }
                Some(m)
            }
            Err(e) => {
                println!("couldn't forward the port automatically: {e}");
                None
            }
        }
    } else {
        None
    };
    if let Err(e) = server::run(cfg) {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
