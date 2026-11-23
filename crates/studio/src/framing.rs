//! ZMK Studio message framing (`app/src/studio/msg_framing.c`): a frame is
//! `SOF payload EOF`; `SOF`, `ESC` and `EOF` bytes inside the payload are sent as
//! `ESC <byte>` (the byte itself, not transformed).

pub const SOF: u8 = 0xAB;
pub const ESC: u8 = 0xAC;
pub const EOF: u8 = 0xAD;

pub fn encode(payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.push(SOF);
    for &b in payload {
        if b == SOF || b == ESC || b == EOF {
            out.push(ESC);
        }
        out.push(b);
    }
    out.push(EOF);
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Data,
    Escaped,
    /// Saw an unescaped SOF inside a frame: skip until EOF/SOF.
    Error,
}

/// Incremental decoder: feed any chunks of bytes, get complete payloads.
#[derive(Debug, Clone)]
pub struct Decoder {
    state: State,
    buf: Vec<u8>,
    /// Bytes thrown away (garbage between frames, broken frames).
    pub discarded: usize,
}

impl Default for Decoder {
    fn default() -> Self {
        Decoder {
            state: State::Idle,
            buf: Vec::new(),
            discarded: 0,
        }
    }
}

/// Frames larger than this are dropped (protects against a stream that never ends).
const MAX_FRAME: usize = 64 * 1024;

impl Decoder {
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for &c in bytes {
            match self.state {
                State::Idle => {
                    if c == SOF {
                        self.state = State::Data;
                        self.buf.clear();
                    } else {
                        self.discarded += 1;
                    }
                }
                State::Error => match c {
                    SOF => {
                        self.state = State::Data;
                        self.buf.clear();
                    }
                    EOF => self.state = State::Idle,
                    _ => self.discarded += 1,
                },
                State::Data => match c {
                    SOF => {
                        // Like the firmware: an unescaped SOF mid-frame is an error.
                        self.discarded += self.buf.len() + 1;
                        self.buf.clear();
                        self.state = State::Error;
                    }
                    ESC => self.state = State::Escaped,
                    EOF => {
                        out.push(std::mem::take(&mut self.buf));
                        self.state = State::Idle;
                    }
                    _ => self.push(c),
                },
                State::Escaped => {
                    self.push(c);
                    if self.state == State::Escaped {
                        self.state = State::Data;
                    }
                }
            }
        }
        out
    }

    fn push(&mut self, c: u8) {
        if self.buf.len() >= MAX_FRAME {
            self.discarded += self.buf.len();
            self.buf.clear();
            self.state = State::Error;
            return;
        }
        self.buf.push(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_special_bytes() {
        let payload = vec![1, SOF, 2, ESC, EOF, 3];
        let framed = encode(&payload);
        assert_eq!(framed, vec![SOF, 1, ESC, SOF, 2, ESC, ESC, ESC, EOF, 3, EOF]);
        let mut d = Decoder::default();
        assert_eq!(d.feed(&framed), vec![payload.clone()]);
        // Byte by byte, with garbage in between and two frames back to back.
        let mut d = Decoder::default();
        let mut got = Vec::new();
        let stream: Vec<u8> = [vec![0x00, 0x42], framed.clone(), encode(&[9]), vec![7]].concat();
        for b in stream {
            got.extend(d.feed(&[b]));
        }
        assert_eq!(got, vec![payload, vec![9]]);
        assert_eq!(d.discarded, 3);
    }

    #[test]
    fn broken_frame_is_skipped() {
        let mut d = Decoder::default();
        // SOF, data, unescaped SOF (error: like the firmware, it doesn't start a frame)
        // … EOF, then a good frame.
        let got = d.feed(&[SOF, 1, 2, SOF, 3, EOF, SOF, 4, EOF]);
        assert_eq!(got, vec![vec![4]]);
        let got = d.feed(&encode(&[5]));
        assert_eq!(got, vec![vec![5]]);
        assert_eq!(encode(&[]), vec![SOF, EOF]);
        assert_eq!(Decoder::default().feed(&encode(&[])), vec![Vec::<u8>::new()]);
    }
}
