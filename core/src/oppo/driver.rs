//! The OPPO, OnePlus and realme driver, with no I/O: the init sequence, the replies and pushes, the UI's commands
//! and the feature-sync engine. A shell owns the socket and the clock. It passes in the bytes it reads
//! ([`Driver::on_bytes`]), the UI's commands ([`Driver::command`]) and the time ([`Driver::tick`]). It runs the
//! [`Effect`]s that come back: it writes the frames, logs the lines and shows the [`Snapshot`].
//!
//! A wait between two frames is a [`Step::Wait`] in the step queue. The shell calls [`Driver::tick`] at
//! [`Driver::next_wake`] or sooner. The queue runs one UI command at a time, as the blocking link did:
//! the commands that come during a command wait, then go in one batch through [`eq_logic::sent`].

use std::collections::{HashMap, VecDeque};

use serde_json::Value;

use crate::cmd;
use crate::feature_sync::{Confirm, FeatureSync, Operation, Out, ToastKind};
use crate::model::{Common, Level, Wear};
use crate::oppo::eq_logic;
use crate::oppo::profile::Profile;
use crate::oppo::protocol::{answer_for, decode, Framer};
use crate::oppo::types::*;

/// A failure toast the engine asked for: the UI names the setting and shows the text.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Notice {
    /// The setting the user touched.
    pub key: u8,
    pub kind: ToastKind,
    /// Changes with each toast, so the same text shows again.
    pub seq: u32,
}

pub enum Cmd {
    Anc(String),
    LowLatency(bool),
    EqBuiltIn(u8),
    /// Select and save a custom preset (a band edit or rename is the same frame).
    EqSave(Preset),
    EqCreate(Preset),
    EqDelete(Preset),
    BassWave(bool),
    /// (level, band): band 0 is the one BassWave level, 1-3 a realme dynamic sound slider.
    BassLevel(i8, u8),
    /// `0x0403` switches in this order, then the status re-read.
    Features(Vec<(u8, bool)>),
    /// The locator tone on both buds.
    Find(bool),
    /// The model list's pick changed ([`Settings::model_manual`]): the model is looked up again.
    ModelPicked,
    /// Earbud settings opened: the reads the phone's screen does then (alert volume, tap sensitivity).
    EarbudReads,
    AlertVolume(u8),
    TapLevel(u8),
    GameSoundType(u8),
    /// Game equalizer (feature `21`): the switch, then the state the phone's app watcher sends. The desktop
    /// has no app detection, so a switch that is on means a game app is open.
    GameEq(bool),
    /// `0x0422`: 0 off, 1 fixed, 2 head tracking, then the read-back.
    SpatialType(u8),
    /// `0x041A` codec and Hi-Res; the buds restart and the link comes back by itself.
    Codec(u8, bool),
    /// The Hi-Res switch: one engine operation; `drop_spatial` writes 3D audio off first (OPPO-PROTOCOL.md section 18).
    Hires { on: bool, drop_spatial: bool },
    HeadMotion(u8),
    /// `0x0405` start / stop.
    FitTest(bool),
    /// Personalized ANC: `0x011A` (is a result stored?).
    PncQuery,
    /// `0x0412 <action>`; using the stored result re-reads the status after it.
    Pnc(u8),
    /// `0x010D`, after a Personalized ANC result applied (the buds switch `0C` on themselves).
    StatusRead,
    /// The Dual connection switch.
    Dual(bool),
    /// Dual connection opened: the device list, the status, and the preferred device if shown.
    DualReads(bool),
    /// Connect (true) or disconnect another device on the list.
    ConnectDevice([u8; 6], bool),
    /// None = automatic.
    Preferred(Option<[u8; 6]>),
    /// Controls opened: the gesture table and the hold cycles of these noise types.
    ControlsReads(Vec<u8>),
    /// One gesture on one bud: (dev, act, fn), written into every slot that bud has for it, outside on-call.
    Gesture(u8, u8, u8),
    /// The hold's noise cycle: (mask, type).
    HoldModes(u32, u8),
    /// A both-buds entry (`dev 04`): button, act, fn.
    OnCall(u8, u8, u8),
    /// Hearing profile frames, in order; a switch among them re-reads the status after.
    Hearing(Vec<Frame>),
    /// `0x0106`: the battery levels. The driver sends it every [`BATTERY_POLL_MS`] by itself.
    BatteryRead,
    /// The shell's own: it opens the link. The driver ignores it.
    Connect,
    /// The shell's own: it closes the link. The driver ignores it.
    Disconnect,
}

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Status {
    #[default]
    Off,
    Connecting,
    On,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub status: Status,
    pub name: String,
    /// Left, right, case: (level, charging).
    pub battery: [Option<(u8, bool)>; 3],
    /// Left, right, case: raw wear status (OPPO-PROTOCOL.md section 13); None until the buds report it. Raw 0 is a bud
    /// asleep in the closed case, not an unknown state.
    pub wear: [Option<u8>; 3],
    /// The events the buds offer (`0x8200`), once read.
    pub offered: Option<Vec<u8>>,
    pub anc: Option<String>,
    pub modes: AncModes,
    pub low_latency: Option<bool>,
    /// The model in use: the manual pick, else [detected](Self::detected).
    pub model: Option<&'static Value>,
    /// What the product id (else the name) says.
    pub detected: Option<&'static Value>,
    /// The model was picked by hand: its `models.json` flags count as features (`Capabilities.offered`).
    pub manual: bool,
    pub caps: Caps,
    pub firmware: Option<String>,
    pub eq_current: Option<u8>,
    pub eq_custom: Vec<Preset>,
    pub bass_on: Option<bool>,
    pub bass_level: Option<i8>,
    /// realme dynamic sound: the last level written to each slider (low, medium, high). The buds do not read back.
    pub bass_bands: [Option<i8>; 3],
    /// `0x810D`: every feature id the buds listed, with its value.
    pub features: Vec<(u8, u8)>,
    pub alert_volume: Option<u8>,
    /// (level, the buds' default)
    pub tap_level: Option<(u8, u8)>,
    /// (selected, offered types) from `0x812B`.
    pub game_sound: Option<(u8, Vec<u8>)>,
    pub head_motion: Option<u8>,
    /// The level Smart chose (`ANC-Light` ...), OPPO-PROTOCOL.md section 14.
    pub smart_level: Option<String>,
    /// Counts ANC writes refused with no bud in an ear (status 14, OPPO-PROTOCOL.md section 14): each one shows the hint.
    pub anc_refused: u32,
    /// `0x812A` / push `0x0510`: 0 off, 1 fixed, 2 head tracking.
    pub spatial_type: Option<u8>,
    /// The current codec (`0x8114`) and the offered ones (`0x8123`), OPPO-PROTOCOL.md section 18 Codec picker.
    pub codec: Option<u8>,
    pub codecs: Vec<u8>,
    /// The last fit test result, and a count that changes with each one.
    pub fit: (u8, u8, u32),
    /// The last Personalized ANC event (1 stored, 2 ack, 3 result), its value, and a count that changes with each.
    pub pnc: (u8, u8, u32),
    /// The buds' paired-device list (Dual connection).
    pub devices: Vec<PairedDevice>,
    /// None = automatic.
    pub preferred: Option<[u8; 6]>,
    /// The last gesture table read, `[dev, btn, act, fn]`; None until read (no write without it).
    pub keyfn: Option<Vec<[u8; 4]>>,
    /// The hold's noise cycles read: (type, mask).
    pub hold_masks: Vec<(u8, u32)>,
    /// Settings the feature-sync engine is applying: their rows show "Applying…" and take no tap.
    pub busy: Vec<u8>,
    /// The last failure: the overview shows it for a moment, then hides it.
    pub notice: Option<Notice>,
}

/// The game-mode id the buds write and read. A model's `gameId` wins, for a model whose capture proves another id
/// (OPPO-PROTOCOL.md section 16).
pub fn game_id(s: &Snapshot) -> u8 {
    Profile(s.model).game_id().unwrap_or_else(|| s.caps.game_mode_id())
}

/// A raw wear status (OPPO-PROTOCOL.md section 13): 3 and 7 in the ear, 4 in the case, 0 in the closed case, 1 and 5 out.
fn wear(raw: Option<u8>) -> Wear {
    match raw {
        Some(3 | 7) => Wear::InEar,
        Some(0 | 4) => Wear::InCase,
        Some(1 | 5) => Wear::Out,
        _ => Wear::Unknown,
    }
}

/// The brand-neutral state of an OPPO snapshot. The features are the rows `row_rules` gives; the model's gesture list
/// decides the controls row.
pub fn common(s: &Snapshot, phone_spatial: bool) -> Common {
    use std::collections::HashMap;
    let reply: HashMap<u8, u8> = s.features.iter().copied().collect();
    let features = if s.status == Status::On {
        crate::oppo::row_rules::rows(&crate::oppo::row_rules::Input {
            model: s.model, reply: (!s.features.is_empty()).then_some(&reply), bitmap: s.caps.0.as_deref(),
            product_anc: !s.modes.is_empty(), phone_spatial_ok: phone_spatial, manual: s.manual, gestures: None,
            firmware: s.firmware.as_deref(),
        })
    } else { Default::default() };
    Common {
        connected: s.status == Status::On,
        name: s.name.clone(),
        model_id: s.model.and_then(|m| m["id"].as_str()).map(Into::into),
        battery: s.battery.map(|b| b.map(|(value, charging)| Level { value, max: 100, charging })),
        wear: [wear(s.wear[0]), wear(s.wear[1])],
        noise: s.anc.clone(),
        noise_modes: s.modes.names(),
        low_latency: s.low_latency,
        features,
    }
}

/// The user's settings that the driver reads. The shell loads them and passes them in.
#[derive(Clone, Default)]
pub struct Settings {
    /// The last product id the buds reported.
    pub product_id: Option<String>,
    /// The model id picked by hand in the model list.
    pub model_manual: Option<String>,
    /// Settings › Hide old battery levels.
    pub clear_battery: bool,
    /// The phone makes 3D audio itself (OPPO and OnePlus phones on Android): buds without `0xEF03` then show the row.
    pub phone_spatial: bool,
    /// The last real case level of these buds. A case level 0 (a closed case) shows it (issue #47).
    pub case_level: Option<u8>,
    /// The init sequence reads the EQ. Android reads it when the EQ screen opens (one read set per link).
    pub eq_on_connect: bool,
    /// The EQ saved for these buds: the state shows it until the buds answer.
    pub eq_saved: Option<EqSaved>,
}

/// A saved EQ state of one pair of buds.
#[derive(Clone, Default)]
pub struct EqSaved {
    pub current: Option<u8>,
    pub bass_level: Option<i8>,
    pub custom: Vec<Preset>,
}

/// What the shell does for the driver, in order.
pub enum Effect {
    /// Write these bytes to the socket.
    Send(Vec<u8>),
    /// One packet log line: "TX", "RX", "DISCARDED RX" or "NOTE" (words only, no bytes).
    Log { dir: &'static str, bytes: Vec<u8>, human: Option<String> },
    /// The [`Snapshot`] changed: show it.
    State,
    /// A hearing profile reply or push, for the hearing page.
    Hearing(HearingEv),
    /// Store this setting (`Value::Null` removes it).
    Save(&'static str, Value),
    /// The init sequence is done.
    Ready,
    /// Find phone (realme, model flag `findPhone`): true rings the phone, false stops it.
    FindPhone(bool),
}

/// One entry of the step queue.
enum Step {
    /// A frame: (command, sequence byte or the next counter value, payload).
    Tx(u16, Option<u8>, Vec<u8>),
    /// Read and apply what comes for this long, then go on.
    Wait(u64),
    /// Builds the next steps when it runs, from what the buds answered before.
    Run(fn(&mut Driver) -> Vec<Step>),
    /// One UI command.
    Cmd(Cmd),
    /// Show the snapshot.
    Emit,
    /// The init sequence is done.
    Ready,
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    /// No link.
    Off,
    /// The init sequence runs: UI commands wait.
    Init,
    /// Connected.
    Run,
}

/// Engine seqs travel as `0xC0..0xDF`; the generic counter never uses that range.
fn wire_seq(seq: u32) -> u8 {
    0xC0 + (seq % 0x20) as u8
}

/// The packet log line for a failed setting, as the phone writes it.
fn toast_log(key: u8, kind: ToastKind, codec: Option<u8>) -> String {
    let name = match kind {
        ToastKind::RefusedMissing | ToastKind::RefusedNow => "REFUSED",
        ToastKind::NotChanged => "NOT CHANGED",
        ToastKind::NoAnswer => "NO ANSWER",
    };
    let codec = codec.map_or("null".to_string(), |c| c.to_string());
    format!("FEATURE {name}: key=0x{key:02X} kind={} codec={codec}", kind.name())
}

/// The engine's period while an operation is active.
const ENGINE_TICK_MS: u64 = 500;

/// A stand-in case level counts this long: the phone scans at most once in 5 min.
const STAND_IN_MS: u64 = 5 * 60_000;

/// The battery read period while connected. Some buds push no level change for a long time.
pub const BATTERY_POLL_MS: u64 = 5 * 60_000;

pub struct Driver {
    settings: Settings,
    phase: Phase,
    s: Snapshot,
    out: Vec<Effect>,
    // The feature-sync engine and its state: kept across a reconnect (`docs/FEATURE-SYNC.md`).
    engine: FeatureSync,
    /// Wire sequence byte -> the engine's own number, for the writes and the reads the engine sent.
    wire: HashMap<u8, u32>,
    /// The settings still applying: kept across a reconnect, for a fresh snapshot.
    busy: Vec<u8>,
    /// The last failure: kept, so a disconnect can still show it.
    notice: Option<Notice>,
    last_tick: u64,
    // One link's state.
    /// The byte stream into whole packets (docs/OPPO-PROTOCOL.md section 2).
    framer: Framer,
    seq: u8,
    /// The last ANC level sent, to name a report that says only "noise cancelling".
    level: Option<String>,
    /// Switches just written: (feature id, the value written, until ms). The buds' next reports may still carry the
    /// old value; they do not undo the click. The status read after the write is the last word.
    holds: Vec<(u8, bool, u64)>,
    /// The feature id of each `0x0403` write by sequence byte, so a refusal in the log names its switch.
    feature_writes: HashMap<u8, u8>,
    /// The last EQ pick: older replies do not move it back ([`eq_logic::PickHold`]).
    eq_hold: eq_logic::PickHold,
    steps: VecDeque<Step>,
    /// The end of the running [`Step::Wait`].
    wait_until: Option<u64>,
    /// UI commands that came while a step ran.
    pending: Vec<Cmd>,
    /// The last time the shell gave.
    now: u64,
    /// The case level of the last link report that had a case cell (0: the buds could not read it).
    link_case: Option<u8>,
    /// "Hide old battery levels" is on and the last report left the case out: no case level shows.
    case_hidden: bool,
    /// The last stand-in case level: (level, charging, when).
    stand_in: Option<(u8, bool, u64)>,
    /// The battery levels of the last link and the device name they came from.
    last_battery: Option<(String, [Option<(u8, bool)>; 3])>,
    /// The time of the next battery read; None before the init sequence ends.
    poll_at: Option<u64>,
}

impl Driver {
    pub fn new(settings: Settings) -> Self {
        Driver {
            settings, phase: Phase::Off, s: Snapshot::default(), out: Vec::new(),
            engine: FeatureSync::default(), wire: HashMap::new(), busy: Vec::new(), notice: None, last_tick: 0,
            framer: Framer::default(), seq: 0, level: None, holds: Vec::new(), feature_writes: HashMap::new(),
            eq_hold: Default::default(), steps: VecDeque::new(), wait_until: None, pending: Vec::new(), now: 0,
            link_case: None, case_hidden: false, stand_in: None, last_battery: None, poll_at: None,
        }
    }

    pub fn snapshot(&self) -> &Snapshot { &self.s }

    /// The brand-neutral state (`crate::model`).
    pub fn common(&self) -> Common { common(&self.s, self.settings.phone_spatial) }

    /// A case level the phone read outside the link: the BLE advertisement (`AdvertCaseScan`). The buds give none
    /// while no bud sits in the open case (issue #47, report #184). It shows while the link gives no case level or
    /// 0, and stays as any shown level does. A case 0 from the link takes it while it is under 5 min old.
    pub fn case_stand_in(&mut self, now: u64, level: u8, charging: bool) -> Vec<Effect> {
        self.now = now;
        self.stand_in = Some((level, charging, now));
        if matches!(self.link_case, None | Some(0)) && !self.case_hidden {
            self.s.battery[2] = Some((level, charging));
            self.out.push(Effect::State);
        }
        std::mem::take(&mut self.out)
    }

    /// The shell loaded the settings again (a command can follow a change).
    pub fn set_settings(&mut self, settings: Settings) { self.settings = settings; }

    /// The latest time at which the shell calls [`tick`](Self::tick) again. None: only new bytes or a command matter.
    pub fn next_wake(&self) -> Option<u64> {
        let engine = self.engine.active().then_some(self.last_tick + ENGINE_TICK_MS);
        let step = match self.wait_until {
            Some(t) => Some(t),
            None if !self.steps.is_empty() || !self.pending.is_empty() => Some(self.now),
            None => None,
        };
        let poll = self.poll_at.filter(|_| self.phase == Phase::Run);
        [engine, step, poll].into_iter().flatten().min()
    }

    /// The link is up to buds named `name`. The init sequence starts.
    pub fn connected(&mut self, now: u64, name: String) -> Vec<Effect> {
        self.now = now;
        self.framer = Framer::default();
        self.seq = 0;
        self.level = None;
        self.holds.clear();
        self.feature_writes.clear();
        self.eq_hold = Default::default();
        self.steps.clear();
        self.wait_until = None;
        self.pending.clear();
        self.link_case = None;
        self.case_hidden = false;
        self.stand_in = None;
        self.poll_at = None;
        let detected = find_model(None, Some(&name));
        // The levels of the last link of these buds show until the buds report: a part a report leaves out keeps its
        // level, as Settings › Hide old battery levels off promises.
        let battery = self.last_battery.take().filter(|(n, _)| *n == name && !self.settings.clear_battery).map(|(_, b)| b);
        self.s = Snapshot { status: Status::On, name, detected, battery: battery.unwrap_or_default(), ..Default::default() };
        if let Some(eq) = &self.settings.eq_saved {
            self.s.eq_current = eq.current;
            self.s.bass_level = eq.bass_level;
            self.s.eq_custom = eq.custom.clone();
        }
        self.pick_model();
        // A new link: forget the last link's sequence bytes, let the engine continue a restart wait,
        // and show again what it is still applying.
        self.wire.clear();
        self.s.busy = self.busy.clone();
        self.s.notice = self.notice;
        self.phase = Phase::Init;
        self.last_tick = now;
        let outs = self.engine.reconnect(now);
        self.exec(outs);
        self.out.push(Effect::State);
        // OPPO-PROTOCOL.md "Init sequence", section 6: 300 ms before the first frame, then 200 ms apart; reads only if listed.
        self.steps.extend([Step::Wait(300)]);
        for f in [cmd::handshake(), cmd::product_id(), cmd::broadcast_codes()] {
            self.steps.extend([Step::Tx(f.0, None, f.1), Step::Wait(200)]);
        }
        self.steps.push_back(Step::Run(Driver::init_reads));
        self.drive();
        std::mem::take(&mut self.out)
    }

    /// The link is gone (lost, or closed by the user).
    pub fn disconnected(&mut self, now: u64) -> Vec<Effect> {
        self.now = now;
        if self.phase != Phase::Off {
            self.last_battery = Some((self.s.name.clone(), self.s.battery));
            let outs = self.engine.disconnect(now);
            if self.exec(outs) { self.out.push(Effect::State); }
        }
        self.phase = Phase::Off;
        self.steps.clear();
        self.wait_until = None;
        self.pending.clear();
        self.s = Snapshot { busy: self.busy.clone(), notice: self.notice, ..Default::default() };
        self.out.push(Effect::State);
        std::mem::take(&mut self.out)
    }

    /// A UI command. It runs when the commands before it are done.
    pub fn command(&mut self, now: u64, c: Cmd) -> Vec<Effect> {
        self.now = now;
        // With no link, a command does nothing: the next connect reads the buds again.
        if self.phase != Phase::Off && !matches!(c, Cmd::Connect | Cmd::Disconnect) { self.pending.push(c); }
        self.drive();
        std::mem::take(&mut self.out)
    }

    /// The clock: runs the steps that are due and the engine's failure detector.
    pub fn tick(&mut self, now: u64) -> Vec<Effect> {
        self.now = now;
        if self.engine.active() && now.saturating_sub(self.last_tick) >= ENGINE_TICK_MS {
            self.last_tick = now;
            let outs = self.engine.tick(now);
            if self.exec(outs) { self.out.push(Effect::State); }
        }
        if self.phase == Phase::Run && self.poll_at.is_some_and(|t| now >= t) {
            self.poll_at = Some(now + BATTERY_POLL_MS);
            if self.s.caps.supports(CMD_QUERY_BATTERY) { self.pending.push(Cmd::BatteryRead); }
        }
        self.drive();
        std::mem::take(&mut self.out)
    }

    /// Bytes from the socket. `unix` is the wall clock in seconds, for the buds' time request.
    pub fn on_bytes(&mut self, now: u64, unix: u32, data: &[u8]) -> Vec<Effect> {
        self.now = now;
        let mut changed = false;
        let frames = self.framer.push(data);
        for p in frames.into_iter().flat_map(|p| {
            // A batch reply is logged as it came, then handled as the replies it carries.
            if cmd_of(&p) != CMD_BATCH_REPLY { return vec![p]; }
            let parts = unbatch(&p);
            let n = parts.len();
            self.out.push(Effect::Log { dir: "RX", bytes: p, human: Some(format!("batch reply, {n} answers")) });
            parts
        }).collect::<Vec<_>>() {
            changed |= self.frame(&p, unix);
        }
        let skipped = std::mem::take(&mut self.framer.discarded);
        if !skipped.is_empty() { self.out.push(Effect::Log { dir: "DISCARDED RX", bytes: skipped, human: None }); }
        if changed { self.out.push(Effect::State); }
        self.drive();
        std::mem::take(&mut self.out)
    }

    fn note(&mut self, text: String) {
        self.out.push(Effect::Log { dir: "NOTE", bytes: Vec::new(), human: Some(text) });
    }

    /// The manual pick, else the detected model; the ANC modes follow it.
    fn pick_model(&mut self) {
        let manual = self.settings.model_manual.as_deref()
            .and_then(|id| models().iter().find(|m| m["id"].as_str() == Some(id)));
        self.s.manual = manual.is_some();
        self.s.model = manual.or(self.s.detected);
        self.s.modes = AncModes::of(self.s.model);
    }

    /// A switch written just now keeps the value written for a moment.
    fn hold(&mut self, id: u8, on: bool) {
        self.holds.retain(|h| h.0 != id);
        self.holds.push((id, on, self.now + 2500));
    }

    fn send(&mut self, cmd: u16, seq: Option<u8>, payload: &[u8]) {
        // 0xC0..0xDF belongs to the feature-sync engine: skip it.
        let seq = seq.unwrap_or_else(|| { self.seq = if self.seq >= 0xBF { 0x01 } else { self.seq + 1 }; self.seq });
        let p = build_packet(cmd, seq, payload);
        if cmd == CMD_SET_FEATURE && !payload.is_empty() {
            if self.feature_writes.len() > 16 { self.feature_writes.clear(); }
            self.feature_writes.insert(seq, payload[0]);
        }
        self.out.push(Effect::Log { dir: "TX", bytes: p.clone(), human: describe_tx(cmd, payload) });
        self.out.push(Effect::Send(p));
    }

    /// The engine number for a reply with wire seq `seq`. A model with `"seqless":1` (realme Buds T100, report #100)
    /// answers with its own counter, so the newest outstanding engine request is the one that the reply answers.
    fn take_engine_seq(&mut self, seq: u8) -> Option<u32> {
        if let Some(engine) = self.wire.remove(&seq) { return Some(engine); }
        if !Profile(self.s.model).seqless() { return None; }
        let wire = *self.wire.iter().max_by_key(|(_, e)| **e)?.0;
        self.note(format!("SYNC: reply seq=0x{seq:02X} pairs with request seq=0x{wire:02X} (seqless model)"));
        self.wire.remove(&wire)
    }

    /// Runs the feature-sync engine's output list. True when the snapshot changed. With no link, only the
    /// toasts and the busy list count.
    fn exec(&mut self, outs: Vec<Out>) -> bool {
        let link = self.phase != Phase::Off;
        let mut changed = false;
        for out in outs {
            match out {
                Out::Send { id, value, seq } if link => {
                    self.wire.insert(wire_seq(seq), seq);
                    let (cmd, payload) = cmd::feature(id, value == 1);
                    self.send(cmd, Some(wire_seq(seq)), &payload);
                }
                Out::Read { seq } if link => {
                    self.wire.insert(wire_seq(seq), seq);
                    let q = status_query(self.s.model);
                    self.send(CMD_QUERY_STATUS, Some(wire_seq(seq)), &q);
                }
                Out::Changed { id, value } if link => {
                    match self.s.features.iter_mut().find(|x| x.0 == id) {
                        Some(x) => x.1 = value,
                        None => self.s.features.push((id, value)),
                    }
                    if id == game_id(&self.s) { self.s.low_latency = Some(value == 1); }
                    changed = true;
                }
                Out::Busy { key, on } => {
                    if on {
                        if !self.busy.contains(&key) { self.busy.push(key); }
                    } else {
                        self.busy.retain(|&k| k != key);
                    }
                    changed = true;
                }
                Out::Toast { key, kind } => {
                    let codec = if link { self.s.codec } else { None };
                    self.note(toast_log(key, kind, codec));
                    // The count goes on across reconnects, so the same text shows again.
                    let seq = self.notice.map_or(0, |n| n.seq) + 1;
                    self.notice = Some(Notice { key, kind, seq });
                    changed = true;
                }
                _ => {}
            }
        }
        self.s.busy = self.busy.clone();
        self.s.notice = self.notice;
        changed
    }

    /// Runs the steps that are due. A UI command runs only when the steps before it are done.
    fn drive(&mut self) {
        if self.phase == Phase::Off { return; }
        loop {
            if let Some(until) = self.wait_until {
                if self.now < until { return; }
                self.wait_until = None;
            }
            let step = match self.steps.pop_front() {
                Some(s) => s,
                None => {
                    if self.phase != Phase::Run || self.pending.is_empty() { return; }
                    // Quick EQ clicks queue behind a write: only the last pick is sent. A save with changes is kept.
                    let cmds = std::mem::take(&mut self.pending);
                    let sent = eq_logic::sent(&cmds, &self.s.eq_custom);
                    for (c, keep) in cmds.into_iter().zip(sent) {
                        if keep { self.steps.extend([Step::Cmd(c), Step::Emit]); }
                    }
                    continue;
                }
            };
            match step {
                Step::Tx(cmd, seq, payload) => self.send(cmd, seq, &payload),
                Step::Wait(ms) => if ms > 0 { self.wait_until = Some(self.now + ms) },
                Step::Run(f) => { let next = f(self); self.front(next) }
                Step::Cmd(c) => { let next = self.run_cmd(c); self.front(next) }
                Step::Emit => self.out.push(Effect::State),
                Step::Ready => {
                    self.phase = Phase::Run;
                    self.poll_at = Some(self.now + BATTERY_POLL_MS);
                    self.out.push(Effect::Ready)
                }
            }
        }
    }

    /// Puts `steps` before the queued ones, in their order.
    fn front(&mut self, steps: Vec<Step>) {
        for s in steps.into_iter().rev() { self.steps.push_front(s); }
    }

    /// A status read with the generic sequence byte `00`.
    fn status_read(&self) -> Step { Step::Tx(CMD_QUERY_STATUS, Some(0x00), status_query(self.s.model)) }

    /// Init, after the handshake, the product id and the broadcast codes have had their answers. The reads and their
    /// order are the Android app's (`BudsConnectionManager.runInitSequence`); each goes out only if the buds list it.
    fn init_reads(&mut self) -> Vec<Step> {
        let mut st = Vec::new();
        let caps = self.s.caps.clone();
        let ids = notify_ids(&caps, self.s.offered.as_deref());
        if !ids.is_empty() {
            if caps.supports(CMD_REGISTER_NOTIFY) { let f = cmd::register(&ids); st.push(Step::Tx(f.0, None, f.1)); }
            else { for &id in &ids { let f = cmd::register_one(id); st.push(Step::Tx(f.0, None, f.1)); } }
        }
        let mut queries: Vec<(u16, Option<u8>, Vec<u8>)> = vec![
            (CMD_QUERY_STATUS, Some(0x00), status_query(self.s.model)),
            (CMD_QUERY_ANC, None, cmd::anc_read(1, 1).1),
        ];
        // The level Smart chose, OPPO-PROTOCOL.md section 14.
        if self.s.modes.supports("ANC-Smart") { queries.push((CMD_QUERY_ANC, None, cmd::anc_read(4, 1).1)); }
        queries.extend([
            (CMD_QUERY_ALERT_VOLUME, None, vec![]),
            (CMD_QUERY_BATTERY, Some(0xF0), vec![]),
            (CMD_QUERY_WEARING, Some(0xF2), vec![]),
            // The gesture table every gesture write is built from.
            (CMD_QUERY_KEY_FUNCTION, None, vec![]),
        ]);
        // The hold's noise cycles.
        for t in Profile(self.s.model).hold_types() { queries.push((CMD_QUERY_ANC, None, cmd::anc_read(2, t).1)); }
        for c in [CMD_QUERY_FIRMWARE, CMD_QUERY_SPATIAL_TYPE, CMD_QUERY_CODEC_LIST, CMD_QUERY_CODEC, CMD_QUERY_GAME_SOUND, CMD_QUERY_HEAD_MOTION] {
            queries.push((c, None, vec![]));
        }
        if self.settings.eq_on_connect {
            for c in [CMD_QUERY_EQ, CMD_QUERY_EQ_ALL, CMD_QUERY_BASSWAVE, CMD_QUERY_BASS_BANDS] { queries.push((c, None, vec![])); }
        }
        // Buds with the `0x012F` batch get the reads in one frame, as HeyMelody sends them.
        let batch = caps.supports(CMD_BATCH);
        let mut reads = Vec::new();
        for (cmd, seq, payload) in queries {
            if !caps.supports(cmd) { continue; }
            if batch { reads.push((cmd, payload)); } else { st.extend([Step::Wait(200), Step::Tx(cmd, seq, payload)]); }
        }
        if !reads.is_empty() { let f = cmd::batch(&reads); st.extend([Step::Wait(200), Step::Tx(f.0, None, f.1)]); }
        // realme Link hands 3D audio to the buds on every connect; else the `1B` switch can do nothing (issue #9).
        if Profile(self.s.model).spatial_toggle() && caps.supports(CMD_SPATIAL_SOURCE) {
            let f = cmd::spatial_source(0);
            st.extend([Step::Wait(200), Step::Tx(f.0, None, f.1)]);
        }
        st.push(Step::Ready);
        st
    }

    /// The EQ reads these buds list, in order (`sendThenRead`); after a write too, since only the buds know
    /// the ids after a create or delete.
    fn eq_reads(&self) -> Vec<Step> {
        let mut st = Vec::new();
        for cmd in [CMD_QUERY_EQ, CMD_QUERY_EQ_ALL, CMD_QUERY_BASSWAVE, CMD_QUERY_BASS_BANDS] {
            if !self.s.caps.supports(cmd) { continue; }
            st.extend([Step::Tx(cmd, None, vec![]), Step::Wait(120)]);
        }
        st
    }

    /// A write, then the EQ re-read.
    fn eq_write(&self, f: Frame) -> Vec<Step> {
        let mut st = vec![Step::Emit, Step::Tx(f.0, None, f.1), Step::Wait(250)];
        st.push(Step::Run(|d| d.eq_reads()));
        st
    }

    /// One received packet. True when the snapshot changed.
    fn frame(&mut self, p: &[u8], unix: u32) -> bool {
        let mut changed = false;
        let e = decode(p);
        let human = e.as_ref().map(describe).or_else(|| describe_other(p));
        // Some buds drop the link a few seconds after an unanswered request (issue #2: the time request).
        let answer = answer_for(p, unix).map(|a| (a, seq_of(p)));
        // A refused ANC write (status 14 with no bud in an ear): read the real mode back (OPPO-PROTOCOL.md section 14).
        let refused = cmd_of(p) == CMD_SET_ANC | 0x8000 && payload_of(p).first().is_some_and(|&st| st != 0);
        let need_ear = refused && payload_of(p).first() == Some(&0x0E);
        self.out.push(Effect::Log { dir: "RX", bytes: p.to_vec(), human });
        // A feature-write reply: the engine decides what a refusal means, else the old path logs it.
        // Status 1: the buds lack the switch. Status 2: they have it and will not take it now
        // (high-quality audio needs an LDAC or LHDC link on buds that offer a codec list; #33, #42, #44).
        if cmd_of(p) == CMD_SET_FEATURE | 0x8000 {
            let seq = seq_of(p);
            let status = payload_of(p).first().copied().unwrap_or(0);
            match self.take_engine_seq(seq) {
                Some(engine_seq) => {
                    self.feature_writes.remove(&seq);
                    let outs = self.engine.ack(engine_seq, status, self.now);
                    changed |= self.exec(outs);
                }
                None => if let Some(id) = self.feature_writes.remove(&seq) {
                    if status != 0 {
                        let text = format!("FEATURE REFUSED: id=0x{id:02X} status={status} codec={:?} offered={:?}", self.s.codec, self.s.codecs);
                        self.note(text);
                    }
                }
            }
        }
        if let Some(((cmd, pl), seq)) = answer { self.send(cmd, Some(seq), &pl); }
        // The all-zero wear push comes just before a lid close drops the link (OPPO-PROTOCOL.md section 13): read the
        // case level now, so the last level is not 0 or blank.
        if cmd_of(p) == EVT_PUSH && self.s.caps.supports(CMD_QUERY_BATTERY) {
            if let Some(Event::Wear(v)) = &e {
                let zero = |i: u8| v.iter().any(|&(k, st)| k == i && st == 0);
                if zero(1) && zero(2) && zero(3) {
                    self.note("battery on lid close".into());
                    self.send(CMD_QUERY_BATTERY, None, &[]);
                }
            }
        }
        // Find phone (realme): `0x0502 05` rings the phone, `06` stops it. HeyMelody models use 0x0502 for the
        // camera, so only a model with the `findPhone` flag acts on it (OPPO-PROTOCOL.md section 11).
        if cmd_of(p) == REQ_FIND_PHONE {
            let action = payload_of(p).first().copied();
            self.note(format!("FIND PHONE: action={action:?}"));
            if Profile(self.s.model).find_phone() {
                match action { Some(0x05) => self.out.push(Effect::FindPhone(true)), Some(0x06) => self.out.push(Effect::FindPhone(false)), _ => {} }
            }
        }
        if refused { let f = cmd::anc_read(1, 1); self.send(f.0, None, &f.1); }
        if need_ear { self.s.anc_refused += 1; changed = true; }
        // The engine's own reports: a status read reply, or a game-mode push.
        match &e {
            Some(Event::Features(f)) => {
                let seq = self.take_engine_seq(seq_of(p));
                let outs = self.engine.report(f, seq, self.now, false);
                changed |= self.exec(outs);
            }
            // Confirms a push write, game mode included (realme Buds T200 Lite, report 182).
            Some(Event::FeaturePush(f)) => {
                let outs = self.engine.report(f, None, self.now, true);
                changed |= self.exec(outs);
            }
            Some(Event::GameMode(pl)) => {
                if let Some(on) = game_on(pl, self.settings.product_id.as_deref()) {
                    let key = game_id(&self.s);
                    let outs = self.engine.report(&[(key, on as u8)], None, self.now, true);
                    changed |= self.exec(outs);
                }
            }
            _ => {}
        }
        if let Some(e) = e { changed |= self.apply(e); }
        changed
    }

    fn apply(&mut self, e: Event) -> bool {
        let now = self.now;
        self.holds.retain(|h| h.2 > now);
        let holds = &self.holds;
        // The value a report may set for a switch: the one just written wins while it is held.
        let held = |id: u8, v: bool| holds.iter().find(|h| h.0 == id).map_or(v, |h| h.1);
        let s = &mut self.s;
        match e {
            Event::Caps(c) => s.caps = c,
            Event::ProductId(id) => {
                // Other buds than the manual pick was made for: back to Automatic (`ModelCatalog`).
                if self.settings.product_id.as_deref() != Some(id.as_str()) {
                    self.settings.product_id = Some(id.clone());
                    self.out.push(Effect::Save("product_id", id.as_str().into()));
                    if self.settings.model_manual.take().is_some() { self.out.push(Effect::Save("model_manual", Value::Null)); }
                }
                s.detected = find_model(Some(&id), Some(&s.name));
                self.pick_model();
                let s = &mut self.s;
                // The bridge (or an unknown device name) shows the model's name instead.
                if let Some(n) = s.model.and_then(|m| m["name"].as_str()) {
                    if !is_known_name(&s.name) { s.name = n.to_string(); }
                }
            }
            Event::Firmware(f) => s.firmware = Some(f),
            Event::EqCurrent(id) => if self.eq_hold.accept(id, now) { s.eq_current = Some(id) },
            Event::Offered(o) => s.offered = Some(o),
            Event::EqCustom(list) => s.eq_custom = list,
            Event::BassLevel(l) => s.bass_level = Some(l),
            Event::BassBands(v) => for (band, level) in v {
                if (1..=3).contains(&band) { s.bass_bands[band as usize - 1] = Some(level); }
            },
            Event::Battery(v) => {
                // Settings › Hide old battery levels: a part the report leaves out is cleared, not kept.
                let clear = self.settings.clear_battery;
                if clear { s.battery = [None; 3]; }
                let case_cell = v.iter().find(|c| c.0 == 3).map(|c| c.1);
                if case_cell.is_some() || clear { self.link_case = case_cell; }
                self.case_hidden = clear && case_cell.is_none();
                for (i, level, charging) in v {
                    if !(1..=3).contains(&i) { continue; }
                    // Level 0 is "not known" (a closed case). The case then shows its last real level.
                    if level == 0 {
                        // A fresh stand-in first, then the last real level of these buds.
                        let fresh = self.stand_in.filter(|t| now.saturating_sub(t.2) < STAND_IN_MS).map(|t| (t.0, t.1));
                        if i == 3 { if let Some(l) = fresh.or(self.settings.case_level.map(|c| (c, charging))) { s.battery[2] = Some(l); } }
                        continue;
                    }
                    s.battery[i as usize - 1] = Some((level, charging));
                    if i == 3 && self.settings.case_level != Some(level) {
                        self.settings.case_level = Some(level);
                        self.out.push(Effect::Save("case_level", level.into()));
                    }
                }
            }
            Event::Wear(v) => for (i, st) in v {
                if (1..=3).contains(&i) { s.wear[i as usize - 1] = Some(st); }
            },
            Event::AncRaw(raw) => match s.modes.mode_for_raw(raw, self.level.as_deref()) {
                Some(m) => s.anc = Some(m),
                None => return false,
            },
            Event::GameMode(pl) => match game_on(&pl, self.settings.product_id.as_deref()) {
                // The engine moves this switch: a tap never holds it.
                Some(on) => s.low_latency = Some(on),
                None => return false,
            },
            Event::AlertVolume(l) => s.alert_volume = Some(l),
            Event::TapLevel(l, d) => s.tap_level = Some((l, d)),
            Event::TapPush(l) => s.tap_level = Some((l, s.tap_level.map_or(0, |t| t.1))),
            Event::GameSound(t, all) => s.game_sound = Some((t, all)),
            Event::HeadMotion(t) => s.head_motion = Some(t),
            Event::SmartLevel(m) => s.smart_level = (m != 0).then(|| s.modes.level_for_bit(m.trailing_zeros() as u8)).flatten().map(Into::into),
            Event::SpatialType(t) => s.spatial_type = Some(t),
            Event::Codec(c) => s.codec = Some(c),
            Event::Codecs(c) => s.codecs = c,
            Event::FitResult(l, r) => s.fit = (l, r, s.fit.2 + 1),
            Event::PncStored(e) => s.pnc = (1, e as u8, s.pnc.2 + 1),
            Event::PncAck(st) => s.pnc = (2, st, s.pnc.2 + 1),
            Event::PncResult(r) => s.pnc = (3, r, s.pnc.2 + 1),
            Event::Devices(d) => s.devices = d,
            Event::Preferred(m) => s.preferred = m,
            Event::Hearing(h) => self.out.push(Effect::Hearing(h)),
            Event::KeyFunctions(t) => s.keyfn = Some(t),
            Event::HoldMask(k, m) => match s.hold_masks.iter_mut().find(|x| x.0 == k) { Some(x) => x.1 = m, None => s.hold_masks.push((k, m)) },
            // A read reply and a push carry the same `(id, value)` pairs.
            Event::Features(f) | Event::FeaturePush(f) => {
                let get = |id: u8| f.iter().find(|x| x.0 == id).map(|x| x.1 == 1);
                // Game mode and Hi-Res follow the engine: no hold, so only the buds move them.
                let game = game_id(s);
                s.low_latency = get(game).or(s.low_latency);
                s.bass_on = get(FEATURE_BASSWAVE).map(|v| held(FEATURE_BASSWAVE, v)).or(s.bass_on);
                for (id, v) in f {
                    let v = if id == game || id == FEATURE_HIRES { v } else { held(id, v == 1) as u8 };
                    match s.features.iter_mut().find(|x| x.0 == id) { Some(x) => x.1 = v, None => s.features.push((id, v)) }
                }
            }
        }
        true
    }

    /// One UI command: the state it shows at once, and the steps it sends.
    fn run_cmd(&mut self, cmd: Cmd) -> Vec<Step> {
        let tx = |f: Frame| Step::Tx(f.0, None, f.1);
        match cmd {
            Cmd::Anc(mode) => {
                // A mode this model does not list is never sent: its bit would be another mode's.
                let Some(bit) = self.s.modes.bit(&mode) else { return vec![] };
                if LEVELS.contains(&mode.as_str()) { self.level = Some(mode.clone()); }
                self.s.anc = Some(mode);
                vec![tx(cmd::anc_set(bit))]
            }
            Cmd::LowLatency(on) => {
                // One engine operation: the switch moves when the buds push the state.
                let key = game_id(&self.s);
                // A model whose buds apply the write and never push sets "gameConfirm":"read" (T300, T310).
                let confirm = if Profile(self.s.model).game_confirm_read() { Confirm::Read } else { Confirm::Push };
                self.note(format!("SYNC: tap key=0x{key:02X} on={on} confirm={confirm:?}"));
                let op = Operation { key, writes: vec![(key, on as u8)], confirm, restarts: false };
                let outs = self.engine.tap(op, self.now);
                self.exec(outs);
                vec![]
            }
            // Each EQ write updates the state first, so the replies before the new values
            // do not snap the UI back.
            Cmd::EqBuiltIn(id) => {
                self.eq_hold.pick(id, self.now);
                self.s.eq_current = Some(id);
                self.eq_write(cmd::eq_select(id))
            }
            Cmd::EqSave(p) => {
                self.eq_hold.pick(p.id, self.now);
                self.s.eq_current = Some(p.id);
                if let Some(x) = self.s.eq_custom.iter_mut().find(|x| x.id == p.id) { *x = p.clone(); }
                self.eq_write((CMD_SAVE_CUSTOM_EQ, p.encode(EQ_SAVE)))
            }
            // No local update: the buds assign and renumber ids.
            Cmd::EqCreate(p) => self.eq_write((CMD_SAVE_CUSTOM_EQ, p.encode(EQ_CREATE))),
            Cmd::EqDelete(p) => self.eq_write((CMD_SAVE_CUSTOM_EQ, p.encode(EQ_DELETE))),
            Cmd::BassWave(on) => {
                self.hold(FEATURE_BASSWAVE, on);
                self.s.bass_on = Some(on);
                vec![Step::Emit, tx(cmd::feature(FEATURE_BASSWAVE, on)), Step::Wait(250), self.status_read()]
            }
            Cmd::BassLevel(l, band) => {
                if band == 0 { self.s.bass_level = Some(l); } else { self.s.bass_bands[band as usize - 1] = Some(l); }
                self.eq_write(cmd::bass_set(l, band))
            }
            // As `setFeatures`: the write, then the status read that shows what the buds kept.
            // Power saving restarts the buds: the link drops and the idle rescan reconnects.
            // Game mode and Hi-Res follow the engine: no hold, and no local move.
            Cmd::Features(changes) => {
                let game = game_id(&self.s);
                for &(id, on) in &changes {
                    if id == FEATURE_HIRES || id == game { continue; }
                    self.hold(id, on);
                    if let Some(x) = self.s.features.iter_mut().find(|x| x.0 == id) { x.1 = on as u8; }
                }
                let mut st = vec![Step::Emit];
                for (id, on) in changes { st.extend([tx(cmd::feature(id, on)), Step::Wait(100)]); }
                st.extend([Step::Wait(300), self.status_read()]);
                st
            }
            Cmd::Hires { on, drop_spatial } => {
                // One engine operation; the Hi-Res write restarts the link. 3D audio off goes first (OPPO-PROTOCOL.md section 18).
                let writes = if drop_spatial { vec![(FEATURE_SPATIAL, 0), (FEATURE_HIRES, on as u8)] } else { vec![(FEATURE_HIRES, on as u8)] };
                let op = Operation { key: FEATURE_HIRES, writes, confirm: Confirm::Read, restarts: true };
                let outs = self.engine.tap(op, self.now);
                self.exec(outs);
                vec![]
            }
            Cmd::Find(on) => vec![tx(cmd::find(on))],
            Cmd::FitTest(on) => vec![tx(cmd::fit_test(on))],
            Cmd::PncQuery => vec![tx(cmd::personal_anc_stored())],
            Cmd::Pnc(action) => {
                let mut st = vec![tx(cmd::personal_anc(action))];
                if action == 2 { st.extend([Step::Wait(600), self.status_read()]); }
                st
            }
            Cmd::StatusRead => vec![self.status_read()],
            // HeyMelody's order: the switch, the status re-read, then the follow-up; the buds push the list.
            Cmd::Dual(on) => {
                self.hold(FEATURE_DUAL, on);
                if let Some(x) = self.s.features.iter_mut().find(|x| x.0 == FEATURE_DUAL) { x.1 = on as u8; }
                vec![Step::Emit, tx(cmd::feature(FEATURE_DUAL, on)), Step::Wait(250), self.status_read(), Step::Wait(250),
                    tx(cmd::phone_state(cmd::PhoneState { auto_switch: Some(!on), ..Default::default() }))]
            }
            Cmd::DualReads(preferred) => {
                let mut st = vec![tx(cmd::devices_read()), Step::Wait(120), self.status_read()];
                if preferred && self.s.caps.supports(CMD_QUERY_PREFERRED) {
                    st.extend([Step::Wait(120), tx(cmd::preferred_read())]);
                }
                st
            }
            Cmd::ConnectDevice(mac, on) => vec![tx(cmd::device_connect(mac, on))],
            Cmd::Preferred(mac) => vec![tx(cmd::preferred_set(mac)), Step::Wait(250), tx(cmd::preferred_read())],
            // As `writeThenRead`: shown at once, then read back.
            Cmd::GameSoundType(t) => {
                if let Some(g) = self.s.game_sound.as_mut() { g.0 = t; }
                vec![Step::Emit, tx(cmd::game_sound(t)), Step::Wait(400), tx(cmd::game_sound_read())]
            }
            // The switch, then the state the phone's watcher would send: the desktop has no app detection.
            Cmd::GameEq(on) => {
                self.hold(FEATURE_GAME_EQ, on);
                if let Some(x) = self.s.features.iter_mut().find(|x| x.0 == FEATURE_GAME_EQ) { x.1 = on as u8; }
                vec![Step::Emit, tx(cmd::feature(FEATURE_GAME_EQ, on)), Step::Wait(150), tx(cmd::game_eq(on)),
                    Step::Wait(300), self.status_read()]
            }
            Cmd::SpatialType(t) => {
                self.s.spatial_type = Some(t);
                vec![Step::Emit, tx(cmd::spatial(t)), Step::Wait(400), tx(cmd::spatial_read())]
            }
            Cmd::Codec(c, hires) => vec![tx(cmd::codec(c, hires))],
            Cmd::HeadMotion(t) => {
                self.s.head_motion = Some(t);
                vec![Step::Emit, tx(cmd::head_gesture(t)), Step::Wait(400), tx(cmd::head_gesture_read())]
            }
            Cmd::ModelPicked => { self.pick_model(); vec![] }
            Cmd::EarbudReads => {
                let mut st = Vec::new();
                for cmd in [CMD_QUERY_ALERT_VOLUME, CMD_QUERY_TAP_LEVEL, CMD_QUERY_GAME_SOUND, CMD_QUERY_HEAD_MOTION] {
                    if !self.s.caps.supports(cmd) { continue; }
                    st.extend([Step::Tx(cmd, None, vec![]), Step::Wait(120)]);
                }
                st
            }
            // Sent on release only, so the buds play one prompt per change; the ack carries the level.
            Cmd::AlertVolume(l) => {
                self.s.alert_volume = Some(l);
                vec![tx(cmd::alert_volume(l.clamp(1, 10)))]
            }
            // Unverified on hardware: read back, so the slider shows what the buds kept.
            Cmd::TapLevel(l) => {
                if let Some(t) = self.s.tap_level.as_mut() { t.0 = l; }
                vec![tx(cmd::tap_level(l.clamp(1, 5))), Step::Wait(250), tx(cmd::tap_level_read())]
            }
            Cmd::ControlsReads(types) => {
                if !self.s.caps.supports(CMD_QUERY_KEY_FUNCTION) { return vec![]; }
                let mut st = vec![Step::Tx(CMD_QUERY_KEY_FUNCTION, None, vec![])];
                for t in types { st.extend([Step::Wait(120), tx(cmd::anc_read(2, t))]); }
                st
            }
            // The whole table goes back with this bud's slots changed; adopted at once so a second
            // write before the read-back does not undo the first. No table read: nothing is sent.
            Cmd::Gesture(dev, act, f) => {
                let Some(table) = self.s.keyfn.as_mut() else { return vec![] };
                let mut hit = false;
                for e in table.iter_mut().filter(|e| e[0] == dev && e[2] == act && e[1] != BUTTON_ON_CALL) { e[3] = f; hit = true; }
                if !hit { return vec![]; }
                let p = cmd::gesture_table(&table.iter().map(|e| crate::msg::Binding { dev: e[0], button: e[1], act: e[2], func: e[3] }).collect::<Vec<_>>());
                vec![Step::Emit, tx(p), Step::Wait(600), Step::Tx(CMD_QUERY_KEY_FUNCTION, None, vec![])]
            }
            Cmd::HoldModes(mask, t) => vec![tx(cmd::hold_cycle(t, mask)), Step::Wait(400), tx(cmd::anc_read(2, t))],
            Cmd::OnCall(btn, act, f) => vec![
                tx(cmd::gesture_table(&[crate::msg::Binding { dev: 4, button: btn, act, func: f }])),
                Step::Wait(400), Step::Tx(CMD_QUERY_KEY_FUNCTION, None, vec![])],
            Cmd::Hearing(frames) => {
                let switch = frames.iter().any(|f| f.0 == CMD_SET_FEATURE);
                let mut st = Vec::new();
                for (cmd, p) in frames { st.extend([Step::Tx(cmd, None, p), Step::Wait(60)]); }
                if switch { st.extend([Step::Wait(400), self.status_read()]); }
                st
            }
            Cmd::BatteryRead => vec![Step::Tx(CMD_QUERY_BATTERY, None, vec![])],
            Cmd::Connect | Cmd::Disconnect => vec![],
        }
    }
}
