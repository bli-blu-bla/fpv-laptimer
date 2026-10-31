//! What a board sends: binary frames and text lines, mixed in one byte stream.
//!
//! The wire format is defined in `firmware/src/proto/frames.h` of fpv-laptimer-firmware:
//!
//! ```text
//! frame: A5 'R' | type | version | len (u16) | payload (len bytes) | crc32 (u32)
//! ```
//!
//! All numbers are little-endian; the CRC (zlib's CRC-32) covers everything
//! before it. Text lines (command replies) end in '\n' and never contain the
//! byte 0xA5, so the two can be told apart.

/// Frame type of `FRAME_LEVEL_BATCH` (the `LEVELBATCH` command).
pub const FRAME_LEVEL_BATCH: u8 = 7;

const SYNC0: u8 = 0xA5;
const SYNC1: u8 = b'R';
/// Header (6 bytes) and CRC (4 bytes) around the payload.
const OVERHEAD: usize = 10;
/// More than this without a complete frame or line means we are lost.
const MAX_BUFFER: usize = 64 * 1024;

/// One complete thing read from a board.
#[derive(Debug, PartialEq)]
pub enum Item {
    Frame { kind: u8, payload: Vec<u8> },
    Line(String),
}

/// Splits the byte stream into frames and lines. Feed it whatever the serial
/// port returned; it keeps incomplete data for the next call.
#[derive(Default)]
pub struct Parser {
    buf: Vec<u8>,
    /// Frames thrown away because their CRC did not match.
    pub crc_errors: u64,
}

impl Parser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `data` and appends every complete item to `out`.
    pub fn push(&mut self, data: &[u8], out: &mut Vec<Item>) {
        self.buf.extend_from_slice(data);
        // `pos`: how much of `buf` is used up. We remove it once at the end,
        // which is cheaper than removing every item from the front.
        let mut pos = 0;
        while pos < self.buf.len() {
            let rest = &self.buf[pos..];
            if rest[0] == SYNC0 {
                if rest.len() < 2 {
                    break; // wait for the second sync byte
                }
                if rest[1] == SYNC1 {
                    match parse_frame(rest) {
                        FrameResult::Incomplete => break,
                        FrameResult::Bad => {
                            self.crc_errors += 1;
                            pos += 1; // skip the sync byte and look again
                        }
                        FrameResult::Ok { kind, payload, used } => {
                            out.push(Item::Frame {
                                kind,
                                payload: payload.to_vec(),
                            });
                            pos += used;
                        }
                    }
                    continue;
                }
            }
            // Text: up to '\n'. A 0xA5 before it means the bytes were not a
            // line after all (we started in the middle of something): drop them.
            match rest.iter().position(|&b| b == b'\n' || b == SYNC0) {
                None => break, // the rest of the line is still on its way
                Some(n) if rest[n] == SYNC0 => pos += n.max(1),
                Some(n) => {
                    let line = String::from_utf8_lossy(&rest[..n]).trim().to_string();
                    if !line.is_empty() {
                        out.push(Item::Line(line));
                    }
                    pos += n + 1;
                }
            }
        }
        self.buf.drain(..pos);
        if self.buf.len() > MAX_BUFFER {
            self.buf.clear();
        }
    }
}

enum FrameResult<'a> {
    Incomplete,
    Bad,
    Ok { kind: u8, payload: &'a [u8], used: usize },
}

/// `data` starts with the two sync bytes.
fn parse_frame(data: &[u8]) -> FrameResult<'_> {
    if data.len() < 6 {
        return FrameResult::Incomplete;
    }
    let len = u16::from_le_bytes([data[4], data[5]]) as usize;
    let total = len + OVERHEAD;
    if data.len() < total {
        return FrameResult::Incomplete;
    }
    let crc = u32::from_le_bytes([data[total - 4], data[total - 3], data[total - 2], data[total - 1]]);
    if crc32fast::hash(&data[..total - 4]) != crc {
        return FrameResult::Bad;
    }
    FrameResult::Ok {
        kind: data[2],
        payload: &data[6..6 + len],
        used: total,
    }
}

/// One channel measurement out of a `FRAME_LEVEL_BATCH`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelRecord {
    /// Board clock (µs since its boot), at the middle of the capture.
    pub board_us: u64,
    /// Index in the channel plan (the order of the `CHANNELS` command).
    pub channel: u8,
    /// `CHF_*` flags (fpv-laptimer-firmware: firmware/src/scan/metrics.h); 2 = the ADC clipped.
    pub flags: u8,
    /// Level code in 0.25 dB steps, see [`rssi_dbfs`].
    pub rssi: u8,
    /// Only in `METRICS SNR` mode, else 0.
    pub snr: u8,
}

/// The payload of a `FRAME_LEVEL_BATCH`.
#[derive(Debug, PartialEq)]
pub struct LevelBatch {
    pub seq: u32,
    /// Records the board could not send since the previous batch.
    pub lost: u16,
    /// Manual gain index, 255 = hardware AGC.
    pub gain: u8,
    pub records: Vec<LevelRecord>,
}

const BATCH_HEADER: usize = 18;
const BATCH_RECORD: usize = 8;

impl LevelBatch {
    /// None if the payload is too short for what its header says.
    pub fn parse(p: &[u8]) -> Option<LevelBatch> {
        if p.len() < BATCH_HEADER {
            return None;
        }
        let u16_at = |i: usize| u16::from_le_bytes([p[i], p[i + 1]]);
        let u32_at = |i: usize| u32::from_le_bytes(p[i..i + 4].try_into().unwrap());
        let seq = u32_at(0);
        let t0_us = u64::from_le_bytes(p[4..12].try_into().unwrap());
        let lost = u16_at(12);
        let gain = p[14];
        let n = u16_at(16) as usize;
        if p.len() < BATCH_HEADER + n * BATCH_RECORD {
            return None;
        }
        let records = (0..n)
            .map(|k| {
                let o = BATCH_HEADER + k * BATCH_RECORD;
                LevelRecord {
                    board_us: t0_us + u32_at(o) as u64,
                    channel: p[o + 4],
                    flags: p[o + 5],
                    rssi: p[o + 6],
                    snr: p[o + 7],
                }
            })
            .collect();
        Some(LevelBatch {
            seq,
            lost,
            gain,
            records,
        })
    }
}

/// A level code as dBFS (decibels below the ADC's full scale).
pub fn rssi_dbfs(code: u8) -> f32 {
    code as f32 / 4.0 - 63.75
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
        let mut f = vec![SYNC0, SYNC1, kind, 1];
        f.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        f.extend_from_slice(payload);
        let crc = crc32fast::hash(&f);
        f.extend_from_slice(&crc.to_le_bytes());
        f
    }

    #[test]
    fn frames_and_lines_in_one_stream() {
        let mut data = b"OK\n".to_vec();
        data.extend(frame(3, &[1, 2, 3]));
        data.extend(b"TIME 12345\n");
        let mut p = Parser::new();
        let mut out = Vec::new();
        p.push(&data, &mut out);
        assert_eq!(
            out,
            vec![
                Item::Line("OK".into()),
                Item::Frame {
                    kind: 3,
                    payload: vec![1, 2, 3]
                },
                Item::Line("TIME 12345".into()),
            ]
        );
    }

    #[test]
    fn split_anywhere() {
        let mut data = frame(7, &[9; 40]);
        data.extend(b"OK\n");
        // Feed one byte at a time: the result must be the same.
        let mut p = Parser::new();
        let mut out = Vec::new();
        for b in &data {
            p.push(&[*b], &mut out);
        }
        assert_eq!(out.len(), 2);
        assert_eq!(out[1], Item::Line("OK".into()));
    }

    #[test]
    fn bad_crc_is_skipped() {
        let mut bad = frame(7, &[1, 2, 3, 4]);
        bad[7] ^= 0xff;
        let mut data = bad;
        data.extend(frame(5, &[42]));
        let mut p = Parser::new();
        let mut out = Vec::new();
        p.push(&data, &mut out);
        assert_eq!(p.crc_errors, 1);
        assert_eq!(
            out.last(),
            Some(&Item::Frame {
                kind: 5,
                payload: vec![42]
            })
        );
    }

    #[test]
    fn level_batch() {
        let mut p = Vec::new();
        p.extend(7u32.to_le_bytes()); // seq
        p.extend(1_000_000u64.to_le_bytes()); // t0_us
        p.extend(0u16.to_le_bytes()); // lost
        p.push(45); // gain
        p.push(1); // mode RSSI
        p.extend(2u16.to_le_bytes()); // n
        for (dt, ch, rssi) in [(0u32, 0u8, 100u8), (2000, 1, 200)] {
            p.extend(dt.to_le_bytes());
            p.extend([ch, 0, rssi, 0]);
        }
        let b = LevelBatch::parse(&p).unwrap();
        assert_eq!(b.seq, 7);
        assert_eq!(
            b.records[1],
            LevelRecord {
                board_us: 1_002_000,
                channel: 1,
                flags: 0,
                rssi: 200,
                snr: 0
            }
        );
        assert_eq!(rssi_dbfs(b.records[0].rssi), -38.75);
        assert!(LevelBatch::parse(&p[..p.len() - 1]).is_none());
    }
}
