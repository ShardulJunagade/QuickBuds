//! The Soundcore driver: sans-IO, the same shape as the OPPO driver (docs/CORE-API.md). The model layouts, the
//! commands and the equalizer math come from OpenSCQ30 (`lib/src/devices/soundcore/common`, `a3936`, `a3957`).
//! Copyright 2026 Kyle Scheuing, GPL-3.0-or-later.
//!
//! Connect: the driver asks for the full state (`01 01`). The serial number in the answer names the model (it
//! starts with the model number), and the model's layout reads the rest. The driver keeps the raw state bytes: a
//! write changes its field there and sends the model's command, so every other field keeps its value. Pushes update
//! the bytes after that. docs/SOUNDCORE-PROTOCOL.md has every field and command.

use super::msg::{self, ambient, Msg};
use super::packet::{encode, encode_in, Frame, Packet, Reader};
use crate::model::{Common, Level};
use serde_json::{json, Value};

/// The equalizer command of a model.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EqWrite {
    /// `03 87` with gender, age, the HearID fields and the DRC bands (A3936).
    HearIdDrc,
    /// `03 87` with the HearID block and the active bands, each followed by a zero (A3957).
    HearIdActive,
}

/// The sound mode group of a model (`06 01`, `06 81`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Modes {
    /// 6 bytes. Noise cancelling: manual (level 1 to 3) or adaptive.
    A3936,
    /// 7 bytes. Noise cancelling: manual (level 1 to 5) or transport (plane, car).
    A3957,
}

/// A press kind and its button id in the `04 81` and `04 83` commands.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Press {
    pub key: &'static str,
    pub id: u8,
    /// The action ids this press can have (see [`ACTIONS`]).
    pub actions: &'static [u8],
}

/// The action ids of the buttons, in OpenSCQ30's order. 0xF is "no action".
pub const ACTIONS: [(u8, &str); 8] = [
    (0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"),
    (4, "noiseMode"), (5, "assistant"), (6, "playPause"), (9, "gaming"),
];
const NO_ACTION: u8 = 0xF;
const ACTIONS_MINIMAL: &[u8] = &[0, 1, 2, 3, 6];
const ACTIONS_COMMON: &[u8] = &[0, 1, 2, 3, 4, 5, 6];
const ACTIONS_GAMING: &[u8] = &[0, 1, 2, 3, 4, 5, 6, 9];

/// Where a model keeps its values in the state packet (`01 01`). Offsets count from the first body byte.
pub struct Layout {
    /// The model number, as the serial number starts.
    pub id: &'static str,
    pub name: &'static str,
    /// The bud battery: (max level, offset). The level shown is `raw + offset` out of `max`.
    pub battery: (u8, u8),
    /// The case battery: (offset in the packet, max level, level offset).
    pub case: (usize, u8, u8),
    /// The equalizer: preset id (u16 little-endian), then 2 channels of 10 bands.
    pub eq: usize,
    pub eq_write: EqWrite,
    pub age: usize,
    /// The HearID block, 48 bytes: on, 2 x 10 bands, time (u32 big-endian), type, 2 x 10 custom bands, genre (u16).
    pub hear_id: usize,
    /// The buttons: offset, then one entry per press kind. Each press has a left and a right button.
    pub buttons: usize,
    pub presses: &'static [Press],
    /// The buttons carry an "on" byte before the action byte (A3936). Without it, action 0xF is "off".
    pub button_on_byte: bool,
    /// Turning one side's button off turns the other side's off too (A3936).
    pub button_pair_off: bool,
    pub cycle: usize,
    pub sound_modes: (usize, Modes),
    pub touch_tone: usize,
    pub ldac: usize,
    pub dual: usize,
    /// Auto power off: offset (on, index) and the minutes of each index.
    pub auto_off: (usize, &'static [u16]),
    /// Gaming mode: offset and the set command.
    pub gaming: (usize, [u8; 2]),
    pub wearing_tone: Option<usize>,
    pub low_battery: Option<usize>,
    pub wear_detection: Option<usize>,
    pub leak: Option<usize>,
    /// Limit high volume: on, the limit in dB, the reading refresh rate.
    pub volume_limit: Option<usize>,
    /// A sound mode field changes only while its parent mode is on (OpenSCQ30 `MigrationSteps`, A3957):
    /// transparency mode in transparency, the noise cancelling fields in noise cancelling, wind in either.
    pub modes_need_parent: bool,
    /// The shortest state packet this layout reads.
    pub min_len: usize,
}

const PRESSES_A3936: &[Press] = &[
    Press { key: "single", id: 2, actions: ACTIONS_MINIMAL },
    Press { key: "double", id: 0, actions: ACTIONS_GAMING },
    Press { key: "long", id: 1, actions: ACTIONS_GAMING },
];
const PRESSES_A3957: &[Press] = &[
    Press { key: "single", id: 2, actions: ACTIONS_COMMON },
    Press { key: "double", id: 0, actions: ACTIONS_COMMON },
    Press { key: "triple", id: 5, actions: ACTIONS_COMMON },
    Press { key: "long", id: 1, actions: ACTIONS_COMMON },
];

/// The models this driver knows. Tests: OpenSCQ30's recorded packets (`tools/soundcore-device-faker`).
pub const LAYOUTS: &[Layout] = &[
    // Space A40: tws 0, battery 2, firmware 6, serial 16, EQ 32, age 54, HearID 55, unknown 103, 6 buttons 104,
    // cycle 116, sound modes 117 (6), touch tone 123, case 124, color 125, LDAC 126, dual 127, auto off 128, gaming 130.
    Layout {
        id: "3936", name: "Soundcore Space A40", battery: (5, 0), case: (124, 10, 0), eq: 32, eq_write: EqWrite::HearIdDrc,
        age: 54, hear_id: 55, buttons: 104, presses: PRESSES_A3936, button_on_byte: true, button_pair_off: true,
        cycle: 116, sound_modes: (117, Modes::A3936), touch_tone: 123, ldac: 126, dual: 127,
        auto_off: (128, &[30, 60, 90, 120]), gaming: (130, [0x01, 0x87]),
        wearing_tone: None, low_battery: None, wear_detection: None, leak: None, volume_limit: None,
        modes_need_parent: false, min_len: 131,
    },
    // Liberty 5: tws 0, battery 2, firmware 6, serial 16, "00.00" 32, case 37, EQ 38, age 60, HearID 61, unknown 109,
    // 8 buttons 110, cycle 118, sound modes 119 (7), unknown 126, wearing tone 127, low battery 128, LDAC 129,
    // dual 130, auto off 131, volume limit 133, immersive 136, leak 143, wear 144, touch tone 145, gaming 146.
    Layout {
        id: "3957", name: "Soundcore Liberty 5", battery: (10, 1), case: (37, 10, 1), eq: 38, eq_write: EqWrite::HearIdActive,
        age: 60, hear_id: 61, buttons: 110, presses: PRESSES_A3957, button_on_byte: false, button_pair_off: false,
        cycle: 118, sound_modes: (119, Modes::A3957), touch_tone: 145, ldac: 129, dual: 130,
        auto_off: (131, &[10, 20, 30, 60]), gaming: (146, [0x10, 0x85]),
        wearing_tone: Some(127), low_battery: Some(128), wear_detection: Some(144), leak: Some(143), volume_limit: Some(133),
        modes_need_parent: true, min_len: 148,
    },
];

/// The noise control keys this driver gives (`Common::noise`).
pub const NOISE_OFF: &str = "Off";
pub const NOISE_TRANSPARENCY: &str = "Transparency";
pub const NOISE_ANC: &str = "ANC";

/// The equalizer presets (OpenSCQ30 `common_settings_type_2`): name key, id, 8 bands in tenths of a dB.
pub const PRESETS: [(&str, u16, [i16; 8]); 22] = [
    ("SoundcoreSignature", 0, [0, 0, 0, 0, 0, 0, 0, 0]),
    ("Acoustic", 1, [40, 10, 20, 20, 40, 40, 40, 20]),
    ("BassBooster", 2, [40, 30, 10, 0, 0, 0, 0, 0]),
    ("BassReducer", 3, [-40, -30, -10, 0, 0, 0, 0, 0]),
    ("Classical", 4, [30, 30, -20, -20, 0, 20, 30, 40]),
    ("Podcast", 5, [-30, 20, 40, 40, 30, 20, 0, -20]),
    ("Dance", 6, [20, -30, -10, 10, 20, 20, 10, -30]),
    ("Deep", 7, [20, 10, 30, 30, 20, -20, -40, -50]),
    ("Electronic", 8, [30, 20, -20, 20, 10, 20, 30, 30]),
    ("Flat", 9, [-20, -20, -10, 0, 0, 0, -20, -20]),
    ("HipHop", 10, [20, 30, -10, -10, 20, -10, 20, 30]),
    ("Jazz", 11, [20, 20, -20, -20, 0, 20, 30, 40]),
    ("Latin", 12, [0, 0, -20, -20, -20, 0, 30, 50]),
    ("Lounge", 13, [-10, 20, 40, 30, 0, -20, 20, 10]),
    ("Piano", 14, [0, 30, 30, 20, 40, 50, 30, 40]),
    ("Pop", 15, [-10, 10, 30, 30, 10, -10, -20, -30]),
    ("RnB", 16, [60, 20, -20, -20, 20, 30, 30, 40]),
    ("Rock", 17, [30, 20, -10, -10, 10, 30, 40, 50]),
    ("SmallSpeakers", 18, [40, 30, 10, 0, -20, -30, -40, -40]),
    ("SpokenWord", 19, [-30, -20, 10, 20, 20, 10, 0, -30]),
    ("TrebleBooster", 20, [-20, -20, -20, -10, 10, 20, 20, 40]),
    ("TrebleReducer", 21, [0, 0, 0, -20, -30, -40, -40, -60]),
];
/// The preset id of the custom equalizer.
pub const CUSTOM_EQ: u16 = 0xFEFE;
/// The custom bands: frequencies in Hz, limits in tenths of a dB.
pub const EQ_HZ: [u16; 8] = [100, 200, 400, 800, 1600, 3200, 6400, 12800];
pub const EQ_MIN: i16 = -120;
pub const EQ_MAX: i16 = 134;
/// The 2 bands after the 8 shown ones: a preset has 0 and -12 dB, the custom curve 0 and 0 (OpenSCQ30
/// `InvisibleBandsMode::Fixed`). DRC fills them with the preset values.
const EQ_TAIL: [i16; 2] = [0, -120];
const EQ_TAIL_CUSTOM: [i16; 2] = [0, 0];
/// Limit high volume: the dB range.
pub const VOLUME_LIMIT_DB: (u8, u8) = (75, 100);

pub enum Cmd {
    /// A noise control key.
    Anc(String),
    /// One setting by its key (see [`Driver::state_json`]) and its new value.
    Set(String, Value),
    /// The buttons go back to the factory actions.
    ResetButtons,
    /// A dual connection device: `connect`, `disconnect` or `forget`, by its address `AA:BB:..`.
    Device(String, String),
}

pub enum Effect {
    Send(Vec<u8>),
    Log { dir: &'static str, bytes: Vec<u8>, human: Option<String> },
    State,
    Ready,
}

/// Re-ask for the state when no answer came this long after the connect.
const STATE_RETRY_MS: u64 = 1500;

/// A dual connection device.
#[derive(Clone, PartialEq, Debug)]
pub struct Device {
    pub mac: [u8; 6],
    pub name: String,
    pub connected: bool,
}

#[derive(Default)]
pub struct Driver {
    reader: Reader,
    common: Common,
    layout: Option<&'static Layout>,
    /// The state packet body, kept current by pushes and writes.
    state: Vec<u8>,
    pub firmware: Vec<String>,
    pub serial: Option<String>,
    /// Both buds are connected to each other: the buttons' "connected" action applies.
    tws: bool,
    pub devices: Vec<Device>,
    ready: bool,
    /// A button reset waits for its answer, then the driver reads the state again.
    reset_pending: bool,
    retry_at: Option<u64>,
    out: Vec<Effect>,
}

fn level(raw: u8, (max, offset): (u8, u8)) -> Level {
    Level { value: raw.saturating_add(offset).min(max), max, charging: false }
}

fn eq_byte(v: i16) -> u8 { (v.clamp(EQ_MIN, EQ_MAX) - EQ_MIN) as u8 }
fn eq_value(b: u8) -> i16 { b as i16 + EQ_MIN }

/// OpenSCQ30 `VolumeAdjustments::apply_drc`: the band values the A3936 plays, from the 8 set ones.
fn apply_drc(v: &[i16; 10]) -> [i16; 10] {
    const S: f64 = 0.85;
    const L: f64 = 0.95;
    const C: [[f64; 8]; 8] = [
        [1.26, -0.71 * S, 0.177, -0.0494, 0.0345, -0.0197, 0.0075, -0.00217],
        [-0.71 * S, 1.73 * L, 0.0, 0.204, -0.068, 0.045, -0.0235, 0.0075],
        [0.177, -0.81 * S, 1.73 * L, -0.81 * S, 0.208, -0.07, 0.045, -0.0197],
        [-0.0494, 0.204, 0.0, 1.73 * L, -0.82 * S, 0.208, -0.068, 0.0345],
        [0.0345, -0.068, 0.208, -0.82 * S, 1.73 * L, 0.0, 0.204, -0.0494],
        [-0.0197, 0.045, -0.07, 0.208, -0.81 * S, 1.73 * L, -0.81 * S, 0.177],
        [0.0075, -0.0235, 0.045, -0.068, 0.204, 0.0, 1.83 * L, -0.71 * S],
        [-0.00217, 0.0075, -0.0197, 0.0345, -0.0494, 0.177, -0.71 * S, 1.5],
    ];
    let a: Vec<f64> = v.iter().map(|&x| x as f64 / 10.0).collect();
    let lows = a[2] * 0.81 * S;
    let highs = a[5] * 0.81 * S;
    let mut out = [0i16, 0, 0, 0, 0, 0, 0, 0, EQ_TAIL[0], EQ_TAIL[1]];
    for (band, coef) in C.iter().enumerate() {
        let sum = (0..8).fold(0.0, |acc, i| {
            if (band == 1 || band == 3) && i == 2 { acc - lows }
            else if (band == 4 || band == 6) && i == 5 { acc - highs }
            else { acc + a[i] * coef[i] }
        });
        out[band] = (sum / 10.0 * 10.0).round() as i16;
    }
    out
}

fn mac_text(m: &[u8; 6]) -> String { m.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(":") }
fn mac_bytes(t: &str) -> Option<[u8; 6]> {
    let v: Vec<u8> = t.split(':').map(|x| u8::from_str_radix(x, 16).ok()).collect::<Option<_>>()?;
    v.try_into().ok()
}

/// `0b 01`: total, index, then devices (length incl. itself, connected, 6-byte address, name).
fn devices(b: &[u8]) -> Option<(u8, u8, Vec<Device>)> {
    let (&total, rest) = b.split_first()?;
    let (&index, mut rest) = rest.split_first()?;
    let mut out = Vec::new();
    while rest.len() >= 8 {
        let n = (rest[0] as usize).saturating_sub(8).min(rest.len() - 8);
        let name = &rest[8..8 + n];
        let name = &name[..name.iter().position(|&c| c == 0).unwrap_or(name.len())];
        out.push(Device { mac: rest[2..8].try_into().ok()?, name: String::from_utf8_lossy(name).into(), connected: rest[1] != 0 });
        rest = &rest[8 + n..];
    }
    Some((total, index, out))
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
            Cmd::Set(key, v) => self.set(&key, &v),
            Cmd::ResetButtons => {
                // The buds answer with an empty `04 82` and no state: the answer asks for it again
                // (OpenSCQ30 `reset_button_configuration`).
                self.send([0x04, 0x82], &[]);
                self.reset_pending = true;
            }
            Cmd::Device(action, mac) => self.device(&action, &mac),
        }
        // The driver changed its copy at once: the apps repaint from it.
        self.out.push(Effect::State);
        std::mem::take(&mut self.out)
    }

    pub fn on_bytes(&mut self, _now: u64, data: &[u8]) -> Vec<Effect> {
        let mut changed = false;
        for f in self.reader.push(data) {
            match f {
                Frame::Bad(b) => self.out.push(Effect::Log { dir: "DISCARDED RX", bytes: b, human: None }),
                Frame::Packet(p) => {
                    // The whole frame, as the OPPO log has it: the report Worker and the log decoder look for the header.
                    self.out.push(Effect::Log { dir: "RX", bytes: encode_in(p.cmd, &p.body), human: Some(format!("{:02X} {:02X}", p.cmd[0], p.cmd[1])) });
                    changed |= self.packet(p);
                }
            }
        }
        if changed { self.out.push(Effect::State); }
        std::mem::take(&mut self.out)
    }

    fn send(&mut self, cmd: [u8; 2], body: &[u8]) {
        let frame = encode(cmd, body, true);
        self.out.push(Effect::Log { dir: "TX", bytes: frame.clone(), human: Some(format!("{:02X} {:02X}", cmd[0], cmd[1])) });
        self.out.push(Effect::Send(frame));
    }

    fn note(&mut self, text: String) {
        self.out.push(Effect::Log { dir: "NOTE", bytes: Vec::new(), human: Some(text) });
    }

    /// One packet. True when the state changed.
    fn packet(&mut self, p: Packet) -> bool {
        if p.cmd == msg::REQUEST_STATE {
            return self.state(&p.body);
        }
        let Some(l) = self.layout else { return false };
        let (modes_at, kind) = l.sound_modes;
        let modes_len = modes_len(kind);
        match (p.cmd, p.body.len()) {
            // The model pushes its whole sound mode group. Another length has another layout: OpenSCQ30's parser
            // refuses it too.
            (msg::SOUND_MODES, n) if n == modes_len => {
                self.state[modes_at..modes_at + n].copy_from_slice(&p.body);
                self.refresh();
                return true;
            }
            (msg::SOUND_MODES, n) => { self.note(format!("sound modes push of {n} bytes, the model has {modes_len}")); return false; }
            ([0x04, 0x82], 0) if self.reset_pending => {
                self.reset_pending = false;
                self.send(msg::REQUEST_STATE, &[]);
                return false;
            }
            ([0x01, 0x11], 1) => { self.state[l.gaming.0] = p.body[0]; self.refresh(); return true; }
            ([0x0B, 0x01], _) => {
                let Some((total, index, list)) = devices(&p.body) else { return false };
                if index <= 1 { self.devices.clear(); }
                self.devices.extend(list);
                return index >= total;
            }
            _ => {}
        }
        match msg::decode(p) {
            Msg::Battery(v) => {
                for (i, raw) in v.into_iter().take(2).enumerate() {
                    let charging = self.common.battery[i].is_some_and(|b| b.charging);
                    self.common.battery[i] = Some(Level { charging, ..level(raw, l.battery) });
                }
            }
            Msg::Charging(v) => for (i, c) in v.into_iter().take(2).enumerate() {
                if let Some(b) = self.common.battery[i].as_mut() { b.charging = c; }
            },
            // Handled above: every known model pushes its own length.
            Msg::SoundModes(_) => return false,
            Msg::Ldac(on) => self.state[l.ldac] = on as u8,
            Msg::Firmware { versions, serial } => { self.firmware = versions; self.serial = Some(serial); }
            Msg::Tws { both, .. } => self.tws = both,
            Msg::Other(p) => {
                // A command with the high bit set answers a write: empty, or with the written bytes (`0b 83`, `0b 84`).
                if p.cmd[1] & 0x80 == 0 { self.note(format!("unknown packet {:02X} {:02X}", p.cmd[0], p.cmd[1])); }
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
        self.state = b.to_vec();
        self.serial = serial.map(str::to_string);
        self.firmware = [&b[6..11], &b[11..16]].iter().filter_map(|v| msg::firmware(v)).collect();
        self.tws = b[1] != 0;
        self.common.model_id = Some(format!("A{}", l.id));
        for i in 0..2 {
            self.common.battery[i] = Some(Level { charging: b[4 + i] == 1, ..level(b[2 + i], l.battery) });
        }
        let (at, max, off) = l.case;
        self.common.battery[2] = Some(level(b[at], (max, off)));
        self.common.noise_modes = vec![NOISE_OFF.into(), NOISE_TRANSPARENCY.into(), NOISE_ANC.into()];
        let mut f: std::collections::BTreeSet<&'static str> = ["anc", "game", "eq", "controls", "dual", "hires"].into_iter().collect();
        if l.wear_detection.is_some() { f.insert("wear"); }
        self.common.features = f;
        self.refresh();
        if b[l.dual] != 0 { self.send([0x0B, 0x01], &[]); }
        if !self.ready {
            self.ready = true;
            self.out.push(Effect::Ready);
        }
        true
    }

    /// The shared state from the raw bytes.
    fn refresh(&mut self) {
        let Some(l) = self.layout else { return };
        self.common.noise = Some(noise_key(self.state[l.sound_modes.0]).into());
        self.common.low_latency = Some(self.state[l.gaming.0] != 0);
    }

    fn byte(&self, at: usize) -> u8 { self.state.get(at).copied().unwrap_or(0) }

    /// The 10 bands of channel 0, in tenths of a dB.
    fn bands(&self, l: &Layout) -> [i16; 10] {
        std::array::from_fn(|i| eq_value(self.byte(l.eq + 2 + i)))
    }

    /// Every setting of the connected model, as the apps read it. Keys of absent features are left out.
    pub fn state_json(&self) -> Value {
        let Some(l) = self.layout else { return json!({}) };
        let (m, kind) = l.sound_modes;
        let b = |at: usize| self.byte(at) != 0;
        let preset = u16::from_le_bytes([self.byte(l.eq), self.byte(l.eq + 1)]);
        // The high nibble is the manual level, the low one the adaptive level.
        let nc_level = self.byte(m + 1) >> 4;
        let mut buttons = Vec::new();
        for (pi, p) in l.presses.iter().enumerate() {
            for side in 0..2u8 {
                buttons.push(json!({ "press": p.key, "side": side, "action": self.action(l, pi, side), "actions": p.actions }));
            }
        }
        let cycle = self.byte(l.cycle);
        let mut v = json!({
            "model": format!("A{}", l.id),
            "name": l.name,
            "serial": self.serial,
            "firmware": self.firmware,
            "noise": self.byte(m),
            "transparencyMode": self.byte(m + 2),
            "ancMode": self.byte(m + 3),
            "ancModes": match kind { Modes::A3936 => json!([0, 1]), Modes::A3957 => json!([0, 2]) },
            "ancLevel": nc_level,
            "ancLevels": match kind { Modes::A3936 => 3, Modes::A3957 => 5 },
            "windNoise": self.byte(m + 4) & 1 != 0,
            "windDetected": self.byte(m + 4) & 2 != 0,
            "eqPreset": preset,
            "eqBands": self.bands(l)[..8],
            "eqPresets": PRESETS.iter().map(|(name, id, bands)| json!({"id": id, "name": name, "bands": bands})).collect::<Vec<_>>(),
            "eqHz": EQ_HZ,
            "eqRange": [EQ_MIN, EQ_MAX],
            "buttons": buttons,
            "buttonsPaired": l.button_pair_off,
            "cycle": { "anc": cycle & 1 != 0, "transparency": cycle & 2 != 0, "normal": cycle & 4 != 0 },
            "touchTone": b(l.touch_tone),
            "ldac": b(l.ldac),
            "dual": b(l.dual),
            "devices": self.devices.iter().map(|d| json!({"mac": mac_text(&d.mac), "name": d.name, "connected": d.connected})).collect::<Vec<_>>(),
            "autoOff": { "on": b(l.auto_off.0), "index": self.byte(l.auto_off.0 + 1), "minutes": l.auto_off.1 },
            "gaming": b(l.gaming.0),
        });
        let o = v.as_object_mut().expect("object");
        if kind == Modes::A3936 { o.insert("adaptiveLevel".into(), json!(self.byte(m + 1) & 0x0F)); }
        if kind == Modes::A3957 { o.insert("transport".into(), json!(self.byte(m + 6))); }
        for (key, at) in [("wearingTone", l.wearing_tone), ("lowBattery", l.low_battery), ("wearDetection", l.wear_detection), ("leak", l.leak)] {
            if let Some(at) = at { o.insert(key.into(), json!(b(at))); }
        }
        if let Some(at) = l.volume_limit {
            o.insert("volumeLimit".into(), json!({ "on": b(at), "db": self.byte(at + 1), "rate": self.byte(at + 2), "range": [VOLUME_LIMIT_DB.0, VOLUME_LIMIT_DB.1] }));
        }
        v
    }

    /// The byte offset of a button's action byte, and of its "on" byte when the model has one.
    fn button_at(l: &Layout, press: usize, side: u8) -> (usize, Option<usize>) {
        let i = press * 2 + side as usize;
        if l.button_on_byte { (l.buttons + i * 2 + 1, Some(l.buttons + i * 2)) } else { (l.buttons + i, None) }
    }

    /// The nibble of a byte that applies now: low while both buds are connected, high otherwise.
    fn nibble(&self, b: u8) -> u8 { if self.tws { b & 0x0F } else { b >> 4 } }
    fn with_nibble(&self, b: u8, v: u8) -> u8 { if self.tws { (b & 0xF0) | (v & 0x0F) } else { (b & 0x0F) | (v << 4) } }

    /// A button's action id, or None when it is off.
    fn action(&self, l: &Layout, press: usize, side: u8) -> Option<u8> {
        let (at, on_at) = Self::button_at(l, press, side);
        let action = self.nibble(self.byte(at));
        let on = on_at.map_or(action != NO_ACTION, |o| self.nibble(self.byte(o)) == 1);
        on.then_some(action)
    }

    /// Sets one button. `None` turns it off.
    fn set_button(&mut self, l: &Layout, press: usize, side: u8, action: Option<u8>) {
        let id = l.presses[press].id;
        let (at, on_at) = Self::button_at(l, press, side);
        if let Some(a) = action {
            let byte = self.with_nibble(self.byte(at), a);
            self.state[at] = byte;
            self.send([0x04, 0x81], &[side, id, byte]);
        }
        match on_at {
            Some(o) => {
                let byte = self.with_nibble(self.byte(o), action.is_some() as u8);
                if byte != self.byte(o) {
                    self.state[o] = byte;
                    self.send([0x04, 0x83], &[side, id, byte]);
                }
            }
            None if action.is_none() => {
                let byte = self.with_nibble(self.byte(at), NO_ACTION);
                self.state[at] = byte;
                self.send([0x04, 0x81], &[side, id, byte]);
            }
            None => {}
        }
    }

    fn set(&mut self, key: &str, v: &Value) {
        let Some(l) = self.layout else { return self.note(format!("{key} before the state")) };
        let (m, kind) = l.sound_modes;
        let on = v.as_bool();
        let int = v.as_u64().map(|x| x as u8);
        let modes = |d: &mut Self, at: usize, value: u8| {
            d.state[m + at] = value;
            let body = d.state[m..m + modes_len(kind)].to_vec();
            d.send(msg::SET_SOUND_MODES, &body);
            d.refresh();
        };
        // A3957: a sound mode field needs its parent mode (see `Layout::modes_need_parent`).
        let ambient = self.byte(m);
        let parent_ok = !l.modes_need_parent || match key {
            "transparencyMode" => ambient == ambient::TRANSPARENCY,
            "ancMode" => ambient == ambient::NOISE_CANCELING,
            "ancLevel" => ambient == ambient::NOISE_CANCELING && self.byte(m + 3) == 0,
            "transport" => ambient == ambient::NOISE_CANCELING && self.byte(m + 3) == 2,
            "windNoise" => ambient == ambient::NOISE_CANCELING || ambient == ambient::TRANSPARENCY,
            _ => true,
        };
        if !parent_ok { return self.note(format!("{key}: its noise control mode is not on")); }
        let flag = |d: &mut Self, at: Option<usize>, cmd: [u8; 2]| match (at, on) {
            (Some(at), Some(on)) => { d.state[at] = on as u8; d.send(cmd, &[on as u8]); d.refresh(); }
            _ => d.note(format!("{key}: no such setting or not a switch")),
        };
        match key {
            "transparencyMode" if matches!(int, Some(0 | 1)) => modes(self, 2, int.unwrap_or(0)),
            "ancMode" => match (kind, int) {
                (Modes::A3936, Some(x @ (0 | 1))) | (Modes::A3957, Some(x @ (0 | 2))) => modes(self, 3, x),
                _ => self.note(format!("ancMode {v} not offered")),
            },
            "ancLevel" => {
                let max = match kind { Modes::A3936 => 3, Modes::A3957 => 5 };
                match int.filter(|x| (1..=max).contains(x)) {
                    Some(x) => { let b = (x << 4) | (self.byte(m + 1) & 0x0F); modes(self, 1, b) }
                    None => self.note(format!("ancLevel {v} out of range")),
                }
            }
            "transport" if kind == Modes::A3957 && matches!(int, Some(0 | 3)) => modes(self, 6, int.unwrap_or(0)),
            "windNoise" if on.is_some() => { let b = (self.byte(m + 4) & !1) | on.unwrap_or(false) as u8; modes(self, 4, b) }
            "touchTone" => flag(self, Some(l.touch_tone), [0x01, 0x83]),
            "ldac" => flag(self, Some(l.ldac), [0x01, 0xFF]),
            "dual" => {
                flag(self, Some(l.dual), [0x0B, 0x84]);
                if on == Some(true) { self.send([0x0B, 0x01], &[]); } else { self.devices.clear(); }
            }
            "gaming" => flag(self, Some(l.gaming.0), l.gaming.1),
            "wearingTone" => flag(self, l.wearing_tone, [0x01, 0x8C]),
            "lowBattery" => flag(self, l.low_battery, [0x10, 0x82]),
            "wearDetection" => flag(self, l.wear_detection, [0x01, 0x81]),
            "leak" => flag(self, l.leak, [0x10, 0x86]),
            "autoOff" => {
                let at = l.auto_off.0;
                let on = v["on"].as_bool().unwrap_or(self.byte(at) != 0);
                let index = v["index"].as_u64().map_or(self.byte(at + 1), |x| x as u8);
                if index as usize >= l.auto_off.1.len() { return self.note(format!("autoOff index {index} out of range")); }
                self.state[at] = on as u8;
                self.state[at + 1] = index;
                self.send([0x01, 0x86], &[on as u8, index]);
            }
            "volumeLimit" => {
                let Some(at) = l.volume_limit else { return self.note("volumeLimit: no such setting".into()) };
                let on = v["on"].as_bool().unwrap_or(self.byte(at) != 0);
                // OpenSCQ30 allows steps of 5 dB only.
                let db = (v["db"].as_u64().map_or(self.byte(at + 1), |x| x as u8).clamp(VOLUME_LIMIT_DB.0, VOLUME_LIMIT_DB.1) + 2) / 5 * 5;
                if on != (self.byte(at) != 0) || db != self.byte(at + 1) {
                    self.state[at] = on as u8;
                    self.state[at + 1] = db;
                    self.send([0x20, 0x82], &[on as u8, db]);
                }
                if let Some(rate) = v["rate"].as_u64().map(|x| x as u8).filter(|&r| r <= 2 && r != self.byte(at + 2)) {
                    self.state[at + 2] = rate;
                    self.send([0x20, 0x81], &[rate]);
                }
            }
            "cycle" => {
                let bit = |k: &str, b: u8| if v[k].as_bool().unwrap_or(self.byte(l.cycle) & b != 0) { b } else { 0 };
                let c = bit("anc", 1) | bit("transparency", 2) | bit("normal", 4);
                // The buds need two modes to cycle between.
                if c.count_ones() < 2 { return self.note("cycle needs two modes".into()); }
                self.state[l.cycle] = c;
                self.send([0x06, 0x82], &[c]);
            }
            "button" => {
                let press = v["press"].as_str().and_then(|k| l.presses.iter().position(|p| p.key == k));
                let side = v["side"].as_u64().filter(|&s| s <= 1).map(|s| s as u8);
                let (Some(press), Some(side)) = (press, side) else { return self.note(format!("button {v} unknown")) };
                let action = v["action"].as_u64().map(|a| a as u8);
                if let Some(a) = action.filter(|a| !l.presses[press].actions.contains(a)) {
                    return self.note(format!("button action {a} not offered"));
                }
                self.set_button(l, press, side, action);
                // A3936: one side off turns the other off; one side on turns the other on with its last action.
                if l.button_pair_off {
                    let other = 1 - side;
                    let (other_at, _) = Self::button_at(l, press, other);
                    match (action, self.action(l, press, other)) {
                        (None, Some(_)) => self.set_button(l, press, other, None),
                        (Some(a), None) => {
                            let last = self.nibble(self.byte(other_at));
                            let keep = if l.presses[press].actions.contains(&last) { last } else { a };
                            self.set_button(l, press, other, Some(keep));
                        }
                        _ => {}
                    }
                }
            }
            "eq" => {
                let id = v["preset"].as_u64().map(|x| x as u16);
                let bands: [i16; 8] = match id {
                    Some(CUSTOM_EQ) => {
                        let Some(b) = v["bands"].as_array().filter(|a| a.len() == 8)
                            .and_then(|a| a.iter().map(|x| x.as_i64().map(|y| (y as i16).clamp(EQ_MIN, EQ_MAX))).collect::<Option<Vec<_>>>())
                        else { return self.note(format!("eq bands {v} need 8 values")) };
                        b.try_into().expect("8 values")
                    }
                    Some(p) => match PRESETS.iter().find(|x| x.1 == p) { Some(x) => x.2, None => return self.note(format!("eq preset {p} unknown")) },
                    None => return self.note(format!("eq {v} has no preset")),
                };
                let tail = if id == Some(CUSTOM_EQ) { EQ_TAIL_CUSTOM } else { EQ_TAIL };
                self.set_eq(l, id.unwrap_or(0), bands, tail);
            }
            _ => self.note(format!("setting {key} = {v} unknown")),
        }
    }

    /// Writes the equalizer: both channels get the same bands. HearID stays off, so the bands apply.
    fn set_eq(&mut self, l: &Layout, preset: u16, bands: [i16; 8], tail: [i16; 2]) {
        let all: [i16; 10] = std::array::from_fn(|i| if i < 8 { bands[i] } else { tail[i - 8] });
        let eq: Vec<u8> = all.iter().map(|&x| eq_byte(x)).collect();
        self.state[l.eq..l.eq + 2].copy_from_slice(&preset.to_le_bytes());
        for ch in 0..2 { self.state[l.eq + 2 + ch * 10..l.eq + 12 + ch * 10].copy_from_slice(&eq); }
        let h = l.hear_id;
        self.state[h] = 0;
        let hear = self.state[h..h + 48].to_vec();
        // HearID block: on 0, bands 1..21, time 21..25, type 25, custom bands 26..46, genre 46..48.
        let (bands_h, time, kind, custom, genre) = (&hear[1..21], &hear[21..25], hear[25], &hear[26..46], &hear[46..48]);
        let mut body = preset.to_le_bytes().to_vec();
        body.extend_from_slice(genre);
        body.extend_from_slice(&eq);
        body.extend_from_slice(&eq);
        match l.eq_write {
            EqWrite::HearIdDrc => {
                body.extend_from_slice(&[0, self.byte(l.age), 0]);
                body.extend_from_slice(bands_h);
                body.extend_from_slice(time);
                body.push(kind);
                body.extend_from_slice(custom);
                let drc: Vec<u8> = apply_drc(&all).iter().map(|&x| eq_byte(x)).collect();
                body.extend_from_slice(&drc);
                body.extend_from_slice(&drc);
            }
            EqWrite::HearIdActive => {
                body.extend_from_slice(&[0, 0, 0]);
                body.extend_from_slice(bands_h);
                body.extend_from_slice(time);
                body.push(kind);
                body.extend_from_slice(custom);
                for &x in eq.iter().chain(eq.iter()) { body.extend_from_slice(&[x, 0]); }
                body.extend_from_slice(&[0, 0]);
            }
        }
        self.send([0x03, 0x87], &body);
    }

    fn device(&mut self, action: &str, mac: &str) {
        let Some(m) = mac_bytes(mac) else { return self.note(format!("device address {mac} invalid")) };
        let cmd = match action { "disconnect" => [0x0B, 0x81], "connect" => [0x0B, 0x82], "forget" => [0x0B, 0x83], _ => return self.note(format!("device action {action} unknown")) };
        self.send(cmd, &m);
        match action {
            "forget" => self.devices.retain(|d| d.mac != m),
            _ => if let Some(d) = self.devices.iter_mut().find(|d| d.mac == m) { d.connected = action == "connect" },
        }
    }

    /// Writes the noise control mode; the other sound mode fields keep their values.
    fn set_noise(&mut self, key: &str) {
        let value = match key {
            NOISE_OFF => ambient::NORMAL,
            NOISE_TRANSPARENCY => ambient::TRANSPARENCY,
            NOISE_ANC => ambient::NOISE_CANCELING,
            _ => return self.note(format!("noise mode {key} unknown")),
        };
        let Some(l) = self.layout else { return self.note("noise mode before the state".into()) };
        let (m, kind) = l.sound_modes;
        self.state[m] = value;
        let body = self.state[m..m + modes_len(kind)].to_vec();
        self.send(msg::SET_SOUND_MODES, &body);
        // The buds push `06 01` when the mode changed; shown at once, as the phone app does.
        self.refresh();
        self.out.push(Effect::State);
    }
}

/// Recorded state packets (command 01 01). A3936: OpenSCQ30 `tools/soundcore-device-faker/devices/a3936.toml`.
/// A3957: a Liberty 5 (OpenSCQ30 issue 226, test `test_with_liberty5_packet_from_issue_226`). The tests read them,
/// and the apps' demo mode shows every screen with them, without buds.
pub const DEMO: [(&str, &str); 2] = [
    ("A3936", "01010503010130342E313930342E31393339333661343737353833646461396500007878787878787878780078787878787878787800FF00FFFFFFFFFFFFFFFFFF00FFFFFFFFFFFFFFFFFF000000000000FFFFFFFFFFFFFFFFFF00FFFFFFFFFFFFFFFFFF0000000E001100001163116611441149070230000100000008310001010000FFFFFFFFFFFFFFFFFFFFFFFF"),
    ("A3957", "00010808000030332E393030332E39303339353746343944384143323033333430302E303002000078787878787878787878FFFFFFFFFFFFFFFFFFFF0001918273778B938F960000918273778B938F9600000000000000918273778B938F960000918273778B938F96000000000A66663233FFFF4444330050000000000033010100010102005A0000010000FF01000001010001FFFFFF03FFFF"),
];

fn unhex(t: &str) -> Vec<u8> { (0..t.len() / 2).filter_map(|i| u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok()).collect() }

/// A driver that holds a recorded state ([`DEMO`]): its `state_json` is what the buds would give. No link.
pub fn demo(model: &str) -> Option<Driver> {
    let (_, hex) = DEMO.iter().find(|d| d.0 == model)?;
    let mut d = Driver::new();
    let _ = d.connected(0, String::new());
    d.state(&unhex(hex));
    d.retry_at = None;
    d.out.clear();
    Some(d)
}

fn modes_len(kind: Modes) -> usize { match kind { Modes::A3936 => 6, Modes::A3957 => 7 } }

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

    const A3936: &str = DEMO[0].1;
    const A3957: &str = DEMO[1].1;


    fn inbound(cmd: [u8; 2], body: &[u8]) -> Vec<u8> { encode_in(cmd, body) }

    fn sent(e: &[Effect]) -> Vec<Vec<u8>> { e.iter().filter_map(|e| match e { Effect::Send(b) => Some(b.clone()), _ => None }).collect() }

    fn driver(state: &str) -> Driver {
        let mut d = Driver::new();
        d.connected(0, "x".into());
        d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(state)));
        d
    }

    fn set(d: &mut Driver, k: &str, v: Value) -> Vec<Vec<u8>> { sent(&d.command(6, Cmd::Set(k.into(), v))) }

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
        // Dual connection is on: the driver asks for the device list.
        assert_eq!(sent(&e), [encode([0x0B, 0x01], &[], true)]);
        let c = d.common();
        assert_eq!(c.model_id.as_deref(), Some("A3936"));
        assert_eq!(c.battery[0], Some(Level { value: 5, max: 5, charging: true }));
        assert_eq!(c.battery[1], Some(Level { value: 3, max: 5, charging: true }));
        assert_eq!(c.battery[2], Some(Level { value: 8, max: 10, charging: false }));
        assert_eq!(c.noise.as_deref(), Some(NOISE_OFF));
        assert_eq!(c.low_latency, Some(false));
        assert_eq!(d.firmware, ["04.19", "04.19"]);
        // OpenSCQ30 `a3936.rs` test: adaptive, fully transparent, buttons, signature preset, 30 min auto off.
        let s = d.state_json();
        assert_eq!(s["ancMode"], 1);
        assert_eq!(s["transparencyMode"], 0);
        assert_eq!(s["eqPreset"], 0);
        assert_eq!(s["touchTone"], false);
        assert_eq!(s["windNoise"], false);
        assert_eq!(s["ldac"], false);
        assert_eq!(s["dual"], true);
        assert_eq!(s["autoOff"]["on"], true);
        assert_eq!(s["autoOff"]["index"], 0);
        let b = |press: &str, side: u8| s["buttons"].as_array().unwrap().iter()
            .find(|x| x["press"] == press && x["side"] == side).unwrap()["action"].clone();
        assert_eq!(b("single", 0), Value::Null);
        assert_eq!(b("single", 1), Value::Null);
        assert_eq!(b("double", 0), 3);
        assert_eq!(b("double", 1), 6);
        assert_eq!(b("long", 0), 4);
        assert_eq!(b("long", 1), 9);
    }

    #[test]
    fn reads_the_liberty_5_state() {
        let d = driver(A3957);
        let c = d.common();
        assert_eq!(c.model_id.as_deref(), Some("A3957"));
        assert_eq!(c.battery[0], Some(Level { value: 9, max: 10, charging: false }));
        assert_eq!(c.battery[2], Some(Level { value: 3, max: 10, charging: false }));
        assert_eq!(c.noise.as_deref(), Some(NOISE_ANC));
        assert_eq!(d.firmware, ["03.90", "03.90"]);
        assert!(c.features.contains("wear"));
        // OpenSCQ30 `a3957.rs` test (issue 226).
        let s = d.state_json();
        assert_eq!(s["ancMode"], 0);
        assert_eq!(s["ancLevel"], 5);
        assert_eq!(s["transport"], 0);
        assert_eq!(s["windNoise"], false);
        assert_eq!(s["gaming"], false);
        assert_eq!(s["wearDetection"], true);
        assert_eq!(s["leak"], false);
        assert_eq!(s["touchTone"], true);
        assert_eq!(s["wearingTone"], true);
        assert_eq!(s["lowBattery"], true);
        assert_eq!(s["volumeLimit"]["on"], false);
        assert_eq!(s["volumeLimit"]["db"], 90);
        assert_eq!(s["volumeLimit"]["rate"], 0);
        assert_eq!(s["dual"], true);
        let b = |press: &str, side: u8| s["buttons"].as_array().unwrap().iter()
            .find(|x| x["press"] == press && x["side"] == side).unwrap()["action"].clone();
        assert_eq!(b("single", 0), 6);
        assert_eq!(b("double", 0), 2);
        assert_eq!(b("triple", 0), Value::Null);
        assert_eq!(b("long", 0), 4);
        assert_eq!(b("single", 1), 6);
        assert_eq!(b("double", 1), 3);
        assert_eq!(b("triple", 1), Value::Null);
        assert_eq!(b("long", 1), 4);
    }

    #[test]
    fn a_noise_write_keeps_the_other_fields() {
        let mut d = driver(A3936);
        let e = d.command(6, Cmd::Anc(NOISE_ANC.into()));
        // A3936 sound modes 02 30 00 01 00 00: only the first byte changes.
        assert_eq!(sent(&e), [encode(msg::SET_SOUND_MODES, &[0x00, 0x30, 0x00, 0x01, 0x00, 0x00], true)]);
        assert_eq!(d.common().noise.as_deref(), Some(NOISE_ANC));
    }

    #[test]
    fn sound_mode_fields_write_their_byte() {
        let mut d = driver(A3957);
        // A3957 sound modes 00 50 00 00 00 00 00.
        assert_eq!(set(&mut d, "ancLevel", json!(3)), [encode(msg::SET_SOUND_MODES, &[0, 0x30, 0, 0, 0, 0, 0], true)]);
        assert_eq!(set(&mut d, "ancMode", json!(2)), [encode(msg::SET_SOUND_MODES, &[0, 0x30, 0, 2, 0, 0, 0], true)]);
        assert_eq!(set(&mut d, "transport", json!(3)), [encode(msg::SET_SOUND_MODES, &[0, 0x30, 0, 2, 0, 0, 3], true)]);
        assert_eq!(set(&mut d, "windNoise", json!(true)), [encode(msg::SET_SOUND_MODES, &[0, 0x30, 0, 2, 1, 0, 3], true)]);
        // Adaptive is not offered on the A3957; level 6 is out of range.
        assert!(set(&mut d, "ancMode", json!(1)).is_empty());
        assert!(set(&mut d, "ancLevel", json!(6)).is_empty());
        d.command(7, Cmd::Anc(NOISE_TRANSPARENCY.into()));
        assert_eq!(set(&mut d, "transparencyMode", json!(1)), [encode(msg::SET_SOUND_MODES, &[1, 0x30, 1, 2, 1, 0, 3], true)]);
    }

    #[test]
    fn a3957_fields_need_their_mode() {
        let mut d = driver(A3957);
        // Noise cancelling is on: transparency mode waits, the noise cancelling fields write.
        assert!(set(&mut d, "transparencyMode", json!(1)).is_empty());
        assert_eq!(set(&mut d, "ancLevel", json!(2)).len(), 1);
        d.command(7, Cmd::Anc(NOISE_TRANSPARENCY.into()));
        assert_eq!(set(&mut d, "transparencyMode", json!(1)).len(), 1);
        assert!(set(&mut d, "ancLevel", json!(3)).is_empty());
        d.command(8, Cmd::Anc(NOISE_OFF.into()));
        assert!(set(&mut d, "windNoise", json!(true)).is_empty());
    }

    #[test]
    fn reset_reads_the_state_after_the_answer() {
        let mut d = driver(A3936);
        assert_eq!(sent(&d.command(6, Cmd::ResetButtons)), [encode([0x04, 0x82], &[], true)]);
        assert_eq!(sent(&d.on_bytes(7, &inbound([0x04, 0x82], &[]))), [encode(msg::REQUEST_STATE, &[], true)]);
    }

    #[test]
    fn switches_use_the_model_command() {
        let mut d = driver(A3957);
        // OpenSCQ30 `gaming_mode_uses_correct_command`: 10 85 on the A3957.
        assert_eq!(set(&mut d, "gaming", json!(true)), [encode([0x10, 0x85], &[1], true)]);
        assert_eq!(d.common().low_latency, Some(true));
        assert_eq!(set(&mut d, "leak", json!(true)), [encode([0x10, 0x86], &[1], true)]);
        assert_eq!(set(&mut d, "volumeLimit", json!({"on": true, "db": 83})), [encode([0x20, 0x82], &[1, 85], true)]);
        assert_eq!(set(&mut d, "volumeLimit", json!({"rate": 2})), [encode([0x20, 0x81], &[2], true)]);
        assert_eq!(set(&mut d, "autoOff", json!({"on": true, "index": 3})), [encode([0x01, 0x86], &[1, 3], true)]);
        let mut a = driver(A3936);
        assert_eq!(set(&mut a, "gaming", json!(true)), [encode([0x01, 0x87], &[1], true)]);
        assert!(set(&mut a, "leak", json!(true)).is_empty());
    }

    #[test]
    fn the_cycle_needs_two_modes() {
        let mut d = driver(A3957);
        assert_eq!(set(&mut d, "cycle", json!({"anc": true, "transparency": true, "normal": false})), [encode([0x06, 0x82], &[3], true)]);
        assert!(set(&mut d, "cycle", json!({"anc": false, "transparency": true, "normal": false})).is_empty());
    }

    #[test]
    fn a_button_write_keeps_the_other_nibble() {
        let mut d = driver(A3957);
        // Left double press: action byte 0x32 (2 connected, 3 alone). Next song on the connected nibble.
        assert_eq!(set(&mut d, "button", json!({"press": "double", "side": 0, "action": 3})), [encode([0x04, 0x81], &[0, 0, 0x33], true)]);
        // Off on the A3957 is action 0xF.
        assert_eq!(set(&mut d, "button", json!({"press": "double", "side": 0, "action": null})), [encode([0x04, 0x81], &[0, 0, 0x3F], true)]);
        assert_eq!(d.state_json()["buttons"][2]["action"], Value::Null);
    }

    #[test]
    fn a3936_buttons_go_off_and_on_in_pairs() {
        let mut d = driver(A3936);
        // Left double press: on 0x11, action 0x63. Off turns both sides off with 04 83.
        let s = set(&mut d, "button", json!({"press": "double", "side": 0, "action": null}));
        assert_eq!(s, [encode([0x04, 0x83], &[0, 0, 0x10], true), encode([0x04, 0x83], &[1, 0, 0x10], true)]);
        // On again: the other side comes back with its own last action.
        let s = set(&mut d, "button", json!({"press": "double", "side": 1, "action": 4}));
        assert_eq!(s, [encode([0x04, 0x81], &[1, 0, 0x64], true), encode([0x04, 0x83], &[1, 0, 0x11], true),
            encode([0x04, 0x81], &[0, 0, 0x63], true), encode([0x04, 0x83], &[0, 0, 0x11], true)]);
        // Single press does not offer the noise mode action.
        assert!(set(&mut d, "button", json!({"press": "single", "side": 0, "action": 4})).is_empty());
    }

    #[test]
    fn eq_writes_the_model_body() {
        let mut d = driver(A3957);
        let s = set(&mut d, "eq", json!({"preset": CUSTOM_EQ, "bands": [10, 20, 30, 40, 50, 60, 70, 80]}));
        let body = &s[0][9..s[0].len() - 1];
        assert_eq!(&s[0][5..7], &[0x03, 0x87]);
        assert_eq!(&body[..2], &[0xFE, 0xFE]);
        assert_eq!(body[4], 130);
        assert_eq!(body.len(), 2 + 2 + 20 + 3 + 20 + 4 + 1 + 20 + 40 + 2);
        assert_eq!(d.state_json()["eqBands"], json!([10, 20, 30, 40, 50, 60, 70, 80]));
        let mut a = driver(A3936);
        let s = set(&mut a, "eq", json!({"preset": 1}));
        let body = &s[0][9..s[0].len() - 1];
        assert_eq!(body.len(), 2 + 2 + 20 + 3 + 20 + 4 + 1 + 20 + 20);
        assert_eq!(a.state_json()["eqPreset"], 1);
    }

    #[test]
    fn drc_matches_openscq30() {
        // A flat curve stays flat; the tail keeps 0 and -120.
        assert_eq!(apply_drc(&[0, 0, 0, 0, 0, 0, 0, 0, 0, -120]), [0, 0, 0, 0, 0, 0, 0, 0, 0, -120]);
        // One band: band 0 at +4 dB. OpenSCQ30 divides the sums by 10: 1.26 x 4 = 5.04 gives 5 (0.5 dB),
        // -0.71 x 0.85 x 4 = -2.41 gives -2.
        let v = apply_drc(&[40, 0, 0, 0, 0, 0, 0, 0, 0, -120]);
        assert_eq!(v[0], 5);
        assert_eq!(v[1], -2);
    }

    #[test]
    fn reads_the_device_list() {
        let mut d = driver(A3936);
        let mut body = vec![1, 1];
        for (on, mac, name) in [(1u8, [1u8, 2, 3, 4, 5, 6], "Phone"), (0, [9, 9, 9, 9, 9, 9], "PC")] {
            body.push(8 + name.len() as u8);
            body.push(on);
            body.extend_from_slice(&mac);
            body.extend_from_slice(name.as_bytes());
        }
        d.on_bytes(7, &inbound([0x0B, 0x01], &body));
        let s = d.state_json();
        assert_eq!(s["devices"], json!([{"mac": "01:02:03:04:05:06", "name": "Phone", "connected": true},
            {"mac": "09:09:09:09:09:09", "name": "PC", "connected": false}]));
        assert_eq!(sent(&d.command(8, Cmd::Device("connect".into(), "09:09:09:09:09:09".into()))), [encode([0x0B, 0x82], &[9; 6], true)]);
    }

    #[test]
    fn demo_gives_the_recorded_state() {
        assert_eq!(demo("A3957").unwrap().state_json()["ancLevel"], 5);
        assert!(demo("A9999").is_none());
    }

    #[test]
    fn pushes_update_battery_and_noise() {
        let mut d = driver(A3936);
        d.on_bytes(6, &inbound(msg::BATTERY_LEVEL, &[2, 4]));
        d.on_bytes(7, &inbound(msg::SOUND_MODES, &[1, 0x30, 0, 1, 0, 0]));
        let c = d.common();
        assert_eq!(c.battery[0].map(|b| (b.value, b.charging)), Some((2, true)));
        assert_eq!(c.noise.as_deref(), Some(NOISE_TRANSPARENCY));
        // A push of another length is refused, as OpenSCQ30 does.
        d.on_bytes(8, &inbound(msg::SOUND_MODES, &[0, 0x30, 0, 1]));
        assert_eq!(d.common().noise.as_deref(), Some(NOISE_TRANSPARENCY));
        d.on_bytes(8, &inbound(msg::SOUND_MODES, &[0, 0x20, 1, 0, 1, 0]));
        assert_eq!(d.state_json()["ancLevel"], 2);
        assert_eq!(d.state_json()["windNoise"], true);
        d.on_bytes(9, &inbound([0x01, 0x11], &[1]));
        assert_eq!(d.common().low_latency, Some(true));
    }
}
