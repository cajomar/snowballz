//! Sound: a tiny software mixer. Effects are synthesized at startup; music
//! streams from Ogg files. Natively it runs on its own thread, with output
//! through ALSA on Linux (loaded at runtime like libGL, so a machine without
//! sound just plays silently) and the classic waveOut API on Windows. In a
//! browser there are no threads, so the frame loop tops up a queue of
//! finished blocks that the page plays through Web Audio.

use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::{channel, Sender};

pub const RATE: u32 = 44100;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sfx {
    Throw,
    Hit,
    Splat,
    TreeHit,
    Freeze,
    Chime,
    Powerup,
    Alarm,
    Captured,
    Lost,
    Defended,
    Victory,
    Defeat,
    Click,
    Chat,
    Chop,
    TreeFall,
    Pack,
    Built,
    Crumble,
    /// A Chieftain's War Cry.
    Shout,
    /// A snowball thumping into an igloo.
    IglooHit,
}
const N_SFX: usize = Sfx::IglooHit as usize + 1;
const ALL_SFX: [Sfx; N_SFX] = [
    Sfx::Throw,
    Sfx::Hit,
    Sfx::Splat,
    Sfx::TreeHit,
    Sfx::Freeze,
    Sfx::Chime,
    Sfx::Powerup,
    Sfx::Alarm,
    Sfx::Captured,
    Sfx::Lost,
    Sfx::Defended,
    Sfx::Victory,
    Sfx::Defeat,
    Sfx::Click,
    Sfx::Chat,
    Sfx::Chop,
    Sfx::TreeFall,
    Sfx::Pack,
    Sfx::Built,
    Sfx::Crumble,
    Sfx::Shout,
    Sfx::IglooHit,
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Music {
    Menu,
    Battle,
}

enum Cmd {
    Play { sfx: Sfx, gain: f32, pan: f32 },
    Music(Option<Music>),
    Volume { music: f32, sfx: f32 },
}

pub struct Audio {
    #[cfg(not(target_arch = "wasm32"))]
    tx: Option<Sender<Cmd>>,
    /// In a browser the mixer runs right here.
    #[cfg(target_arch = "wasm32")]
    mixer: Option<Box<Mixer>>,
    music: Option<Music>,
    /// Sounds started this frame, so a volley of fifty throws doesn't
    /// become fifty overlapping whooshes.
    this_frame: [u8; N_SFX],
}

impl Audio {
    pub fn silent() -> Audio {
        Audio {
            #[cfg(not(target_arch = "wasm32"))]
            tx: None,
            #[cfg(target_arch = "wasm32")]
            mixer: None,
            music: None,
            this_frame: [0; N_SFX],
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(assets: &std::path::Path, music_vol: f32, sfx_vol: f32) -> Audio {
        let Some(out) = backend::Output::open() else { return Audio::silent() };
        let (tx, rx) = channel();
        let dir = assets.join("music");
        let spawned = std::thread::Builder::new().name("audio".into()).spawn(move || {
            let mut mixer = Mixer::new(dir, music_vol, sfx_vol);
            mixer.run(rx, out);
        });
        Audio { tx: spawned.ok().map(|_| tx), music: None, this_frame: [0; N_SFX] }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn new(assets: &std::path::Path, music_vol: f32, sfx_vol: f32) -> Audio {
        crate::web::audio_start();
        Audio { mixer: Some(Box::new(Mixer::new(assets.join("music"), music_vol, sfx_vol))), music: None, this_frame: [0; N_SFX] }
    }

    pub fn end_frame(&mut self) {
        self.this_frame = [0; N_SFX];
        #[cfg(target_arch = "wasm32")]
        if let Some(m) = &mut self.mixer {
            // Keep a little sound queued ahead of the speakers.
            let mut mix = vec![0.0f32; BLOCK * 2];
            let mut pcm = vec![0i16; BLOCK * 2];
            for _ in 0..16 {
                if crate::web::audio_queued_ms() >= 120 {
                    break;
                }
                m.render(&mut mix, &mut pcm);
                crate::web::audio_push(&pcm);
            }
        }
    }

    fn send(&mut self, cmd: Cmd) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(tx) = &self.tx {
            let _ = tx.send(cmd);
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(m) = &mut self.mixer {
            m.apply(cmd);
        }
    }

    /// `pan` is -1 (left) to 1 (right).
    pub fn play_at(&mut self, sfx: Sfx, gain: f32, pan: f32) {
        let n = &mut self.this_frame[sfx as usize];
        let limit = match sfx {
            Sfx::Throw | Sfx::Hit | Sfx::Splat | Sfx::TreeHit | Sfx::Chop | Sfx::Pack => 3,
            _ => 1,
        };
        if *n >= limit || gain < 0.02 {
            return;
        }
        *n += 1;
        self.send(Cmd::Play { sfx, gain, pan: pan.clamp(-1.0, 1.0) });
    }

    pub fn play(&mut self, sfx: Sfx) {
        self.play_at(sfx, 1.0, 0.0);
    }

    pub fn music(&mut self, m: Option<Music>) {
        if m != self.music {
            self.music = m;
            self.send(Cmd::Music(m));
        }
    }

    pub fn set_volume(&mut self, music: f32, sfx: f32) {
        self.send(Cmd::Volume { music, sfx });
    }
}

// -------------------------------------------------------------------------
// Mixer

struct Voice {
    sfx: usize,
    pos: usize,
    gl: f32,
    gr: f32,
}

struct Stream {
    kind: Music,
    reader: lewton::inside_ogg::OggStreamReader<std::io::Cursor<Vec<u8>>>,
    channels: usize,
    buf: Vec<i16>,
    at: usize,
}

/// Frames per mixed block.
const BLOCK: usize = 512;

struct Mixer {
    dir: PathBuf,
    sounds: Vec<Vec<f32>>,
    voices: Vec<Voice>,
    music_vol: f32,
    sfx_vol: f32,
    want: Option<Music>,
    stream: Option<Stream>,
    /// Fades the current track out before switching.
    fade: f32,
    battle_track: usize,
}

const BATTLE_TRACKS: [&str; 2] = ["battle1.ogg", "battle2.ogg"];

impl Mixer {
    fn new(dir: PathBuf, music_vol: f32, sfx_vol: f32) -> Mixer {
        let sounds = ALL_SFX
            .iter()
            .map(|&s| {
                // Battle sounds come in crowds, so they sit lower in the mix.
                let level = match s {
                    Sfx::Hit | Sfx::TreeHit | Sfx::IglooHit => 0.5,
                    Sfx::Splat | Sfx::Freeze => 0.6,
                    Sfx::Throw => 1.4,
                    _ => 1.0,
                };
                synth(s).into_iter().map(|x| x * level).collect()
            })
            .collect();
        Mixer { dir, sounds, voices: Vec::new(), music_vol, sfx_vol, want: None, stream: None, fade: 0.0, battle_track: 0 }
    }

    fn open_track(&mut self) {
        let Some(kind) = self.want else { return };
        let name = match kind {
            Music::Menu => "menu.ogg",
            Music::Battle => {
                self.battle_track += 1;
                BATTLE_TRACKS[self.battle_track % BATTLE_TRACKS.len()]
            }
        };
        let file = crate::assets::read(&self.dir.join(name)).ok();
        match file.and_then(|f| lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(f)).ok()) {
            Some(reader) => {
                let channels = reader.ident_hdr.audio_channels as usize;
                self.stream = Some(Stream { kind, reader, channels, buf: Vec::new(), at: 0 });
            }
            // Missing or broken file: stay quiet rather than retrying every block.
            None => self.want = None,
        }
    }

    fn apply(&mut self, cmd: Cmd) {
        match cmd {
            Cmd::Play { sfx, gain, pan } => {
                if self.voices.len() >= 32 {
                    self.voices.remove(0);
                }
                let a = (pan + 1.0) * std::f32::consts::FRAC_PI_4;
                self.voices.push(Voice { sfx: sfx as usize, pos: 0, gl: gain * a.cos(), gr: gain * a.sin() });
            }
            Cmd::Music(m) => self.want = m,
            Cmd::Volume { music, sfx } => {
                self.music_vol = music;
                self.sfx_vol = sfx;
            }
        }
    }

    /// Mix the next block of stereo output into `pcm` (`mix` is scratch).
    fn render(&mut self, mix: &mut [f32], pcm: &mut [i16]) {
        let frames = pcm.len() / 2;
        mix.fill(0.0);
        self.mix_music(mix);
        for v in &mut self.voices {
            let s = &self.sounds[v.sfx];
            let n = (s.len() - v.pos).min(frames);
            for i in 0..n {
                let x = s[v.pos + i] * self.sfx_vol;
                mix[i * 2] += x * v.gl;
                mix[i * 2 + 1] += x * v.gr;
            }
            v.pos += n;
        }
        self.voices.retain(|v| v.pos < self.sounds[v.sfx].len());
        for (o, &x) in pcm.iter_mut().zip(mix.iter()) {
            // Soft clip so pile-ups distort gently instead of wrapping.
            let y = x / (1.0 + x.abs() * 0.3);
            *o = (y.clamp(-1.0, 1.0) * 32000.0) as i16;
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn run(&mut self, rx: std::sync::mpsc::Receiver<Cmd>, mut out: backend::Output) {
        let mut mix = vec![0.0f32; BLOCK * 2];
        let mut pcm = vec![0i16; BLOCK * 2];
        loop {
            loop {
                match rx.try_recv() {
                    Ok(cmd) => self.apply(cmd),
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
                }
            }
            self.render(&mut mix, &mut pcm);
            if !out.write(&pcm) {
                return;
            }
        }
    }

    fn mix_music(&mut self, mix: &mut [f32]) {
        let frames = mix.len() / 2;
        let step = frames as f32 / RATE as f32;
        // Fade out a track that's no longer wanted, then start the new one.
        if self.stream.as_ref().is_some_and(|st| Some(st.kind) != self.want) {
            self.fade -= step / 0.8;
            if self.fade <= 0.0 {
                self.stream = None;
            }
        } else {
            self.fade = (self.fade + step / 1.5).min(1.0);
        }
        if self.stream.is_none() && self.want.is_some() {
            self.open_track();
            self.fade = 0.0;
        }
        let Some(st) = &mut self.stream else { return };
        let g = self.fade.clamp(0.0, 1.0) * self.music_vol * 0.55 / 32768.0;
        for i in 0..frames {
            while st.at >= st.buf.len() {
                st.at = 0;
                st.buf = match st.reader.read_dec_packet_itl() {
                    Ok(Some(p)) => p,
                    _ => {
                        // Track finished; the next block starts another.
                        self.stream = None;
                        return;
                    }
                };
            }
            let l = st.buf[st.at] as f32;
            let r = if st.channels >= 2 { st.buf[st.at + 1] as f32 } else { l };
            st.at += st.channels.max(1);
            mix[i * 2] += l * g;
            mix[i * 2 + 1] += r * g;
        }
    }
}

// -------------------------------------------------------------------------
// Synthesized effects

struct Noise(u32);
impl Noise {
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        (self.0 as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

/// One-pole low-pass.
struct Lp(f32);
impl Lp {
    fn run(&mut self, x: f32, cutoff: f32) -> f32 {
        let a = 1.0 - (-std::f32::consts::TAU * cutoff / RATE as f32).exp();
        self.0 += (x - self.0) * a;
        self.0
    }
}

fn len(secs: f32) -> usize {
    (secs * RATE as f32) as usize
}

fn tone(out: &mut [f32], start: f32, dur: f32, freq: f32, gain: f32, decay: f32) {
    let s = len(start);
    for i in 0..len(dur).min(out.len().saturating_sub(s)) {
        let t = i as f32 / RATE as f32;
        let env = (t * 200.0).min(1.0) * (-t * decay).exp();
        // A couple of partials for a soft bell.
        let p = std::f32::consts::TAU * freq * t;
        out[s + i] += gain * env * (p.sin() + 0.3 * (2.0 * p).sin() + 0.12 * (3.0 * p).sin());
    }
}

fn synth(sfx: Sfx) -> Vec<f32> {
    use std::f32::consts::TAU;
    let mut n = Noise(0x9e37_79b9 ^ (sfx as u32 * 7919 + 1));
    match sfx {
        Sfx::Throw => {
            // Whoosh: band-limited noise swelling then fading.
            let d = len(0.2);
            let (mut a, mut b) = (Lp(0.0), Lp(0.0));
            (0..d)
                .map(|i| {
                    let t = i as f32 / d as f32;
                    let cut = 900.0 + 2500.0 * t;
                    let x = n.next();
                    let band = a.run(x, cut) - b.run(x, cut * 0.35);
                    band * (t * std::f32::consts::PI).sin().powf(1.5) * 0.5
                })
                .collect()
        }
        Sfx::Hit | Sfx::Splat => {
            // Snow "pomf": muffled crunch over a low thump.
            let direct = sfx == Sfx::Hit;
            let d = len(if direct { 0.22 } else { 0.16 });
            let mut lp = Lp(0.0);
            let mut ph = 0.0f32;
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    let crunch = lp.run(n.next(), 2200.0) * (-t * 28.0).exp() * 1.4;
                    let f = 140.0 * (-t * 12.0).exp() + 50.0;
                    ph += TAU * f / RATE as f32;
                    let thump = ph.sin() * (-t * 22.0).exp() * if direct { 0.7 } else { 0.25 };
                    (crunch + thump) * if direct { 0.9 } else { 0.55 }
                })
                .collect()
        }
        Sfx::TreeHit => {
            let d = len(0.3);
            let (mut a, mut ph) = (Lp(0.0), 0.0f32);
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    ph += TAU * 95.0 / RATE as f32;
                    let knock = ph.sin() * (-t * 30.0).exp() * 0.6;
                    let rustle = a.run(n.next(), 4000.0) * (-t * 9.0).exp() * (t * 60.0).min(1.0) * 0.35;
                    knock + rustle
                })
                .collect()
        }
        Sfx::Chop => {
            // Axe on frozen wood: a hollow knock with a woody click.
            let d = len(0.18);
            let (mut a, mut ph) = (Lp(0.0), 0.0f32);
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    ph += TAU * (310.0 - t * 400.0) / RATE as f32;
                    let knock = (ph.sin() + 0.4 * (2.7 * ph).sin()) * (-t * 38.0).exp() * 0.5;
                    let x = n.next();
                    let click = (x - a.run(x, 1500.0)) * (-t * 160.0).exp() * 0.6;
                    knock + click
                })
                .collect()
        }
        Sfx::Pack => {
            // Patting snow into a block: two soft crunchy pats.
            let d = len(0.22);
            let mut lp = Lp(0.0);
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    let tt = if t < 0.1 { t } else { t - 0.1 };
                    lp.run(n.next(), 1400.0) * (-tt * 45.0).exp() * if t < 0.1 { 0.8 } else { 0.55 }
                })
                .collect()
        }
        Sfx::Built => {
            let mut out = vec![0.0; len(0.6)];
            for (k, f) in [392.0, 523.0, 659.0].iter().enumerate() {
                tone(&mut out, k as f32 * 0.06, 0.45, *f, 0.18, 6.0);
            }
            out
        }
        Sfx::Crumble => {
            // A wall slumping: low rumble with crunchy bits.
            let d = len(0.8);
            let (mut a, mut b) = (Lp(0.0), Lp(0.0));
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    let x = n.next();
                    let rumble = a.run(x, 220.0) * 3.0 * (-t * 4.0).exp();
                    let bits = if x.abs() > 0.85 { b.run(x, 3000.0) * 1.5 } else { 0.0 } * (-t * 5.0).exp();
                    (rumble + bits) * (t * 30.0).min(1.0) * 0.7
                })
                .collect()
        }
        Sfx::Shout => {
            // A war cry: a low call rising and breaking, with the wind in it.
            let d = len(0.9);
            let mut ph = 0.0f32;
            let mut lp = Lp(0.0);
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    let rise = (t * 2.6).min(1.0);
                    let f = (170.0 + 150.0 * rise + 12.0 * (t * 26.0).sin()) * (1.0 - 0.25 * t);
                    ph += TAU * f / RATE as f32;
                    let voice = (ph.sin() + 0.5 * (2.0 * ph).sin() + 0.25 * (3.0 * ph).sin()) * 0.5;
                    let breath = lp.run(n.next(), 1800.0) * 0.5;
                    let env = (t * 60.0).min(1.0) * (-t * 2.6).exp();
                    (voice + breath) * env * 0.8
                })
                .collect()
        }
        Sfx::IglooHit => {
            // A snowball thumping into an ice dome: low and soft, gone in a blink.
            let d = len(0.18);
            let (mut lp, mut ph) = (Lp(0.0), 0.0f32);
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    ph += TAU * (75.0 * (-t * 9.0).exp() + 45.0) / RATE as f32;
                    let thump = ph.sin() * (-t * 26.0).exp() * 0.7;
                    let puff = lp.run(n.next(), 900.0) * (-t * 40.0).exp() * 0.25;
                    thump + puff
                })
                .collect()
        }
        Sfx::TreeFall => {
            // A creak, then the trunk thumping into the snow and a shower of it.
            let d = len(1.3);
            let (mut a, mut b, mut ph, mut th) = (Lp(0.0), Lp(0.0), 0.0f32, 0.0f32);
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    let creak = if t < 0.5 {
                        ph += TAU * (180.0 + 60.0 * (t * 23.0).sin()) / RATE as f32;
                        let saw = (ph / TAU).fract() * 2.0 - 1.0;
                        a.run(saw, 900.0) * (t * 6.0).min(1.0) * (1.0 - t * 2.0) * 0.35
                    } else {
                        0.0
                    };
                    let tt = (t - 0.55).max(0.0);
                    let hit = if t >= 0.55 {
                        th += TAU * (70.0 * (-tt * 6.0).exp() + 38.0) / RATE as f32;
                        th.sin() * (-tt * 9.0).exp() * 0.8
                    } else {
                        0.0
                    };
                    let shower = if t >= 0.55 { b.run(n.next(), 2500.0) * (-tt * 4.5).exp() * (tt * 40.0).min(1.0) * 0.6 } else { 0.0 };
                    creak + hit + shower
                })
                .collect()
        }
        Sfx::Freeze => {
            // Ice crackle and a glassy shimmer.
            let mut out = vec![0.0; len(0.9)];
            for k in 0..7 {
                let f = 1800.0 + 2600.0 * (n.next() * 0.5 + 0.5);
                tone(&mut out, k as f32 * 0.05, 0.8, f, 0.08, 6.0);
            }
            let mut lp = Lp(0.0);
            for (i, o) in out.iter_mut().enumerate().take(len(0.35)) {
                let t = i as f32 / RATE as f32;
                let x = n.next();
                let crackle = if x.abs() > 0.93 { x * 2.0 } else { 0.0 };
                *o += (crackle - lp.run(crackle, 600.0)) * 0.5 * (-t * 8.0).exp();
            }
            out
        }
        Sfx::Chime => {
            let d = len(0.14);
            let mut ph = 0.0f32;
            (0..d)
                .map(|i| {
                    let t = i as f32 / d as f32;
                    ph += TAU * (380.0 + 700.0 * t) / RATE as f32;
                    ph.sin() * (1.0 - t).powi(2) * (t * 40.0).min(1.0) * 0.5
                })
                .collect()
        }
        Sfx::Powerup => {
            let mut out = vec![0.0; len(0.7)];
            for (k, f) in [784.0, 988.0, 1175.0, 1568.0].iter().enumerate() {
                tone(&mut out, k as f32 * 0.07, 0.5, *f, 0.18, 7.0);
            }
            out
        }
        Sfx::Alarm => {
            let mut out = vec![0.0; len(0.75)];
            for k in 0..3 {
                tone(&mut out, k as f32 * 0.22, 0.2, if k % 2 == 0 { 660.0 } else { 523.0 }, 0.3, 5.0);
            }
            out
        }
        Sfx::Captured => {
            let mut out = vec![0.0; len(1.0)];
            for (k, f) in [523.0, 659.0, 784.0, 1047.0].iter().enumerate() {
                tone(&mut out, k as f32 * 0.09, 0.8, *f, 0.22, 4.0);
            }
            out
        }
        Sfx::Lost => {
            let mut out = vec![0.0; len(1.0)];
            for (k, f) in [494.0, 415.0, 349.0].iter().enumerate() {
                tone(&mut out, k as f32 * 0.16, 0.7, *f, 0.25, 4.0);
            }
            out
        }
        Sfx::Defended => {
            let mut out = vec![0.0; len(0.7)];
            tone(&mut out, 0.0, 0.5, 587.0, 0.22, 5.0);
            tone(&mut out, 0.12, 0.5, 880.0, 0.22, 5.0);
            out
        }
        Sfx::Victory => {
            let mut out = vec![0.0; len(2.2)];
            let notes = [(0.0, 523.0), (0.15, 659.0), (0.3, 784.0), (0.45, 1047.0), (0.75, 784.0), (0.9, 1047.0)];
            for (t, f) in notes {
                tone(&mut out, t, 1.2, f, 0.2, 2.5);
            }
            tone(&mut out, 0.9, 1.3, 523.0, 0.15, 1.5);
            out
        }
        Sfx::Defeat => {
            let mut out = vec![0.0; len(2.0)];
            for (k, f) in [392.0, 370.0, 349.0, 262.0].iter().enumerate() {
                tone(&mut out, k as f32 * 0.3, 1.0, *f, 0.22, 2.5);
            }
            out
        }
        Sfx::Click => {
            let d = len(0.04);
            let mut ph = 0.0f32;
            (0..d)
                .map(|i| {
                    let t = i as f32 / RATE as f32;
                    ph += TAU * 1500.0 / RATE as f32;
                    ph.sin() * (-t * 120.0).exp() * 0.25
                })
                .collect()
        }
        Sfx::Chat => {
            let mut out = vec![0.0; len(0.3)];
            tone(&mut out, 0.0, 0.25, 1175.0, 0.12, 14.0);
            tone(&mut out, 0.06, 0.25, 1568.0, 0.12, 14.0);
            out
        }
    }
}

// -------------------------------------------------------------------------
// Output

#[cfg(all(target_os = "linux", not(target_arch = "wasm32")))]
mod backend {
    use std::ffi::{c_char, c_int, c_long, c_uint, c_ulong, c_void};

    extern "C" {
        fn dlopen(file: *const c_char, mode: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    type Open = unsafe extern "C" fn(*mut *mut c_void, *const c_char, c_int, c_int) -> c_int;
    type SetParams = unsafe extern "C" fn(*mut c_void, c_int, c_int, c_uint, c_uint, c_int, c_uint) -> c_int;
    type Writei = unsafe extern "C" fn(*mut c_void, *const c_void, c_ulong) -> c_long;
    type Recover = unsafe extern "C" fn(*mut c_void, c_int, c_int) -> c_int;

    pub struct Output {
        pcm: *mut c_void,
        writei: Writei,
        recover: Recover,
    }

    // The handle is only ever used from the audio thread.
    unsafe impl Send for Output {}

    impl Output {
        pub fn open() -> Option<Output> {
            unsafe {
                let lib = dlopen(c"libasound.so.2".as_ptr(), 2);
                if lib.is_null() {
                    return None;
                }
                let sym = |name: &std::ffi::CStr| {
                    let p = dlsym(lib, name.as_ptr());
                    (!p.is_null()).then_some(p)
                };
                let open: Open = std::mem::transmute(sym(c"snd_pcm_open")?);
                let set_params: SetParams = std::mem::transmute(sym(c"snd_pcm_set_params")?);
                let writei: Writei = std::mem::transmute(sym(c"snd_pcm_writei")?);
                let recover: Recover = std::mem::transmute(sym(c"snd_pcm_recover")?);
                let mut pcm = std::ptr::null_mut();
                // Playback, blocking.
                if open(&mut pcm, c"default".as_ptr(), 0, 0) < 0 {
                    return None;
                }
                // S16_LE, interleaved, stereo, resampling allowed, 60 ms latency.
                if set_params(pcm, 2, 3, 2, super::RATE, 1, 60_000) < 0 {
                    return None;
                }
                Some(Output { pcm, writei, recover })
            }
        }

        /// Blocks until the device takes the samples. False if it's gone.
        pub fn write(&mut self, pcm: &[i16]) -> bool {
            let mut off = 0;
            while off < pcm.len() {
                let frames = ((pcm.len() - off) / 2) as c_ulong;
                let n = unsafe { (self.writei)(self.pcm, pcm[off..].as_ptr() as *const c_void, frames) };
                if n < 0 {
                    if unsafe { (self.recover)(self.pcm, n as c_int, 1) } < 0 {
                        return false;
                    }
                } else {
                    off += n as usize * 2;
                }
            }
            true
        }
    }
}

#[cfg(windows)]
mod backend {
    use std::ffi::c_void;

    #[repr(C, packed)]
    struct WaveFormatEx {
        format_tag: u16,
        channels: u16,
        samples_per_sec: u32,
        avg_bytes_per_sec: u32,
        block_align: u16,
        bits_per_sample: u16,
        cb_size: u16,
    }

    #[repr(C)]
    struct WaveHdr {
        data: *mut c_void,
        buffer_length: u32,
        bytes_recorded: u32,
        user: usize,
        flags: u32,
        loops: u32,
        next: *mut WaveHdr,
        reserved: usize,
    }

    const WAVE_MAPPER: u32 = 0xFFFF_FFFF;
    const WHDR_DONE: u32 = 1;
    const BUFFERS: usize = 6;

    #[link(name = "winmm")]
    extern "system" {
        fn waveOutOpen(h: *mut *mut c_void, dev: u32, fmt: *const WaveFormatEx, cb: usize, inst: usize, flags: u32) -> u32;
        fn waveOutPrepareHeader(h: *mut c_void, hdr: *mut WaveHdr, size: u32) -> u32;
        fn waveOutWrite(h: *mut c_void, hdr: *mut WaveHdr, size: u32) -> u32;
    }

    /// A ring of buffers handed to waveOut in turn; `write` waits for the
    /// oldest to finish playing before refilling it.
    pub struct Output {
        dev: *mut c_void,
        hdrs: Box<[WaveHdr]>,
        bufs: Vec<Vec<i16>>,
        next: usize,
        used: usize,
    }

    // The device is only ever used from the audio thread.
    unsafe impl Send for Output {}

    impl Output {
        pub fn open() -> Option<Output> {
            let fmt = WaveFormatEx {
                format_tag: 1, // PCM
                channels: 2,
                samples_per_sec: super::RATE,
                avg_bytes_per_sec: super::RATE * 4,
                block_align: 4,
                bits_per_sample: 16,
                cb_size: 0,
            };
            let mut dev = std::ptr::null_mut();
            if unsafe { waveOutOpen(&mut dev, WAVE_MAPPER, &fmt, 0, 0, 0) } != 0 {
                return None;
            }
            let hdrs = (0..BUFFERS)
                .map(|_| WaveHdr {
                    data: std::ptr::null_mut(),
                    buffer_length: 0,
                    bytes_recorded: 0,
                    user: 0,
                    flags: 0,
                    loops: 0,
                    next: std::ptr::null_mut(),
                    reserved: 0,
                })
                .collect();
            Some(Output { dev, hdrs, bufs: vec![Vec::new(); BUFFERS], next: 0, used: 0 })
        }

        pub fn write(&mut self, pcm: &[i16]) -> bool {
            let i = self.next;
            self.next = (i + 1) % BUFFERS;
            let size = std::mem::size_of::<WaveHdr>() as u32;
            let hdr: *mut WaveHdr = &mut self.hdrs[i];
            if self.used > i {
                // Wait for this buffer's previous contents to finish playing.
                while unsafe { std::ptr::read_volatile(&(*hdr).flags) } & WHDR_DONE == 0 {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
            }
            let buf = &mut self.bufs[i];
            if buf.len() != pcm.len() {
                // (Re)prepare when the block size changes (only the first time).
                *buf = pcm.to_vec();
                unsafe {
                    (*hdr).data = buf.as_mut_ptr() as *mut c_void;
                    (*hdr).buffer_length = (pcm.len() * 2) as u32;
                    (*hdr).flags = 0;
                    if waveOutPrepareHeader(self.dev, hdr, size) != 0 {
                        return false;
                    }
                }
            } else {
                buf.copy_from_slice(pcm);
            }
            self.used = self.used.max(i + 1);
            unsafe { waveOutWrite(self.dev, hdr, size) == 0 }
        }
    }
}

#[cfg(not(any(target_os = "linux", windows, target_arch = "wasm32")))]
mod backend {
    pub struct Output;
    impl Output {
        pub fn open() -> Option<Output> {
            None
        }
        pub fn write(&mut self, _pcm: &[i16]) -> bool {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effects_are_sane() {
        for s in ALL_SFX {
            let w = synth(s);
            let peak = w.iter().fold(0.0f32, |a, x| a.max(x.abs()));
            assert!(!w.is_empty() && w.iter().all(|x| x.is_finite()), "{s:?}");
            assert!(peak > 0.05 && peak < 1.5, "{s:?} peak {peak}");
        }
    }

    #[test]
    fn music_decodes() {
        // The repo ships no music; check whatever tracks have been dropped in.
        let dir = server::find_assets().join("music");
        for name in ["menu.ogg", BATTLE_TRACKS[0], BATTLE_TRACKS[1]] {
            let Ok(f) = std::fs::read(dir.join(name)) else { continue };
            let mut r = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(f)).unwrap();
            assert_eq!(r.ident_hdr.audio_sample_rate, RATE, "{name}");
            assert!(r.read_dec_packet_itl().unwrap().is_some());
        }
    }
}
