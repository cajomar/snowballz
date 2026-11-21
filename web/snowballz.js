// Browser glue for Snowballz: the functions the Rust side imports (window.sb,
// see crates/client/src/web.rs), plus the loader that downloads the game's
// files before the game starts. The game itself is game.js (wasm-bindgen).
"use strict";

(function () {

    // ---- Files -------------------------------------------------------------

    const files = new Map();

    async function download(onProgress) {
        const manifest = await (await fetch("assets/manifest.txt")).text();
        const list = manifest.split("\n").filter(l => l.trim()).map(l => {
            const [path, size] = l.split("\t");
            return { path, size: parseInt(size, 10) || 1 };
        });
        const total = list.reduce((a, f) => a + f.size, 0);
        let done = 0;
        let next = 0;
        async function worker() {
            while (next < list.length) {
                const f = list[next++];
                const r = await fetch("assets/" + f.path);
                if (!r.ok) throw new Error("couldn't download " + f.path + " (" + r.status + ")");
                files.set("assets/" + f.path, new Uint8Array(await r.arrayBuffer()));
                done += f.size;
                onProgress(done / total);
            }
        }
        await Promise.all([worker(), worker(), worker(), worker(), worker(), worker()]);
    }

    // ---- WebSockets --------------------------------------------------------

    const sockets = [];

    function pageServerUrl() {
        const q = new URLSearchParams(location.search).get("server");
        if (q) return /^wss?:\/\//.test(q) ? q : (location.protocol === "https:" ? "wss://" : "ws://") + q;
        return (location.protocol === "https:" ? "wss://" : "ws://") + location.host + "/ws";
    }

    // ---- Sound -------------------------------------------------------------

    // The game mixes in Rust and pushes finished blocks here, which are
    // queued back to back. Browsers only allow sound after a click or key.
    let actx = null;
    let playAt = 0;

    function startAudio() {
        if (actx) return;
        const AC = window.AudioContext || window.webkitAudioContext;
        if (!AC) return;
        actx = new AC();
        const resume = () => { if (actx.state !== "running") actx.resume(); };
        for (const ev of ["pointerdown", "keydown", "touchend"]) window.addEventListener(ev, resume);
    }

    // ---- Imports for the Rust side ----------------------------------------

    // Browsers that block storage throw even on a read; the game then
    // just plays with its default settings.
    function stored(key) {
        try { return localStorage.getItem("snowballz." + key); } catch (e) { return null; }
    }

    window.sb = {
        asset: path => files.get(path),
        serverUrl: pageServerUrl,
        pageHttps: () => location.protocol === "https:",
        wsOpen: url => {
            const s = { ws: null, queue: [], queued: 0, state: 0 };
            try {
                s.ws = new WebSocket(url);
                s.ws.binaryType = "arraybuffer";
                s.ws.onopen = () => { s.state = 1; };
                s.ws.onclose = () => { s.state = 2; };
                s.ws.onerror = () => { s.state = 2; };
                s.ws.onmessage = e => {
                    s.queue.push(new Uint8Array(e.data));
                    s.queued += e.data.byteLength;
                    // A hidden tab stops drawing frames, so nothing drains this.
                    // Snapshots are deltas and can't be skipped; give up instead
                    // of eating memory, and the game shows "disconnected".
                    if (s.queue.length > 4000 || s.queued > (64 << 20)) {
                        console.error("snowballz: fell too far behind the server");
                        s.queue = [];
                        s.queued = 0;
                        s.ws.onclose = s.ws.onerror = s.ws.onmessage = null;
                        s.ws.close();
                        s.state = 2;
                    }
                };
            } catch (e) {
                console.error(e);
                s.state = 2;
            }
            sockets.push(s);
            return sockets.length - 1;
        },
        wsState: id => sockets[id].state,
        wsSend: (id, bytes) => {
            const s = sockets[id];
            if (s.state === 1) s.ws.send(bytes.slice());
        },
        wsTake: id => {
            const s = sockets[id];
            const m = s.queue.shift();
            if (m) s.queued -= m.length;
            return m;
        },
        wsClose: id => {
            const s = sockets[id];
            s.queue = [];
            s.queued = 0;
            if (s.ws) { s.ws.onclose = s.ws.onerror = s.ws.onmessage = null; s.ws.close(); s.ws = null; }
            s.state = 2;
        },
        storeGet: key => { const v = stored(key); return v === null ? undefined : v; },
        storeSet: (key, value) => {
            try { localStorage.setItem("snowballz." + key, value); } catch (e) { /* private mode */ }
        },
        logError: msg => console.error(msg),
        ready: () => window.snowballzReady(),
        audioStart: startAudio,
        // Milliseconds of sound already queued ahead of the speakers.
        audioQueuedMs: () => {
            if (!actx) return 1000;
            return Math.max(0, (playAt - actx.currentTime) * 1000) | 0;
        },
        // Interleaved stereo 16-bit samples at 44.1 kHz.
        audioPush: pcm => {
            if (!actx) return;
            const frames = pcm.length >> 1;
            const buf = actx.createBuffer(2, frames, 44100);
            const l = buf.getChannelData(0), r = buf.getChannelData(1);
            for (let i = 0; i < frames; i++) {
                l[i] = pcm[i * 2] / 32768;
                r[i] = pcm[i * 2 + 1] / 32768;
            }
            const src = actx.createBufferSource();
            src.buffer = buf;
            src.connect(actx.destination);
            // Start a little ahead after a gap, so we don't underrun again at once.
            playAt = Math.max(playAt, actx.currentTime + 0.05);
            src.start(playAt);
            playAt += frames / 44100;
        },
    };

    // ---- Start-up ----------------------------------------------------------

    const $ = id => document.getElementById(id);
    const canvas = $("glcanvas");
    canvas.addEventListener("contextmenu", e => e.preventDefault());
    canvas.focus();

    window.snowballzLoad = async function () {
        const status = $("status");
        try {
            if (!navigator.gpu && !window.WebGL2RenderingContext) throw new Error("This browser has neither WebGPU nor WebGL 2, which the game needs.");
            await download(f => {
                $("fill").style.width = (f * 100).toFixed(0) + "%";
                status.textContent = "Downloading game files... " + (f * 100).toFixed(0) + "%";
            });
            status.textContent = "Starting...";
            // Let the browser paint "Starting..." before the game takes over the thread.
            await new Promise(r => setTimeout(r, 30));
            const game = await import("./game.js");
            await game.default({ module_or_path: "game_bg.wasm" });
        } catch (e) {
            console.error(e);
            status.textContent = "Couldn't start: " + e.message;
            status.style.color = "#ff9d9d";
        }
    };

    // The game calls this once its first frame is on screen.
    window.snowballzReady = function () {
        const l = $("loading");
        if (l) l.remove();
        canvas.focus();
    };
})();
