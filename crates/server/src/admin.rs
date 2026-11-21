//! The admin page: every computer player's decisions in the running game,
//! live. The server can be public, so the page asks for a key: set
//! `SNOWBALLZ_ADMIN_KEY`, or the server makes one on first start and keeps
//! it in `~/.config/snowballz/admin-key`. Open `/admin?key=<key>`.

use crate::ws::respond;
use sim::game::Game;
use std::path::PathBuf;

/// Where the key is kept between runs.
fn key_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("snowballz").join("admin-key"))
}

/// The admin key: from the environment, the key file, or a new random one
/// saved there. `None` turns the page off.
pub fn load_key() -> Option<String> {
    if let Some(k) = std::env::var("SNOWBALLZ_ADMIN_KEY").ok().filter(|k| k.len() >= 8) {
        return Some(k);
    }
    let file = key_file()?;
    if let Some(k) = std::fs::read_to_string(&file).ok().map(|s| s.trim().to_string()).filter(|k| k.len() >= 8) {
        return Some(k);
    }
    let mut bytes = [0u8; 16];
    use std::io::Read;
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes)).ok()?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(file.parent()?).ok()?;
    std::fs::write(&file, format!("{key}\n")).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    }
    eprintln!("Admin page key written to {}", file.display());
    Some(key)
}

/// One query parameter's value.
fn param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|kv| kv.split_once('=').filter(|(k, _)| *k == name).map(|(_, v)| v))
}

/// Equal, taking the same time whatever the first difference.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Is this request for the admin page?
pub fn wants(path: &str) -> bool {
    path == "/admin" || path.starts_with("/admin?") || path.starts_with("/admin/")
}

/// The response to an admin request.
pub fn answer(path: &str, key: Option<&str>, game: Option<&Game>) -> Vec<u8> {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let given = param(query, "key").unwrap_or("");
    let Some(key) = key.filter(|k| same(k, given)) else {
        return respond("403 Forbidden", "text/plain", "", b"wrong or missing admin key\n");
    };
    let _ = key;
    match route {
        "/admin" | "/admin/" => respond("200 OK", "text/html; charset=utf-8", "Cache-Control: no-store\r\n", PAGE.as_bytes()),
        "/admin/data" => {
            let since = param(query, "since").and_then(|s| s.parse().ok()).unwrap_or(0);
            respond("200 OK", "application/json", "Cache-Control: no-store\r\n", data(game, since).as_bytes())
        }
        _ => respond("404 Not Found", "text/plain", "", b"not found\n"),
    }
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The game's computers as JSON: names, summaries, and log lines from tick `since`.
fn data(game: Option<&Game>, since: u32) -> String {
    let Some(g) = game else { return "{\"game\":false}".into() };
    let mut out = format!("{{\"game\":true,\"tick\":{},\"rate\":{},\"players\":[", g.tick, sim::TICK_RATE);
    for (i, p) in g.players.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let level = p.ai.map_or("person".to_string(), |l| format!("{l:?}"));
        out += &format!("{{\"id\":{},\"name\":{},\"color\":{},\"level\":{},\"out\":{}}}", p.id, json_str(&p.name), p.color, json_str(&level), p.out);
    }
    out += "],\"ais\":[";
    for (i, (player, summary, log)) in g.ai_admin(since).into_iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let summary: Vec<String> = summary.iter().map(|s| json_str(s)).collect();
        let log: Vec<String> = log.iter().map(|(t, s)| format!("[{t},{}]", json_str(s))).collect();
        out += &format!("{{\"player\":{player},\"summary\":[{}],\"log\":[{}]}}", summary.join(","), log.join(","));
    }
    out += "]}";
    out
}

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><title>Snowballz admin: computer players</title>
<style>
body{font:14px/1.4 system-ui,sans-serif;margin:0;background:#10161f;color:#dde6f0}
header{padding:10px 16px;background:#1a2433;display:flex;gap:14px;align-items:center;flex-wrap:wrap}
h1{font-size:17px;margin:0}
#tabs button{background:#263449;color:#dde6f0;border:0;padding:6px 12px;border-radius:5px;cursor:pointer;margin-right:4px}
#tabs button.on{background:#4b7bd0}
input[type=search]{background:#0c1118;color:#dde6f0;border:1px solid #334;padding:5px 8px;border-radius:5px;width:220px}
#summary{padding:8px 16px;background:#141c28;white-space:pre-wrap;font-family:ui-monospace,monospace;font-size:13px}
#log{padding:8px 16px;font-family:ui-monospace,monospace;font-size:13px}
.l{padding:1px 0;border-bottom:1px solid #18202c;white-space:pre-wrap}
.t{color:#7d8ca0;margin-right:8px}.who{font-weight:bold;margin-right:8px}
.attack{color:#ff9b7a}.off{color:#ffd36b}.think{color:#8fa3bb;font-style:italic}.say{color:#9fe39a}.plan{color:#c7a6ff}.deal{color:#7fd8e8}
#state{color:#7d8ca0}
</style></head><body>
<header><h1>Computer players</h1><span id="tabs"></span>
<input type="search" id="filter" placeholder="filter lines">
<label><input type="checkbox" id="follow" checked> follow</label><span id="state">connecting...</span></header>
<div id="summary"></div><div id="log"></div>
<script>
const key = new URLSearchParams(location.search).get('key') || '';
const colors = ['#e04040','#3070e0','#40b040','#e0c020','#a050e0','#f08020','#30c0c0','#e060b0'];
let logs = {}, summaries = {}, players = [], lastTick = 0, shown = 'all', rate = 20;
const clock = t => { const s = Math.floor(t / rate); return Math.floor(s / 60) + ':' + String(s % 60).padStart(2, '0'); };
const kind = s => /ATTACKS|Sends .* fighters/.test(s) ? 'attack' : /Calls the attack off|under attack/.test(s) ? 'off'
  : s.startsWith('Thinks') ? 'think' : s.startsWith('Says') ? 'say' : /plan/i.test(s) ? 'plan' : /truce|pact|Deal/i.test(s) ? 'deal' : '';
const name = p => (players.find(q => q.id == p) || {name: 'P' + p}).name;
const esc = s => s.replace(/[&<>]/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;'}[c]));
function tabs() {
  const ids = Object.keys(logs);
  const el = document.getElementById('tabs');
  el.innerHTML = '';
  for (const id of ['all', ...ids]) {
    const b = document.createElement('button');
    b.textContent = id == 'all' ? 'All' : name(id);
    if (id != 'all') b.style.borderLeft = '4px solid ' + colors[(players.find(q => q.id == id) || {}).color % 8];
    b.className = id == shown ? 'on' : '';
    b.onclick = () => { shown = id; tabs(); draw(); };
    el.appendChild(b);
  }
}
function draw() {
  const f = document.getElementById('filter').value.toLowerCase();
  document.getElementById('summary').textContent = shown == 'all'
    ? Object.keys(summaries).map(id => summaries[id].slice(0, 4).join('\n')).join('\n\n') : (summaries[shown] || []).join('\n');
  let rows = [];
  for (const id of shown == 'all' ? Object.keys(logs) : [shown])
    for (const [t, s] of logs[id] || []) rows.push([t, id, s]);
  rows.sort((a, b) => a[0] - b[0]);
  if (f) rows = rows.filter(r => r[2].toLowerCase().includes(f) || name(r[1]).toLowerCase().includes(f));
  rows = rows.slice(-3000);
  document.getElementById('log').innerHTML = rows.map(([t, id, s]) =>
    `<div class="l ${kind(s)}"><span class="t">${clock(t)}</span>${shown == 'all' ? `<span class="who" style="color:${colors[(players.find(q => q.id == id) || {}).color % 8]}">${esc(name(id))}</span>` : ''}${esc(s)}</div>`).join('');
  if (document.getElementById('follow').checked) window.scrollTo(0, document.body.scrollHeight);
}
async function poll() {
  try {
    const r = await fetch(`/admin/data?key=${encodeURIComponent(key)}&since=${lastTick}`, {cache: 'no-store'});
    if (!r.ok) { document.getElementById('state').textContent = r.status == 403 ? 'wrong key' : 'error ' + r.status; return; }
    const d = await r.json();
    if (!d.game) { document.getElementById('state').textContent = 'no game running'; logs = {}; summaries = {}; lastTick = 0; tabs(); draw(); return; }
    rate = d.rate;
    if (d.tick < lastTick) { logs = {}; lastTick = 0; }
    players = d.players;
    for (const a of d.ais) {
      const old = (logs[a.player] || []).filter(([t]) => t < lastTick);
      logs[a.player] = old.concat(a.log);
      summaries[a.player] = a.summary;
    }
    lastTick = d.tick;
    document.getElementById('state').textContent = 'game time ' + clock(d.tick);
    tabs(); draw();
  } catch (e) { document.getElementById('state').textContent = 'server unreachable'; }
  finally { setTimeout(poll, 2000); }
}
document.getElementById('filter').oninput = draw;
poll();
</script></body></html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_wants_the_key() {
        let body = |r: Vec<u8>| String::from_utf8_lossy(&r).into_owned();
        assert!(body(answer("/admin", Some("secretkey"), None)).starts_with("HTTP/1.1 403"));
        assert!(body(answer("/admin?key=nope", Some("secretkey"), None)).starts_with("HTTP/1.1 403"));
        assert!(body(answer("/admin?key=secretkey", None, None)).starts_with("HTTP/1.1 403"), "no key set: the page is off");
        assert!(body(answer("/admin?key=secretkey", Some("secretkey"), None)).contains("Computer players"));
        assert!(body(answer("/admin/data?key=secretkey", Some("secretkey"), None)).ends_with("{\"game\":false}"));
    }

    #[test]
    fn the_data_has_every_computers_log() {
        let path = format!("{}/../../assets/maps/pinewood-hollow.sbmap", env!("CARGO_MANIFEST_DIR"));
        let map = sim::map::Map::load_file(std::path::Path::new(&path)).unwrap();
        let setups = (0..2).map(|i| sim::game::PlayerSetup { name: format!("Bot \"{i}\""), color: i as u8, team: i as u8, ai: Some(sim::ai::AiLevel::Hard) }).collect();
        let mut g = Game::new(map, setups, 3);
        for _ in 0..60 * sim::TICK_RATE {
            g.tick();
        }
        let all = data(Some(&g), 0);
        assert!(all.contains("\"ais\":[{\"player\":0") && all.contains("{\"player\":1"), "{all}");
        assert!(all.contains("Bot \\\"0\\\""), "names are escaped: {all}");
        assert!(all.contains("Not attacking"), "the decisions are there: {all}");
        // Only what is new since a tick.
        let later = data(Some(&g), g.tick);
        assert!(later.len() < all.len());
    }
}
