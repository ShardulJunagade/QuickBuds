//! The Soundcore driver: sans-IO, the same shape as the OPPO driver (docs/CORE-API.md). The models, the commands
//! and the equalizer math come from OpenSCQ30 (`lib/src/devices/soundcore`). Copyright 2026 Kyle Scheuing,
//! GPL-3.0-or-later.
//!
//! Connect: the driver asks for the full state (`01 01`). The serial number in the answer names the model (it
//! starts with the model number); a model without a serial number in its state is found by its Bluetooth name.
//! The model's field table ([`super::models`]) gives the offset of each field. The driver keeps the raw state
//! bytes: a write changes its field there and sends the model's command, so every other field keeps its value.
//! Pushes update the bytes after that. docs/SOUNDCORE-PROTOCOL.md has every field and command.

use super::models::{self, Act, Bat, Buttons, Cycle, En, EqW, Hid, Kind, Model, Off, Scale, Sk};
use super::msg::{self, ambient, Msg};
use super::packet::{encode, encode_in, Frame, Packet, Reader};
use crate::model::{Common, Level};
use serde_json::{json, Value};

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

/// The noise control keys this driver gives (`Common::noise`).
pub const NOISE_OFF: &str = "Off";
pub const NOISE_TRANSPARENCY: &str = "Transparency";
pub const NOISE_ANC: &str = "ANC";

/// The preset id of the custom equalizer.
pub const CUSTOM_EQ: u16 = 0xFEFE;
/// The custom bands: frequencies in Hz, limits in tenths of a dB.
pub const EQ_HZ: [u16; 8] = [100, 200, 400, 800, 1600, 3200, 6400, 12800];
pub const EQ_MIN: i16 = -120;
pub const EQ_MAX: i16 = 134;
/// Limit high volume: the dB range.
pub const VOLUME_LIMIT_DB: (u8, u8) = (75, 100);

/// Re-ask for the state when no answer came this long after the connect.
const STATE_RETRY_MS: u64 = 1500;
const NO_ACTION: u8 = 0xF;

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
    model: Option<&'static Model>,
    /// The fields of the state packet: key, offset, length.
    at: Vec<(&'static str, usize, usize)>,
    /// The state packet body, kept current by pushes and writes.
    state: Vec<u8>,
    /// The 8 bands of a model whose state holds only the preset id (A3909): the last ones written.
    eq_bands: [i16; 8],
    pub firmware: Vec<String>,
    pub serial: Option<String>,
    /// Both buds are connected to each other: the buttons' and the cycle's "connected" value applies.
    tws: bool,
    pub devices: Vec<Device>,
    ready: bool,
    /// A button reset waits for its answer, then the driver reads the state again.
    reset_pending: bool,
    retry_at: Option<u64>,
    out: Vec<Effect>,
}

fn level(raw: u8, s: Scale) -> Level {
    Level { value: raw.saturating_add(s.offset).min(s.max), max: s.max, charging: false }
}

/// OpenSCQ30 `VolumeAdjustments::apply_drc`: the band values the buds play, from the set ones. Bands 9 and 10
/// get their defaults, 0 and -12 dB.
fn apply_drc(v: &[i16]) -> Vec<i16> {
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
    const DEFAULTS: [i16; 10] = [0, 0, 0, 0, 0, 0, 0, 0, 0, -120];
    let a: Vec<f64> = v.iter().take(8).map(|&x| x as f64 / 10.0).collect();
    let lows = a[2] * 0.81 * S;
    let highs = a[5] * 0.81 * S;
    (0..v.len()).map(|band| {
        if band >= 8 { return DEFAULTS.get(band).copied().unwrap_or(0) }
        let sum = (0..8).fold(0.0, |acc, i| {
            if (band == 1 || band == 3) && i == 2 { acc - lows }
            else if (band == 4 || band == 6) && i == 5 { acc - highs }
            else { acc + a[i] * C[band][i] }
        });
        (sum / 10.0 * 10.0).round() as i16
    }).collect()
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

fn noise_key(ambient_byte: u8) -> &'static str {
    match ambient_byte {
        ambient::NOISE_CANCELING => NOISE_ANC,
        ambient::TRANSPARENCY => NOISE_TRANSPARENCY,
        _ => NOISE_OFF,
    }
}

/// A HearID band group as OpenSCQ30 writes it, one channel of `bands` bytes at a time: an absent channel (first
/// byte 0xFF) is all 0xFF, or 0xFF with a last 0 when `zero_last`.
fn hid_bands(raw: &[u8], bands: usize, zero_last: bool) -> Vec<u8> {
    raw.chunks(bands).flat_map(|c| {
        if c[0] == 0xFF {
            let mut v = vec![0xFF; c.len()];
            if zero_last { if let Some(l) = v.last_mut() { *l = 0 } }
            v
        } else {
            c.to_vec()
        }
    }).collect()
}

impl Driver {
    pub fn new() -> Self { Driver { reader: Reader::new(true), ..Default::default() } }

    pub fn common(&self) -> Common { self.common.clone() }
    pub fn model(&self) -> Option<&'static Model> { self.model }

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

    /// The first field with this key: offset, length.
    fn field(&self, key: &str) -> Option<(usize, usize)> {
        self.at.iter().find(|f| f.0 == key).map(|f| (f.1, f.2))
    }

    fn byte(&self, at: usize) -> u8 { self.state.get(at).copied().unwrap_or(0) }
    fn key_byte(&self, key: &str) -> Option<u8> { self.field(key).map(|(at, _)| self.byte(at)) }

    /// One packet. True when the state changed.
    fn packet(&mut self, p: Packet) -> bool {
        if p.cmd == msg::REQUEST_STATE {
            return self.state(&p.body);
        }
        let Some(m) = self.model else { return false };
        match (p.cmd, p.body.len()) {
            // The model pushes its whole sound mode group. Another length has another layout: OpenSCQ30's parser
            // refuses it too.
            (msg::SOUND_MODES, n) => {
                let Some((at, len)) = self.field("modes") else { return false };
                if n != len {
                    self.note(format!("sound modes push of {n} bytes, the model has {len}"));
                    return false;
                }
                self.state[at..at + n].copy_from_slice(&p.body);
                self.refresh();
                return true;
            }
            ([0x04, 0x82], 0) if self.reset_pending => {
                self.reset_pending = false;
                self.send(msg::REQUEST_STATE, &[]);
                return false;
            }
            // Pushes of one switch: gaming mode, LDAC, voice prompt.
            ([0x01, 0x11], 1) | ([0x01, 0x7F], 1) | ([0x01, 0x10], 1) => {
                let key = match p.cmd[1] { 0x11 => "gaming", 0x7F => "ldac", _ => "voicePrompt" };
                let Some((at, _)) = self.field(key) else { return false };
                self.state[at] = p.body[0];
                self.refresh();
                return true;
            }
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
                let n = if matches!(m.battery.0, Bat::Single | Bat::SingleLevel) { 1 } else { 2 };
                for (i, raw) in v.into_iter().take(n).enumerate() {
                    let charging = self.common.battery[i].is_some_and(|b| b.charging);
                    self.common.battery[i] = Some(Level { charging, ..level(raw, m.battery.1) });
                }
            }
            Msg::Charging(v) => for (i, c) in v.into_iter().take(2).enumerate() {
                if let Some(b) = self.common.battery[i].as_mut() { b.charging = c; }
            },
            // Handled above.
            Msg::SoundModes(_) | Msg::Ldac(_) => return false,
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

    /// The full state packet: the serial number, else the Bluetooth name, picks the model.
    fn state(&mut self, b: &[u8]) -> bool {
        self.retry_at = None;
        let Some(m) = models::by_serial(b).or_else(|| models::by_name(&self.common.name).filter(|m| models::layout(m, b).is_some())) else {
            self.note(format!("state packet of an unknown model or a short one: {} bytes, name {:?}", b.len(), self.common.name));
            return false;
        };
        let Some(at) = models::layout(m, b) else { return false };
        self.model = Some(m);
        self.at = at;
        self.state = b.to_vec();
        if let Some((s, len)) = self.field("serial") {
            self.serial = std::str::from_utf8(&b[s..s + len]).ok().map(str::to_string);
        }
        self.firmware = match self.field("fw") {
            Some((f, len)) => b[f..f + len].chunks(5).filter_map(msg::firmware).collect(),
            None => Vec::new(),
        };
        self.tws = self.field("tws").is_some_and(|(t, _)| b[t + 1] != 0);
        // The serial names the model; else the name does, when the table pairs each name with one id.
        let by_name = m.names.iter().position(|n| n.eq_ignore_ascii_case(self.common.name.trim()))
            .filter(|_| m.names.len() == m.ids.len()).map(|i| &m.ids[i]);
        let id = self.serial.as_deref().and_then(|s| m.ids.iter().find(|id| s.starts_with(**id))).or(by_name).unwrap_or(&m.ids[0]);
        self.common.model_id = Some(format!("{}{id}", if id.starts_with('1') { "D" } else { "A" }));
        self.read_battery();
        let noise: Vec<String> = self.mode_field("noise").map_or(Vec::new(), |f| {
            [(ambient::NORMAL, NOISE_OFF), (ambient::TRANSPARENCY, NOISE_TRANSPARENCY), (ambient::NOISE_CANCELING, NOISE_ANC)]
                .iter().filter(|(v, _)| f.values.contains(v)).map(|(_, k)| k.to_string()).collect()
        });
        self.common.noise_modes = noise;
        let has = |k: &str| m.sets.iter().any(|s| s.key == k) && self.field(k).is_some();
        let mut f: std::collections::BTreeSet<&'static str> = Default::default();
        if !self.common.noise_modes.is_empty() { f.insert("anc"); }
        if has("gaming") { f.insert("game"); }
        if m.eq.is_some() { f.insert("eq"); }
        if !m.buttons.is_empty() || m.cycle != Cycle::No { f.insert("controls"); }
        if m.dual && self.field("dual").is_some() { f.insert("dual"); }
        if has("ldac") { f.insert("hires"); }
        if has("wearDetection") { f.insert("wear"); }
        self.common.features = f;
        if let Some(e) = m.eq.filter(|e| e.preset_only) {
            let id = self.eq_preset();
            self.eq_bands = e.presets.iter().find(|p| p.1 == id).map_or([0; 8], |p| p.2);
        }
        self.refresh();
        if m.dual && self.key_byte("dual").is_some_and(|d| d != 0) { self.send([0x0B, 0x01], &[]); }
        if !self.ready {
            self.ready = true;
            self.out.push(Effect::Ready);
        }
        true
    }

    fn read_battery(&mut self) {
        let Some(m) = self.model else { return };
        let Some((at, _)) = self.field("battery") else { return };
        let b: Vec<u8> = (0..4).map(|i| self.byte(at + i)).collect();
        let (kind, s) = m.battery;
        let mut out = [None, None, None];
        match kind {
            Bat::Single => out[0] = Some(Level { charging: b[1] == 1, ..level(b[0], s) }),
            Bat::SingleLevel => out[0] = Some(level(b[0], s)),
            Bat::Dual => for i in 0..2 { out[i] = Some(Level { charging: b[2 + i] == 1, ..level(b[i], s) }) },
            Bat::DualLevel => for i in 0..2 { out[i] = Some(level(b[i], s)) },
        }
        self.common.battery = out;
        if let (Some(s), Some(c)) = (m.case, self.key_byte("case")) {
            self.common.battery[2] = Some(level(c, s));
        }
    }

    /// The shared state from the raw bytes.
    fn refresh(&mut self) {
        if let Some(n) = self.mode_value("noise") { self.common.noise = Some(noise_key(n).into()); }
        if self.model.is_some_and(|m| m.sets.iter().any(|s| s.key == "gaming")) {
            self.common.low_latency = self.key_byte("gaming").map(|g| g != 0);
        }
    }

    // ---- Sound modes ----

    fn mode_field(&self, key: &str) -> Option<&'static models::Sf> {
        self.model?.modes?.fields.iter().find(|f| f.key == key)
    }

    /// A sound mode field's value, or None when the model lacks it (or the A3954 slider is on the other side).
    fn mode_value(&self, key: &str) -> Option<u8> {
        let f = self.mode_field(key)?;
        let (at, _) = self.field("modes")?;
        let b = self.byte(at + f.at);
        match f.kind {
            Sk::Byte => Some(b),
            Sk::Hi => Some(b >> 4),
            Sk::Lo => Some(b & 0x0F),
            Sk::Bit0 => Some(b & 1),
            Sk::Bit1 => Some((b >> 1) & 1),
            Sk::SliderAnc => (1..=5).contains(&b).then(|| 6 - b),
            Sk::SliderTransparency => (7..=11).contains(&b).then(|| b - 6),
        }
    }

    /// The field applies now: each parent holds.
    fn mode_shown(&self, f: &models::Sf) -> bool {
        f.parent.iter().all(|(k, vals)| self.mode_value(k).is_some_and(|v| vals.contains(&v)))
    }

    /// Writes one sound mode field; the other fields keep their values.
    fn set_mode(&mut self, key: &str, value: u8) {
        let (Some(m), Some(f), Some((at, len))) = (self.model, self.mode_field(key), self.field("modes")) else {
            return self.note(format!("{key}: no such sound mode"));
        };
        let modes = m.modes.expect("mode field");
        if !f.values.contains(&value) { return self.note(format!("{key} {value} not offered")); }
        if modes.enforce && !self.mode_shown(f) { return self.note(format!("{key}: its noise control mode is not on")); }
        let i = at + f.at;
        let b = self.byte(i);
        self.state[i] = match f.kind {
            Sk::Byte => value,
            Sk::Hi => (value << 4) | (b & 0x0F),
            Sk::Lo => (b & 0xF0) | value,
            Sk::Bit0 => (b & !1) | value,
            Sk::Bit1 => (b & !2) | (value << 1),
            Sk::SliderAnc => 6 - value,
            Sk::SliderTransparency => 6 + value,
        };
        if let Some(mi) = modes.mirror { self.state[at + mi] = self.byte(at); }
        for &(fi, fv) in modes.fixed { self.state[at + fi] = fv; }
        let body = self.state[at..at + len].to_vec();
        self.send(msg::SET_SOUND_MODES, &body);
        // The buds push `06 01` when the mode changed; shown at once, as the phone app does.
        self.refresh();
    }

    fn set_noise(&mut self, key: &str) {
        let value = match key {
            NOISE_OFF => ambient::NORMAL,
            NOISE_TRANSPARENCY => ambient::TRANSPARENCY,
            NOISE_ANC => ambient::NOISE_CANCELING,
            _ => return self.note(format!("noise mode {key} unknown")),
        };
        if self.model.is_none() { return self.note("noise mode before the state".into()) }
        self.set_mode("noise", value);
    }

    // ---- Buttons ----

    /// The offsets of a button: its "on" byte (when it has one) and its action byte.
    fn button_at(&self, g: &Buttons, i: usize) -> Option<(Option<usize>, usize)> {
        let segs: Vec<(usize, usize)> = self.at.iter().filter(|f| f.0 == g.key).map(|f| (f.1, f.2)).collect();
        let mut rel: usize = g.order[..i].iter().map(|&(p, _)| g.presses[p].len()).sum();
        let p = &g.presses[g.order[i].0];
        let abs = |mut r: usize| -> Option<usize> {
            for &(s, len) in &segs { if r < len { return Some(s + r) } r -= len; }
            None
        };
        let on = if p.en == En::No { None } else { let a = abs(rel)?; rel += 1; Some(a) };
        Some((on, abs(rel)?))
    }

    /// The part of a byte that applies now: low nibble while both buds are connected, high nibble otherwise.
    fn nibble(&self, b: u8) -> u8 { if self.tws { b & 0x0F } else { b >> 4 } }
    fn with_nibble(&self, b: u8, v: u8) -> u8 { if self.tws { (b & 0xF0) | (v & 0x0F) } else { (b & 0x0F) | (v << 4) } }

    /// A button's action id, or None when it is off.
    fn action(&self, g: &Buttons, i: usize) -> Option<u8> {
        let p = &g.presses[g.order[i].0];
        let (on_at, at) = self.button_at(g, i)?;
        let action = match p.act { Act::Tws => self.nibble(self.byte(at)), Act::One => self.byte(at) };
        let on = match (on_at, p.en) {
            (Some(o), En::One) if !g.ignore_enabled => self.byte(o) != 0,
            (Some(o), En::Tws) if !g.ignore_enabled => self.nibble(self.byte(o)) == 1,
            _ => action != NO_ACTION,
        };
        on.then_some(action)
    }

    /// Changes one button in the state copy (OpenSCQ30 `ButtonStatus::with_current_action_id`). No write.
    fn put_button(&mut self, g: &Buttons, i: usize, action: Option<u8>) {
        let p = &g.presses[g.order[i].0];
        let Some((on_at, at)) = self.button_at(g, i) else { return };
        // With the "on" byte ignored, off is action 0xF and the button stays on.
        let (action, on) = match action {
            None if on_at.is_none() || g.ignore_enabled => (Some(NO_ACTION), true),
            a => (a, a.is_some()),
        };
        if let Some(a) = action {
            self.state[at] = match p.act { Act::Tws => self.with_nibble(self.byte(at), a), Act::One => a };
        }
        if let Some(o) = on_at {
            self.state[o] = match p.en { En::Tws => self.with_nibble(self.byte(o), on as u8), _ => on as u8 };
        }
    }

    fn set_button(&mut self, v: &Value) {
        let Some(m) = self.model else { return self.note("button before the state".into()) };
        let gkey = v["group"].as_str().unwrap_or("buttons");
        let Some(g) = m.buttons.iter().find(|g| g.key == gkey) else { return self.note(format!("button group {gkey} unknown")) };
        let side = v["side"].as_u64().filter(|&s| s <= 1).map(|s| s as u8);
        let pi = v["press"].as_str().and_then(|k| g.presses.iter().position(|p| p.key == k));
        let (Some(side), Some(pi)) = (side, pi) else { return self.note(format!("button {v} unknown")) };
        let Some(i) = g.order.iter().position(|&o| o == (pi, side)) else { return self.note(format!("button {v} absent")) };
        let p = g.presses[pi];
        let action = v["action"].as_u64().map(|a| a as u8);
        match action {
            Some(a) if !p.actions.iter().any(|x| x.0 == a) => return self.note(format!("button action {a} not offered")),
            None if p.off == Off::No => return self.note("this button cannot be off".into()),
            _ => {}
        }
        let before: Vec<Option<(Option<u8>, u8)>> = (0..g.order.len())
            .map(|j| self.button_at(g, j).map(|(o, a)| (o.map(|o| self.byte(o)), self.byte(a)))).collect();
        self.put_button(g, i, action);
        // One side off turns the other off; one side on turns the other on with its last action.
        if p.off == Off::Pair {
            if let Some(j) = g.order.iter().position(|&o| o == (pi, 1 - side)) {
                match (action, self.action(g, j)) {
                    (None, Some(_)) => self.put_button(g, j, None),
                    (Some(a), None) => {
                        let at = self.button_at(g, j).map(|x| x.1).unwrap_or(0);
                        let last = match p.act { Act::Tws => self.nibble(self.byte(at)), Act::One => self.byte(at) };
                        let keep = if p.actions.iter().any(|x| x.0 == last) { last } else { a };
                        self.put_button(g, j, Some(keep));
                    }
                    _ => {}
                }
            }
        }
        let changed: Vec<usize> = (0..g.order.len()).filter(|&j| {
            self.button_at(g, j).map(|(o, a)| (o.map(|o| self.byte(o)), self.byte(a))) != before[j]
        }).collect();
        if g.set_all && changed.len() > 1 {
            let body: Vec<u8> = self.at.iter().filter(|f| f.0 == g.key).flat_map(|f| self.state[f.1..f.1 + f.2].to_vec()).collect();
            return self.send([0x04, 0x84], &body);
        }
        for j in changed {
            let (s, id) = (g.order[j].1, g.presses[g.order[j].0].id);
            let Some((on_at, at)) = self.button_at(g, j) else { continue };
            let Some((on_before, act_before)) = before[j] else { continue };
            if self.byte(at) != act_before { let b = self.byte(at); self.send(g.cmd, &[s, id, b]); }
            if let Some(o) = on_at {
                if Some(self.byte(o)) != on_before { let b = self.byte(o); self.send([0x04, 0x83], &[s, id, b]); }
            }
        }
    }

    // ---- Equalizer ----

    fn eq_preset(&self) -> u16 {
        self.field("eq").map_or(0, |(at, _)| u16::from_le_bytes([self.byte(at), self.byte(at + 1)]))
    }

    fn eq_value(&self, whole_db: bool, b: u8) -> i16 { if whole_db { (b as i16 - 12) * 10 } else { b as i16 + EQ_MIN } }
    fn eq_byte(whole_db: bool, v: i16) -> u8 {
        if whole_db { ((v / 10).clamp(-12, 12) + 12) as u8 } else { (v.clamp(EQ_MIN, EQ_MAX) - EQ_MIN) as u8 }
    }

    /// The 8 shown bands, in tenths of a dB.
    fn eq_bands(&self) -> [i16; 8] {
        let (Some(e), Some((at, _))) = (self.model.and_then(|m| m.eq), self.field("eq")) else { return [0; 8] };
        if e.preset_only { return self.eq_bands }
        std::array::from_fn(|i| self.eq_value(e.whole_db, self.byte(at + 2 + i)))
    }

    fn set_eq(&mut self, v: &Value) {
        let Some(m) = self.model else { return self.note("eq before the state".into()) };
        let Some(e) = m.eq else { return self.note("eq: no equalizer".into()) };
        let Some(id) = v["preset"].as_u64().map(|x| x as u16) else { return self.note(format!("eq {v} has no preset")) };
        let custom = id == CUSTOM_EQ;
        let shown: [i16; 8] = if custom {
            if !e.custom { return self.note("eq: no custom curve on this model".into()) }
            let Some(b) = v["bands"].as_array().filter(|a| a.len() == 8)
                .and_then(|a| a.iter().map(|x| x.as_i64().map(|y| (y as i16).clamp(EQ_MIN, EQ_MAX))).collect::<Option<Vec<_>>>())
            else { return self.note(format!("eq bands {v} need 8 values")) };
            b.try_into().expect("8 values")
        } else {
            match e.presets.iter().find(|p| p.1 == id) { Some(p) => p.2, None => return self.note(format!("eq preset {id} unknown")) }
        };
        // Bands 9 and 10: 0 and -12 dB for a preset, 0 and 0 for the custom curve (OpenSCQ30 `InvisibleBandsMode`).
        let all: Vec<i16> = (0..e.bands).map(|i| match i { 0..=7 => shown[i], 8 => 0, _ => if custom { 0 } else { -120 } }).collect();
        let bytes: Vec<u8> = all.iter().map(|&x| Self::eq_byte(e.whole_db, x)).collect();
        let drc: Vec<u8> = apply_drc(&all).iter().map(|&x| Self::eq_byte(e.whole_db, x)).collect();
        // The state copy: preset id and every channel the state holds.
        let (at, len) = self.field("eq").expect("eq field");
        self.state[at..at + 2].copy_from_slice(&id.to_le_bytes());
        for ch in 0..(len - 2) / e.bands { self.state[at + 2 + ch * e.bands..at + 2 + (ch + 1) * e.bands].copy_from_slice(&bytes); }
        if e.preset_only { self.eq_bands = shown; }
        let channels = e.channels;
        let eq_all: Vec<u8> = (0..channels).flat_map(|_| bytes.clone()).collect();
        let drc_all: Vec<u8> = (0..channels).flat_map(|_| drc.clone()).collect();
        let mut body = id.to_le_bytes().to_vec();
        let cmd = match (e.write, e.hid) {
            (EqW::Plain, _) => { body.extend(&eq_all); [0x02, 0x81] }
            (EqW::Drc, _) => { body.extend(&eq_all); body.extend(&drc_all); [0x02, 0x83] }
            (EqW::Raw83, _) => { body.extend(&eq_all); [0x02, 0x83] }
            (EqW::A3876, _) => { body.extend(&bytes); body.extend(&drc); [0x02, 0x83] }
            (w, Some(h)) => return self.set_eq_hearid(w, h, id, &bytes, &eq_all, &drc, &drc_all),
            (_, None) => return self.note("eq: the model table lacks its HearID block".into()),
        };
        self.send(cmd, &body);
    }

    /// The `03 86` / `03 87` equalizer writes. HearID goes off, so the bands apply.
    #[allow(clippy::too_many_arguments)]
    fn set_eq_hearid(&mut self, w: EqW, h: Hid, id: u16, bytes: &[u8], eq_all: &[u8], drc: &[u8], drc_all: &[u8]) {
        let Some(e) = self.model.and_then(|m| m.eq) else { return };
        let Some((ho, _)) = self.field("hearId") else { return self.note("eq: no HearID field".into()) };
        self.state[ho] = 0;
        let s = &self.state;
        let raw = |at: usize, n: usize| s[ho + at..ho + at + n].to_vec();
        let genre: Vec<u8> = match h.genre { Some((g, 1)) => vec![s[ho + g], 0], Some((g, _)) => raw(g, 2), None => vec![0, 0] };
        let hv = |zero: bool| hid_bands(&raw(h.bands, h.n), e.bands, zero);
        let hc = |zero: bool| h.custom.map_or(vec![0xFF; h.n], |c| hid_bands(&raw(c, h.n), e.bands, zero));
        let time = raw(h.time, 4);
        let kind = h.kind.map_or(0, |k| s[ho + k]);
        let age = self.key_byte("age");
        let mut body = id.to_le_bytes().to_vec();
        let cmd = match w {
            EqW::HearId { force } => {
                let on = age.is_some_and(|a| a != 0xFF) || force;
                let wave = vec![0xFF; eq_all.len()];
                if on { body.extend(&genre); }
                body.extend(eq_all);
                body.push(if on { self.key_byte("gender").unwrap_or(0) } else { 0xFF });
                body.push(if on { age.unwrap_or(0) } else { 0xFF });
                body.push(0);
                body.extend(if on { hv(false) } else { wave.clone() });
                body.extend(if on { time } else { vec![0; 4] });
                body.push(kind);
                body.extend(if on { hc(false) } else { wave });
                body.extend(drc_all);
                if on { [0x03, 0x87] } else { [0x03, 0x86] }
            }
            EqW::A3035 => {
                body.extend(&genre); body.extend(eq_all); body.extend([0xFF, 0xFF, 0]);
                body.extend(hv(true)); body.extend(&time); body.push(kind); body.extend(hc(true)); body.extend(drc_all); body.push(0);
                [0x03, 0x87]
            }
            EqW::A3040 => {
                // Channel 1 twice, the custom bands of the left side twice, the DRC bands twice.
                let left: Vec<u8> = hc(false)[..e.bands].to_vec();
                body.extend(&genre); body.extend(bytes); body.extend(bytes); body.extend([0, 1, 0]);
                body.extend(hv(false)); body.extend(&time); body.push(kind); body.extend(&left); body.extend(&left);
                body.extend(drc); body.extend(drc); body.push(0);
                [0x03, 0x87]
            }
            EqW::A3062 => {
                body.extend(&genre); body.extend(eq_all); body.extend([0, 0, 0]);
                body.extend(hv(false)); body.extend(&time); body.push(kind); body.extend(hc(false)); body.extend(eq_all); body.push(0);
                [0x03, 0x87]
            }
            EqW::A3947 => {
                body.extend(&genre); body.extend(eq_all); body.extend([0, 0, 0]);
                body.extend(hv(false)); body.extend(&time); body.push(kind); body.extend(hc(false));
                for c in drc_all.chunks(e.bands) { let mut c = c.to_vec(); if let Some(x) = c.get_mut(9) { *x = 0 } body.extend(c); }
                body.push(0);
                [0x03, 0x87]
            }
            EqW::A3953 => {
                let init = self.key_byte("hidStatus").is_some_and(|b| b != 255 && b != 254);
                body.extend(&genre); body.extend(eq_all); body.extend(if init { [0, 0] } else { [0xFF, 0xFF] }); body.push(0);
                body.extend(hv(true)); body.extend(&time); body.push(kind); body.extend(hc(true)); body.extend(drc_all); body.push(0);
                [0x03, 0x87]
            }
            EqW::A3954 => {
                body.extend(&genre); body.extend(eq_all); body.extend([0, 0, 0]);
                body.extend(hv(false)); body.extend(&time); body.push(kind); body.extend(hc(false));
                for &x in eq_all { body.extend([x, 0]); }
                body.extend([0, 0]);
                [0x03, 0x87]
            }
            EqW::D1402 => {
                body.extend(&genre); body.extend(eq_all); body.push(self.key_byte("eqByte").unwrap_or(0)); body.push(age.unwrap_or(0)); body.push(0);
                body.extend(hv(true)); body.extend(&time); body.push(kind); body.extend(hc(true)); body.extend(drc_all); body.push(0);
                [0x03, 0x87]
            }
            EqW::Plain | EqW::Drc | EqW::Raw83 | EqW::A3876 => unreachable!("handled in set_eq"),
        };
        self.send(cmd, &body);
    }

    // ---- Settings ----

    fn set(&mut self, key: &str, v: &Value) {
        let Some(m) = self.model else { return self.note(format!("{key} before the state")) };
        if self.mode_field(key).is_some() {
            let value = v.as_bool().map(u8::from).or_else(|| v.as_u64().map(|x| x as u8));
            return match value { Some(x) => self.set_mode(key, x), None => self.note(format!("{key} = {v} not a value")) };
        }
        if let Some(s) = m.sets.iter().find(|s| s.key == key) {
            let Some((at, _)) = self.field(key) else { return self.note(format!("{key}: absent from this state")) };
            let value = match (s.kind, v.as_bool(), v.as_u64()) {
                (Kind::Switch, Some(on), _) => on as u8,
                (Kind::Inverted, Some(on), _) => !on as u8,
                (Kind::Choice(vals), _, Some(x)) if vals.contains(&(x as u8)) => x as u8,
                _ => return self.note(format!("{key} = {v} not offered")),
            };
            self.state[at] = value;
            self.send(s.cmd, &[value]);
            self.refresh();
            return;
        }
        match key {
            "dual" if m.dual => {
                let (Some((at, _)), Some(on)) = (self.field("dual"), v.as_bool()) else { return self.note(format!("dual {v}")) };
                self.state[at] = on as u8;
                self.send([0x0B, 0x84], &[on as u8]);
                if on { self.send([0x0B, 0x01], &[]); } else { self.devices.clear(); }
            }
            "autoOff" if !m.auto_off.is_empty() => {
                let Some((at, _)) = self.field("autoOff") else { return self.note("autoOff: absent from this state".into()) };
                let on = v["on"].as_bool().unwrap_or(self.byte(at) != 0);
                let index = v["index"].as_u64().map_or(self.byte(at + 1), |x| x as u8);
                if index as usize >= m.auto_off.len() { return self.note(format!("autoOff index {index} out of range")); }
                self.state[at] = on as u8;
                self.state[at + 1] = index;
                self.send([0x01, 0x86], &[on as u8, index]);
            }
            "volumeLimit" => {
                let Some((at, _)) = self.field("volumeLimit") else { return self.note("volumeLimit: no such setting".into()) };
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
            "cycle" if m.cycle != Cycle::No => {
                let Some((at, _)) = self.field("cycle") else { return self.note("cycle: absent from this state".into()) };
                let now = match m.cycle { Cycle::Tws => self.nibble(self.byte(at)), _ => self.byte(at) };
                let bit = |k: &str, b: u8| if v[k].as_bool().unwrap_or(now & b != 0) { b } else { 0 };
                let c = bit("anc", 1) | bit("transparency", 2) | bit("normal", 4);
                // The buds need two modes to cycle between.
                if c.count_ones() < 2 { return self.note("cycle needs two modes".into()); }
                let byte = match m.cycle { Cycle::Tws => self.with_nibble(self.byte(at), c), _ => c };
                self.state[at] = byte;
                self.send([0x06, 0x82], &[byte]);
            }
            "button" => self.set_button(v),
            "eq" => self.set_eq(v),
            _ => self.note(format!("setting {key} = {v} unknown")),
        }
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

    /// Every setting of the connected model, as the apps read it. Keys of absent features are left out.
    ///
    /// `modes`: the sound mode fields, each with its values (empty: read only) and `shown` (its parent mode is on).
    /// `switches` and `choices`: the one-byte settings by key. The home rows read `gaming`, `ldac` and `dual`.
    pub fn state_json(&self) -> Value {
        let Some(m) = self.model else { return json!({}) };
        let mut v = json!({
            "model": self.common.model_id,
            "name": m.names[0],
            "serial": self.serial,
            "firmware": self.firmware,
        });
        let o = v.as_object_mut().expect("object");
        if let Some(modes) = m.modes {
            let list: Vec<Value> = modes.fields.iter().filter_map(|f| {
                let value = self.mode_value(f.key);
                let labels: serde_json::Map<String, Value> = f.labels.iter().map(|(v, n)| (v.to_string(), json!(n))).collect();
                // The A3954 slider holds one side at a time: the other side has no value.
                if value.is_none() && matches!(f.kind, Sk::SliderAnc | Sk::SliderTransparency) {
                    return Some(json!({"key": f.key, "value": Value::Null, "values": f.values, "labels": labels, "shown": false}));
                }
                Some(json!({"key": f.key, "value": value?, "values": f.values, "labels": labels, "shown": self.mode_shown(f)}))
            }).collect();
            o.insert("modes".into(), json!(list));
            if let Some(n) = self.mode_value("noise") { o.insert("noise".into(), json!(n)); }
        }
        if let Some(e) = m.eq {
            o.insert("eqPreset".into(), json!(self.eq_preset()));
            o.insert("eqBands".into(), json!(self.eq_bands()));
            o.insert("eqPresets".into(), json!(e.presets.iter().map(|(name, id, bands)| json!({"id": id, "name": name, "bands": bands})).collect::<Vec<_>>()));
            o.insert("eqHz".into(), json!(EQ_HZ));
            o.insert("eqRange".into(), if e.whole_db { json!([-120, 120]) } else { json!([EQ_MIN, EQ_MAX]) });
            o.insert("eqStep".into(), json!(if e.whole_db { 10 } else { 1 }));
            o.insert("eqCustom".into(), json!(e.custom));
        }
        if !m.buttons.is_empty() {
            let mut list = Vec::new();
            for g in m.buttons {
                for (i, &(pi, side)) in g.order.iter().enumerate() {
                    let p = &g.presses[pi];
                    list.push(json!({
                        "group": g.key, "press": p.key, "side": side, "action": self.action(g, i),
                        "actions": p.actions.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
                        "canOff": p.off != Off::No,
                    }));
                }
            }
            o.insert("buttons".into(), json!(list));
            o.insert("buttonsPaired".into(), json!(m.buttons.iter().any(|g| g.presses.iter().any(|p| p.off == Off::Pair))));
            o.insert("buttonsReset".into(), json!(m.buttons.iter().any(|g| g.key == "buttons" && g.cmd == [0x04, 0x81] && g.presses.len() > 1)));
        }
        if let (Cycle::Bits | Cycle::Tws, Some(c)) = (m.cycle, self.key_byte("cycle")) {
            let c = if m.cycle == Cycle::Tws { self.nibble(c) } else { c };
            o.insert("cycle".into(), json!({ "anc": c & 1 != 0, "transparency": c & 2 != 0, "normal": c & 4 != 0 }));
        }
        let mut switches = Vec::new();
        let mut choices = Vec::new();
        for s in m.sets {
            let Some(b) = self.key_byte(s.key) else { continue };
            match s.kind {
                Kind::Switch | Kind::Inverted => {
                    let on = (b != 0) != (s.kind == Kind::Inverted);
                    if matches!(s.key, "gaming" | "ldac") { o.insert(s.key.into(), json!(on)); } else { switches.push(json!({"key": s.key, "on": on})); }
                }
                Kind::Choice(vals) => choices.push(json!({"key": s.key, "value": b, "values": vals})),
            }
        }
        o.insert("switches".into(), json!(switches));
        o.insert("choices".into(), json!(choices));
        if m.dual {
            if let Some(d) = self.key_byte("dual") {
                o.insert("dual".into(), json!(d != 0));
                o.insert("devices".into(), json!(self.devices.iter().map(|d| json!({"mac": mac_text(&d.mac), "name": d.name, "connected": d.connected})).collect::<Vec<_>>()));
            }
        }
        if let (false, Some((at, _))) = (m.auto_off.is_empty(), self.field("autoOff")) {
            o.insert("autoOff".into(), json!({ "on": self.byte(at) != 0, "index": self.byte(at + 1), "minutes": m.auto_off }));
        }
        if let Some((at, _)) = self.field("volumeLimit") {
            o.insert("volumeLimit".into(), json!({ "on": self.byte(at) != 0, "db": self.byte(at + 1), "rate": self.byte(at + 2), "range": [VOLUME_LIMIT_DB.0, VOLUME_LIMIT_DB.1] }));
        }
        v
    }
}

/// The recorded state packets of the Dev tools demo, by model id: the OpenSCQ30 device fakers
/// ([`super::demo::FAKERS`]) and a recorded Liberty 5 (OpenSCQ30 issue 226).
pub const LIBERTY_5: &str = "00010808000030332E393030332E39303339353746343944384143323033333430302E303002000078787878787878787878FFFFFFFFFFFFFFFFFFFF0001918273778B938F960000918273778B938F9600000000000000918273778B938F960000918273778B938F96000000000A66663233FFFF4444330050000000000033010100010102005A0000010000FF01000001010001FFFFFF03FFFF";

fn unhex(t: &str) -> Vec<u8> { (0..t.len() / 2).filter_map(|i| u8::from_str_radix(t.get(i * 2..i * 2 + 2)?, 16).ok()).collect() }

/// The demo models: (model id, Bluetooth name, state packet in hex). One per model the driver reads.
pub fn demos() -> Vec<(String, &'static str, &'static str)> {
    let mut out: Vec<(String, &'static str, &'static str)> = super::demo::FAKERS.iter().filter_map(|&(_, name, hex)| {
        let mut d = Driver::new();
        let _ = d.connected(0, name.into());
        d.state(&unhex(hex)).then(|| (d.common.model_id.clone().unwrap_or_default(), name, hex))
    }).collect();
    // The Liberty 5 faker packet is shorter than a real one: the recorded packet replaces it.
    out.retain(|x| x.0 != "A3957");
    out.push(("A3957".into(), "Soundcore Liberty 5", LIBERTY_5));
    out.sort();
    out
}

/// A driver that holds a recorded state: its `state_json` is what the buds would give. No link.
pub fn demo(model: &str) -> Option<Driver> {
    let (_, name, hex) = demos().into_iter().find(|d| d.0 == model)?;
    let mut d = Driver::new();
    let _ = d.connected(0, name.into());
    d.state(&unhex(hex)).then_some(())?;
    d.retry_at = None;
    d.out.clear();
    Some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a3936() -> &'static str { super::super::demo::FAKERS.iter().find(|f| f.0 == "a3936").unwrap().2 }

    fn inbound(cmd: [u8; 2], body: &[u8]) -> Vec<u8> { encode_in(cmd, body) }

    fn sent(e: &[Effect]) -> Vec<Vec<u8>> { e.iter().filter_map(|e| match e { Effect::Send(b) => Some(b.clone()), _ => None }).collect() }

    fn driver_named(state: &str, name: &str) -> Driver {
        let mut d = Driver::new();
        d.connected(0, name.into());
        d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(state)));
        d
    }
    fn driver(state: &str) -> Driver { driver_named(state, "x") }

    fn set(d: &mut Driver, k: &str, v: Value) -> Vec<Vec<u8>> { sent(&d.command(6, Cmd::Set(k.into(), v))) }

    fn mode(s: &Value, key: &str) -> Value {
        s["modes"].as_array().unwrap().iter().find(|m| m["key"] == key).map_or(Value::Null, |m| m["value"].clone())
    }
    fn switch(s: &Value, key: &str) -> Value {
        s["switches"].as_array().unwrap().iter().find(|m| m["key"] == key).map_or(Value::Null, |m| m["on"].clone())
    }
    fn button(s: &Value, press: &str, side: u8) -> Value {
        s["buttons"].as_array().unwrap().iter().find(|x| x["press"] == press && x["side"] == side).unwrap()["action"].clone()
    }

    #[test]
    fn every_faker_packet_parses() {
        // Each faker of a model this driver knows gives a model, a serial or a name, and a state.
        let skip = ["a3116", "a3130", "a3958", "a6611"];
        for (file, name, hex) in super::super::demo::FAKERS {
            if skip.contains(&file) { continue }
            let d = driver_named(hex, name);
            let s = d.state_json();
            assert!(d.model().is_some(), "{file}: no model");
            assert!(s["name"].is_string(), "{file}: no state");
            assert!(d.common().battery[0].is_some(), "{file}: no battery");
        }
        assert!(demos().len() >= 30);
    }

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
        let e = d.on_bytes(5, &inbound(msg::REQUEST_STATE, &unhex(a3936())));
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
        assert_eq!(mode(&s, "ancMode"), 1);
        assert_eq!(mode(&s, "transparencyMode"), 0);
        assert_eq!(mode(&s, "windNoise"), 0);
        assert_eq!(s["eqPreset"], 0);
        assert_eq!(switch(&s, "touchTone"), false);
        assert_eq!(s["ldac"], false);
        assert_eq!(s["dual"], true);
        assert_eq!(s["autoOff"]["on"], true);
        assert_eq!(s["autoOff"]["index"], 0);
        assert_eq!(button(&s, "single", 0), Value::Null);
        assert_eq!(button(&s, "single", 1), Value::Null);
        assert_eq!(button(&s, "double", 0), 3);
        assert_eq!(button(&s, "double", 1), 6);
        assert_eq!(button(&s, "long", 0), 4);
        assert_eq!(button(&s, "long", 1), 9);
    }

    #[test]
    fn reads_the_liberty_5_state() {
        let d = driver(LIBERTY_5);
        let c = d.common();
        assert_eq!(c.model_id.as_deref(), Some("A3957"));
        assert_eq!(c.battery[0], Some(Level { value: 9, max: 10, charging: false }));
        assert_eq!(c.battery[2], Some(Level { value: 3, max: 10, charging: false }));
        assert_eq!(c.noise.as_deref(), Some(NOISE_ANC));
        assert_eq!(d.firmware, ["03.90", "03.90"]);
        assert!(c.features.contains("wear"));
        // OpenSCQ30 `a3957.rs` test (issue 226).
        let s = d.state_json();
        assert_eq!(mode(&s, "ancMode"), 0);
        assert_eq!(mode(&s, "ancLevel"), 5);
        assert_eq!(mode(&s, "transport"), 0);
        assert_eq!(mode(&s, "windNoise"), 0);
        assert_eq!(s["gaming"], false);
        assert_eq!(switch(&s, "wearDetection"), true);
        assert_eq!(switch(&s, "leak"), false);
        assert_eq!(switch(&s, "touchTone"), true);
        assert_eq!(switch(&s, "wearingTone"), true);
        assert_eq!(switch(&s, "lowBattery"), true);
        assert_eq!(s["volumeLimit"]["on"], false);
        assert_eq!(s["volumeLimit"]["db"], 90);
        assert_eq!(s["volumeLimit"]["rate"], 0);
        assert_eq!(s["dual"], true);
        assert_eq!(button(&s, "single", 0), 6);
        assert_eq!(button(&s, "double", 0), 2);
        assert_eq!(button(&s, "triple", 0), Value::Null);
        assert_eq!(button(&s, "long", 0), 4);
        assert_eq!(button(&s, "single", 1), 6);
        assert_eq!(button(&s, "double", 1), 3);
        assert_eq!(button(&s, "triple", 1), Value::Null);
        assert_eq!(button(&s, "long", 1), 4);
    }

    #[test]
    fn a_model_without_a_serial_is_found_by_its_name() {
        let f = super::super::demo::FAKERS.iter().find(|f| f.0 == "a3931").unwrap();
        assert!(driver_named(f.2, "Bose").model().is_none());
        let d = driver_named(f.2, "Soundcore Life Dot 2 NC");
        assert_eq!(d.common().model_id.as_deref(), Some("A3931"));
    }

    #[test]
    fn a_noise_write_keeps_the_other_fields() {
        let mut d = driver(a3936());
        let e = d.command(6, Cmd::Anc(NOISE_ANC.into()));
        // A3936 sound modes 02 30 00 01 00 00: only the first byte changes.
        assert_eq!(sent(&e), [encode(msg::SET_SOUND_MODES, &[0x00, 0x30, 0x00, 0x01, 0x00, 0x00], true)]);
        assert_eq!(d.common().noise.as_deref(), Some(NOISE_ANC));
    }

    #[test]
    fn sound_mode_fields_write_their_byte() {
        let mut d = driver(LIBERTY_5);
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
        let mut d = driver(LIBERTY_5);
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
    fn the_a3954_slider_holds_one_side() {
        let f = super::super::demo::FAKERS.iter().find(|f| f.0 == "a3954").unwrap();
        let mut d = driver(f.2);
        d.command(6, Cmd::Anc(NOISE_ANC.into()));
        let s = set(&mut d, "ancLevel", json!(5));
        // Level 5 is slider 1, the far noise cancelling end.
        assert_eq!(s[0][9 + 1], 1);
        assert_eq!(mode(&d.state_json(), "ancLevel"), 5);
        assert_eq!(mode(&d.state_json(), "transparencyLevel"), Value::Null);
    }

    #[test]
    fn reset_reads_the_state_after_the_answer() {
        let mut d = driver(a3936());
        assert_eq!(sent(&d.command(6, Cmd::ResetButtons)), [encode([0x04, 0x82], &[], true)]);
        assert_eq!(sent(&d.on_bytes(7, &inbound([0x04, 0x82], &[]))), [encode(msg::REQUEST_STATE, &[], true)]);
    }

    #[test]
    fn switches_use_the_model_command() {
        let mut d = driver(LIBERTY_5);
        // OpenSCQ30 `gaming_mode_uses_correct_command`: 10 85 on the A3957.
        assert_eq!(set(&mut d, "gaming", json!(true)), [encode([0x10, 0x85], &[1], true)]);
        assert_eq!(d.common().low_latency, Some(true));
        assert_eq!(set(&mut d, "leak", json!(true)), [encode([0x10, 0x86], &[1], true)]);
        assert_eq!(set(&mut d, "volumeLimit", json!({"on": true, "db": 83})), [encode([0x20, 0x82], &[1, 85], true)]);
        assert_eq!(set(&mut d, "volumeLimit", json!({"rate": 2})), [encode([0x20, 0x81], &[2], true)]);
        assert_eq!(set(&mut d, "autoOff", json!({"on": true, "index": 3})), [encode([0x01, 0x86], &[1, 3], true)]);
        let mut a = driver(a3936());
        assert_eq!(set(&mut a, "gaming", json!(true)), [encode([0x01, 0x87], &[1], true)]);
        assert!(set(&mut a, "leak", json!(true)).is_empty());
    }

    #[test]
    fn an_inverted_switch_writes_the_opposite() {
        let f = super::super::demo::FAKERS.iter().find(|f| f.0 == "d1101").unwrap();
        let mut d = driver(f.2);
        assert_eq!(set(&mut d, "buttonsOff", json!(true)), [encode([0x10, 0x94], &[0], true)]);
        assert_eq!(switch(&d.state_json(), "buttonsOff"), true);
    }

    #[test]
    fn the_cycle_needs_two_modes() {
        let mut d = driver(LIBERTY_5);
        assert_eq!(set(&mut d, "cycle", json!({"anc": true, "transparency": true, "normal": false})), [encode([0x06, 0x82], &[3], true)]);
        assert!(set(&mut d, "cycle", json!({"anc": false, "transparency": true, "normal": false})).is_empty());
    }

    #[test]
    fn a_button_write_keeps_the_other_nibble() {
        let mut d = driver(LIBERTY_5);
        // Left double press: action byte 0x32 (2 connected, 3 alone). Next song on the connected nibble.
        assert_eq!(set(&mut d, "button", json!({"press": "double", "side": 0, "action": 3})), [encode([0x04, 0x81], &[0, 0, 0x33], true)]);
        // Off on the A3957 is action 0xF.
        assert_eq!(set(&mut d, "button", json!({"press": "double", "side": 0, "action": null})), [encode([0x04, 0x81], &[0, 0, 0x3F], true)]);
        assert_eq!(button(&d.state_json(), "double", 0), Value::Null);
    }

    #[test]
    fn a3936_buttons_go_off_and_on_in_pairs() {
        let mut d = driver(a3936());
        // Left double press: on 0x11, action 0x63. Off turns both sides off with 04 83.
        let s = set(&mut d, "button", json!({"press": "double", "side": 0, "action": null}));
        assert_eq!(s, [encode([0x04, 0x83], &[0, 0, 0x10], true), encode([0x04, 0x83], &[1, 0, 0x10], true)]);
        // On again: the other side comes back with its own last action, so only its "on" byte goes out
        // (OpenSCQ30 sends the bytes that changed).
        let s = set(&mut d, "button", json!({"press": "double", "side": 1, "action": 4}));
        assert_eq!(s, [encode([0x04, 0x83], &[0, 0, 0x11], true),
            encode([0x04, 0x81], &[1, 0, 0x64], true), encode([0x04, 0x83], &[1, 0, 0x11], true)]);
        // Single press does not offer the noise mode action.
        assert!(set(&mut d, "button", json!({"press": "single", "side": 0, "action": 4})).is_empty());
    }

    #[test]
    fn a_set_all_model_writes_both_sides_at_once() {
        // A3951 (OpenSCQ30 COMMON_SETTINGS): the faker's single presses are off. On for one side turns both on,
        // with one 04 84.
        let f = super::super::demo::FAKERS.iter().find(|f| f.0 == "a3951").unwrap();
        let mut d = driver_named(f.2, "Soundcore Liberty Air 2 Pro");
        let s = set(&mut d, "button", json!({"press": "single", "side": 0, "action": 6}));
        assert_eq!(s.len(), 1);
        assert_eq!(&s[0][5..7], &[0x04, 0x84]);
        assert_eq!(s[0].len(), 10 + 12);
        // A double press cannot be off.
        assert!(set(&mut d, "button", json!({"press": "double", "side": 0, "action": null})).is_empty());
    }

    #[test]
    fn eq_writes_the_model_body() {
        let mut d = driver(LIBERTY_5);
        let s = set(&mut d, "eq", json!({"preset": CUSTOM_EQ, "bands": [10, 20, 30, 40, 50, 60, 70, 80]}));
        let body = &s[0][9..s[0].len() - 1];
        assert_eq!(&s[0][5..7], &[0x03, 0x87]);
        assert_eq!(&body[..2], &[0xFE, 0xFE]);
        assert_eq!(body[4], 130);
        assert_eq!(body.len(), 2 + 2 + 20 + 3 + 20 + 4 + 1 + 20 + 40 + 2);
        assert_eq!(d.state_json()["eqBands"], json!([10, 20, 30, 40, 50, 60, 70, 80]));
        let mut a = driver(a3936());
        let s = set(&mut a, "eq", json!({"preset": 1}));
        let body = &s[0][9..s[0].len() - 1];
        assert_eq!(body.len(), 2 + 2 + 20 + 3 + 20 + 4 + 1 + 20 + 20);
        assert_eq!(a.state_json()["eqPreset"], 1);
    }

    #[test]
    fn plain_and_drc_eq_match_openscq30() {
        // OpenSCQ30 `it_matches_an_example_soundcore_signature_packet` (02 81, 1 channel of 8 bands): A3033.
        let f = super::super::demo::FAKERS.iter().find(|f| f.0 == "a3028").unwrap();
        let mut d = driver(f.2);
        let s = set(&mut d, "eq", json!({"preset": 0}));
        assert_eq!(s[0], [0x08, 0xee, 0x00, 0x00, 0x00, 0x02, 0x81, 0x14, 0x00, 0x00, 0x00, 0x78, 0x78, 0x78, 0x78, 0x78, 0x78, 0x78, 0x78, 0x4d]);
        // `it_matches_an_example_custom_eq_packet`.
        let s = set(&mut d, "eq", json!({"preset": CUSTOM_EQ, "bands": [-60, 60, 23, 40, 22, 60, -4, 16]}));
        assert_eq!(s[0], [0x08, 0xee, 0x00, 0x00, 0x00, 0x02, 0x81, 0x14, 0x00, 0xfe, 0xfe, 0x3c, 0xb4, 0x8f, 0xa0, 0x8e, 0xb4, 0x74, 0x88, 0xe6]);
    }

    #[test]
    fn the_03_87_body_matches_openscq30_without_hearid() {
        // OpenSCQ30 `it_matches_a_generated_packet_with_no_hear_id`: age 0xFF gives 03 86 with 0xFF waves (A3930).
        let f = super::super::demo::FAKERS.iter().find(|f| f.0 == "a3930").unwrap();
        let mut d = driver_named(f.2, "Soundcore Liberty 2 Pro+");
        let age = d.field("age").unwrap().0;
        d.state[age] = 0xFF;
        let s = set(&mut d, "eq", json!({"preset": 0}));
        assert_eq!(&s[0][5..7], &[0x03, 0x86]);
        let body = &s[0][9..s[0].len() - 1];
        // preset, 2 x 8 bands, FF FF 00, 16 x FF, 4 x 0, type, 16 x FF, 2 x 8 DRC bands
        assert_eq!(body.len(), 2 + 16 + 3 + 16 + 4 + 1 + 16 + 16);
        assert_eq!(&body[18..21], &[0xFF, 0xFF, 0]);
    }

    #[test]
    fn drc_matches_openscq30() {
        // A flat curve stays flat; the tail keeps 0 and -120.
        assert_eq!(apply_drc(&[0, 0, 0, 0, 0, 0, 0, 0, 0, -120]), [0, 0, 0, 0, 0, 0, 0, 0, 0, -120]);
        // OpenSCQ30 `set_equalizer_with_drc` test: -60 60 23 120 22 -120 -4 16 gives these DRC bytes.
        let v: Vec<u8> = apply_drc(&[-60, 60, 23, 120, 22, -120, -4, 16]).iter().map(|&x| Driver::eq_byte(false, x)).collect();
        assert_eq!(v, [0x6d, 0x86, 0x70, 0x88, 0x7b, 0x66, 0x7e, 0x79]);
    }

    #[test]
    fn reads_the_device_list() {
        let mut d = driver(a3936());
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
        assert_eq!(mode(&demo("A3957").unwrap().state_json(), "ancLevel"), 5);
        assert!(demo("A9999").is_none());
    }

    #[test]
    fn pushes_update_battery_and_noise() {
        let mut d = driver(a3936());
        d.on_bytes(6, &inbound(msg::BATTERY_LEVEL, &[2, 4, 0]));
        d.on_bytes(7, &inbound(msg::SOUND_MODES, &[1, 0x30, 0, 1, 0, 0]));
        let c = d.common();
        assert_eq!(c.battery[0].map(|b| (b.value, b.charging)), Some((2, true)));
        assert_eq!(c.noise.as_deref(), Some(NOISE_TRANSPARENCY));
        // A push of another length is refused, as OpenSCQ30 does.
        d.on_bytes(8, &inbound(msg::SOUND_MODES, &[0, 0x30, 0, 1]));
        assert_eq!(d.common().noise.as_deref(), Some(NOISE_TRANSPARENCY));
        d.on_bytes(8, &inbound(msg::SOUND_MODES, &[0, 0x20, 1, 0, 1, 0]));
        assert_eq!(mode(&d.state_json(), "ancLevel"), 2);
        assert_eq!(mode(&d.state_json(), "windNoise"), 1);
        d.on_bytes(9, &inbound([0x01, 0x11], &[1]));
        assert_eq!(d.common().low_latency, Some(true));
    }
}

#[cfg(test)]
mod dump {
    use super::*;
    #[test]
    #[ignore]
    fn dump_fakers() {
        for (file, name, hex) in super::super::demo::FAKERS {
            let mut d = Driver::new();
            d.connected(0, name.to_string());
            d.on_bytes(1, &encode_in(msg::REQUEST_STATE, &unhex(hex)));
            let c = d.common();
            let s = d.state_json();
            let modes: Vec<String> = s["modes"].as_array().map(|a| a.iter().map(|m| format!("{}={}", m["key"].as_str().unwrap(), m["value"])).collect()).unwrap_or_default();
            let sw: Vec<String> = s["switches"].as_array().map(|a| a.iter().map(|m| format!("{}={}", m["key"].as_str().unwrap(), m["on"])).collect()).unwrap_or_default();
            println!("{file} {:?} fw={:?} serial={:?} bat={:?} eq={} bands={} modes=[{}] sw=[{}] auto={} vol={} cycle={} btn0={}",
                c.model_id, d.firmware, d.serial, c.battery.map(|b| b.map(|l| (l.value, l.charging))), s["eqPreset"], s["eqBands"],
                modes.join(" "), sw.join(" "), s["autoOff"], s["volumeLimit"], s["cycle"], s["buttons"].get(0).map_or(Value::Null, |b| b["action"].clone()));
        }
    }
}
