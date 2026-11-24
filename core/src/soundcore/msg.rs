//! The Soundcore packets that every model shares. Ported from OpenSCQ30
//! (`lib/src/devices/soundcore/common/packet/{inbound,outbound}`).
//! Copyright 2026 Kyle Scheuing, GPL-3.0-or-later.
//!
//! The full state packet (`01 01`) has one layout per model; the model files parse it.

use super::packet::Packet;

pub const REQUEST_STATE: [u8; 2] = [0x01, 0x01];
pub const TWS_STATUS: [u8; 2] = [0x01, 0x02];
pub const BATTERY_LEVEL: [u8; 2] = [0x01, 0x03];
pub const BATTERY_CHARGING: [u8; 2] = [0x01, 0x04];
pub const FIRMWARE: [u8; 2] = [0x01, 0x05];
pub const LDAC: [u8; 2] = [0x01, 0x7F];
pub const SOUND_MODES: [u8; 2] = [0x06, 0x01];
pub const SET_SOUND_MODES: [u8; 2] = [0x06, 0x81];

/// Byte values of the noise control mode (`AmbientSoundMode`).
pub mod ambient {
    pub const NOISE_CANCELING: u8 = 0;
    pub const TRANSPARENCY: u8 = 1;
    pub const NORMAL: u8 = 2;
}

/// The 4-byte sound mode group: noise control mode, noise cancelling mode (transport 0, outdoor 1, indoor 2,
/// custom 3), transparency mode (fully 0, vocal 1), custom noise cancelling level (0 to 10; 255 also occurs).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct SoundModes {
    pub ambient: u8,
    pub noise_canceling: u8,
    pub transparency: u8,
    pub custom_level: u8,
}

impl SoundModes {
    pub fn parse(b: &[u8]) -> Option<Self> {
        match *b {
            [ambient, noise_canceling, transparency, custom_level, ..] => Some(Self { ambient, noise_canceling, transparency, custom_level }),
            _ => None,
        }
    }
    pub fn bytes(&self) -> [u8; 4] { [self.ambient, self.noise_canceling, self.transparency, self.custom_level] }
}

/// A firmware version, two ASCII numbers: "02.43".
pub fn firmware(b: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(b.get(..5)?).ok()?;
    let ok = s.as_bytes()[2] == b'.' && s[..2].bytes().chain(s[3..].bytes()).all(|c| c.is_ascii_digit());
    ok.then(|| s.to_string())
}

/// One push from the buds that does not need the model's layout.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Msg {
    /// The host bud (0 left, 1 right) and whether both buds are connected to each other.
    Tws { host: u8, both: bool },
    /// Levels from 0 to 5. One value on a single-battery device, two (left, right) on earbuds.
    Battery(Vec<u8>),
    Charging(Vec<bool>),
    /// The firmware versions (one, or left and right) and the 16-character serial number.
    Firmware { versions: Vec<String>, serial: String },
    Ldac(bool),
    SoundModes(SoundModes),
    /// A packet this module does not decode: the model file may.
    Other(Packet),
}

pub fn decode(p: Packet) -> Msg {
    let b = &p.body;
    let m = match p.cmd {
        TWS_STATUS if b.len() == 2 => Some(Msg::Tws { host: b[0], both: b[1] != 0 }),
        // Dual: left, right, and on newer firmware 1 or 2 more bytes (OpenSCQ30 ignores them; a Space A40 sends 3).
        BATTERY_LEVEL if (1..=4).contains(&b.len()) => Some(Msg::Battery(b[..b.len().min(2)].to_vec())),
        BATTERY_CHARGING if matches!(b.len(), 1 | 2) => Some(Msg::Charging(b.iter().map(|&c| c == 1).collect())),
        FIRMWARE if b.len() == 26 => firmware(&b[..5]).zip(firmware(&b[5..10])).zip(serial(&b[10..]))
            .map(|((l, r), serial)| Msg::Firmware { versions: vec![l, r], serial }),
        FIRMWARE if b.len() == 21 => firmware(&b[..5]).zip(serial(&b[5..]))
            .map(|(v, serial)| Msg::Firmware { versions: vec![v], serial }),
        LDAC if b.len() == 1 => Some(Msg::Ldac(b[0] != 0)),
        SOUND_MODES if b.len() == 4 => SoundModes::parse(b).map(Msg::SoundModes),
        _ => None,
    };
    m.unwrap_or(Msg::Other(p))
}

fn serial(b: &[u8]) -> Option<String> {
    (b.len() == 16).then(|| std::str::from_utf8(b).ok().map(str::to_string)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(cmd: [u8; 2], body: &[u8]) -> Packet { Packet { cmd, body: body.to_vec() } }

    #[test]
    fn decodes_the_common_pushes() {
        assert_eq!(decode(p(BATTERY_LEVEL, &[4, 5, 90, 100])), Msg::Battery(vec![4, 5]));
        assert_eq!(decode(p(BATTERY_CHARGING, &[1, 0])), Msg::Charging(vec![true, false]));
        assert_eq!(decode(p(TWS_STATUS, &[1, 1])), Msg::Tws { host: 1, both: true });
        assert_eq!(decode(p(SOUND_MODES, &[0, 3, 1, 7])),
            Msg::SoundModes(SoundModes { ambient: 0, noise_canceling: 3, transparency: 1, custom_level: 7 }));
        let mut fw = b"02.4302.44".to_vec();
        fw.extend_from_slice(b"3951ABCDEF012345");
        assert_eq!(decode(p(FIRMWARE, &fw)),
            Msg::Firmware { versions: vec!["02.43".into(), "02.44".into()], serial: "3951ABCDEF012345".into() });
    }

    #[test]
    fn an_odd_body_stays_other() {
        // A Space A40 sends 3 bytes (left, right, one more): a battery level. 5 bytes is no battery packet.
        assert!(matches!(decode(p(BATTERY_LEVEL, &[4, 4, 0])), Msg::Battery(ref v) if v == &[4, 4]));
        assert!(matches!(decode(p(BATTERY_LEVEL, &[1, 2, 3, 4, 5])), Msg::Other(_)));
        assert!(matches!(decode(p(FIRMWARE, b"2x.43")), Msg::Other(_)));
    }
}
