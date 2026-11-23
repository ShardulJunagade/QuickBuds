//! The Soundcore driver: sans-IO, the same shape as the OPPO driver (docs/CORE-API.md). The model layouts come
//! from OpenSCQ30 (`lib/src/devices/soundcore/a3936`, `a3957`). Copyright 2026 Kyle Scheuing, GPL-3.0-or-later.
//!
//! Connect: the driver asks for the full state (`01 01`). The serial number in the answer names the model
//! (it starts with the model number), and the model's layout reads the rest. Pushes update the state after that.

use super::msg::{self, ambient, Msg};
use super::packet::{encode, Frame, Packet, Reader};
use crate::model::{Common, Level};

/// Where a model keeps its values in the state packet (`01 01`). Offsets count from the first body byte.
pub struct Layout {
    /// The model number, as the serial number starts.
    pub id: &'static str,
    pub name: &'static str,
    /// The bud battery: (max level, offset). The level shown is `raw + offset` out of `max`.
    pub battery: (u8, u8),
    /// The case battery: (offset in the packet, max level, level offset).
    pub case: (usize, u8, u8),
    /// The sound modes: (offset, length).
    pub sound_modes: (usize, usize),
    pub ldac: usize,
    pub gaming: usize,
    /// The shortest state packet this layout reads.
    pub min_len: usize,
}

/// The models this driver knows. Tests: OpenSCQ30's recorded packets (`tools/soundcore-device-faker`).
pub const LAYOUTS: &[Layout] = &[
    // Space A40: tws 0, battery 2, firmware 6, serial 16, EQ 32, age 54, HearID 55, unknown 103, 6 buttons 104,
    // cycle 116, sound modes 117 (6), touch tone 123, case 124, color 125, LDAC 126, dual 127, auto off 128, gaming 130.
    Layout { id: "3936", name: "Soundcore Space A40", battery: (5, 0), case: (124, 10, 0), sound_modes: (117, 6), ldac: 126, gaming: 130, min_len: 131 },
    // Liberty 5: tws 0, battery 2, firmware 6, serial 16, "00.00" 32, case 37, EQ 38, age 60, HearID 61, unknown 109,
    // 8 buttons 110, cycle 118, sound modes 119 (7), unknown 126, wearing tone 127, low battery 128, LDAC 129,
    // dual 130, auto off 131, volume limit 133, immersive 136, leak 143, wear 144, touch tone 145, gaming 146.
    Layout { id: "3957", name: "Soundcore Liberty 5", battery: (10, 1), case: (37, 10, 1), sound_modes: (119, 7), ldac: 129, gaming: 146, min_len: 148 },
];

/// The noise control keys this driver gives (`Common::noise`).
pub const NOISE_OFF: &str = "Off";
pub const NOISE_TRANSPARENCY: &str = "Transparency";
pub const NOISE_ANC: &str = "ANC";

pub enum Cmd {
    /// A noise control key.
    Anc(String),
}

pub enum Effect {
    Send(Vec<u8>),
    Log { dir: &'static str, bytes: Vec<u8>, human: Option<String> },
    State,
    Ready,
}

/// Re-ask for the state when no answer came this long after the connect.
const STATE_RETRY_MS: u64 = 1500;

#[derive(Default)]
pub struct Driver {
    reader: Reader,
    common: Common,
    layout: Option<&'static Layout>,
    /// The raw sound mode bytes of the model, for a write that changes one field only.
    sound_modes: Option<Vec<u8>>,
    pub firmware: Vec<String>,
    pub serial: Option<String>,
    pub ldac: Option<bool>,
    ready: bool,
    retry_at: Option<u64>,
    out: Vec<Effect>,
}

fn level(raw: u8, (max, offset): (u8, u8)) -> Level {
    Level { value: raw.saturating_add(offset).min(max), max, charging: false }
}

impl Driver {
    pub fn new() -> Self { Driver { reader: Reader::new(true), ..Default::default() } }

    pub fn common(&self) -> Common { self.common.clone() }
    pub fn layout(&self) -> Option<&'static Layout> { self.layout }

    pub fn next_wake(&self) -> Option<u64> { self.retry_at }

    pub fn connected(&mut self, now: u64, name: String) -> Vec<Effect> {
        *self = Driver { common: Common { connected: true, name, ..Default::default() }, ..Driver::new() };
        self.send(msg::REQUEST_STATE, &[]);
        self.retry_at = Some(now + STATE_RETRY_MS);
        self.out.push(Effect::State);
        std::mem::take(&mut self.out)
    }

    pub fn disconnected(&mut self, _now: u64) -> Vec<Effect> {
        *self = Driver::new();
        vec![Effect::State]
    }

    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        if self.retry_at.is_some_and(|t| now >= t) {
            self.retry_at = None;
            self.send(msg::REQUEST_STATE, &[]);
        }
        std::mem::take(&mut self.out)
    }

    pub fn command(&mut self, _now: u64, c: Cmd) -> Vec<Effect> {
        match c {
            Cmd::Anc(key) => self.set_noise(&key),
        }
        std::mem::take(&mut self.out)
    }

    pub fn on_bytes(&mut self, _now: u64, data: &[u8]) -> Vec<Effect> {
        let mut changed = false;
        for f in self.reader.push(data) {
            match f {
                Frame::Bad(b) => self.out.push(Effect::Log { dir: "DISCARDED RX", bytes: b, human: None }),
                Frame::Packet(p) => {
                    self.out.push(Effect::Log { dir: "RX", bytes: p.body.clone(), human: Some(format!("{:02X} {:02X}", p.cmd[0], p.cmd[1])) });
                    changed |= self.packet(p);
                }
            }
        }
        if changed { self.out.push(Effect::State); }
        std::mem::take(&mut self.out)
    }

    fn send(&mut self, cmd: [u8; 2], body: &[u8]) {
        self.out.push(Effect::Send(encode(cmd, body, true)));
    }

    fn note(&mut self, text: String) {
        self.out.push(Effect::Log { dir: "NOTE", bytes: Vec::new(), human: Some(text) });
    }

    /// One packet. True when the state changed.
    fn packet(&mut self, p: Packet) -> bool {
        if p.cmd == msg::REQUEST_STATE {
            return self.state(&p.body);
        }
        // A model with a longer sound mode group pushes all of it.
        if let (msg::SOUND_MODES, Some(raw)) = (p.cmd, self.sound_modes.as_mut()) {
            if p.body.len() == raw.len() && p.body.len() != 4 {
                raw.copy_from_slice(&p.body);
                self.common.noise = Some(noise_key(p.body[0]).into());
                return true;
            }
        }
        match msg::decode(p) {
            Msg::Battery(v) => {
                let Some(l) = self.layout else { return false };
                for (i, raw) in v.into_iter().take(2).enumerate() {
                    let charging = self.common.battery[i].is_some_and(|b| b.charging);
                    self.common.battery[i] = Some(Level { charging, ..level(raw, l.battery) });
                }
            }
            Msg::Charging(v) => for (i, c) in v.into_iter().take(2).enumerate() {
                if let Some(b) = self.common.battery[i].as_mut() { b.charging = c; }
            },
            Msg::SoundModes(m) => {
                let Some(raw) = self.sound_modes.as_mut() else { return false };
                raw[..4].copy_from_slice(&m.bytes());
                self.common.noise = Some(noise_key(m.ambient).into());
            }
            Msg::Ldac(on) => self.ldac = Some(on),
            Msg::Firmware { versions, serial } => { self.firmware = versions; self.serial = Some(serial); }
            Msg::Tws { .. } => return false,
            Msg::Other(p) => {
                // An empty packet with a sent command is its acknowledgement.
                if !p.body.is_empty() { self.note(format!("unknown packet {:02X} {:02X}", p.cmd[0], p.cmd[1])); }
                return false;
            }
        }
        true
    }

    /// The full state packet: the serial number picks the layout.
    fn state(&mut self, b: &[u8]) -> bool {
        self.retry_at = None;
        let serial = b.get(16..32).and_then(|s| std::str::from_utf8(s).ok());
        let Some(l) = serial.and_then(|s| LAYOUTS.iter().find(|l| s.starts_with(l.id))) else {
            self.note(format!("state packet of an unknown model, serial {serial:?}, {} bytes", b.len()));
            return false;
        };
        if b.len() < l.min_len {
            self.note(format!("state packet too short for A{}: {} < {}", l.id, b.len(), l.min_len));
            return false;
        }
        self.layout = Some(l);
        self.serial = serial.map(str::to_string);
        self.firmware = [&b[6..11], &b[11..16]].iter().filter_map(|v| msg::firmware(v)).collect();
        self.common.model_id = Some(format!("A{}", l.id));
        for i in 0..2 {
            self.common.battery[i] = Some(Level { charging: b[4 + i] == 1, ..level(b[2 + i], l.battery) });
        }
        let (at, max, off) = l.case;
        self.common.battery[2] = Some(level(b[at], (max, off)));
        let (at, n) = l.sound_modes;
        self.sound_modes = Some(b[at..at + n].to_vec());
        self.common.noise = Some(noise_key(b[at]).into());
        self.common.noise_modes = vec![NOISE_OFF.into(), NOISE_TRANSPARENCY.into(), NOISE_ANC.into()];
        self.ldac = Some(b[l.ldac] != 0);
        self.common.low_latency = Some(b[l.gaming] != 0);
        self.common.features = ["anc"].into_iter().collect();
        if !self.ready {
            self.ready = true;
            self.out.push(Effect::Ready);
        }
        true
    }

    /// Writes the noise control mode; the other sound mode fields keep their values (a one-step change, as
    /// OpenSCQ30's migration plans it when only the mode changes).
    fn set_noise(&mut self, key: &str) {
        let value = match key {
            NOISE_OFF => ambient::NORMAL,
            NOISE_TRANSPARENCY => ambient::TRANSPARENCY,
            NOISE_ANC => ambient::NOISE_CANCELING,
            _ => return self.note(format!("noise mode {key} unknown")),
        };
        let Some(mut raw) = self.sound_modes.clone() else { return self.note("noise mode before the state".into()) };
        raw[0] = value;
        self.send(msg::SET_SOUND_MODES, &raw);
        // The buds push `06 01` when the mode changed; shown at once, as the phone app does.
        self.sound_modes = Some(raw);
        self.common.noise = Some(key.into());
        self.out.push(Effect::State);
    }
}

fn noise_key(ambient_byte: u8) -> &'static str {
    match ambient_byte {
        ambient::NOISE_CANCELING => NOISE_ANC,
        ambient::TRANSPARENCY => NOISE_TRANSPARENCY,
        _ => NOISE_OFF,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::packet::IN;

    fn unhex(t: &str) -> Vec<u8> { (0..t.len()).step_by(2).map(|i| u8::from_str_radix(&t[i..i + 2], 16).unwrap()).collect() }

    // OpenSCQ30 `tools/soundcore-device-faker/devices/a3936.toml` and `a3957.toml`, command 01 01.
    const A3936: &str = "01010503010130342E313930342E31393339333661343737353833646461396500007878787878787878780078787878787878787800FF00FFFFFFFFFFFFFFFFFF00FFFFFFFFFFFFFFFFFF000000000000FFFFFFFFFFFFFFFFFF00FFFFFFFFFFFFFFFFFF0000000E001100001163116611441149070230000100000008310001010000FFFFFFFFFFFFFFFFFFFFFFFF";
    const A3957: &str = "00010808000030332E393030332E39303339353746343944384143323033333430302E303002000078787878787878787878FFFFFFFFFFFFFFFFFFFF0001918273778B938F960000918273778B938F9600000000000000918273778B938F960000918273778B938F96000000000A66663233FFFF4444330050000000000033010100010102005A0000010000FF01000001000100FFFFFF03FFFF";

    fn inbound(cmd: [u8; 2], body: &[u8]) -> Vec<u8> {
        let mut f = encode(cmd, body, true);
        f[..5].copy_from_slice(&IN);
        let n = f.len() - 1;
        f[n] = f[..n].iter().fold(0u8, |a, x| a.wrapping_add(*x));
        f
    }

    fn sent(e: &[Effect]) -> Vec<Vec<u8>> { e.iter().filter_map(|e| match e { Effect::Send(b) => Some(b.clone()), _ => None }).collect() }

    #[test]
    fn connect_asks_for_the_state_and_retries_once() {
        let mut d = Driver::new();
        assert_eq!(sent(&d.connected(0, "x".into())), [encode(msg::REQUEST_STATE, &[], true)]);
        assert!(sent(&d.tick(1000)).is_empty());
        assert_eq!(sent(&d.tick(STATE_RETRY_MS)).len(), 1);
        assert_eq!(d.next_wake(), None);
    }

    #[test]
    fn reads_the_space_a40_state() {
        let mut d = Driver::new();
        d.connected(0, "soundcore Space A40".into());
        let e = d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(A3936)));
        assert!(e.iter().any(|e| matches!(e, Effect::Ready)));
        let c = d.common();
        assert_eq!(c.model_id.as_deref(), Some("A3936"));
        assert_eq!(c.battery[0], Some(Level { value: 5, max: 5, charging: true }));
        assert_eq!(c.battery[1], Some(Level { value: 3, max: 5, charging: true }));
        assert_eq!(c.battery[2], Some(Level { value: 8, max: 10, charging: false }));
        assert_eq!(c.noise.as_deref(), Some(NOISE_OFF));
        assert_eq!(c.low_latency, Some(false));
        assert_eq!(d.firmware, ["04.19", "04.19"]);
        assert_eq!(d.ldac, Some(false));
    }

    #[test]
    fn reads_the_liberty_5_state() {
        let mut d = Driver::new();
        d.connected(0, "soundcore Liberty 5".into());
        d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(A3957)));
        let c = d.common();
        assert_eq!(c.model_id.as_deref(), Some("A3957"));
        assert_eq!(c.battery[0], Some(Level { value: 9, max: 10, charging: false }));
        assert_eq!(c.battery[2], Some(Level { value: 3, max: 10, charging: false }));
        assert_eq!(c.noise.as_deref(), Some(NOISE_ANC));
        assert_eq!(d.firmware, ["03.90", "03.90"]);
    }

    #[test]
    fn a_noise_write_keeps_the_other_fields() {
        let mut d = Driver::new();
        d.connected(0, "x".into());
        d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(A3936)));
        let e = d.command(6, Cmd::Anc(NOISE_ANC.into()));
        // A3936 sound modes 02 30 00 01 00 00: only the first byte changes.
        assert_eq!(sent(&e), [encode(msg::SET_SOUND_MODES, &[0x00, 0x30, 0x00, 0x01, 0x00, 0x00], true)]);
        assert_eq!(d.common().noise.as_deref(), Some(NOISE_ANC));
    }

    #[test]
    fn pushes_update_battery_and_noise() {
        let mut d = Driver::new();
        d.connected(0, "x".into());
        d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(A3936)));
        d.on_bytes(6, &inbound(msg::BATTERY_LEVEL, &[2, 4]));
        d.on_bytes(7, &inbound(msg::SOUND_MODES, &[1, 0x30, 0, 1]));
        let c = d.common();
        assert_eq!(c.battery[0].map(|b| (b.value, b.charging)), Some((2, true)));
        assert_eq!(c.noise.as_deref(), Some(NOISE_TRANSPARENCY));
    }
}
