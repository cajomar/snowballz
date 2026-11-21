//! Persistent player settings (a small key=value file).

#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Settings {
    pub name: String,
    pub last_server: String,
    pub shadows: bool,
    pub edge_scroll: bool,
    pub ui_scale: f32,
    pub fullscreen: bool,
    /// 0 to 10.
    pub music: u8,
    pub sound: u8,
    /// Local language model for making maps. Empty means find one.
    pub llm_url: String,
    /// Empty means the server's first model.
    pub llm_model: String,
    /// Shell command that starts a local model server; empty means none.
    pub llm_start: String,
}

impl Default for Settings {
    fn default() -> Settings {
        // (No $USER in a browser, so it's just "Penguin" there.)
        let name = std::env::var("USER").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "Penguin".into());
        let mut first = name.chars();
        let name = first.next().map_or(String::new(), |c| c.to_uppercase().collect::<String>() + first.as_str());
        Settings {
            name,
            last_server: String::new(),
            shadows: true,
            edge_scroll: true,
            ui_scale: 1.0,
            fullscreen: false,
            music: 6,
            sound: 8,
            llm_url: String::new(),
            llm_model: String::new(),
            llm_start: String::new(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))?;
    Some(base.join("snowballz").join("settings.txt"))
}

impl Settings {
    pub fn music_volume(&self) -> f32 {
        (self.music as f32 / 10.0).powi(2)
    }

    pub fn sound_volume(&self) -> f32 {
        (self.sound as f32 / 10.0).powi(2)
    }

    pub fn load() -> Settings {
        let mut s = Settings::default();
        let Some(text) = read_stored() else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "name" if !v.is_empty() => s.name = v.chars().take(24).collect(),
                "last_server" => s.last_server = v.into(),
                "shadows" => s.shadows = v == "true",
                "edge_scroll" => s.edge_scroll = v == "true",
                "ui_scale" => s.ui_scale = v.parse().unwrap_or(1.0f32).clamp(0.5, 3.0),
                "fullscreen" => s.fullscreen = v == "true",
                "music" => s.music = v.parse().unwrap_or(6u8).min(10),
                "sound" => s.sound = v.parse().unwrap_or(8u8).min(10),
                "llm_url" => s.llm_url = v.into(),
                "llm_model" => s.llm_model = v.into(),
                "llm_start" => s.llm_start = v.into(),
                _ => {}
            }
        }
        s
    }

    pub fn save(&self) {
        let text = format!(
            "name={}\nlast_server={}\nshadows={}\nedge_scroll={}\nui_scale={}\nfullscreen={}\nmusic={}\nsound={}\nllm_url={}\nllm_model={}\nllm_start={}\n",
            self.name,
            self.last_server,
            self.shadows,
            self.edge_scroll,
            self.ui_scale,
            self.fullscreen,
            self.music,
            self.sound,
            self.llm_url,
            self.llm_model,
            self.llm_start,
        );
        write_stored(&text);
    }
}

/// Settings live in a file natively and in the page's local storage in a browser.
#[cfg(not(target_arch = "wasm32"))]
fn read_stored() -> Option<String> {
    path().and_then(|p| std::fs::read_to_string(p).ok())
}

#[cfg(target_arch = "wasm32")]
fn read_stored() -> Option<String> {
    crate::web::store_get("settings")
}

#[cfg(not(target_arch = "wasm32"))]
fn write_stored(text: &str) {
    let Some(p) = path() else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(p, text);
}

#[cfg(target_arch = "wasm32")]
fn write_stored(text: &str) {
    crate::web::store_set("settings", text);
}
