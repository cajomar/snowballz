//! Network protocol.
//!
//! Every message is a frame: `u32 length` followed by `length` bytes, the
//! first of which is the message tag. The server is authoritative; each tick
//! it sends every client a delta snapshot containing only what changed since
//! the last snapshot that client received (TCP guarantees it got it).

use std::io::{self, Read, Write};

use crate::ai::AiLevel;
use crate::game::{Levels, BRANCHES};
use crate::game::{
    Act, BuildKind, Carry, Command, Event, Game, HitKind, Kind, PowerKind, SealState, Season, Snowball, Takeover, Weather,
    NEUTRAL,
};
use crate::map::{Ground, Map, Obstacle, ResourceKind};
use crate::math::{v2, Vec2};
use crate::wire::{Reader, Result, WireError, Writer};

pub const PROTOCOL_VERSION: u16 = 57;
pub const DEFAULT_PORT: u16 = 7777;
/// UDP port servers announce themselves on while in the lobby.
pub const DISCOVERY_PORT: u16 = 7778;
pub const DISCOVERY_MAGIC: &[u8] = b"SNOWBALLZ1";
const MAX_FRAME: usize = 8 << 20;
/// Largest `.sbmap` a host may send with `ClientMsg::AddMap`.
pub const MAX_MAP_BYTES: usize = 2 << 20;

// -------------------------------------------------------------------------
// Framing

pub fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    w.write_all(&(payload.len() as u32).to_le_bytes())?;
    w.write_all(payload)
}

/// Incrementally assembles frames from a non-blocking stream.
#[derive(Default)]
pub struct FrameReader {
    buf: Vec<u8>,
    /// Bytes at the front of `buf` already handed out as frames. They are
    /// dropped in one go when more arrives, so a pile of small frames costs
    /// a linear pass, not one shift of the buffer per frame.
    start: usize,
    /// Largest frame this reader accepts; 0 means `MAX_FRAME`.
    limit: usize,
}

impl FrameReader {
    /// Refuse frames bigger than `n` bytes (a server won't take big ones
    /// from just anybody).
    pub fn set_limit(&mut self, n: usize) {
        self.limit = n;
    }

    /// Bytes read but not yet handed out as frames.
    pub fn buffered(&self) -> usize {
        self.buf.len() - self.start
    }

    fn compact(&mut self) {
        if self.start > 0 {
            self.buf.drain(..self.start);
            self.start = 0;
        }
    }

    /// Reads what's available; returns Err on disconnect.
    pub fn fill(&mut self, r: &mut impl Read) -> io::Result<()> {
        self.compact();
        let mut tmp = [0u8; 16384];
        loop {
            match r.read(&mut tmp) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "closed")),
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
    }

    /// Feed bytes that didn't come straight from a socket (a WebSocket
    /// message, say).
    pub fn push(&mut self, bytes: &[u8]) {
        self.compact();
        self.buf.extend_from_slice(bytes);
    }

    // Not an Iterator: it can fail, and it owns the buffer it reads from.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        let buf = &self.buf[self.start..];
        if buf.len() < 4 {
            return Ok(None);
        }
        let n = u32::from_le_bytes(buf[..4].try_into().unwrap()) as usize;
        if n > if self.limit == 0 { MAX_FRAME } else { self.limit } {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "frame too large"));
        }
        if buf.len() < 4 + n {
            return Ok(None);
        }
        let frame = buf[4..4 + n].to_vec();
        self.start += 4 + n;
        Ok(Some(frame))
    }
}

// -------------------------------------------------------------------------
// Lobby

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Open,
    Closed,
    Human,
    Ai(AiLevel),
}

impl SlotKind {
    fn encode(self, w: &mut Writer) {
        match self {
            SlotKind::Open => w.u8(0),
            SlotKind::Closed => w.u8(1),
            SlotKind::Human => w.u8(2),
            SlotKind::Ai(l) => w.u8(3 + l as u8),
        };
    }
    fn decode(r: &mut Reader) -> Result<SlotKind> {
        Ok(match r.u8()? {
            0 => SlotKind::Open,
            1 => SlotKind::Closed,
            2 => SlotKind::Human,
            n => SlotKind::Ai(AiLevel::from_u8(n - 3)),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Slot {
    pub kind: SlotKind,
    pub name: String,
    pub color: u8,
    pub team: u8,
    /// Connection id of the human in this slot (0 = none).
    pub conn: u32,
}

#[derive(Clone, Debug)]
pub struct MapInfo {
    pub name: String,
    pub description: String,
    pub players: u8,
}

#[derive(Clone, Debug)]
pub struct LobbyState {
    pub server_name: String,
    pub host: u32,
    pub maps: Vec<MapInfo>,
    pub map: u8,
    pub slots: Vec<Slot>,
    pub spectators: Vec<String>,
    pub in_game: bool,
}

/// Static per-game data sent once at game start.
#[derive(Clone, Debug)]
pub struct GameInfo {
    pub map: Map,
    pub players: Vec<PlayerInfo>,
    pub you: u8,
    pub igloos: Vec<IglooStatic>,
    pub resources: Vec<ResourceStatic>,
    /// Igloo tiers and research are on (parked; see `CoreRules::tech`).
    pub tech: bool,
    /// The tick the Long Night begins.
    pub long_night_at: u32,
}

#[derive(Clone, Debug)]
pub struct PlayerInfo {
    pub name: String,
    pub color: u8,
    pub team: u8,
    pub ai: bool,
}

#[derive(Clone, Debug)]
pub struct IglooStatic {
    pub tile: (i32, i32),
    pub center: Vec2,
    pub door: Vec2,
}

#[derive(Clone, Debug)]
pub struct ResourceStatic {
    pub tile: (i32, i32),
    pub pos: Vec2,
    pub kind: ResourceKind,
    pub max: u8,
}

impl GameInfo {
    pub fn from_game(g: &Game, you: u8) -> GameInfo {
        GameInfo {
            tech: g.core.tech,
            map: g.map.clone(),
            players: g
                .players
                .iter()
                .map(|p| PlayerInfo { name: p.name.clone(), color: p.color, team: p.team, ai: p.ai.is_some() })
                .collect(),
            you,
            igloos: g
                .igloos
                .iter()
                .map(|i| IglooStatic { tile: i.tile, center: i.center, door: i.door })
                .collect(),
            resources: g
                .resources
                .iter()
                .map(|r| ResourceStatic { tile: r.tile, pos: r.pos, kind: r.kind, max: r.max })
                .collect(),
            long_night_at: g.long_night_at,
        }
    }
}

// -------------------------------------------------------------------------
// Messages

pub enum ClientMsg {
    /// `token` is the one the server gave this client last time (0 if none): it
    /// is what proves a rejoin is the same player, not just the same name.
    Hello { version: u16, name: String, token: u64 },
    SetMap(u8),
    SetSlot { slot: u8, kind: SlotKind },
    SetTeam { slot: u8, team: u8 },
    SetColor { slot: u8, color: u8 },
    TakeSlot(u8),
    Start,
    Cmd(Command),
    Ping(u32),
    BackToLobby,
    /// A host's new map: the `.sbmap` bytes from `Map::encode`.
    AddMap(Vec<u8>),
}

/// Most lines of notes sent per computer player, and the longest line.
pub const AI_NOTE_LINES: usize = 70;
pub const AI_NOTE_WIDTH: usize = 300;

pub enum ServerMsg {
    /// `token` is this client's secret for rejoining a running game.
    Welcome { conn: u32, token: u64 },
    Reject(String),
    Lobby(LobbyState),
    GameStart(Box<GameInfo>),
    Snapshot(Vec<u8>),
    Chat { from: String, color: u8, text: String, private: bool },
    Pong(u32),
    /// What each computer player is thinking, as lines of text. Sent to
    /// spectators and beaten players, and to players when the server shares it.
    AiNotes(Vec<(u8, Vec<String>)>),
}

fn encode_units(w: &mut Writer, units: &[u16]) {
    w.u16(units.len().min(2000) as u16);
    for &u in units.iter().take(2000) {
        w.u16(u);
    }
}

fn decode_units(r: &mut Reader) -> Result<Vec<u16>> {
    let n = r.u16()?;
    (0..n).map(|_| r.u16()).collect()
}

impl ClientMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            ClientMsg::Hello { version, name, token } => {
                w.u8(0).u16(*version).str(name).u64(*token);
            }
            ClientMsg::SetMap(m) => {
                w.u8(1).u8(*m);
            }
            ClientMsg::SetSlot { slot, kind } => {
                w.u8(2).u8(*slot);
                kind.encode(&mut w);
            }
            ClientMsg::SetTeam { slot, team } => {
                w.u8(3).u8(*slot).u8(*team);
            }
            ClientMsg::SetColor { slot, color } => {
                w.u8(4).u8(*slot).u8(*color);
            }
            ClientMsg::TakeSlot(s) => {
                w.u8(5).u8(*s);
            }
            ClientMsg::Start => {
                w.u8(6);
            }
            ClientMsg::Cmd(c) => {
                w.u8(7);
                match c {
                    Command::Move { units, target, run, queue } => {
                        w.u8(0);
                        encode_units(&mut w, units);
                        w.f32(target.x).f32(target.y).bool(*run).bool(*queue);
                    }
                    Command::Enter { units, igloo } => {
                        w.u8(1);
                        encode_units(&mut w, units);
                        w.u8(*igloo);
                    }
                    Command::Gather { units, resource } => {
                        w.u8(2);
                        encode_units(&mut w, units);
                        w.u16(*resource);
                    }
                    Command::Stop { units } => {
                        w.u8(3);
                        encode_units(&mut w, units);
                    }
                    Command::Chat { to, text } => {
                        w.u8(5).u8(to.unwrap_or(NEUTRAL)).str(text);
                    }
                    Command::Chop { units, tile } => {
                        w.u8(6);
                        encode_units(&mut w, units);
                        w.u16(tile.0 as u16).u16(tile.1 as u16);
                    }
                    Command::Build { units, kind, tiles } => {
                        w.u8(7);
                        encode_units(&mut w, units);
                        w.u8(*kind as u8).u8(tiles.len().min(64) as u8);
                        for t in tiles.iter().take(64) {
                            w.u16(t.0 as u16).u16(t.1 as u16);
                        }
                    }
                    Command::Repair { units, building } => {
                        w.u8(8);
                        encode_units(&mut w, units);
                        w.u16(*building);
                    }
                    Command::Demolish { building } => {
                        w.u8(9).u16(*building);
                    }
                    Command::Capture { units, penguin } => {
                        w.u8(10);
                        encode_units(&mut w, units);
                        w.u16(*penguin);
                    }
                    Command::Release { igloo } => {
                        w.u8(11).u8(*igloo);
                    }
                    Command::Upgrade { igloo } => {
                        w.u8(14).u8(*igloo);
                    }
                    Command::StartResearch { igloo, branch } => {
                        w.u8(15).u8(*igloo).u8(*branch);
                    }
                    Command::WarCry { unit } => {
                        w.u8(16).u16(*unit);
                    }
                    Command::Thaw { units, penguin } => {
                        w.u8(17);
                        encode_units(&mut w, units);
                        w.u16(*penguin);
                    }
                    Command::Hold { units } => {
                        w.u8(18);
                        encode_units(&mut w, units);
                    }
                    Command::SetRally { building, at } => {
                        w.u8(20);
                        w.u16(*building).f32(at.x).f32(at.y);
                    }
                    Command::Tend { units, building } => {
                        w.u8(21);
                        encode_units(&mut w, units);
                        w.u16(*building);
                    }
                    Command::Fish { units, tile } => {
                        w.u8(22);
                        encode_units(&mut w, units);
                        w.u16(tile.0 as u16).u16(tile.1 as u16);
                    }
                    Command::Garrison { units, building } => {
                        w.u8(23);
                        encode_units(&mut w, units);
                        w.u16(*building);
                    }
                    Command::Concede => {
                        w.u8(26);
                    }
                    Command::UpgradeBuilding { building } => {
                        w.u8(28).u16(*building);
                    }
                    Command::SetWant { building, want } => {
                        w.u8(29).u16(*building).u8(*want);
                    }
                    Command::SetIglooFishers { igloo, want } => {
                        w.u8(30).u8(*igloo).u8(*want);
                    }
                }
            }
            ClientMsg::Ping(t) => {
                w.u8(8).u32(*t);
            }
            ClientMsg::BackToLobby => {
                w.u8(9);
            }
            ClientMsg::AddMap(bytes) => {
                w.u8(10).bytes(bytes);
            }
        }
        w.buf
    }

    pub fn decode(b: &[u8]) -> Result<ClientMsg> {
        let mut r = Reader::new(b);
        Ok(match r.u8()? {
            0 => ClientMsg::Hello { version: r.u16()?, name: r.str()?, token: r.u64()? },
            1 => ClientMsg::SetMap(r.u8()?),
            2 => ClientMsg::SetSlot { slot: r.u8()?, kind: SlotKind::decode(&mut r)? },
            3 => ClientMsg::SetTeam { slot: r.u8()?, team: r.u8()? },
            4 => ClientMsg::SetColor { slot: r.u8()?, color: r.u8()? },
            5 => ClientMsg::TakeSlot(r.u8()?),
            6 => ClientMsg::Start,
            7 => ClientMsg::Cmd(match r.u8()? {
                0 => {
                    let units = decode_units(&mut r)?;
                    let target = v2(r.f32()?, r.f32()?);
                    if !target.x.is_finite() || !target.y.is_finite() {
                        return Err(WireError("bad target"));
                    }
                    Command::Move { units, target, run: r.bool()?, queue: r.bool()? }
                }
                1 => Command::Enter { units: decode_units(&mut r)?, igloo: r.u8()? },
                2 => Command::Gather { units: decode_units(&mut r)?, resource: r.u16()? },
                3 => Command::Stop { units: decode_units(&mut r)? },
                5 => {
                    let to = r.u8()?;
                    Command::Chat { to: (to != NEUTRAL).then_some(to), text: r.str()? }
                }
                6 => Command::Chop { units: decode_units(&mut r)?, tile: (r.u16()? as i32, r.u16()? as i32) },
                7 => {
                    let units = decode_units(&mut r)?;
                    let kind = BuildKind::from_u8(r.u8()?);
                    let n = r.u8()?;
                    let tiles = (0..n).map(|_| Ok((r.u16()? as i32, r.u16()? as i32))).collect::<Result<Vec<_>>>()?;
                    Command::Build { units, kind, tiles }
                }
                8 => Command::Repair { units: decode_units(&mut r)?, building: r.u16()? },
                9 => Command::Demolish { building: r.u16()? },
                10 => Command::Capture { units: decode_units(&mut r)?, penguin: r.u16()? },
                11 => Command::Release { igloo: r.u8()? },
                14 => Command::Upgrade { igloo: r.u8()? },
                15 => Command::StartResearch { igloo: r.u8()?, branch: r.u8()? },
                16 => Command::WarCry { unit: r.u16()? },
                17 => Command::Thaw { units: decode_units(&mut r)?, penguin: r.u16()? },
                18 => Command::Hold { units: decode_units(&mut r)? },
                20 => {
                    let (building, at) = (r.u16()?, v2(r.f32()?, r.f32()?));
                    if !at.x.is_finite() || !at.y.is_finite() {
                        return Err(WireError("bad rally point"));
                    }
                    Command::SetRally { building, at }
                }
                21 => Command::Tend { units: decode_units(&mut r)?, building: r.u16()? },
                26 => Command::Concede,
                28 => Command::UpgradeBuilding { building: r.u16()? },
                29 => Command::SetWant { building: r.u16()?, want: r.u8()? },
                30 => Command::SetIglooFishers { igloo: r.u8()?, want: r.u8()? },
                23 => Command::Garrison { units: decode_units(&mut r)?, building: r.u16()? },
                22 => Command::Fish { units: decode_units(&mut r)?, tile: (r.u16()? as i32, r.u16()? as i32) },
                _ => return Err(WireError("bad command")),
            }),
            8 => ClientMsg::Ping(r.u32()?),
            9 => ClientMsg::BackToLobby,
            10 => {
                let n = r.u32()? as usize;
                if n > MAX_MAP_BYTES {
                    return Err(WireError("map too big"));
                }
                ClientMsg::AddMap(r.raw(n)?.to_vec())
            }
            _ => return Err(WireError("bad client message")),
        })
    }
}

impl ServerMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        match self {
            ServerMsg::Welcome { conn, token } => {
                w.u8(0).u16(PROTOCOL_VERSION).u32(*conn).u64(*token);
            }
            ServerMsg::Reject(s) => {
                w.u8(1).str(s);
            }
            ServerMsg::Lobby(l) => {
                w.u8(2).str(&l.server_name).u32(l.host).u8(l.maps.len() as u8);
                for m in &l.maps {
                    w.str(&m.name).str(&m.description).u8(m.players);
                }
                w.u8(l.map).u8(l.slots.len() as u8);
                for s in &l.slots {
                    s.kind.encode(&mut w);
                    w.str(&s.name).u8(s.color).u8(s.team).u32(s.conn);
                }
                w.u8(l.spectators.len() as u8);
                for s in &l.spectators {
                    w.str(s);
                }
                w.bool(l.in_game);
            }
            ServerMsg::GameStart(g) => {
                w.u8(3).bool(g.tech).bytes(&g.map.encode()).u8(g.you).u8(g.players.len() as u8);
                for p in &g.players {
                    w.str(&p.name).u8(p.color).u8(p.team).bool(p.ai);
                }
                w.u8(g.igloos.len() as u8);
                for i in &g.igloos {
                    w.u16(i.tile.0 as u16).u16(i.tile.1 as u16);
                    w.f32(i.center.x).f32(i.center.y).f32(i.door.x).f32(i.door.y);
                }
                w.u16(g.resources.len() as u16);
                for r in &g.resources {
                    w.u16(r.tile.0 as u16).u16(r.tile.1 as u16).u8(r.kind as u8).u8(r.max);
                }
                w.u32(g.long_night_at);
            }
            ServerMsg::Snapshot(b) => {
                w.u8(4).raw(b);
            }
            ServerMsg::Chat { from, color, text, private } => {
                w.u8(5).str(from).u8(*color).str(text).bool(*private);
            }
            ServerMsg::Pong(t) => {
                w.u8(6).u32(*t);
            }
            ServerMsg::AiNotes(notes) => {
                w.u8(7).u8(notes.len().min(16) as u8);
                for (player, lines) in notes.iter().take(16) {
                    w.u8(*player).u8(lines.len().min(AI_NOTE_LINES) as u8);
                    for line in lines.iter().take(AI_NOTE_LINES) {
                        w.str(&line.chars().take(AI_NOTE_WIDTH).collect::<String>());
                    }
                }
            }
        }
        w.buf
    }

    pub fn decode(b: &[u8]) -> Result<ServerMsg> {
        let mut r = Reader::new(b);
        Ok(match r.u8()? {
            0 => {
                let v = r.u16()?;
                if v != PROTOCOL_VERSION {
                    return Ok(ServerMsg::Reject(format!("server speaks protocol v{v}")));
                }
                ServerMsg::Welcome { conn: r.u32()?, token: r.u64()? }
            }
            1 => ServerMsg::Reject(r.str()?),
            2 => {
                let server_name = r.str()?;
                let host = r.u32()?;
                let mut maps = Vec::new();
                for _ in 0..r.u8()? {
                    maps.push(MapInfo { name: r.str()?, description: r.str()?, players: r.u8()? });
                }
                let map = r.u8()?;
                let mut slots = Vec::new();
                for _ in 0..r.u8()? {
                    slots.push(Slot {
                        kind: SlotKind::decode(&mut r)?,
                        name: r.str()?,
                        color: r.u8()?,
                        team: r.u8()?,
                        conn: r.u32()?,
                    });
                }
                let mut spectators = Vec::new();
                for _ in 0..r.u8()? {
                    spectators.push(r.str()?);
                }
                let in_game = r.bool()?;
                ServerMsg::Lobby(LobbyState { server_name, host, maps, map, slots, spectators, in_game })
            }
            3 => {
                let tech = r.bool()?;
                let map = Map::parse(&r.bytes()?)?;
                let you = r.u8()?;
                let mut players = Vec::new();
                for _ in 0..r.u8()? {
                    players.push(PlayerInfo { name: r.str()?, color: r.u8()?, team: r.u8()?, ai: r.bool()? });
                }
                let mut igloos = Vec::new();
                for _ in 0..r.u8()? {
                    igloos.push(IglooStatic {
                        tile: (r.u16()? as i32, r.u16()? as i32),
                        center: v2(r.f32()?, r.f32()?),
                        door: v2(r.f32()?, r.f32()?),
                    });
                }
                let mut resources = Vec::new();
                for _ in 0..r.u16()? {
                    let tile = (r.u16()? as i32, r.u16()? as i32);
                    resources.push(ResourceStatic {
                        tile,
                        pos: v2(tile.0 as f32 + 0.5, tile.1 as f32 + 0.5),
                        kind: ResourceKind::from_u8(r.u8()?),
                        max: r.u8()?,
                    });
                }
                ServerMsg::GameStart(Box::new(GameInfo { map, players, you, igloos, resources, tech, long_night_at: r.u32()? }))
            }
            4 => ServerMsg::Snapshot(b[1..].to_vec()),
            5 => ServerMsg::Chat { from: r.str()?, color: r.u8()?, text: r.str()?, private: r.bool()? },
            6 => ServerMsg::Pong(r.u32()?),
            7 => {
                let mut notes = Vec::new();
                for _ in 0..r.u8()? {
                    let player = r.u8()?;
                    let mut lines = Vec::new();
                    for _ in 0..r.u8()? {
                        lines.push(r.str()?);
                    }
                    notes.push((player, lines));
                }
                ServerMsg::AiNotes(notes)
            }
            _ => return Err(WireError("bad server message")),
        })
    }
}

// -------------------------------------------------------------------------
// Snapshots

const POS_SCALE: f32 = 64.0;

/// Quantized penguin state, compared to decide what to send.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
struct PenguinRec {
    x: u16,
    y: u16,
    facing: u8,
    act: u8,
    anim_seq: u8,
    kind: u8,
    owner: u8,
    home: u8,
    warmth: u8,
    max_warmth: u8,
    snowballs: u8,
    carry: u8,
    flags: u8,
    power: u8,
    courage: u8,
    /// More flags: see `F2_*`.
    flags2: u8,
    /// Chieftains: seconds until the next War Cry.
    cry_cd: u8,
    /// Warmer's target: the penguin it is warming or thawing.
    tend: u16,
}

const F_COLD: u8 = 1;
const F_INSIDE: u8 = 2;
const F_POWER: u8 = 4;
const F_PANIC: u8 = 8;
const F_CARRIED: u8 = 16;
const F_ROUTED: u8 = 32;
const F_PRISONER: u8 = 64;
/// Out in the snow, where the cold bites.
const F_EXPOSED: u8 = 128;
/// Under a War Cry.
const F2_SHOUTING: u8 = 1;
/// Standing on a watchtower's roof.
const F2_POSTED: u8 = 2;
/// A recruit on its way to its first meal: not yet in its tribe's colours.
const F2_DINING: u8 = 4;

fn quant_angle(a: f32) -> u8 {
    let t = a.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU;
    ((t * 256.0).round() as u32 & 255) as u8
}

impl PenguinRec {
    fn from(g: &Game, p: &crate::game::Penguin) -> PenguinRec {
        let mut flags = 0;
        if p.cold {
            flags |= F_COLD;
        }
        if p.inside.is_some() {
            flags |= F_INSIDE;
        }
        if p.power.is_some() {
            flags |= F_POWER;
        }
        if p.panicking(g) {
            flags |= F_PANIC;
        }
        if p.carried_by.is_some() {
            flags |= F_CARRIED;
        }
        if p.routed {
            flags |= F_ROUTED;
        }
        if p.prison_t > 0 {
            flags |= F_PRISONER;
        }
        if g.core.cold && !g.sheltered(p.id as usize) {
            flags |= F_EXPOSED;
        }
        let flags2 = if p.cry > 0 { F2_SHOUTING } else { 0 } | if p.post.is_some() { F2_POSTED } else { 0 } | if p.dining { F2_DINING } else { 0 };
        PenguinRec {
            x: (p.pos.x * POS_SCALE) as u16,
            y: (p.pos.y * POS_SCALE) as u16,
            facing: quant_angle(p.facing),
            act: p.act as u8,
            anim_seq: p.anim_seq,
            kind: p.kind as u8,
            owner: p.owner,
            home: p.home,
            warmth: p.warmth,
            max_warmth: p.max_warmth,
            snowballs: p.snowballs,
            carry: p.carry as u8,
            flags,
            power: p.power.map_or(0, |pw| g.powerups[pw as usize].kind as u8),
            courage: p.courage,
            flags2,
            cry_cd: p.cry_cd.div_ceil(crate::TICK_RATE as u16).min(255) as u8,
            tend: p.tend.unwrap_or(u16::MAX),
        }
    }
    /// A penguin the viewer cannot see: nowhere, nobody's, tucked away inside.
    fn hidden() -> PenguinRec {
        PenguinRec { owner: NEUTRAL, flags: F_INSIDE, max_warmth: 1, tend: u16::MAX, ..Default::default() }
    }
    /// Write only the fields that differ from `old` (everything if the
    /// client has never seen this penguin). A walking penguin costs a
    /// handful of bytes instead of the whole record.
    fn write_delta(&self, old: Option<&PenguinRec>, w: &mut Writer) {
        let base = old.copied().unwrap_or_default();
        let fresh = old.is_none();
        let mut mask = 0u16;
        let (dx, dy) = (self.x as i32 - base.x as i32, self.y as i32 - base.y as i32);
        let small = !fresh && (-128..128).contains(&dx) && (-128..128).contains(&dy);
        if fresh || (self.x, self.y) != (base.x, base.y) {
            mask |= if small { D_POS_NEAR } else { D_POS };
        }
        let differs = |bit: u16, changed: bool| if fresh || changed { bit } else { 0 };
        mask |= differs(D_FACING, self.facing != base.facing);
        mask |= differs(D_ACT, self.act != base.act);
        mask |= differs(D_ANIM, self.anim_seq != base.anim_seq);
        mask |= differs(
            D_WHO,
            (self.kind, self.owner, self.home, self.max_warmth) != (base.kind, base.owner, base.home, base.max_warmth),
        );
        mask |= differs(D_WARMTH, self.warmth != base.warmth);
        mask |= differs(D_SNOWBALLS, self.snowballs != base.snowballs);
        mask |= differs(D_CARRY, self.carry != base.carry);
        mask |= differs(D_FLAGS, self.flags != base.flags);
        mask |= differs(D_POWER, self.power != base.power);
        mask |= differs(D_COURAGE, self.courage != base.courage);
        mask |= differs(D_FLAGS2, self.flags2 != base.flags2);
        mask |= differs(D_CRY, self.cry_cd != base.cry_cd);
        mask |= differs(D_TEND, self.tend != base.tend);
        w.u16(mask);
        if mask & D_POS != 0 {
            w.u16(self.x).u16(self.y);
        }
        if mask & D_POS_NEAR != 0 {
            w.u8(dx as i8 as u8).u8(dy as i8 as u8);
        }
        let bytes = [
            (D_FACING, self.facing),
            (D_ACT, self.act),
            (D_ANIM, self.anim_seq),
            (D_WHO, self.kind),
            (D_WHO, self.owner),
            (D_WHO, self.home),
            (D_WHO, self.max_warmth),
            (D_WARMTH, self.warmth),
            (D_SNOWBALLS, self.snowballs),
            (D_CARRY, self.carry),
            (D_FLAGS, self.flags),
            (D_POWER, self.power),
            (D_COURAGE, self.courage),
            (D_FLAGS2, self.flags2),
            (D_CRY, self.cry_cd),
        ];
        for (bit, v) in bytes {
            if mask & bit != 0 {
                w.u8(v);
            }
        }
        if mask & D_TEND != 0 {
            w.u16(self.tend);
        }
    }

    /// The other half of `write_delta`: apply a delta to what we had.
    fn read_delta(&mut self, r: &mut Reader) -> Result<()> {
        let mask = r.u16()?;
        if mask & D_POS != 0 {
            self.x = r.u16()?;
            self.y = r.u16()?;
        }
        if mask & D_POS_NEAR != 0 {
            self.x = (self.x as i32 + r.u8()? as i8 as i32) as u16;
            self.y = (self.y as i32 + r.u8()? as i8 as i32) as u16;
        }
        let mut byte = |bit: u16, field: &mut u8| -> Result<()> {
            if mask & bit != 0 {
                *field = r.u8()?;
            }
            Ok(())
        };
        byte(D_FACING, &mut self.facing)?;
        byte(D_ACT, &mut self.act)?;
        byte(D_ANIM, &mut self.anim_seq)?;
        byte(D_WHO, &mut self.kind)?;
        byte(D_WHO, &mut self.owner)?;
        byte(D_WHO, &mut self.home)?;
        byte(D_WHO, &mut self.max_warmth)?;
        byte(D_WARMTH, &mut self.warmth)?;
        byte(D_SNOWBALLS, &mut self.snowballs)?;
        byte(D_CARRY, &mut self.carry)?;
        byte(D_FLAGS, &mut self.flags)?;
        byte(D_POWER, &mut self.power)?;
        byte(D_COURAGE, &mut self.courage)?;
        byte(D_FLAGS2, &mut self.flags2)?;
        byte(D_CRY, &mut self.cry_cd)?;
        if mask & D_TEND != 0 {
            self.tend = r.u16()?;
        }
        Ok(())
    }

    fn view(&self, id: u16, tick: u32) -> PenguinView {
        let f = self.flags;
        PenguinView {
            id,
            kind: Kind::from_u8(self.kind),
            owner: self.owner,
            home: self.home,
            pos: v2(self.x as f32 / POS_SCALE, self.y as f32 / POS_SCALE),
            facing: self.facing as f32 / 256.0 * std::f32::consts::TAU,
            act: Act::from_u8(self.act),
            anim_seq: self.anim_seq,
            warmth: self.warmth,
            max_warmth: self.max_warmth,
            snowballs: self.snowballs,
            carry: Carry::from_u8(self.carry),
            cold: f & F_COLD != 0,
            inside: f & F_INSIDE != 0,
            panic: f & F_PANIC != 0,
            carried: f & F_CARRIED != 0,
            routed: f & F_ROUTED != 0,
            prisoner: f & F_PRISONER != 0,
            exposed: f & F_EXPOSED != 0,
            courage: self.courage,
            shouting: self.flags2 & F2_SHOUTING != 0,
            posted: self.flags2 & F2_POSTED != 0,
            dining: self.flags2 & F2_DINING != 0,
            cry_cd: self.cry_cd,
            power: (f & F_POWER != 0).then(|| PowerKind::from_u8(self.power)),
            tend: (self.tend != u16::MAX).then_some(self.tend),
            act_since: tick,
        }
    }
}

// Which fields a penguin's delta carries (see `PenguinRec::write_delta`).
const D_POS: u16 = 1;
/// Position as a small step from the last one.
const D_POS_NEAR: u16 = 2;
const D_FACING: u16 = 4;
const D_ACT: u16 = 8;
const D_ANIM: u16 = 16;
/// Kind, owner, home and top warmth: things that rarely change.
const D_WHO: u16 = 32;
const D_WARMTH: u16 = 64;
const D_SNOWBALLS: u16 = 128;
const D_CARRY: u16 = 256;
const D_FLAGS: u16 = 512;
const D_POWER: u16 = 1024;
const D_COURAGE: u16 = 2048;
const D_FLAGS2: u16 = 4096;
const D_CRY: u16 = 8192;
const D_TEND: u16 = 16384;

#[derive(Clone, PartialEq, Default)]
struct IglooRec {
    owner: u8,
    fish: u16,
    crystal: u16,
    wood: u16,
    tier: u8,
    hp: u16,
    /// Upgrade under way: ticks left, then the total it started with.
    upgrade: Option<(u16, u16)>,
    /// Research under way: branch, ticks left, total.
    research: Option<(u8, u16, u16)>,
    takeover: Option<Takeover>,
    occupants: u8,
    prisoners: u8,
    fish_want: u8,
    fishers: u8,
}

#[derive(Clone, Copy, PartialEq, Default)]
struct PowerRec {
    x: u16,
    y: u16,
    holder: u16,
    kind: u8,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct BuildingView {
    pub kind: BuildKind,
    pub owner: u8,
    pub tile: (i32, i32),
    pub hp: u8,
    pub built: u8,
    pub wood_in: u8,
    pub alive: bool,
    pub starved: bool,
    /// A sauna whose fire is lit.
    pub lit: bool,
    /// Camps: where graduates walk to.
    pub rally: Option<Vec2>,
    /// Camps with armies: slots, how many are filled, and how far the
    /// furthest-along trainee is (0-255; 0 when nobody is training).
    pub slots: u8,
    pub filled: u8,
    pub training: u8,
    /// How many of the filled slots are still training.
    pub trainees: u8,
    /// Being built up for more slots: steps and logs still to do (both 0
    /// when not).
    pub up_steps: u8,
    pub up_wood: u8,
    /// How many of its slots the tribe wants filled.
    pub want: u8,
}

impl BuildingView {
    pub fn done(&self) -> bool {
        self.built >= self.kind.snow()
    }
    /// Steps the wood delivered so far pays for.
    pub fn steps_allowed(&self) -> u8 {
        let (snow, wood) = (self.kind.snow() as u16, self.kind.wood() as u16);
        if wood == 0 {
            return snow as u8;
        }
        (snow * self.wood_in.min(wood as u8) as u16 / wood) as u8
    }
    /// Still waiting for wood to be carried in.
    pub fn needs_wood(&self) -> bool {
        (!self.done() && self.wood_in < self.kind.wood() as u8) || self.up_wood > 0
    }
    /// No step can be built until more wood comes.
    pub fn waiting_for_wood(&self) -> bool {
        (!self.done() && self.built >= self.steps_allowed()) || self.up_wood > 0
    }
    /// Being built up for more slots.
    pub fn upgrading(&self) -> bool {
        self.up_steps > 0
    }
    pub fn center(&self) -> Vec2 {
        let s = self.kind.size() as f32 / 2.0;
        v2(self.tile.0 as f32 + s, self.tile.1 as f32 + s)
    }
    pub fn contains(&self, p: Vec2) -> bool {
        let s = self.kind.size() as f32;
        p.x >= self.tile.0 as f32 && p.y >= self.tile.1 as f32 && p.x < self.tile.0 as f32 + s && p.y < self.tile.1 as f32 + s
    }
}

fn building_view(g: &Game, b: &crate::game::Building) -> BuildingView {
    let camp = b.kind.trains().is_some() && b.alive;
    // A Sushi House's fishers show the same way, as filled slots.
    let store = b.kind == crate::game::BuildKind::Store && b.alive && b.done();
    // A fire's stoker, with armies.
    let fire = matches!(b.kind, crate::game::BuildKind::Sauna | crate::game::BuildKind::Campfire) && b.alive && b.done();
    // And a tower's roof, its sentry places.
    let tower = b.kind == crate::game::BuildKind::Tower && b.alive && b.done();
    // A catapult's crew.
    let catapult = b.kind == crate::game::BuildKind::Catapult && b.alive && b.done();
    BuildingView {
        kind: b.kind,
        owner: b.owner,
        tile: b.tile,
        hp: b.hp,
        built: b.built,
        wood_in: b.wood_in,
        alive: b.alive,
        starved: b.starved,
        lit: b.lit,
        rally: b.rally,
        slots: if camp { b.slots } else if store { b.slots } else if tower { crate::game::TOWER_SLOTS as u8 } else if catapult { b.slots } else if fire { 1 } else { 0 },
        filled: if camp {
            g.camp_filled(b.id).min(255) as u8
        } else if store {
            g.store_crew(b.id).min(255) as u8
        } else if tower {
            g.garrison(b.id).count().min(255) as u8
        } else if catapult {
            g.crew(b.id).count().min(255) as u8
        } else if fire {
            g.stokers(b.id).min(255) as u8
        } else {
            0
        },
        training: if camp { g.camp_training(b.id).map_or(0, |f| (f * 254.0) as u8 + 1) } else { 0 },
        trainees: if camp { g.penguins.iter().filter(|p| p.order == crate::game::Order::Train { building: b.id } && p.act != crate::game::Act::Frozen).count().min(255) as u8 } else { 0 },
        up_steps: b.upgrading.map_or(0, |u| u.steps),
        up_wood: b.upgrading.map_or(0, |u| u.wood),
        want: if camp || store || tower || catapult { b.want } else if fire { 1 } else { 0 },
    }
}

/// Per-client snapshot delta state, kept by the server.
#[derive(Default)]
pub struct SnapshotEncoder {
    buildings: Vec<Option<BuildingView>>,
    seals: Vec<Option<(u8, u8)>>,
    penguins: Vec<Option<PenguinRec>>,
    igloos: Vec<Option<IglooRec>>,
    resources: Vec<Option<u8>>,
    powerups: Vec<Option<PowerRec>>,
    out: Vec<bool>,
    research: Vec<Levels>,
    winner: Option<u8>,
    first: bool,
    /// Igloos whose place the client has been sent.
    igloo_statics: usize,
    /// Changed ground this client could not see yet, sent once it can.
    pending_tiles: std::collections::BTreeSet<u32>,
}

impl SnapshotEncoder {
    pub fn new() -> SnapshotEncoder {
        SnapshotEncoder { first: true, ..Default::default() }
    }

    /// Encode everything that changed. `events` are this tick's events,
    /// already filtered for this client (private chat etc).
    pub fn encode(&mut self, g: &Game, events: &[Event]) -> Vec<u8> {
        self.encode_for(g, events, None)
    }

    /// Like [`encode`](Self::encode), but with a field of view: whatever the
    /// viewer's team cannot see is left out. Hidden penguins go out as blank
    /// placeholders, and hidden buildings, igloos and ground keep their
    /// last seen state, so the client's map is a memory, not a window.
    pub fn encode_for(&mut self, g: &Game, events: &[Event], fov: Option<&crate::fog::Fov>) -> Vec<u8> {
        let mut w = Writer::new();
        w.u32(g.tick);
        let vis = |at: Vec2| fov.is_none_or(|f| f.sees(at));
        let mine = |owner: u8| fov.is_none_or(|f| owner != NEUTRAL && g.team(owner) == f.team);

        self.penguins.resize(g.penguins.len(), None);
        let mut changed = Vec::new();
        for p in &g.penguins {
            let rec = if mine(p.owner) || vis(p.pos) { PenguinRec::from(g, p) } else { PenguinRec::hidden() };
            let old = self.penguins[p.id as usize];
            if old != Some(rec) {
                self.penguins[p.id as usize] = Some(rec);
                changed.push((p.id, old, rec));
            }
        }
        w.u16(changed.len() as u16);
        for (id, old, rec) in changed {
            w.u16(id);
            rec.write_delta(old.as_ref(), &mut w);
        }

        // Igloos built since the game started: where they stand, sent once.
        let known = self.igloo_statics.min(g.igloos.len());
        w.u8(g.igloos.len() as u8).u8(known as u8);
        for ig in &g.igloos[known..] {
            w.u16(ig.tile.0 as u16).u16(ig.tile.1 as u16).f32(ig.center.x).f32(ig.center.y).f32(ig.door.x).f32(ig.door.y);
        }
        self.igloo_statics = g.igloos.len();
        self.igloos.resize(g.igloos.len(), None);
        let mut changed = Vec::new();
        for ig in &g.igloos {
            if !(mine(ig.owner) || vis(ig.center)) {
                // Out of sight: keep what was last seen (never seen: a wild igloo).
                if self.igloos[ig.id as usize].is_none() {
                    let rec = IglooRec { owner: NEUTRAL, hp: crate::game::igloo_max_hp(0), ..Default::default() };
                    self.igloos[ig.id as usize] = Some(rec.clone());
                    changed.push((ig.id, rec));
                }
                continue;
            }
            let rec = IglooRec {
                owner: ig.owner,
                fish: ig.fish,
                crystal: ig.crystal,
                wood: ig.wood,
                tier: ig.tier,
                hp: ig.hp,
                upgrade: ig.upgrade.map(|u| (u.ticks_left, u.total)),
                research: ig.research.map(|r| (r.branch, r.ticks_left, r.total)),
                takeover: ig.takeover,
                occupants: ig.occupants.len().min(255) as u8,
                prisoners: ig.prisoners.len().min(255) as u8,
                fish_want: ig.fish_want,
                fishers: g.fishers(crate::game::Fishery::Igloo(ig.id)).min(255) as u8,
            };
            if self.igloos[ig.id as usize].as_ref() != Some(&rec) {
                self.igloos[ig.id as usize] = Some(rec.clone());
                changed.push((ig.id, rec));
            }
        }
        w.u8(changed.len() as u8);
        for (id, r) in changed {
            w.u8(id).u8(r.owner).u16(r.fish).u16(r.crystal).u16(r.wood).u8(r.tier).u16(r.hp);
            match r.upgrade {
                Some((left, total)) => w.bool(true).u16(left).u16(total),
                None => w.bool(false),
            };
            match r.research {
                Some((branch, left, total)) => w.bool(true).u8(branch).u16(left).u16(total),
                None => w.bool(false),
            };
            match r.takeover {
                Some(t) => w.u8(t.by).u16(t.ticks_left).u16(t.total),
                None => w.u8(NEUTRAL),
            };
            w.u8(r.occupants).u8(r.prisoners).u8(r.fish_want).u8(r.fishers);
        }

        self.resources.resize(g.resources.len(), None);
        let changed: Vec<(u16, u8)> = g
            .resources
            .iter()
            .filter(|r| vis(r.pos) && self.resources[r.id as usize] != Some(r.amount))
            .map(|r| (r.id, r.amount))
            .collect();
        w.u16(changed.len() as u16);
        for (id, amount) in changed {
            self.resources[id as usize] = Some(amount);
            w.u16(id).u8(amount);
        }

        self.powerups.resize(g.powerups.len(), None);
        let mut changed = Vec::new();
        for pw in &g.powerups {
            if !vis(pw.pos) {
                // Never seen (or out of sight since): the client keeps its last picture.
                continue;
            }
            let rec = PowerRec {
                x: (pw.pos.x * POS_SCALE) as u16,
                y: (pw.pos.y * POS_SCALE) as u16,
                holder: pw.holder.unwrap_or(u16::MAX),
                kind: pw.kind as u8,
            };
            if self.powerups[pw.id as usize] != Some(rec) {
                self.powerups[pw.id as usize] = Some(rec);
                changed.push((pw.id, rec));
            }
        }
        w.u8(changed.len() as u8);
        for (id, r) in changed {
            w.u8(id).u16(r.x).u16(r.y).u16(r.holder).u8(r.kind);
        }

        self.out.resize(g.players.len(), false);
        let changed: Vec<u8> = g
            .players
            .iter()
            .filter(|p| self.out[p.id as usize] != p.out || self.first)
            .map(|p| p.id)
            .collect();
        w.u8(changed.len() as u8);
        for id in changed {
            self.out[id as usize] = g.players[id as usize].out;
            w.u8(id).bool(g.players[id as usize].out);
        }
        self.research.resize(g.players.len(), [0; BRANCHES]);
        let changed: Vec<u8> = g
            .players
            .iter()
            .filter(|p| self.research[p.id as usize] != p.research || self.first)
            .map(|p| p.id)
            .collect();
        w.u8(changed.len() as u8);
        for id in changed {
            let levels = g.players[id as usize].research;
            self.research[id as usize] = levels;
            w.u8(id);
            for l in levels {
                w.u8(l);
            }
        }
        self.winner = g.winner;
        w.u8(g.winner.unwrap_or(NEUTRAL));
        let wt = &g.weather;
        w.u8(wt.season as u8).u32(wt.season_t).u16(wt.blizzard);

        self.buildings.resize(g.buildings.len(), None);
        let mut changed = Vec::new();
        for (i, b) in g.buildings.iter().enumerate() {
            let v = if mine(b.owner) || vis(b.center()) {
                building_view(g, b)
            } else if let Some(old) = self.buildings[i] {
                old
            } else {
                // Never seen: no building there yet, as far as this client knows.
                BuildingView { alive: false, owner: NEUTRAL, ..building_view(g, b) }
            };
            if self.buildings[i] != Some(v) {
                self.buildings[i] = Some(v);
                changed.push((i as u16, v));
            }
        }
        w.u16(changed.len() as u16);
        for (id, v) in changed {
            let flags = v.alive as u8 | (v.starved as u8) << 1 | (v.rally.is_some() as u8) << 2 | (v.lit as u8) << 3;
            w.u16(id).u8(v.kind as u8).u8(v.owner).u16(v.tile.0 as u16).u16(v.tile.1 as u16);
            w.u8(v.hp).u8(v.built).u8(v.wood_in).u8(flags);
            w.u8(v.slots).u8(v.filled).u8(v.training).u8(v.trainees).u8(v.up_steps).u8(v.up_wood).u8(v.want);
            if let Some(at) = v.rally {
                w.f32(at.x).f32(at.y);
            }
        }

        // Seals: where they lurk, whether they are up, and how hurt.
        self.seals.resize(g.seals.len(), None);
        let changed: Vec<&crate::game::Seal> =
            g.seals.iter().filter(|s| vis(s.pos) && self.seals[s.id as usize] != Some((s.state as u8, s.hp))).collect();
        w.u8(changed.len() as u8);
        for s in changed {
            self.seals[s.id as usize] = Some((s.state as u8, s.hp));
            w.u8(s.id).u16((s.pos.x * POS_SCALE) as u16).u16((s.pos.y * POS_SCALE) as u16).u8(s.state as u8).u8(s.hp);
        }

        // Changed ground: everything ever changed for a new client (its map
        // is current, but not its snow depths), then just this tick's.
        let tiles = if self.first { &g.terrain.touched } else { &g.terrain.dirty };
        let mut send: Vec<u32> = Vec::new();
        if let Some(f) = fov {
            self.pending_tiles.extend(tiles.iter().copied());
            let w_ = g.map.w as u32;
            let seen: Vec<u32> = self.pending_tiles.iter().copied().filter(|&i| f.sees_tile((i % w_) as i32, (i / w_) as i32)).collect();
            for i in &seen {
                self.pending_tiles.remove(i);
            }
            send = seen;
        } else {
            send.extend(tiles.iter().copied());
        }
        w.u32(send.len() as u32);
        for i in send {
            w.u32(i).u8(g.terrain.snow[i as usize]).u8(g.map.obstacle[i as usize] as u8);
        }

        // In-flight snowballs are sent once (on first snapshot) so late
        // joiners see them; afterwards only Thrown events.
        let mut evs: Vec<Event> = Vec::new();
        if self.first {
            evs.extend(g.snowballs.iter().cloned().map(Event::Thrown));
        }
        evs.extend(events.iter().filter(|e| !matches!(e, Event::Chat { .. })).cloned());
        if fov.is_some() {
            evs.retain(|e| match event_spots(g, e) {
                None => true,
                Some(spots) => spots.into_iter().any(vis),
            });
        }
        w.u16(evs.len() as u16);
        for e in &evs {
            encode_event(&mut w, e);
        }
        self.first = false;
        w.buf
    }
}

/// Where an event happens, so a viewer who cannot see the spot is not told.
/// None for events that belong to everybody (seasons, chat, victory...).
fn event_spots(g: &Game, e: &Event) -> Option<Vec<Vec2>> {
    let penguin = |id: &u16| g.penguins.get(*id as usize).map(|p| p.pos);
    let igloo = |id: &u8| g.igloos.get(*id as usize).map(|i| i.center);
    let building = |id: &u16| g.buildings.get(*id as usize).map(|b| b.center());
    let one = |p: Option<Vec2>| Some(p.into_iter().collect());
    match e {
        Event::Thrown(s) => Some(vec![s.from, s.to]),
        Event::Landed { pos, .. } => Some(vec![*pos]),
        Event::WarCry { pos, .. } => Some(vec![*pos]),
        Event::TreeFelled { tile } => Some(vec![Vec2 { x: tile.0 as f32 + 0.5, y: tile.1 as f32 + 0.5 }]),
        Event::Frozen { penguin: p }
        | Event::Thawed { penguin: p, .. }
        | Event::Persuaded { penguin: p, .. }
        | Event::Routed { penguin: p }
        | Event::PickedUp { penguin: p, .. }
        | Event::Dropped { penguin: p }
        | Event::Imprisoned { penguin: p, .. }
        | Event::Converted { penguin: p, .. }
        | Event::Released { penguin: p }
        | Event::PowerupTaken { penguin: p, .. }
        | Event::SealSnatched { penguin: p, .. }
        | Event::NoFire { penguin: p }
        | Event::SwamIn { penguin: p } => one(penguin(p)),
        Event::TakeoverStart { igloo: i, .. }
        | Event::Collapsed { igloo: i, .. }
        | Event::TakeoverFailed { igloo: i }
        | Event::Captured { igloo: i, .. }
        | Event::Upgraded { igloo: i, .. } => one(igloo(i)),
        Event::Trained { building: b, .. } | Event::Placed { building: b } | Event::Built { building: b } | Event::BuildingLost { building: b } | Event::TakenOver { building: b, .. } => one(building(b)),
        Event::SealSurfaced { seal } | Event::SealDriven { seal, .. } => one(g.seals.get(*seal as usize).map(|s| s.pos)),
        _ => None,
    }
}

fn encode_event(w: &mut Writer, e: &Event) {
    match e {
        Event::Thrown(s) => {
            w.u8(0).u32(s.id);
            w.f32(s.from.x).f32(s.from.y).f32(s.from_z);
            w.f32(s.to.x).f32(s.to.y).f32(s.to_z);
            w.u32(s.start_tick).u16(s.flight).u16(s.thrower).u8(s.team).u8(s.kind as u8).bool(s.big);
        }
        Event::Landed { id, pos, hit } => {
            w.u8(1).u32(*id).f32(pos.x).f32(pos.y).u8(*hit as u8);
        }
        Event::Frozen { penguin } => {
            w.u8(2).u16(*penguin);
        }
        Event::TakeoverStart { igloo, by } => {
            w.u8(3).u8(*igloo).u8(*by);
        }
        Event::TakeoverFailed { igloo } => {
            w.u8(4).u8(*igloo);
        }
        Event::Captured { igloo, from, to } => {
            w.u8(5).u8(*igloo).u8(*from).u8(*to);
        }
        Event::PowerupTaken { powerup, penguin } => {
            w.u8(7).u8(*powerup).u16(*penguin);
        }
        Event::PlayerOut { player } => {
            w.u8(8).u8(*player);
        }
        Event::Victory { team } => {
            w.u8(9).u8(*team);
        }
        Event::TreeFelled { tile } => {
            w.u8(10).u16(tile.0 as u16).u16(tile.1 as u16);
        }
        Event::Placed { building } => {
            w.u8(11).u16(*building);
        }
        Event::Built { building } => {
            w.u8(12).u16(*building);
        }
        Event::BuildingLost { building } => {
            w.u8(13).u16(*building);
        }
        Event::Routed { penguin } => {
            w.u8(14).u16(*penguin);
        }
        Event::PickedUp { penguin, by } => {
            w.u8(15).u16(*penguin).u16(*by);
        }
        Event::Dropped { penguin } => {
            w.u8(16).u16(*penguin);
        }
        Event::Imprisoned { penguin, igloo } => {
            w.u8(17).u16(*penguin).u8(*igloo);
        }
        Event::Converted { penguin, to } => {
            w.u8(18).u16(*penguin).u8(*to);
        }
        Event::Released { penguin } => {
            w.u8(19).u16(*penguin);
        }
        Event::SeasonChanged { season } => {
            w.u8(20).u8(*season as u8);
        }
        Event::Blizzard { on } => {
            w.u8(21).bool(*on);
        }
        Event::Upgraded { igloo, tier } => {
            w.u8(27).u8(*igloo).u8(*tier);
        }
        Event::ResearchDone { player, branch, level } => {
            w.u8(28).u8(*player).u8(*branch).u8(*level);
        }
        Event::WarCry { penguin, pos } => {
            w.u8(29).u16(*penguin).f32(pos.x).f32(pos.y);
        }
        Event::SealSurfaced { seal } => {
            w.u8(30).u8(*seal);
        }
        Event::SealSnatched { seal, penguin } => {
            w.u8(31).u8(*seal).u16(*penguin);
        }
        Event::SealDriven { seal, by } => {
            w.u8(32).u8(*seal).u8(*by);
        }
        Event::Thawed { penguin, by } => {
            w.u8(33).u16(*penguin).u8(*by);
        }
        Event::Persuaded { penguin, by } => {
            w.u8(34).u16(*penguin).u8(*by);
        }
        Event::Collapsed { igloo, owner } => {
            w.u8(35).u8(*igloo).u8(*owner);
        }
        Event::Trained { building, penguin } => {
            w.u8(36).u16(*building).u16(*penguin);
        }
        Event::KnockedDown { penguin, by } => {
            w.u8(38).u16(*penguin).u16(*by);
        }
        Event::NoFire { penguin } => {
            w.u8(40).u16(*penguin);
        }
        Event::SwamIn { penguin } => {
            w.u8(41).u16(*penguin);
        }
        Event::TakenOver { building, from, by } => {
            w.u8(42).u16(*building).u8(*from).u8(*by);
        }
        Event::Bounced { skier, shield } => {
            w.u8(39).u16(*skier).u16(*shield);
        }
        Event::Chat { .. } => {}
    }
}

fn decode_event(r: &mut Reader) -> Result<Event> {
    Ok(match r.u8()? {
        0 => Event::Thrown(Snowball {
            id: r.u32()?,
            from: v2(r.f32()?, r.f32()?),
            from_z: r.f32()?,
            to: v2(r.f32()?, r.f32()?),
            to_z: r.f32()?,
            start_tick: r.u32()?,
            flight: r.u16()?,
            thrower: r.u16()?,
            team: r.u8()?,
            kind: Kind::from_u8(r.u8()?),
            big: r.bool()?,
        }),
        1 => Event::Landed { id: r.u32()?, pos: v2(r.f32()?, r.f32()?), hit: HitKind::from_u8(r.u8()?) },
        2 => Event::Frozen { penguin: r.u16()? },
        3 => Event::TakeoverStart { igloo: r.u8()?, by: r.u8()? },
        4 => Event::TakeoverFailed { igloo: r.u8()? },
        5 => Event::Captured { igloo: r.u8()?, from: r.u8()?, to: r.u8()? },
        7 => Event::PowerupTaken { powerup: r.u8()?, penguin: r.u16()? },
        8 => Event::PlayerOut { player: r.u8()? },
        9 => Event::Victory { team: r.u8()? },
        10 => Event::TreeFelled { tile: (r.u16()? as i32, r.u16()? as i32) },
        11 => Event::Placed { building: r.u16()? },
        12 => Event::Built { building: r.u16()? },
        13 => Event::BuildingLost { building: r.u16()? },
        14 => Event::Routed { penguin: r.u16()? },
        15 => Event::PickedUp { penguin: r.u16()?, by: r.u16()? },
        16 => Event::Dropped { penguin: r.u16()? },
        17 => Event::Imprisoned { penguin: r.u16()?, igloo: r.u8()? },
        18 => Event::Converted { penguin: r.u16()?, to: r.u8()? },
        19 => Event::Released { penguin: r.u16()? },
        20 => Event::SeasonChanged { season: Season::from_u8(r.u8()?) },
        21 => Event::Blizzard { on: r.bool()? },
        27 => Event::Upgraded { igloo: r.u8()?, tier: r.u8()? },
        28 => Event::ResearchDone { player: r.u8()?, branch: r.u8()?, level: r.u8()? },
        29 => Event::WarCry { penguin: r.u16()?, pos: v2(r.f32()?, r.f32()?) },
        30 => Event::SealSurfaced { seal: r.u8()? },
        31 => Event::SealSnatched { seal: r.u8()?, penguin: r.u16()? },
        32 => Event::SealDriven { seal: r.u8()?, by: r.u8()? },
        33 => Event::Thawed { penguin: r.u16()?, by: r.u8()? },
        34 => Event::Persuaded { penguin: r.u16()?, by: r.u8()? },
        35 => Event::Collapsed { igloo: r.u8()?, owner: r.u8()? },
        36 => Event::Trained { building: r.u16()?, penguin: r.u16()? },
        38 => Event::KnockedDown { penguin: r.u16()?, by: r.u16()? },
        39 => Event::Bounced { skier: r.u16()?, shield: r.u16()? },
        40 => Event::NoFire { penguin: r.u16()? },
        41 => Event::SwamIn { penguin: r.u16()? },
        42 => Event::TakenOver { building: r.u16()?, from: r.u8()?, by: r.u8()? },
        _ => return Err(WireError("bad event")),
    })
}

// -------------------------------------------------------------------------
// Client-side world

#[derive(Clone, Debug)]
pub struct PenguinView {
    pub id: u16,
    pub kind: Kind,
    pub owner: u8,
    pub home: u8,
    pub pos: Vec2,
    pub facing: f32,
    pub act: Act,
    pub anim_seq: u8,
    pub warmth: u8,
    pub max_warmth: u8,
    pub snowballs: u8,
    pub carry: Carry,
    pub cold: bool,
    pub inside: bool,
    pub panic: bool,
    pub carried: bool,
    pub routed: bool,
    pub prisoner: bool,
    /// Out in the snow where the cold bites.
    pub exposed: bool,
    pub courage: u8,
    /// Under a War Cry: runs and throws faster.
    pub shouting: bool,
    pub posted: bool,
    /// On its way to its first meal: it wears no colours yet.
    pub dining: bool,
    /// Chieftains: seconds until the next War Cry.
    pub cry_cd: u8,
    pub power: Option<PowerKind>,
    /// Warmer: the penguin it is warming or thawing.
    pub tend: Option<u16>,
    /// Client-only: tick when the current act/anim started (for animation).
    pub act_since: u32,
}

#[derive(Clone, Debug, Default)]
pub struct IglooView {
    pub owner: u8,
    pub fish: u16,
    pub crystal: u16,
    pub wood: u16,
    /// Igloo, Snow Hall or Ice Palace.
    pub tier: u8,
    /// How much more pelting it can take.
    pub hp: u16,
    /// Ticks left on the upgrade under way, and how long it takes in all.
    pub upgrade: Option<(u16, u16)>,
    /// Research under way: branch, ticks left, and how long it takes in all.
    pub research: Option<(u8, u16, u16)>,
    pub takeover: Option<Takeover>,
    pub occupants: u8,
    pub prisoners: u8,
    /// How many of its `IGLOO_FISH_SLOTS` fishing slots the tribe wants
    /// filled (with armies), and how many are.
    pub fish_want: u8,
    pub fishers: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SealView {
    pub pos: Vec2,
    pub state: SealState,
    pub hp: u8,
    /// Client-only: tick of the last state change (for animation).
    pub since: u32,
}

#[derive(Clone, Debug)]
pub struct PowerupView {
    pub kind: PowerKind,
    pub pos: Vec2,
    pub holder: Option<u16>,
}

/// What a client knows about the game, rebuilt from snapshots.
pub struct WorldView {
    pub info: GameInfo,
    pub tick: u32,
    pub penguins: Vec<PenguinView>,
    /// What the server last said about each penguin: the base for the next delta.
    recs: Vec<PenguinRec>,
    pub igloos: Vec<IglooView>,
    pub resources: Vec<u8>,
    pub powerups: Vec<PowerupView>,
    pub buildings: Vec<BuildingView>,
    /// Leopard seals: where they lurk and what they are doing.
    pub seals: Vec<SealView>,
    pub snowballs: Vec<Snowball>,
    pub out: Vec<bool>,
    /// What each player's tribe has researched.
    pub research: Vec<Levels>,
    pub winner: Option<u8>,
    pub weather: Weather,
    /// Layers of snow per tile (see [`crate::game::SNOW_DEPTH`]).
    pub snow: Vec<u8>,
    /// Bumped whenever the map's ground, obstacles or snow change.
    pub terrain_version: u32,
    /// Recent penguin positions for interpolation: (tick, [(pos, facing)]).
    pub history: std::collections::VecDeque<(u32, Vec<(Vec2, f32)>)>,
}

impl WorldView {
    pub fn new(info: GameInfo) -> WorldView {
        let n_ig = info.igloos.len();
        let n_res = info.resources.len();
        let n_pl = info.players.len();
        WorldView {
            tick: 0,
            penguins: Vec::new(),
            recs: Vec::new(),
            igloos: vec![IglooView { owner: NEUTRAL, ..Default::default() }; n_ig],
            resources: info.resources.iter().map(|r| r.max).collect::<Vec<_>>().into_iter().take(n_res).collect(),
            powerups: Vec::new(),
            buildings: Vec::new(),
            seals: Vec::new(),
            snowballs: Vec::new(),
            out: vec![false; n_pl],
            research: vec![[0; BRANCHES]; n_pl],
            winner: None,
            weather: Weather::default(),
            snow: info.map.ground.iter().map(|&g| crate::game::initial_snow(g)).collect(),
            terrain_version: 1,
            history: Default::default(),
            info,
        }
    }

    /// Fish across all of a player's igloos.
    pub fn fish_of(&self, player: u8) -> u32 {
        self.igloos.iter().filter(|ig| ig.owner == player).map(|ig| ig.fish as u32).sum()
    }

    /// Wood across all of a player's igloos (the tribe pools it).
    pub fn wood_of(&self, player: u8) -> u32 {
        self.igloos.iter().filter(|ig| ig.owner == player).map(|ig| ig.wood as u32).sum()
    }

    /// Crystal across all of a player's igloos (the tribe pools it).
    pub fn crystal_of(&self, player: u8) -> u32 {
        self.igloos.iter().filter(|ig| ig.owner == player).map(|ig| ig.crystal as u32).sum()
    }

    pub fn team(&self, player: u8) -> u8 {
        self.info.players.get(player as usize).map_or(NEUTRAL, |p| p.team)
    }

    pub fn hostile(&self, a: u8, b: u8) -> bool {
        a != NEUTRAL && b != NEUTRAL && self.team(a) != self.team(b)
    }

    /// Apply a snapshot; returns the events it carried.
    pub fn apply(&mut self, b: &[u8]) -> Result<Vec<Event>> {
        let mut r = Reader::new(b);
        let tick = r.u32()?;
        self.tick = tick;

        for _ in 0..r.u16()? {
            let id = r.u16()?;
            let idx = id as usize;
            if idx >= self.recs.len() {
                self.recs.resize(idx + 1, PenguinRec::hidden());
            }
            self.recs[idx].read_delta(&mut r)?;
            let view = self.recs[idx].view(id, tick);
            if idx >= self.penguins.len() {
                let mut v = view.clone();
                while self.penguins.len() <= idx {
                    v.id = self.penguins.len() as u16;
                    self.penguins.push(v.clone());
                }
                self.penguins[idx] = view;
            } else {
                let old = &self.penguins[idx];
                let mut view = view;
                if old.act == view.act && old.anim_seq == view.anim_seq {
                    view.act_since = old.act_since;
                }
                self.penguins[idx] = view;
            }
        }

        // New igloos, built since the start.
        let (total, from) = (r.u8()? as usize, r.u8()? as usize);
        for k in from..total {
            let tile = (r.u16()? as i32, r.u16()? as i32);
            let at = IglooStatic { tile, center: v2(r.f32()?, r.f32()?), door: v2(r.f32()?, r.f32()?) };
            if k >= self.info.igloos.len() {
                self.info.igloos.push(at);
                self.info.map.igloos.push((tile.0 as u16, tile.1 as u16));
                self.igloos.push(IglooView { owner: NEUTRAL, ..Default::default() });
            }
        }

        for _ in 0..r.u8()? {
            let id = r.u8()? as usize;
            let owner = r.u8()?;
            let fish = r.u16()?;
            let crystal = r.u16()?;
            let wood = r.u16()?;
            let tier = r.u8()?;
            let hp = r.u16()?;
            let upgrade = if r.bool()? {
                let left = r.u16()?;
                let total = r.u16()?;
                Some((left, total))
            } else {
                None
            };
            let research = if r.bool()? {
                let branch = r.u8()?;
                let left = r.u16()?;
                let total = r.u16()?;
                Some((branch, left, total))
            } else {
                None
            };
            let by = r.u8()?;
            let takeover = if by == NEUTRAL {
                None
            } else {
                Some(Takeover { by, ticks_left: r.u16()?, total: r.u16()? })
            };
            let occupants = r.u8()?;
            let prisoners = r.u8()?;
            let fish_want = r.u8()?;
            let fishers = r.u8()?;
            if let Some(ig) = self.igloos.get_mut(id) {
                *ig = IglooView {
                    owner,
                    fish,
                    crystal,
                    wood,
                    tier,
                    hp,
                    upgrade,
                    research,
                    takeover,
                    occupants,
                    prisoners,
                    fish_want,
                    fishers,
                };
            }
        }

        for _ in 0..r.u16()? {
            let id = r.u16()? as usize;
            let amount = r.u8()?;
            if let Some(a) = self.resources.get_mut(id) {
                *a = amount;
            }
        }

        for _ in 0..r.u8()? {
            let id = r.u8()? as usize;
            let pos = v2(r.u16()? as f32 / POS_SCALE, r.u16()? as f32 / POS_SCALE);
            let holder = r.u16()?;
            let kind = PowerKind::from_u8(r.u8()?);
            while self.powerups.len() <= id {
                // A filler for ids not sent (never seen): held by nobody real, so never drawn.
                self.powerups.push(PowerupView { kind, pos, holder: Some(u16::MAX) });
            }
            self.powerups[id] = PowerupView { kind, pos, holder: (holder != u16::MAX).then_some(holder) };
        }

        for _ in 0..r.u8()? {
            let id = r.u8()? as usize;
            let out = r.bool()?;
            if let Some(o) = self.out.get_mut(id) {
                *o = out;
            }
        }
        for _ in 0..r.u8()? {
            let id = r.u8()? as usize;
            let mut levels = [0; BRANCHES];
            for l in levels.iter_mut() {
                *l = r.u8()?;
            }
            if let Some(o) = self.research.get_mut(id) {
                *o = levels;
            }
        }
        let w = r.u8()?;
        self.winner = (w != NEUTRAL).then_some(w);
        self.weather = Weather { season: Season::from_u8(r.u8()?), season_t: r.u32()?, blizzard: r.u16()?, calm: false };

        for _ in 0..r.u16()? {
            let id = r.u16()? as usize;
            let kind = BuildKind::from_u8(r.u8()?);
            let owner = r.u8()?;
            let tile = (r.u16()? as i32, r.u16()? as i32);
            let (hp, built, wood_in, flags) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
            let (slots, filled, training, trainees) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
            let (up_steps, up_wood, want) = (r.u8()?, r.u8()?, r.u8()?);
            let rally = if flags & 4 != 0 { Some(v2(r.f32()?, r.f32()?)) } else { None };
            let v = BuildingView { kind, owner, tile, hp, built, wood_in, alive: flags & 1 != 0, lit: flags & 8 != 0, starved: flags & 2 != 0, rally, slots, filled, training, trainees, up_steps, up_wood, want };
            if id >= crate::game::MAX_BUILDINGS {
                return Err(WireError("bad building id"));
            }
            while self.buildings.len() <= id {
                self.buildings.push(BuildingView { alive: false, ..v });
            }
            self.buildings[id] = v;
        }

        for _ in 0..r.u8()? {
            let id = r.u8()? as usize;
            let pos = v2(r.u16()? as f32 / POS_SCALE, r.u16()? as f32 / POS_SCALE);
            let (state, hp) = (SealState::from_u8(r.u8()?), r.u8()?);
            while self.seals.len() <= id {
                self.seals.push(SealView { pos, state: SealState::Lurking, hp, since: tick });
            }
            let old = &self.seals[id];
            let since = if old.state == state { old.since } else { tick };
            self.seals[id] = SealView { pos, state, hp, since };
        }

        let n = r.u32()?;
        for _ in 0..n {
            let i = r.u32()? as usize;
            let snow = r.u8()?;
            let obstacle = Obstacle::from_u8(r.u8()?);
            let map = &mut self.info.map;
            if i < map.ground.len() {
                self.snow[i] = snow;
                if map.ground[i] != Ground::Water {
                    map.ground[i] = if snow == 0 { Ground::Ice } else { Ground::Snow };
                }
                map.obstacle[i] = obstacle;
            }
        }
        if n > 0 {
            self.terrain_version += 1;
        }

        let mut events = Vec::new();
        for _ in 0..r.u16()? {
            let e = decode_event(&mut r)?;
            match &e {
                Event::Thrown(s) => self.snowballs.push(s.clone()),
                Event::Landed { id, .. } => self.snowballs.retain(|s| s.id != *id),
                _ => {}
            }
            // The client indexes its tables with what events carry, so one
            // that points outside them is dropped here, once.
            if self.event_in_range(&e) {
                events.push(e);
            }
        }

        self.history.push_back((tick, self.penguins.iter().map(|p| (p.pos, p.facing)).collect()));
        while self.history.len() > 24 {
            self.history.pop_front();
        }
        Ok(events)
    }

    /// Do all the ids in this event name things the view has?
    fn event_in_range(&self, e: &Event) -> bool {
        let igloo = |i: &u8| (*i as usize) < self.igloos.len();
        let penguin = |p: &u16| (*p as usize) < self.penguins.len();
        let building = |b: &u16| (*b as usize) < self.buildings.len();
        match e {
            Event::TakeoverStart { igloo: i, .. }
            | Event::Collapsed { igloo: i, .. }
            | Event::TakeoverFailed { igloo: i }
            | Event::Captured { igloo: i, .. }
            | Event::Upgraded { igloo: i, .. } => igloo(i),
            Event::Imprisoned { penguin: p, igloo: i } => igloo(i) && penguin(p),
            Event::Frozen { penguin: p }
            | Event::Thawed { penguin: p, .. }
            | Event::Persuaded { penguin: p, .. }
            | Event::Routed { penguin: p }
            | Event::SealSnatched { penguin: p, .. }
            | Event::WarCry { penguin: p, .. } => penguin(p),
            Event::Trained { building: b, penguin: p } => building(b) && penguin(p),
            Event::Placed { building: b } | Event::Built { building: b } | Event::BuildingLost { building: b } | Event::TakenOver { building: b, .. } => building(b),
            _ => true,
        }
    }

    /// Interpolated (position, facing) of a penguin at fractional tick `t`.
    pub fn penguin_at(&self, id: u16, t: f32) -> (Vec2, f32) {
        let p = &self.penguins[id as usize];
        let idx = id as usize;
        let mut prev: Option<(u32, Vec2, f32)> = None;
        for (tick, ps) in self.history.iter() {
            let Some(&(pos, facing)) = ps.get(idx) else { continue };
            if (*tick as f32) >= t {
                if let Some((pt, ppos, pf)) = prev {
                    let span = (*tick - pt) as f32;
                    let k = ((t - pt as f32) / span).clamp(0.0, 1.0);
                    if ppos.dist(pos) > 3.0 {
                        return (pos, facing); // teleport (igloo exit)
                    }
                    let d = crate::math::angle_diff(pf, facing);
                    return (ppos.lerp(pos, k), pf + d * k);
                }
                return (pos, facing);
            }
            prev = Some((*tick, pos, facing));
        }
        (p.pos, p.facing)
    }

    /// Position of a snowball at fractional tick `t`: (xy, height).
    pub fn snowball_at(s: &Snowball, t: f32) -> Option<(Vec2, f32)> {
        let k = (t - s.start_tick as f32) / s.flight as f32;
        if !(0.0..=1.0).contains(&k) {
            return None;
        }
        let xy = s.from.lerp(s.to, k);
        let dist = s.from.dist(s.to);
        let arc = dist * 0.22 + 0.4;
        let z = s.from_z + (s.to_z - s.from_z) * k + arc * 4.0 * k * (1.0 - k);
        Some((xy, z))
    }
}

/// Encode a LAN discovery beacon.
pub fn discovery_beacon(port: u16, name: &str, map: &str, players: u8, max: u8, in_game: bool) -> Vec<u8> {
    let mut w = Writer::new();
    w.raw(DISCOVERY_MAGIC).u16(port).str(name).str(map).u8(players).u8(max).bool(in_game);
    w.buf
}

#[derive(Clone, Debug)]
pub struct Beacon {
    pub port: u16,
    pub name: String,
    pub map: String,
    pub players: u8,
    pub max: u8,
    pub in_game: bool,
}

pub fn parse_beacon(b: &[u8]) -> Option<Beacon> {
    let mut r = Reader::new(b);
    if r.raw(DISCOVERY_MAGIC.len()).ok()? != DISCOVERY_MAGIC {
        return None;
    }
    Some(Beacon {
        port: r.u16().ok()?,
        name: r.str().ok()?,
        map: r.str().ok()?,
        players: r.u8().ok()?,
        max: r.u8().ok()?,
        in_game: r.bool().ok()?,
    })
}
