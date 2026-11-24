//! The Soundcore models: one table per model. Each table lists the state packet (`01 01`) field by field, in
//! order, and the features the model has. The driver walks the fields and records each field's offset, so a
//! write finds its bytes by the field key. Source: OpenSCQ30 `lib/src/devices/soundcore/<model>`.
//! Copyright 2026 Kyle Scheuing, GPL-3.0-or-later.

use super::presets::{self, Preset};

/// One field of the state packet: its key and its length in bytes. An empty key is a byte the driver keeps
/// but does not read. An optional field is absent when the packet ends before it.
#[derive(Clone, Copy, Debug)]
pub struct Fd {
    pub key: &'static str,
    pub len: usize,
    pub opt: bool,
}

const fn f(key: &'static str, len: usize) -> Fd { Fd { key, len, opt: false } }
const fn o(key: &'static str, len: usize) -> Fd { Fd { key, len, opt: true } }
const fn skip(len: usize) -> Fd { f("", len) }
/// A3953: the length of this gap is the byte before the buttons minus 18.
pub const GAP: usize = usize::MAX;

/// The equalizer field: preset id (u16 LE), then `channels` x `bands` bytes.
const fn eq(channels: usize, bands: usize) -> usize { 2 + channels * bands }

/// Where the parts of a HearID block are, from its start. `n` = channels x bands.
#[derive(Clone, Copy, Debug)]
pub struct Hid {
    pub len: usize,
    /// The HearID bands, `n` bytes.
    pub bands: usize,
    pub n: usize,
    /// Time (u32 big-endian), type, the custom bands (`n` bytes) and the music genre (1 or 2 bytes):
    /// None for a block without them.
    pub time: usize,
    pub kind: Option<usize>,
    pub custom: Option<usize>,
    pub genre: Option<(usize, usize)>,
}

/// OpenSCQ30 `BasicHearId`: on, bands, time.
const fn hid_basic(n: usize) -> Hid { Hid { len: 5 + n, bands: 1, n, time: 1 + n, kind: None, custom: None, genre: None } }
/// `CustomHearId::take`: on, bands, time, type, genre (1 byte), custom bands.
const fn hid_mid(n: usize) -> Hid {
    Hid { len: 7 + 2 * n, bands: 1, n, time: 1 + n, kind: Some(5 + n), custom: Some(7 + n), genre: Some((6 + n, 1)) }
}
/// `CustomHearId::take_with_music_genre_at_end`: on, bands, time, type, custom bands, genre (2 bytes).
const fn hid_end(n: usize) -> Hid {
    Hid { len: 8 + 2 * n, bands: 1, n, time: 1 + n, kind: Some(5 + n), custom: Some(6 + n), genre: Some((6 + 2 * n, 2)) }
}
/// A3035: on, bands, time, type, custom bands, genre (1 byte).
const fn hid_a3035(n: usize) -> Hid {
    Hid { len: 7 + 2 * n, bands: 1, n, time: 1 + n, kind: Some(5 + n), custom: Some(6 + n), genre: Some((6 + 2 * n, 1)) }
}
/// A3040: on, bands (20), time, type, custom bands (20), 10 DRC bytes, genre (2 bytes).
const HID_A3040: Hid = Hid { len: 58, bands: 1, n: 20, time: 21, kind: Some(25), custom: Some(26), genre: Some((56, 2)) };
/// A3947: on, bands (20), time, type, custom bands (20), genre (1 byte), 1 unknown byte.
const HID_A3947: Hid = Hid { len: 48, bands: 1, n: 20, time: 21, kind: Some(25), custom: Some(26), genre: Some((46, 1)) };

/// The battery field (`battery`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Bat {
    /// Level, charging.
    Single,
    /// Left, right, left charging, right charging.
    Dual,
    /// Level only.
    SingleLevel,
    /// Left, right.
    DualLevel,
}

/// A battery level as the app shows it: `raw + offset` out of `max`.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    pub max: u8,
    pub offset: u8,
}
const S5: Scale = Scale { max: 5, offset: 0 };
const S10: Scale = Scale { max: 10, offset: 0 };
const S10_1: Scale = Scale { max: 10, offset: 1 };

/// The equalizer command of a model, with the HearID block turned off so the bands apply.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum EqW {
    /// `02 81`: preset, bands.
    Plain,
    /// `02 83`: preset, bands, the DRC bands.
    Drc,
    /// `02 83`: preset, bands (A3388).
    Raw83,
    /// `02 83`: preset, channel 1, channel 1 with DRC (A3876).
    A3876,
    /// `03 87` (`03 86` when the age range is 0xFF and not forced): OpenSCQ30 `SetEqualizerAndCustomHearId`.
    HearId { force: bool },
    A3035,
    A3040,
    A3062,
    A3947,
    /// A3953, A3955: the bytes after the bands depend on the HearID status byte (`hidStatus`).
    A3953,
    /// A3954, A3957, D1202: the active bands, each followed by a 0.
    A3954,
    D1402,
}

#[derive(Clone, Copy, Debug)]
pub struct Eq {
    pub channels: usize,
    pub bands: usize,
    pub presets: &'static [Preset],
    /// The model takes a custom curve (preset `FE FE`).
    pub custom: bool,
    pub write: EqW,
    pub hid: Option<Hid>,
    /// The state has one channel, the write sends it on both (A3388, D1101).
    pub dup: bool,
    /// The bands count whole dB (A3909): a band byte is dB + 12.
    pub whole_db: bool,
    /// The state holds the preset id only (A3909).
    pub preset_only: bool,
}

const fn eqs(channels: usize, bands: usize, presets: &'static [Preset], write: EqW) -> Eq {
    Eq { channels, bands, presets, custom: true, write, hid: None, dup: false, whole_db: false, preset_only: false }
}
const fn eqh(channels: usize, bands: usize, presets: &'static [Preset], write: EqW, hid: Hid) -> Eq {
    Eq { hid: Some(hid), ..eqs(channels, bands, presets, write) }
}

/// The "on" byte of a button.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum En {
    No,
    /// 0 or 1.
    One,
    /// Low nibble: on while both buds are connected; high nibble: on while one bud is alone.
    Tws,
}
/// The action byte of a button.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    One,
    /// Low nibble while both buds are connected, high nibble while one bud is alone.
    Tws,
}
/// How a button turns off.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Off {
    No,
    /// Off on one side turns the other side off too; on turns it on with its last action.
    Pair,
    Each,
}

pub type Actions = &'static [(u8, &'static str)];
pub const A_MIN: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (6, "playPause")];
pub const A_NO_MODES: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (5, "assistant"), (6, "playPause")];
pub const A_COMMON: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (4, "noiseMode"), (5, "assistant"), (6, "playPause")];
pub const A_GAME: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (4, "noiseMode"), (5, "assistant"), (6, "playPause"), (9, "gaming")];
const A_A3876: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (5, "assistant"), (6, "playPause"), (14, "lights")];
const A_A3909: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (5, "assistant")];
const A_D1101_MULTI: Actions = &[(2, "previous"), (3, "next"), (5, "assistant"), (6, "playPause")];
const A_D1101_LONG: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (5, "assistant")];
const A_D1301: Actions = &[(0, "volumeUp"), (1, "volumeDown"), (2, "previous"), (3, "next"), (6, "playPause"), (8, "noiseCanceling"), (13, "changeMode")];
const A_CALL: Actions = &[(0, "declineCall"), (1, "answerCall")];
const A_SLIDE_UP: Actions = &[(0, "volumeUp")];
const A_SLIDE_DOWN: Actions = &[(1, "volumeDown")];
const A_BASS: Actions = &[(7, "bassUp")];
const A_MFB: Actions = &[(5, "assistant"), (11, "ankaAssistant"), (10, "translation"), (13, "napMode")];

/// A press kind: its key, its button id in the commands, its bytes and its actions.
#[derive(Clone, Copy, Debug)]
pub struct Press {
    pub key: &'static str,
    pub id: u8,
    pub en: En,
    pub act: Act,
    pub actions: Actions,
    pub off: Off,
}
const fn press(key: &'static str, id: u8, en: En, act: Act, actions: Actions, off: Off) -> Press { Press { key, id, en, act, actions, off } }

impl Press {
    pub fn len(&self) -> usize { if self.en == En::No { 1 } else { 2 } }
}

/// A button group: the field key that holds it, the buttons in state order (press index, side 0 left 1 right).
#[derive(Clone, Copy, Debug)]
pub struct Buttons {
    pub key: &'static str,
    pub presses: &'static [Press],
    pub order: &'static [(usize, u8)],
    /// `04 84` writes every button at once when two change (one side off turns the other off).
    pub set_all: bool,
    /// The "on" byte is not read: action 0xF is off.
    pub ignore_enabled: bool,
    /// The action command: `04 81`, or another one (A3330 call buttons `04 87`, D1402 MFB `04 61`).
    pub cmd: [u8; 2],
}

impl Buttons {
    pub fn len(&self) -> usize { self.order.iter().map(|&(p, _)| self.presses[p].len()).sum() }
}

const LR3: &[(usize, u8)] = &[(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1)];
const LR4: &[(usize, u8)] = &[(0, 0), (0, 1), (1, 0), (1, 1), (2, 0), (2, 1), (3, 0), (3, 1)];
const LR2: &[(usize, u8)] = &[(0, 0), (0, 1), (1, 0), (1, 1)];
/// OpenSCQ30 `COMMON_SETTINGS` order: left double, left long, right double, right long, left single, right single.
const COMMON_ORDER: &[(usize, u8)] = &[(0, 0), (1, 0), (0, 1), (1, 1), (2, 0), (2, 1)];

const fn btns(presses: &'static [Press], order: &'static [(usize, u8)], ignore_enabled: bool) -> Buttons {
    Buttons { key: "buttons", presses, order, set_all: false, ignore_enabled, cmd: [0x04, 0x81] }
}

const COMMON_BUTTONS: Buttons = Buttons {
    set_all: true,
    ..btns(&[
        press("double", 0, En::One, Act::Tws, A_COMMON, Off::No),
        press("long", 1, En::One, Act::Tws, A_COMMON, Off::No),
        press("single", 2, En::One, Act::One, A_COMMON, Off::Pair),
    ], COMMON_ORDER, false)
};
const fn no_tws(key: &'static str, id: u8, actions: Actions) -> Press { press(key, id, En::No, Act::Tws, actions, Off::Each) }
const fn tws_tws(key: &'static str, id: u8, actions: Actions, off: Off) -> Press { press(key, id, En::Tws, Act::Tws, actions, off) }
/// The double press of the headphones with one button action (A3035, A3040, A3062, D1402).
const BASS_BUTTON: Buttons = Buttons { key: "buttons", presses: &[press("double", 0, En::No, Act::One, A_BASS, Off::Each)], order: &[(0, 0)], set_all: false, ignore_enabled: false, cmd: [0x04, 0x81] };

/// How a sound mode field sits in its byte.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sk {
    Byte,
    /// High nibble.
    Hi,
    /// Low nibble.
    Lo,
    Bit0,
    Bit1,
    /// A3954: one slider byte, 1..5 noise cancelling (6 - level), 7..11 transparency (6 + level).
    SliderAnc,
    SliderTransparency,
}

/// A field of the sound mode group (`06 01`, `06 81`).
#[derive(Clone, Copy, Debug)]
pub struct Sf {
    pub key: &'static str,
    pub at: usize,
    pub kind: Sk,
    /// The values the field takes. Empty: read only.
    pub values: &'static [u8],
    /// The field applies only while each (key, values) holds.
    pub parent: &'static [(&'static str, &'static [u8])],
    /// The name of each value, as the apps translate it. A value without a name shows as its number.
    pub labels: &'static [(u8, &'static str)],
}
const fn sf(key: &'static str, at: usize, kind: Sk, values: &'static [u8], parent: &'static [(&'static str, &'static [u8])]) -> Sf {
    Sf { key, at, kind, values, parent, labels: &[] }
}
const fn sfl(key: &'static str, at: usize, kind: Sk, values: &'static [u8], parent: &'static [(&'static str, &'static [u8])],
    labels: &'static [(u8, &'static str)]) -> Sf {
    Sf { key, at, kind, values, parent, labels }
}

/// Value names: the noise cancelling modes, the transparency modes, the transport types, the strengths.
const N_V1: &[(u8, &str)] = &[(0, "transport"), (1, "outdoor"), (2, "indoor"), (3, "custom")];
const N_MA: &[(u8, &str)] = &[(0, "manual"), (1, "adaptive")];
const N_MAT: &[(u8, &str)] = &[(0, "manual"), (1, "adaptive"), (2, "transport")];
const N_MAS: &[(u8, &str)] = &[(0, "manual"), (1, "adaptive"), (2, "multiScene")];
const N_CA: &[(u8, &str)] = &[(0, "custom"), (1, "adaptive")];
const N_TR: &[(u8, &str)] = &[(0, "full"), (1, "vocal")];
const N_TALK: &[(u8, &str)] = &[(0, "talk"), (1, "manual")];
const N_PLANE_CAR: &[(u8, &str)] = &[(0, "plane"), (3, "car")];
const N_TRANSPORT: &[(u8, &str)] = &[(0, "plane"), (1, "train"), (2, "bus"), (3, "car")];
const N_SCENE: &[(u8, &str)] = &[(0, "transport"), (1, "outdoor"), (2, "indoor")];
const N_STRENGTH: &[(u8, &str)] = &[(1, "weak"), (2, "moderate"), (3, "strong")];
const N_NOISE_LEVEL: &[(u8, &str)] = &[(0, "lowNoise"), (1, "mediumNoise"), (2, "highNoise")];
const N_STRENGTH0: &[(u8, &str)] = &[(0, "weak"), (1, "moderate"), (2, "strong")];
const N_AIRPLANE: &[(u8, &str)] = &[(0, "manualUpdate"), (1, "automaticUpdate")];

#[derive(Clone, Copy, Debug)]
pub struct Modes {
    pub fields: &'static [Sf],
    /// The buds take a field only while its parent mode is on (OpenSCQ30 `MigrationSteps`): the driver refuses it.
    pub enforce: bool,
    /// A byte that repeats the noise control mode (A3035, A3959).
    pub mirror: Option<usize>,
    /// Bytes with a fixed value in every write.
    pub fixed: &'static [(usize, u8)],
}

const NC: &[(&str, &[u8])] = &[("noise", &[0])];
const TR: &[(&str, &[u8])] = &[("noise", &[1])];
const NC_TR: &[(&str, &[u8])] = &[("noise", &[0, 1])];
const L5: &[u8] = &[1, 2, 3, 4, 5];
const L3: &[u8] = &[1, 2, 3];
const ONOFF: &[u8] = &[0, 1];
const NOISE3: &[u8] = &[0, 1, 2];

const fn modes(fields: &'static [Sf]) -> Modes { Modes { fields, enforce: false, mirror: None, fixed: &[] } }
const fn enforced(fields: &'static [Sf]) -> Modes { Modes { fields, enforce: true, mirror: None, fixed: &[] } }

// OpenSCQ30 `SoundModes` (4 bytes): noise control, noise cancelling mode (0 transport, 1 outdoor, 2 indoor,
// 3 custom), transparency mode, custom level 0..10.
const V1_NOISE: Modes = modes(&[sf("noise", 0, Sk::Byte, NOISE3, &[])]);
const V1_ANC: Modes = modes(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 1, Sk::Byte, &[0, 2, 1], NC, N_V1),
]);
const V1_FULL: Modes = modes(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 1, Sk::Byte, &[0, 1, 2], NC, N_V1),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
]);
const V1_CUSTOM: Modes = modes(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 1, Sk::Byte, &[0, 2, 1, 3], NC, N_V1),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
    sf("customAnc", 3, Sk::Byte, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[("ancMode", &[3])]),
]);
const V1_A3930: Modes = modes(&[sf("noise", 0, Sk::Byte, &[1, 2], &[])]);

/// The noise control mode of a model with a noise cancelling mode byte at 3.
const fn manual(id: u8) -> &'static [(&'static str, &'static [u8])] {
    match id { 0 => &[("noise", &[0]), ("ancMode", &[0])], _ => &[("noise", &[0]), ("ancMode", &[1])] }
}
const ADAPTIVE: &[(&str, &[u8])] = &[("noise", &[0]), ("ancMode", &[1])];
const TRANSPORT: &[(&str, &[u8])] = &[("noise", &[0]), ("ancMode", &[2])];

const A3936_MODES: Modes = modes(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 3, Sk::Byte, ONOFF, NC, N_MA),
    sfl("ancLevel", 1, Sk::Hi, L3, manual(0), N_STRENGTH),
    sfl("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE, N_NOISE_LEVEL),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
    sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
    sf("windDetected", 4, Sk::Bit1, &[], &[]),
]);
const A3957_MODES: Modes = enforced(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 3, Sk::Byte, &[0, 2], NC, N_MAT),
    sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
    sfl("transport", 6, Sk::Byte, &[0, 3], TRANSPORT, N_PLANE_CAR),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
    sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
    sf("windDetected", 4, Sk::Bit1, &[], &[]),
]);
// A3035, A3062: noise cancelling mode 0 custom, 1 adaptive; custom level high nibble; transparency level at 5.
const A3035_MODES: Modes = Modes {
    mirror: Some(2),
    ..enforced(&[
        sf("noise", 0, Sk::Byte, NOISE3, &[]),
        sfl("ancMode", 3, Sk::Byte, ONOFF, NC, N_CA),
        sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
        sf("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE),
        sf("transparencyLevel", 5, Sk::Byte, L5, TR),
        sf("windNoise", 4, Sk::Byte, ONOFF, NC_TR),
    ])
};
const A3062_MODES: Modes = Modes { mirror: None, ..A3035_MODES };
const A3040_MODES: Modes = enforced(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 3, Sk::Byte, ONOFF, NC, N_MA),
    sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TALK),
    sf("transparencyLevel", 5, Sk::Byte, L5, &[("noise", &[1]), ("transparencyMode", &[1])]),
    sf("windNoise", 4, Sk::Byte, ONOFF, NC_TR),
]);
const A3947_MODES: Modes = enforced(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 3, Sk::Byte, NOISE3, NC, N_MAT),
    sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
    sf("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE),
    sf("environmentDetection", 5, Sk::Byte, ONOFF, ADAPTIVE),
    sfl("transport", 6, Sk::Byte, &[0, 1, 2, 3], TRANSPORT, N_TRANSPORT),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
    sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
]);
const A3952_MODES: Modes = enforced(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 3, Sk::Byte, ONOFF, NC, N_MA),
    sfl("ancLevel", 1, Sk::Hi, L3, manual(0), N_STRENGTH),
    sfl("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE, N_NOISE_LEVEL),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
    sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
]);
const A3953_MODES: Modes = modes(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sf("windNoise", 4, Sk::Byte, ONOFF, &[]),
]);
const A3954_MODES: Modes = modes(&[
    sf("noise", 0, Sk::Byte, &[0, 1, 2, 3], &[]),
    sf("ancLevel", 1, Sk::SliderAnc, L5, NC),
    sf("transparencyLevel", 1, Sk::SliderTransparency, L5, TR),
    sfl("airplane", 2, Sk::Byte, ONOFF, &[("noise", &[3])], N_AIRPLANE),
    sf("windNoise", 3, Sk::Bit0, ONOFF, &[]),
    sf("windDetected", 3, Sk::Bit1, &[], &[]),
]);
const MULTI_SCENE: &[(&str, &[u8])] = &[("noise", &[0]), ("ancMode", &[2])];
const A3955_MODES: Modes = enforced(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sfl("ancMode", 3, Sk::Byte, NOISE3, NC, N_MAS),
    sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
    sfl("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE, N_STRENGTH),
    sfl("multiScene", 6, Sk::Byte, NOISE3, MULTI_SCENE, N_SCENE),
    sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
    sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
    sf("windDetected", 4, Sk::Bit1, &[], &[]),
]);
const A3959_MODES: Modes = Modes {
    mirror: Some(2),
    ..enforced(&[
        sf("noise", 0, Sk::Byte, NOISE3, &[]),
        sfl("ancMode", 3, Sk::Byte, NOISE3, NC, N_MAS),
        sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
        sf("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE),
        sfl("multiScene", 6, Sk::Byte, NOISE3, MULTI_SCENE, N_SCENE),
        sf("sensitivity", 5, Sk::Byte, &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10], &[]),
        sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
        sf("windDetected", 4, Sk::Bit1, &[], &[]),
    ])
};
const A3968_MODES: Modes = Modes {
    fixed: &[(5, 0xFF)],
    ..modes(&[
        sf("noise", 0, Sk::Byte, NOISE3, &[]),
        sfl("ancMode", 3, Sk::Byte, ONOFF, NC, N_MA),
        sfl("ancLevel", 1, Sk::Hi, L3, manual(0), N_STRENGTH),
        sfl("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE, N_NOISE_LEVEL),
        sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
        sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
        sf("windDetected", 4, Sk::Bit1, &[], &[]),
    ])
};
const D1202_MODES: Modes = Modes {
    fixed: &[(5, 0)],
    ..modes(&[
        sf("noise", 0, Sk::Byte, NOISE3, &[]),
        sfl("ancMode", 3, Sk::Byte, NOISE3, NC, N_MAS),
        sf("ancLevel", 1, Sk::Hi, L5, manual(0)),
        sfl("adaptiveLevel", 1, Sk::Lo, &[], ADAPTIVE, N_STRENGTH0),
        sfl("multiScene", 6, Sk::Byte, NOISE3, MULTI_SCENE, N_SCENE),
        sf("realTimeAdaptive", 7, Sk::Byte, ONOFF, ADAPTIVE),
        sfl("transparencyMode", 2, Sk::Byte, ONOFF, TR, N_TR),
        sf("windNoise", 4, Sk::Bit0, ONOFF, NC_TR),
        sf("windDetected", 4, Sk::Bit1, &[], &[]),
    ])
};
const D1402_MODES: Modes = enforced(&[
    sf("noise", 0, Sk::Byte, NOISE3, &[]),
    sf("ancLevel", 1, Sk::Hi, L5, NC),
]);

/// The noise control cycle (`06 82`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Cycle {
    No,
    /// Bit 0 noise cancelling, bit 1 transparency, bit 2 normal.
    Bits,
    /// The same bits per nibble: low while both buds are connected, high while one bud is alone.
    Tws,
}

/// A one-byte setting: its field key, its write command, and its values.
#[derive(Clone, Copy, Debug)]
pub struct Set {
    pub key: &'static str,
    pub cmd: [u8; 2],
    pub kind: Kind,
}
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Switch,
    /// A switch whose byte is the opposite (1 = buttons off).
    Inverted,
    Choice(&'static [u8]),
}
const fn sw(key: &'static str, cmd: [u8; 2]) -> Set { Set { key, cmd, kind: Kind::Switch } }
const fn choice(key: &'static str, cmd: [u8; 2], values: &'static [u8]) -> Set { Set { key, cmd, kind: Kind::Choice(values) } }

const TOUCH: Set = sw("touchTone", [0x01, 0x83]);
const GAMING: Set = sw("gaming", [0x01, 0x87]);
const GAMING_10: Set = sw("gaming", [0x10, 0x85]);
const LEAK: Set = sw("leak", [0x10, 0x86]);
const SURROUND: Set = sw("surround", [0x02, 0x86]);
const AUTO_PLAY: Set = sw("autoPlay", [0x01, 0x81]);
const WEARING_TONE: Set = sw("wearingTone", [0x01, 0x8C]);
const TOUCH_LOCK: Set = sw("touchLock", [0x04, 0x86]);
const LOW_BATTERY: Set = sw("lowBattery", [0x10, 0x82]);
const WEAR: Set = sw("wearDetection", [0x01, 0x81]);
const VOICE: Set = sw("voicePrompt", [0x01, 0x90]);
const VOICE_AE: Set = sw("voicePrompt", [0x01, 0xAE]);
const LOW_BATTERY_AF: Set = sw("lowBattery", [0x01, 0xAF]);
const LDAC: Set = sw("ldac", [0x01, 0xFF]);
const BUTTONS_OFF: Set = Set { key: "buttonsOff", cmd: [0x10, 0x94], kind: Kind::Inverted };

/// A setting of several parts, or with its own write body (spatial audio, the case features, easy chat, lights).
#[derive(Clone, Copy, Debug)]
pub struct Group {
    pub key: &'static str,
    pub parts: &'static [Part],
    /// The writes: a part change sends each write that lists the part.
    pub writes: &'static [GroupWrite],
}

/// One part of a group: a byte of a field.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub key: &'static str,
    pub field: &'static str,
    pub at: usize,
    pub kind: PartKind,
    pub labels: &'static [(u8, &'static str)],
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PartKind {
    Switch,
    Choice(&'static [u8]),
    /// A number from min to max.
    Range(u8, u8),
    /// Read only.
    Info,
    /// 3 bytes: red, green, blue.
    Color,
}

#[derive(Clone, Copy, Debug)]
pub struct GroupWrite {
    pub cmd: [u8; 2],
    pub when: &'static [&'static str],
    pub body: &'static [Src],
}

/// A byte of a group write body.
#[derive(Clone, Copy, Debug)]
pub enum Src {
    Part(&'static str),
    /// The 3 color bytes of a part.
    Rgb(&'static str),
    Byte(u8),
}

const fn part(key: &'static str, field: &'static str, at: usize, kind: PartKind) -> Part { Part { key, field, at, kind, labels: &[] } }
const fn partl(key: &'static str, field: &'static str, at: usize, kind: PartKind, labels: &'static [(u8, &'static str)]) -> Part {
    Part { key, field, at, kind, labels }
}

const N_EFFECT: &[(u8, &str)] = &[(1, "fixed"), (2, "headTracking")];
const SPATIAL_A3953: Group = Group {
    key: "spatial",
    parts: &[
        part("on", "spatial", 0, PartKind::Switch),
        partl("effect", "spatial", 1, PartKind::Choice(&[1, 2]), N_EFFECT),
        partl("content", "spatial", 2, PartKind::Choice(&[0, 1]), &[(0, "music"), (1, "movie")]),
    ],
    writes: &[GroupWrite { cmd: [0x10, 0x81], when: &["on", "effect", "content"], body: &[Src::Part("on"), Src::Part("effect"), Src::Part("content")] }],
};
const SPATIAL_A3954: Group = Group {
    key: "spatial",
    parts: &[
        part("on", "spatial", 0, PartKind::Switch),
        partl("effect", "spatial", 1, PartKind::Choice(&[1, 2]), N_EFFECT),
        partl("content", "spatial", 2, PartKind::Choice(&[0, 1, 2, 3]), &[(0, "music"), (1, "podcast"), (2, "movie"), (3, "gaming")]),
    ],
    writes: &[GroupWrite { cmd: [0x10, 0x81], when: &["on", "effect", "content"], body: &[Src::Part("on"), Src::Part("effect"), Src::Part("content")] }],
};
/// D1202: the state holds on and the mode; the write puts a 0 between them.
const SPATIAL_D1202: Group = Group {
    key: "spatial",
    parts: &[
        part("on", "spatial", 0, PartKind::Switch),
        partl("content", "spatial", 1, PartKind::Choice(&[0, 2, 3]), &[(0, "music"), (2, "movie"), (3, "gaming")]),
    ],
    writes: &[GroupWrite { cmd: [0x10, 0x81], when: &["on", "content"], body: &[Src::Part("on"), Src::Byte(0), Src::Part("content")] }],
};
const CASE_A3954: Group = Group {
    key: "caseFeatures",
    parts: &[
        part("atmospheric", "caseFeatures", 0, PartKind::Switch),
        part("remoteCamera", "caseFeatures", 1, PartKind::Switch),
        part("findDevice", "caseFeatures", 2, PartKind::Switch),
        part("caseSpatial", "caseFeatures", 3, PartKind::Switch),
    ],
    writes: &[GroupWrite { cmd: [0x07, 0x87], when: &["atmospheric", "remoteCamera", "findDevice", "caseSpatial"],
        body: &[Src::Part("atmospheric"), Src::Part("remoteCamera"), Src::Part("findDevice"), Src::Part("caseSpatial")] }],
};
const EASY_CHAT_A3954: Group = Group {
    key: "easyChat",
    parts: &[
        part("on", "easyChat", 0, PartKind::Switch),
        partl("wait", "easyChatWait", 0, PartKind::Choice(&[0, 1, 2]), &[(0, "s5"), (1, "s10"), (2, "s15")]),
        part("airPressure", "airPressure", 0, PartKind::Info),
    ],
    writes: &[GroupWrite { cmd: [0x10, 0x9D], when: &["on", "wait"], body: &[Src::Part("on"), Src::Part("wait")] }],
};
const LIGHTS_A3876: Group = Group {
    key: "lights",
    parts: &[
        part("on", "lights", 0, PartKind::Switch),
        part("brightness", "lights", 1, PartKind::Range(1, 10)),
        part("autoOff", "lights", 2, PartKind::Range(1, 120)),
        partl("mode", "lights", 3, PartKind::Choice(&[0, 1, 2, 3]), &[(0, "lightOn"), (1, "breathing"), (2, "flash"), (3, "musicFollows")]),
        part("color", "lights", 4, PartKind::Color),
    ],
    writes: &[
        GroupWrite { cmd: [0x0E, 0x87], when: &["on"], body: &[Src::Part("on")] },
        GroupWrite { cmd: [0x0E, 0x82], when: &["brightness"], body: &[Src::Part("brightness")] },
        GroupWrite { cmd: [0x0E, 0x84], when: &["autoOff"], body: &[Src::Part("autoOff")] },
        GroupWrite { cmd: [0x0E, 0x86], when: &["mode"], body: &[Src::Part("mode")] },
        GroupWrite { cmd: [0x0E, 0x85], when: &["color"], body: &[Src::Rgb("color")] },
    ],
};
/// D1301: what the earbuds do once you sleep. The official app fills the timer bytes of `15 85` with 0xFF.
const POST_SLEEP_D1301: Group = Group {
    key: "postSleep",
    parts: &[partl("action", "postSleep", 0, PartKind::Choice(&[0, 1]), &[(0, "pause"), (1, "playLocal")])],
    writes: &[GroupWrite { cmd: [0x15, 0x85], when: &["action"], body: &[Src::Byte(0xFF), Src::Byte(0xFF), Src::Byte(0xFF), Src::Part("action")] }],
};

/// Minutes of each auto power off index.
const HALF_HOURS: &[u16] = &[30, 60, 90, 120];
const TENS: &[u16] = &[10, 20, 30, 60];
const ALL_DURATIONS: &[u16] = &[10, 20, 30, 60, 90, 120, 180, 300];
const LONG_DURATIONS: &[u16] = &[30, 60, 180, 300];

pub struct Model {
    /// The model numbers this table serves; a serial number starts with one of them.
    pub ids: &'static [&'static str],
    /// The Bluetooth names of the model, for the models whose state has no serial number.
    pub names: &'static [&'static str],
    pub fields: &'static [Fd],
    pub battery: (Bat, Scale),
    pub case: Option<Scale>,
    pub eq: Option<Eq>,
    pub buttons: &'static [Buttons],
    pub modes: Option<Modes>,
    pub cycle: Cycle,
    pub auto_off: &'static [u16],
    pub sets: &'static [Set],
    /// Dual connection (`dual` field): `0b 84` and the device list.
    pub dual: bool,
    pub groups: &'static [Group],
    /// The D1301 sleep timer: asked with `15 03`, written with `15 85`.
    pub timer: bool,
}

const fn model(ids: &'static [&'static str], names: &'static [&'static str], fields: &'static [Fd], battery: (Bat, Scale)) -> Model {
    Model { ids, names, fields, battery, case: None, eq: None, buttons: &[], modes: None, cycle: Cycle::No, auto_off: &[], sets: &[], dual: false, groups: &[], timer: false }
}

const SINGLE5: (Bat, Scale) = (Bat::Single, S5);
const DUAL5: (Bat, Scale) = (Bat::Dual, S5);

/// Every model. The order does not matter: the serial number or the name picks one.
pub const MODELS: &[Model] = &[
    Model {
        eq: Some(eqs(1, 10, &presets::COMMON, EqW::Drc)),
        modes: Some(V1_NOISE),
        ..model(&["3004"], &["Soundcore Q20i"], &[f("battery", 2), f("fw", 5), f("serial", 16), f("eq", eq(1, 10)), f("modes", 4)], SINGLE5)
    },
    Model {
        eq: Some(eqs(1, 10, &presets::A3005, EqW::Drc)),
        auto_off: HALF_HOURS,
        dual: true,
        ..model(&["3005"], &["Soundcore Q11i"],
            &[f("battery", 2), f("fw", 5), f("serial", 16), f("eq", eq(1, 10)), skip(6), f("dual", 1), skip(4), f("autoOff", 2)],
            (Bat::Single, S10_1))
    },
    Model {
        eq: Some(eqs(1, 8, &presets::COMMON, EqW::Plain)),
        modes: Some(V1_ANC),
        sets: &[WEAR],
        ..model(&["3027", "3030"], &["Soundcore Life Q35", "Soundcore Life Tune Pro"],
            &[f("battery", 2), f("eq", eq(1, 8)), f("gender", 1), f("age", 1), f("hearId", hid_basic(16).len), f("modes", 4),
              f("fw", 5), f("serial", 16), f("wearDetection", 1), o("", 1)], SINGLE5)
    },
    Model {
        eq: Some(eqs(1, 8, &presets::COMMON, EqW::Plain)),
        modes: Some(V1_ANC),
        auto_off: HALF_HOURS,
        ..model(&["3028", "3029"], &["Soundcore Life Q30", "Soundcore Q30", "Soundcore Life Tune"],
            &[f("battery", 2), f("eq", eq(1, 8)), f("gender", 1), f("age", 1), f("hearId", hid_basic(16).len), f("modes", 4),
              f("fw", 5), f("serial", 16), o("", 3), o("autoOff", 2), o("", 2)], SINGLE5)
    },
    Model {
        eq: Some(eqs(2, 8, &presets::COMMON, EqW::Plain)),
        modes: Some(V1_ANC),
        auto_off: HALF_HOURS,
        sets: &[TOUCH],
        ..model(&["3031"], &["Soundcore Vortex"],
            &[f("tws", 2), f("battery", 4), f("eq", eq(2, 8)), skip(12), f("modes", 4), skip(1), f("touchTone", 1), f("autoOff", 2)], DUAL5)
    },
    Model {
        eq: Some(eqs(1, 8, &presets::COMMON, EqW::Plain)),
        sets: &[WEAR],
        ..model(&["3033"], &["Soundcore Life 2 Neo"],
            &[f("battery", 2), f("eq", eq(1, 8)), f("fw", 5), f("serial", 16), f("wearDetection", 1)], SINGLE5)
    },
    Model {
        eq: Some(eqh(1, 10, &presets::A3035, EqW::A3035, hid_a3035(10))),
        buttons: &[BASS_BUTTON],
        modes: Some(A3035_MODES),
        cycle: Cycle::Bits,
        auto_off: HALF_HOURS,
        sets: &[AUTO_PLAY, LDAC, VOICE_AE, LOW_BATTERY_AF],
        dual: true,
        ..model(&["3035"], &["Soundcore Space One"],
            &[f("battery", 1), skip(1), f("fw", 5), f("serial", 16), f("eq", eq(1, 10)), skip(2), f("hearId", hid_a3035(10).len),
              skip(3), f("buttons", 1), f("cycle", 1), f("modes", 6), skip(1), f("autoPlay", 1), skip(4), f("ldac", 1), f("dual", 1),
              skip(2), f("autoOff", 2), f("volumeLimit", 3), f("voicePrompt", 1), f("lowBattery", 1)],
            (Bat::SingleLevel, S5))
    },
    Model {
        eq: Some(eqh(1, 10, &presets::A3040, EqW::A3040, HID_A3040)),
        buttons: &[BASS_BUTTON],
        modes: Some(A3040_MODES),
        cycle: Cycle::Bits,
        auto_off: HALF_HOURS,
        sets: &[LDAC, VOICE_AE, LOW_BATTERY_AF],
        dual: true,
        ..model(&["3040"], &["Soundcore Space Q45"],
            &[f("battery", 1), skip(1), f("fw", 5), f("serial", 16), f("eq", eq(1, 10)), skip(10), skip(2), f("buttons", 1),
              f("cycle", 1), f("modes", 6), skip(6), f("ldac", 1), f("dual", 1), skip(2), f("autoOff", 2), f("volumeLimit", 3),
              f("voicePrompt", 1), f("lowBattery", 1), skip(5), f("hearId", HID_A3040.len)],
            (Bat::SingleLevel, S5))
    },
    Model {
        eq: Some(eqh(1, 10, &presets::A3062, EqW::A3062, hid_end(10))),
        buttons: &[BASS_BUTTON],
        modes: Some(A3062_MODES),
        cycle: Cycle::Bits,
        auto_off: HALF_HOURS,
        sets: &[LOW_BATTERY, sw("dolby", [0x02, 0x86]), LDAC, sw("sideTone", [0x01, 0x84]), VOICE_AE],
        dual: true,
        ..model(&["3062"], &["Soundcore Space One Pro"],
            &[f("battery", 2), f("fw", 5), f("serial", 16), f("eq", eq(1, 10)), skip(2), f("hearId", hid_end(10).len), skip(2),
              f("buttons", 1), f("cycle", 1), f("modes", 6), skip(1), f("lowBattery", 1), f("dolby", 1), f("ldac", 1), f("dual", 1),
              f("autoOff", 2), f("volumeLimit", 3), f("sideTone", 1), f("voicePrompt", 1)],
            (Bat::Single, S10_1))
    },
    Model {
        case: Some(S5),
        eq: Some(eqs(1, 10, &presets::A3330, EqW::Drc)),
        buttons: &[
            btns(&[no_tws("double", 0, A_NO_MODES), no_tws("triple", 5, A_NO_MODES), no_tws("long", 1, A_NO_MODES)], LR3, true),
            Buttons { key: "callButtons", cmd: [0x04, 0x87], ..btns(&[no_tws("double", 0, A_CALL), no_tws("long", 1, A_CALL)], LR2, true) },
        ],
        sets: &[SURROUND, TOUCH, LOW_BATTERY, BUTTONS_OFF],
        dual: true,
        ..model(&["3330"], &["Soundcore C30i"],
            &[f("tws", 2), f("battery", 2), f("fw", 10), f("serial", 16), skip(5), f("case", 1), skip(1), f("buttons", 6), skip(1),
              f("surround", 1), f("touchTone", 1), f("lowBattery", 1), f("dual", 1), f("buttonsOff", 1), skip(1), f("eq", eq(1, 10)),
              skip(1), f("callButtons", 4)],
            (Bat::DualLevel, S5))
    },
    Model {
        case: Some(S10_1),
        eq: Some(Eq { dup: true, ..eqs(2, 10, &presets::TYPE_2, EqW::Raw83) }),
        buttons: &[btns(&[no_tws("double", 0, A_NO_MODES), no_tws("triple", 5, A_NO_MODES)], LR2, true)],
        sets: &[SURROUND, TOUCH, LOW_BATTERY, BUTTONS_OFF],
        dual: true,
        ..model(&["3388"], &["Soundcore AeroClip"],
            &[f("tws", 2), f("battery", 2), f("fw", 10), f("serial", 16), skip(5), f("case", 1), skip(1), f("buttons", 4), skip(2),
              skip(1), f("surround", 1), f("touchTone", 1), f("lowBattery", 1), f("dual", 1), f("buttonsOff", 1), skip(1),
              f("eq", eq(1, 10))],
            (Bat::DualLevel, S10_1))
    },
    Model {
        eq: Some(eqs(2, 10, &presets::A3876, EqW::A3876)),
        buttons: &[btns(&[no_tws("single", 2, A_MIN), no_tws("double", 0, A_A3876), no_tws("triple", 5, A_A3876), no_tws("long", 1, A_A3876)], LR4, true)],
        auto_off: TENS,
        sets: &[VOICE, GAMING, choice("volumeBalance", [0x10, 0x8C], &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12])],
        dual: true,
        groups: &[LIGHTS_A3876],
        ..model(&["3876"], &["Soundcore V20i"],
            &[f("tws", 2), f("battery", 2), f("fw", 10), f("serial", 16), skip(6), f("eq", eq(2, 10)), skip(1), f("buttons", 8),
              f("lights", 7), skip(1), f("voicePrompt", 1), f("autoOff", 2), skip(1), f("gaming", 1), f("volumeBalance", 1), f("dual", 1)],
            (Bat::DualLevel, S10_1))
    },
    Model {
        eq: Some(Eq { whole_db: true, preset_only: true, ..eqs(2, 8, &presets::A3909, EqW::Plain) }),
        buttons: &[btns(&[press("double", 0, En::No, Act::One, A_A3909, Off::No), press("long", 1, En::No, Act::One, A_A3909, Off::No)],
            &[(0, 0), (1, 0), (0, 1), (1, 1)], false)],
        ..model(&["3909"], &["Soundcore Liberty 2 Pro"],
            &[f("tws", 2), f("battery", 4), f("eq", 2), f("gender", 1), f("age", 1), f("hearId", hid_basic(16).len), f("buttons", 4)], DUAL5)
    },
    Model {
        eq: Some(eqh(2, 8, &presets::COMMON, EqW::HearId { force: false }, hid_basic(16))),
        buttons: &[COMMON_BUTTONS],
        ..model(&["3926"], &["Soundcore Life Dot 2S"],
            &[f("tws", 2), f("battery", 4), f("eq", eq(2, 8)), f("gender", 1), f("age", 1), f("hearId", hid_basic(16).len), f("buttons", 12)], DUAL5)
    },
    Model {
        eq: Some(eqh(2, 8, &presets::COMMON, EqW::HearId { force: false }, hid_mid(16))),
        buttons: &[Buttons { set_all: false, presses: &[
            press("double", 0, En::One, Act::Tws, A_NO_MODES, Off::No),
            press("long", 1, En::One, Act::Tws, A_NO_MODES, Off::No),
            press("single", 2, En::One, Act::One, A_NO_MODES, Off::No),
        ], ..COMMON_BUTTONS }],
        modes: Some(V1_A3930),
        ..model(&["3930"], &["Soundcore Liberty 2 Pro+"],
            &[f("tws", 2), f("battery", 4), f("eq", eq(2, 8)), f("gender", 1), f("age", 1), f("hearId", hid_mid(16).len), f("buttons", 12),
              f("modes", 4), skip(1), o("", 2)], DUAL5)
    },
    Model {
        eq: Some(eqs(2, 8, &presets::COMMON, EqW::Drc)),
        buttons: &[Buttons { set_all: false, presses: &[
            press("double", 0, En::One, Act::Tws, A_COMMON, Off::No),
            press("long", 1, En::One, Act::Tws, A_COMMON, Off::No),
            press("single", 2, En::One, Act::One, A_COMMON, Off::No),
        ], ..COMMON_BUTTONS }],
        modes: Some(V1_FULL),
        auto_off: ALL_DURATIONS,
        sets: &[TOUCH],
        ..model(&["3931", "3935"], &["Soundcore Life Dot 2 NC", "Soundcore Life A2 NC"],
            &[f("tws", 2), f("battery", 4), f("eq", eq(2, 8)), f("buttons", 12), f("modes", 4), skip(1), f("touchTone", 1), f("autoOff", 2)], DUAL5)
    },
    Model {
        case: Some(S5),
        eq: Some(eqs(2, 10, &presets::COMMON, EqW::Plain)),
        buttons: &[COMMON_BUTTONS],
        modes: Some(V1_FULL),
        cycle: Cycle::Bits,
        sets: &[TOUCH, GAMING, WEAR],
        ..model(&["3933", "3939"], &["Soundcore Life Note 3", "Soundcore Life P3"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(2, 10)), f("age", 1), f("hearId", 48), f("buttons", 12),
              f("cycle", 1), f("modes", 4), skip(2), o("touchTone", 1), o("wearDetection", 1), o("gaming", 1), o("case", 1), o("", 2),
              o("windDetected", 1), o("", 3)], DUAL5)
    },
    Model {
        case: Some(S10),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::HearId { force: true }, hid_end(20))),
        buttons: &[btns(&[
            tws_tws("single", 2, A_MIN, Off::Pair),
            tws_tws("double", 0, A_GAME, Off::Pair),
            tws_tws("long", 1, A_GAME, Off::Pair),
        ], LR3, false)],
        modes: Some(A3936_MODES),
        cycle: Cycle::Bits,
        auto_off: HALF_HOURS,
        sets: &[TOUCH, LDAC, GAMING],
        dual: true,
        ..model(&["3936"], &["Soundcore Space A40"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(2, 10)), f("age", 1), f("hearId", hid_end(20).len),
              skip(1), f("buttons", 12), f("cycle", 1), f("modes", 6), f("touchTone", 1), f("case", 1), skip(1), f("ldac", 1), f("dual", 1),
              f("autoOff", 2), f("gaming", 1)], DUAL5)
    },
    Model {
        eq: Some(Eq { custom: false, ..eqs(1, 10, &presets::A3944, EqW::Drc) }),
        buttons: &[btns(&[
            press("single", 2, En::One, Act::One, A_NO_MODES, Off::Each),
            press("double", 0, En::One, Act::Tws, A_NO_MODES, Off::Each),
            press("long", 1, En::One, Act::Tws, A_NO_MODES, Off::Each),
        ], LR3, true)],
        sets: &[TOUCH],
        ..model(&["3944"], &["Soundcore Life P2 Mini"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(1, 10)), skip(11), f("buttons", 12), skip(5),
              f("touchTone", 1)], DUAL5)
    },
    Model {
        case: Some(S5),
        eq: Some(eqs(2, 10, &presets::COMMON, EqW::Plain)),
        buttons: &[Buttons { presses: &[
            press("double", 0, En::One, Act::Tws, A_NO_MODES, Off::No),
            press("long", 1, En::One, Act::Tws, A_NO_MODES, Off::No),
            press("single", 2, En::One, Act::One, A_NO_MODES, Off::Pair),
        ], ..COMMON_BUTTONS }],
        sets: &[TOUCH, WEAR, GAMING],
        ..model(&["3945"], &["Soundcore Life Note 3S"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(2, 10)), f("buttons", 12), f("touchTone", 1),
              f("wearDetection", 1), f("gaming", 1), f("case", 1), skip(2)], DUAL5)
    },
    Model {
        case: Some(S5),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::A3947, HID_A3947)),
        buttons: &[btns(&[
            tws_tws("single", 2, A_COMMON, Off::Each),
            tws_tws("double", 0, A_COMMON, Off::Each),
            tws_tws("long", 1, A_COMMON, Off::Each),
            tws_tws("triple", 5, A_COMMON, Off::Each),
        ], LR4, true)],
        modes: Some(A3947_MODES),
        cycle: Cycle::Bits,
        auto_off: TENS,
        sets: &[LEAK, GAMING_10, TOUCH, SURROUND, AUTO_PLAY, WEARING_TONE, TOUCH_LOCK, LOW_BATTERY],
        dual: true,
        ..model(&["3947"], &["Soundcore Liberty 4 NC"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), skip(5), f("eq", eq(2, 10)), skip(1), f("hearId", HID_A3947.len),
              skip(1), f("buttons", 16), f("cycle", 1), f("modes", 7), skip(6), f("case", 1), skip(1), f("leak", 1), skip(1), f("gaming", 1),
              f("touchTone", 1), skip(2), f("dual", 1), f("surround", 1), f("volumeLimit", 3), f("autoPlay", 1), f("wearingTone", 1),
              f("autoOff", 2), f("touchLock", 1), f("lowBattery", 1)], DUAL5)
    },
    Model {
        eq: Some(eqs(1, 10, &presets::TYPE_2, EqW::Drc)),
        buttons: &[btns(&[
            press("single", 2, En::One, Act::Tws, A_NO_MODES, Off::Each),
            press("double", 0, En::One, Act::Tws, A_NO_MODES, Off::Each),
            press("long", 1, En::One, Act::Tws, A_NO_MODES, Off::Each),
        ], LR3, true)],
        sets: &[TOUCH],
        ..model(&["3948"], &["Soundcore A20i"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(1, 10)), skip(11), f("buttons", 12), skip(5),
              f("touchTone", 1), skip(15)], DUAL5)
    },
    Model {
        eq: Some(Eq { custom: false, ..eqs(1, 10, &presets::TYPE_2, EqW::Drc) }),
        buttons: &[btns(&[
            press("single", 2, En::One, Act::Tws, A_NO_MODES, Off::Each),
            press("double", 0, En::One, Act::Tws, A_NO_MODES, Off::Each),
            press("long", 1, En::One, Act::Tws, A_NO_MODES, Off::Each),
        ], LR3, true)],
        sets: &[GAMING, TOUCH],
        ..model(&["3949"], &["Soundcore P20i", "Soundcore P25i", "Soundcore R50i"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(1, 10)), skip(11), f("buttons", 12), skip(4),
              f("gaming", 1), f("touchTone", 1)], DUAL5)
    },
    Model {
        eq: Some(eqh(2, 8, &presets::COMMON, EqW::HearId { force: false }, hid_mid(16))),
        buttons: &[COMMON_BUTTONS],
        modes: Some(V1_CUSTOM),
        // OpenSCQ30 lists LDAC for the A3951, but its state packet has no LDAC byte to read.
        sets: &[TOUCH, WEAR],
        ..model(&["3951"], &["Soundcore Liberty Air 2 Pro"],
            &[f("tws", 2), f("battery", 4), f("eq", eq(2, 8)), f("gender", 1), f("age", 1), f("hearId", hid_mid(16).len), f("buttons", 12),
              f("modes", 4), skip(1), f("wearDetection", 1), f("touchTone", 1), o("", 2), o("", 2)], DUAL5)
    },
    Model {
        case: Some(S5),
        eq: Some(eqh(2, 10, &presets::A3952, EqW::HearId { force: false }, hid_end(20))),
        buttons: &[btns(&[
            tws_tws("single", 2, A_COMMON, Off::Pair),
            tws_tws("double", 0, A_COMMON, Off::Pair),
            tws_tws("long", 1, A_COMMON, Off::Pair),
        ], LR3, false)],
        modes: Some(A3952_MODES),
        cycle: Cycle::Bits,
        auto_off: HALF_HOURS,
        sets: &[LDAC, TOUCH, WEAR, WEARING_TONE],
        ..model(&["3952"], &["Soundcore Liberty 3 Pro"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(2, 10)), f("age", 1), f("hearId", hid_end(20).len),
              skip(1), f("buttons", 12), skip(4), f("cycle", 1), f("modes", 6), f("touchTone", 1), f("wearDetection", 1), skip(1), f("case", 1),
              skip(2), f("ldac", 1), skip(2), f("wearingTone", 1), skip(1), f("autoOff", 2)], DUAL5)
    },
    Model {
        case: Some(S5),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::A3953, hid_end(20))),
        buttons: &[btns(&[
            tws_tws("single", 2, A_COMMON, Off::Each),
            tws_tws("double", 0, A_COMMON, Off::Each),
            tws_tws("triple", 5, A_COMMON, Off::Each),
        ], LR3, true)],
        modes: Some(A3953_MODES),
        cycle: Cycle::Bits,
        auto_off: TENS,
        sets: &[WEAR, LDAC, WEARING_TONE, LOW_BATTERY, sw("ambientPrompt", [0x10, 0x83]), choice("pressSensitivity", [0x04, 0x85], &[0, 1, 2])],
        dual: true,
        groups: &[SPATIAL_A3953],
        ..model(&["3953"], &["Soundcore Liberty 4"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(2, 10)), f("hidStatus", 1), f("hearId", hid_end(20).len),
              f("gapBase", 1), f("buttons", 8), skip(4), f("buttons", 4), f("gap", GAP), f("cycle", 1), f("modes", 6), skip(6),
              f("wearDetection", 1), skip(2), f("case", 1), skip(1), f("ldac", 1), f("dual", 1), f("autoOff", 2), skip(1),
              f("wearingTone", 1), f("lowBattery", 1), f("ambientPrompt", 1), f("spatial", 3), skip(5), o("", 1), o("pressSensitivity", 1)],
            DUAL5)
    },
    Model {
        case: Some(S10_1),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::A3954, hid_end(20))),
        buttons: &[btns(&[
            no_tws("single", 2, A_COMMON), no_tws("double", 0, A_COMMON), no_tws("triple", 5, A_COMMON), no_tws("long", 1, A_COMMON),
        ], LR4, false), Buttons { key: "slideButtons", ..btns(&[no_tws("slideUp", 6, A_SLIDE_UP), no_tws("slideDown", 7, A_SLIDE_DOWN)], LR2, false) }],
        modes: Some(A3954_MODES),
        cycle: Cycle::Bits,
        auto_off: TENS,
        sets: &[LOW_BATTERY, LDAC, LEAK, WEAR, choice("caseLanguage", [0x07, 0x8A], &[0, 1, 2])],
        dual: true,
        groups: &[SPATIAL_A3954, CASE_A3954, EASY_CHAT_A3954],
        ..model(&["3954"], &["Soundcore Liberty 4 Pro"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("caseFw", 5), f("case", 1), f("caseSerial", 6),
              f("eq", eq(2, 10)), skip(1), f("hearId", hid_end(20).len), skip(1), f("buttons", 8), f("cycle", 1), f("modes", 4), skip(3),
              f("caseFeatures", 4), f("airPressure", 1), skip(3), f("lowBattery", 1), f("ldac", 1), f("dual", 1), f("autoOff", 2),
              f("volumeLimit", 3), f("spatial", 3), f("easyChat", 1), skip(1), f("leak", 1), skip(3), f("caseLanguage", 1),
              f("easyChatWait", 1), f("wearDetection", 1), skip(1), f("slideButtons", 4)],
            (Bat::Dual, Scale { max: 100, offset: 0 }))
    },
    Model {
        case: Some(S5),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::A3953, hid_end(20))),
        buttons: &[btns(&[
            no_tws("single", 2, A_COMMON), no_tws("double", 0, A_COMMON), no_tws("triple", 5, A_COMMON), no_tws("long", 1, A_COMMON),
        ], LR4, false)],
        modes: Some(A3955_MODES),
        cycle: Cycle::Bits,
        auto_off: TENS,
        sets: &[sw("personalizedAnc", [0x03, 0x90]), TOUCH, LOW_BATTERY],
        dual: true,
        ..model(&["3955"], &["Soundcore P40i"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), skip(5), f("case", 1), f("eq", eq(2, 10)), f("hidStatus", 1),
              f("hearId", hid_end(20).len), skip(1), f("buttons", 8), f("cycle", 1), f("modes", 7), skip(6), f("personalizedAnc", 1),
              skip(1), f("touchTone", 1), f("dual", 1), f("volumeLimit", 3), f("autoOff", 2), skip(1), f("lowBattery", 1)], DUAL5)
    },
    Model {
        case: Some(S10_1),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::A3954, hid_end(20))),
        buttons: &[btns(&[
            no_tws("single", 2, A_COMMON), no_tws("double", 0, A_COMMON), no_tws("triple", 5, A_COMMON), no_tws("long", 1, A_COMMON),
        ], LR4, false)],
        modes: Some(A3957_MODES),
        cycle: Cycle::Bits,
        auto_off: TENS,
        sets: &[WEARING_TONE, LOW_BATTERY, LDAC, LEAK, WEAR, TOUCH, GAMING_10],
        dual: true,
        ..model(&["3957"], &["Soundcore Liberty 5"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), skip(5), f("case", 1), f("eq", eq(2, 10)), f("age", 1),
              f("hearId", hid_end(20).len), skip(1), f("buttons", 8), f("cycle", 1), f("modes", 7), skip(1), f("wearingTone", 1),
              f("lowBattery", 1), f("ldac", 1), f("dual", 1), f("autoOff", 2), f("volumeLimit", 3), f("immersive", 1), skip(6),
              f("leak", 1), f("wearDetection", 1), f("touchTone", 1), f("gaming", 1), f("pressure", 1), skip(6)],
            (Bat::Dual, S10_1))
    },
    Model {
        eq: Some(eqs(1, 10, &presets::TYPE_2, EqW::Drc)),
        buttons: &[btns(&[
            no_tws("single", 2, A_COMMON), no_tws("double", 0, A_COMMON), no_tws("triple", 5, A_COMMON), no_tws("long", 1, A_COMMON),
        ], LR4, true)],
        modes: Some(A3959_MODES),
        cycle: Cycle::Bits,
        auto_off: TENS,
        sets: &[SURROUND, TOUCH, LOW_BATTERY, GAMING],
        dual: true,
        ..model(&["3959"], &["Soundcore P30i", "Soundcore R50i NC"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), f("eq", eq(1, 10)), skip(10), skip(1), f("buttons", 8),
              f("cycle", 1), f("modes", 7), skip(1), f("touchTone", 1), f("dual", 1), f("surround", 1), f("autoOff", 2),
              f("lowBattery", 1), f("gaming", 1), skip(12)],
            (Bat::Dual, S10))
    },
    Model {
        case: Some(S5),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::HearId { force: false }, hid_end(20))),
        buttons: &[btns(&[no_tws("single", 2, A_MIN), no_tws("double", 0, A_COMMON), no_tws("long", 1, A_COMMON)], LR3, true)],
        modes: Some(A3968_MODES),
        cycle: Cycle::Tws,
        auto_off: TENS,
        sets: &[SURROUND, TOUCH],
        dual: true,
        ..model(&["3968"], &["Soundcore Sport X20"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), skip(5), f("case", 1), f("eq", eq(2, 10)), skip(1),
              f("hearId", hid_end(20).len), skip(1), f("buttons", 6), f("cycle", 1), f("modes", 6), skip(1), f("surround", 1), skip(1),
              f("dual", 1), f("touchTone", 1), f("autoOff", 2)], DUAL5)
    },
    Model {
        eq: Some(Eq { dup: true, ..eqs(2, 10, &presets::D1101, EqW::Plain) }),
        buttons: &[btns(&[
            no_tws("single", 2, A_NO_MODES), no_tws("double", 0, A_D1101_MULTI), no_tws("triple", 5, A_D1101_MULTI), no_tws("long", 1, A_D1101_LONG),
        ], LR4, false)],
        auto_off: HALF_HOURS,
        sets: &[LOW_BATTERY, BUTTONS_OFF, LDAC],
        dual: true,
        ..model(&["1101"], &["Soundcore C50i"],
            &[f("tws", 2), f("battery", 2), f("fw", 10), f("serial", 16), f("eq", eq(1, 10)), skip(1), f("buttons", 8), skip(1),
              f("lowBattery", 1), f("dual", 1), f("buttonsOff", 1), f("autoOff", 2), f("ldac", 1)],
            (Bat::DualLevel, S10_1))
    },
    Model {
        case: Some(S10_1),
        eq: Some(eqh(2, 10, &presets::TYPE_2, EqW::A3954, hid_end(20))),
        buttons: &[btns(&[
            no_tws("single", 2, A_MIN), no_tws("double", 0, A_COMMON), no_tws("triple", 5, A_COMMON), no_tws("long", 1, A_COMMON),
        ], LR4, false)],
        modes: Some(D1202_MODES),
        auto_off: TENS,
        sets: &[TOUCH, LOW_BATTERY, LDAC],
        dual: true,
        groups: &[SPATIAL_D1202],
        ..model(&["1202"], &["Soundcore P31i", "Soundcore R60i NC"],
            &[f("tws", 2), f("battery", 4), f("fw", 10), f("serial", 16), skip(5), f("case", 1), f("eq", eq(2, 10)), skip(1),
              f("hearId", hid_end(20).len), skip(1), f("buttons", 8), skip(1), f("modes", 8), skip(1), f("touchTone", 1),
              f("lowBattery", 1), f("ldac", 1), f("dual", 1), f("autoOff", 2), f("volumeLimit", 3), f("spatial", 2)],
            (Bat::Dual, S10_1))
    },
    Model {
        buttons: &[btns(&[no_tws("double", 0, A_D1301), no_tws("triple", 5, A_D1301)], LR2, false)],
        sets: &[
            choice("listeningMode", [0x01, 0xA9], &[0, 1]), choice("defaultListeningMode", [0x10, 0x95], &[0, 1]), LOW_BATTERY,
            sw("autoOffPrompt", [0x10, 0x9E]), sw("listeningModePrompt", [0x10, 0x9F]), sw("sleepAnc", [0x06, 0x87]),
            sw("incomingCalls", [0x10, 0x75]), Set { key: "tapOff", cmd: [0x10, 0x94], kind: Kind::Inverted },
            sw("autoSwitchAsleep", [0x15, 0x8F]),
        ],
        groups: &[POST_SLEEP_D1301],
        timer: true,
        ..model(&["1301"], &["Soundcore Sleep A30"],
            &[f("tws", 2), f("battery", 2), f("fw", 10), f("serial", 16), skip(6), f("caseFw", 5), skip(4), skip(1), f("buttons", 4),
              skip(15), f("listeningMode", 1), f("defaultListeningMode", 1), skip(2), f("lowBattery", 1), skip(67), f("autoOffPrompt", 1),
              f("listeningModePrompt", 1), skip(1), f("sleepAnc", 1), f("incomingCalls", 1), skip(2), f("postSleep", 1), skip(1),
              f("tapOff", 1), f("ancPrompt", 1), f("autoSwitchAsleep", 1)],
            (Bat::DualLevel, S10_1))
    },
    Model {
        eq: Some(eqh(1, 10, &presets::D1402, EqW::D1402, hid_end(10))),
        buttons: &[BASS_BUTTON, Buttons { key: "mfb", presses: &[press("long", 1, En::No, Act::One, A_MFB, Off::Each)], order: &[(0, 0)],
            set_all: false, ignore_enabled: false, cmd: [0x04, 0x61] }],
        modes: Some(D1402_MODES),
        cycle: Cycle::Bits,
        auto_off: LONG_DURATIONS,
        sets: &[SURROUND, VOICE_AE, LOW_BATTERY, LDAC, AUTO_PLAY, sw("heyAnka", [0x18, 0x83])],
        dual: true,
        ..model(&["1402"], &["Soundcore Space 2"],
            &[f("battery", 2), f("fw", 5), f("serial", 16), f("eq", eq(1, 10)), f("eqByte", 1), f("age", 1), f("hearId", hid_end(10).len),
              skip(2), f("buttons", 1), skip(1), f("cycle", 1), f("mfb", 1), f("modes", 6), skip(1), f("surround", 1), skip(2),
              f("voicePrompt", 1), f("lowBattery", 1), f("ldac", 1), f("dual", 1), f("autoOff", 2), f("volumeLimit", 3), skip(3),
              f("autoPlay", 1), f("heyAnka", 1), skip(8)],
            (Bat::Single, S10_1))
    },
];

/// The field offsets of a state packet for a model, or None when the packet is too short.
pub fn layout(m: &Model, b: &[u8]) -> Option<Vec<(&'static str, usize, usize)>> {
    let mut at = 0;
    let mut out = Vec::new();
    for fd in m.fields {
        let len = if fd.len == GAP {
            let base = out.iter().find(|x: &&(&str, usize, usize)| x.0 == "gapBase")?.1;
            (*b.get(base)? as usize).saturating_sub(18)
        } else {
            fd.len
        };
        if at + len > b.len() {
            if fd.opt { break }
            return None;
        }
        if !fd.key.is_empty() { out.push((fd.key, at, len)); }
        at += len;
    }
    Some(out)
}

/// The model of a state packet: a model whose serial number field starts with its number.
pub fn by_serial(b: &[u8]) -> Option<&'static Model> {
    MODELS.iter().find(|m| {
        let Some(l) = layout(m, b) else { return false };
        let Some(&(_, at, len)) = l.iter().find(|x| x.0 == "serial") else { return false };
        std::str::from_utf8(&b[at..at + len]).is_ok_and(|s| m.ids.iter().any(|id| s.starts_with(id)))
    })
}

/// The model of a Bluetooth name ("soundcore Liberty 5" matches "Soundcore Liberty 5").
pub fn by_name(name: &str) -> Option<&'static Model> {
    let n = name.trim().to_lowercase();
    MODELS.iter().find(|m| m.names.iter().any(|x| x.to_lowercase() == n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_group_fits_its_fields() {
        for m in MODELS {
            for g in m.buttons {
                let len: usize = m.fields.iter().filter(|f| f.key == g.key).map(|f| f.len).sum();
                assert_eq!(len, g.len(), "A{} {}", m.ids[0], g.key);
            }
        }
    }

    #[test]
    fn every_hearid_fits_its_field() {
        for m in MODELS {
            if let Some(h) = m.eq.and_then(|e| e.hid) {
                let f = m.fields.iter().find(|f| f.key == "hearId").expect("hearId field");
                assert_eq!(f.len, h.len, "A{}", m.ids[0]);
            }
        }
    }

    #[test]
    fn every_setting_has_its_field() {
        for m in MODELS {
            for s in m.sets {
                assert!(m.fields.iter().any(|f| f.key == s.key), "A{} {}", m.ids[0], s.key);
            }
            if m.dual { assert!(m.fields.iter().any(|f| f.key == "dual"), "A{} dual", m.ids[0]); }
            if !m.auto_off.is_empty() { assert!(m.fields.iter().any(|f| f.key == "autoOff"), "A{} autoOff", m.ids[0]); }
        }
    }

    #[test]
    fn models_json_names_every_model_and_only_known_ones() {
        let v: serde_json::Value = serde_json::from_str(include_str!("../../../app/src/main/assets/models.json")).expect("json");
        let list = v["soundcore"].as_array().expect("soundcore list");
        for e in list {
            let (id, name) = (e["id"].as_str().unwrap(), e["name"].as_str().unwrap());
            let m = by_name(name).unwrap_or_else(|| panic!("{name}: no model"));
            assert!(m.ids.contains(&&id[1..]), "{name}: {id} not in {:?}", m.ids);
        }
        for m in MODELS {
            for n in m.names { assert!(list.iter().any(|e| e["name"] == *n), "{n} missing from models.json"); }
        }
    }

    #[test]
    fn every_group_part_has_its_field() {
        for m in MODELS {
            for g in m.groups {
                for p in g.parts {
                    let f = m.fields.iter().find(|f| f.key == p.field).unwrap_or_else(|| panic!("A{} {}", m.ids[0], p.field));
                    let need = if p.kind == PartKind::Color { 3 } else { 1 };
                    assert!(p.at + need <= f.len, "A{} {}", m.ids[0], p.key);
                }
            }
        }
    }
}
