//! Reading the game's files: from disk natively, from the downloads the page
//! made before starting in the browser.

use std::path::{Path, PathBuf};

/// The `assets` directory (in the browser, the prefix the page stored files under).
pub fn dir() -> PathBuf {
    #[cfg(not(target_arch = "wasm32"))]
    return server::find_assets();
    #[cfg(target_arch = "wasm32")]
    return PathBuf::from("assets");
}

pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    return std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    #[cfg(target_arch = "wasm32")]
    return crate::web::asset(&path.to_string_lossy()).ok_or_else(|| format!("{}: not downloaded", path.display()));
}

pub fn exists(path: &Path) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    return path.is_file();
    #[cfg(target_arch = "wasm32")]
    return crate::web::asset(&path.to_string_lossy()).is_some();
}

pub fn read_string(path: &Path) -> Result<String, String> {
    String::from_utf8(read(path)?).map_err(|e| format!("{}: {e}", path.display()))
}
