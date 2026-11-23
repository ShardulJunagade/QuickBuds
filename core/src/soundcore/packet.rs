//! Soundcore framing over RFCOMM. Ported from OpenSCQ30 (`lib/src/devices/soundcore/common/packet.rs`).
//! Copyright 2026 Kyle Scheuing, GPL-3.0-or-later.
//!
//! A frame: 5 direction bytes, 2 command bytes, a little-endian u16 length of the whole frame, the body, and
//! (on most models) a 1-byte sum of all bytes before it.

/// The phone to the buds.
pub const OUT: [u8; 5] = [0x08, 0xEE, 0x00, 0x00, 0x00];
/// The buds to the phone.
pub const IN: [u8; 5] = [0x09, 0xFF, 0x00, 0x00, 0x01];
const HEAD: usize = 9;

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Packet {
    pub cmd: [u8; 2],
    pub body: Vec<u8>,
}

/// One result of [`Reader::push`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Frame {
    Packet(Packet),
    /// Bytes that are no frame, or a frame with a wrong checksum. The reader skips to the next header.
    Bad(Vec<u8>),
}

fn sum(b: &[u8]) -> u8 { b.iter().fold(0u8, |a, x| a.wrapping_add(*x)) }

/// The bytes of an outbound frame.
pub fn encode(cmd: [u8; 2], body: &[u8], checksum: bool) -> Vec<u8> {
    let len = HEAD + body.len() + checksum as usize;
    let mut b = Vec::with_capacity(len);
    b.extend_from_slice(&OUT);
    b.extend_from_slice(&cmd);
    b.extend_from_slice(&(len as u16).to_le_bytes());
    b.extend_from_slice(body);
    if checksum { b.push(sum(&b)); }
    b
}

/// Joins socket reads into inbound frames. A frame can arrive in parts, and one read can hold many frames.
#[derive(Default)]
pub struct Reader {
    buf: Vec<u8>,
    checksum: bool,
}

impl Reader {
    pub fn new(checksum: bool) -> Self { Self { buf: Vec::new(), checksum } }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Frame> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            // Skip to the next header. A partial header at the end stays for the next read.
            let start = (0..self.buf.len()).find(|&i| {
                let n = (self.buf.len() - i).min(IN.len());
                self.buf[i..i + n] == IN[..n]
            });
            match start {
                None => { if !self.buf.is_empty() { out.push(Frame::Bad(std::mem::take(&mut self.buf))); } break; }
                Some(0) => {}
                Some(i) => out.push(Frame::Bad(self.buf.drain(..i).collect())),
            }
            if self.buf.len() < HEAD { break; }
            let len = u16::from_le_bytes([self.buf[7], self.buf[8]]) as usize;
            let min = HEAD + self.checksum as usize;
            if len < min {
                // No valid frame starts here: drop the header byte and look again.
                out.push(Frame::Bad(self.buf.drain(..1).collect()));
                continue;
            }
            if self.buf.len() < len { break; }
            let f: Vec<u8> = self.buf.drain(..len).collect();
            let end = len - self.checksum as usize;
            if self.checksum && sum(&f[..end]) != f[end] {
                out.push(Frame::Bad(f));
                continue;
            }
            out.push(Frame::Packet(Packet { cmd: [f[5], f[6]], body: f[HEAD..end].to_vec() }));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inbound(cmd: [u8; 2], body: &[u8]) -> Vec<u8> {
        let mut b = encode(cmd, body, true);
        b[..5].copy_from_slice(&IN);
        let n = b.len() - 1;
        b[n] = sum(&b[..n]);
        b
    }

    #[test]
    fn encodes_a_frame_with_its_checksum() {
        // OpenSCQ30 RequestState: 08 EE 00 00 00 01 01 0A 00 02.
        assert_eq!(encode([0x01, 0x01], &[], true), [0x08, 0xEE, 0, 0, 0, 0x01, 0x01, 0x0A, 0x00, 0x02]);
    }

    #[test]
    fn joins_parts_and_splits_many() {
        let a = inbound([0x01, 0x03], &[3, 4]);
        let b = inbound([0x06, 0x01], &[0, 1, 0, 5]);
        let mut r = Reader::new(true);
        assert!(r.push(&a[..4]).is_empty());
        let mut all = a[4..].to_vec();
        all.extend_from_slice(&b);
        let got = r.push(&all);
        assert_eq!(got, vec![
            Frame::Packet(Packet { cmd: [0x01, 0x03], body: vec![3, 4] }),
            Frame::Packet(Packet { cmd: [0x06, 0x01], body: vec![0, 1, 0, 5] }),
        ]);
    }

    #[test]
    fn skips_noise_and_a_bad_checksum() {
        let mut bad = inbound([0x01, 0x03], &[3, 4]);
        *bad.last_mut().unwrap() ^= 0xFF;
        let good = inbound([0x01, 0x04], &[1, 0]);
        let mut all = vec![0x00, 0x11];
        all.extend_from_slice(&bad);
        all.extend_from_slice(&good);
        let got = Reader::new(true).push(&all);
        assert_eq!(got[0], Frame::Bad(vec![0x00, 0x11]));
        assert!(matches!(got[1], Frame::Bad(_)));
        assert_eq!(got[2], Frame::Packet(Packet { cmd: [0x01, 0x04], body: vec![1, 0] }));
    }

    #[test]
    fn reads_frames_with_no_checksum() {
        let mut f = encode([0x01, 0x05], &[9], false);
        f[..5].copy_from_slice(&IN);
        assert_eq!(Reader::new(false).push(&f), vec![Frame::Packet(Packet { cmd: [0x01, 0x05], body: vec![9] })]);
    }
}
