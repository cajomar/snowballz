//! The browser side of the platform: thin wrappers over the functions that
//! `web/snowballz.js` hands to the game. Only built for wasm.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = sb, js_name = asset)]
    fn sb_asset(path: &str) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_namespace = sb, js_name = serverUrl)]
    fn sb_server_url() -> String;
    #[wasm_bindgen(js_namespace = sb, js_name = pageHttps)]
    fn sb_page_https() -> bool;
    #[wasm_bindgen(js_namespace = sb, js_name = wsOpen)]
    fn sb_ws_open(url: &str) -> i32;
    #[wasm_bindgen(js_namespace = sb, js_name = wsState)]
    fn sb_ws_state(id: i32) -> i32;
    #[wasm_bindgen(js_namespace = sb, js_name = wsSend)]
    fn sb_ws_send(id: i32, bytes: &[u8]);
    #[wasm_bindgen(js_namespace = sb, js_name = wsTake)]
    fn sb_ws_take(id: i32) -> Option<Vec<u8>>;
    #[wasm_bindgen(js_namespace = sb, js_name = wsClose)]
    fn sb_ws_close(id: i32);
    #[wasm_bindgen(js_namespace = sb, js_name = storeGet)]
    fn sb_store_get(key: &str) -> Option<String>;
    #[wasm_bindgen(js_namespace = sb, js_name = storeSet)]
    fn sb_store_set(key: &str, value: &str);
    #[wasm_bindgen(js_namespace = sb, js_name = ready)]
    fn sb_ready();
    #[wasm_bindgen(js_namespace = sb, js_name = logError)]
    fn sb_log_error(msg: &str);
    #[wasm_bindgen(js_namespace = sb, js_name = audioStart)]
    fn sb_audio_start();
    #[wasm_bindgen(js_namespace = sb, js_name = audioQueuedMs)]
    fn sb_audio_queued_ms() -> i32;
    #[wasm_bindgen(js_namespace = sb, js_name = audioPush)]
    fn sb_audio_push(pcm: &[i16]);
}

/// A file the page downloaded before starting the game.
pub fn asset(path: &str) -> Option<Vec<u8>> {
    sb_asset(path)
}

/// The server this page was loaded from (or `?server=` points at), as a
/// `ws://` or `wss://` URL.
pub fn server_url() -> String {
    sb_server_url()
}

/// Was this page itself loaded over https? Then only wss:// will connect.
pub fn page_is_https() -> bool {
    sb_page_https()
}

#[derive(PartialEq, Clone, Copy, Debug)]
pub enum SocketState {
    Connecting,
    Open,
    Closed,
}

pub struct Socket(i32);

impl Socket {
    pub fn open(url: &str) -> Socket {
        Socket(sb_ws_open(url))
    }

    pub fn state(&self) -> SocketState {
        match sb_ws_state(self.0) {
            0 => SocketState::Connecting,
            1 => SocketState::Open,
            _ => SocketState::Closed,
        }
    }

    pub fn send(&self, bytes: &[u8]) {
        sb_ws_send(self.0, bytes)
    }

    /// The next message that has arrived, if any.
    pub fn recv(&self) -> Option<Vec<u8>> {
        sb_ws_take(self.0)
    }
}

impl Drop for Socket {
    fn drop(&mut self) {
        sb_ws_close(self.0)
    }
}

pub fn store_get(key: &str) -> Option<String> {
    sb_store_get(key)
}

pub fn store_set(key: &str, value: &str) {
    sb_store_set(key, value)
}

/// Write to the browser console (panics would otherwise be silent).
pub fn log_error(msg: &str) {
    sb_log_error(msg)
}

/// Tell the page the first frame is up, so it can drop the loading screen.
pub fn ready() {
    sb_ready()
}

pub fn audio_start() {
    sb_audio_start()
}

pub fn audio_queued_ms() -> i32 {
    sb_audio_queued_ms()
}

/// Interleaved stereo samples.
pub fn audio_push(pcm: &[i16]) {
    sb_audio_push(pcm)
}
