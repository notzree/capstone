//! Byte-stream transport (UART, TCP): each frame is COBS-encoded and followed by a zero
//! delimiter. A receiver resynchronizes at the next delimiter after corruption or a mid-frame
//! start, so senders may emit extra delimiters at any time (for example at boot); empty frames are
//! ignored.

use crate::message::{Header, Message};
use crate::{DecodeError, EncodeError, MAX_FRAME_LEN, cobs, frame};

/// Longest encoded frame, including its delimiter.
pub const MAX_ENCODED_LEN: usize = cobs::max_encoded_len(MAX_FRAME_LEN) + 1;

/// Reusable buffers for producing stream frames without allocating.
pub struct Encoder {
    frame: [u8; MAX_FRAME_LEN],
    out: [u8; MAX_ENCODED_LEN],
}

impl Encoder {
    pub const fn new() -> Self {
        Self {
            frame: [0; MAX_FRAME_LEN],
            out: [0; MAX_ENCODED_LEN],
        }
    }

    /// Returns the bytes to write, delimiter included.
    pub fn encode(&mut self, header: &Header, message: &Message<'_>) -> Result<&[u8], EncodeError> {
        let len = frame::encode(header, message, &mut self.frame)?;
        let frame = self.frame.get(..len).ok_or(EncodeError::BufferTooSmall)?;
        let len = cobs::encode(frame, &mut self.out)?;
        *self.out.get_mut(len).ok_or(EncodeError::BufferTooSmall)? = 0;
        self.out.get(..=len).ok_or(EncodeError::BufferTooSmall)
    }
}

impl Default for Encoder {
    fn default() -> Self {
        Self::new()
    }
}

/// Splits a received byte stream into frames for [`frame::decode`].
pub struct Deframer {
    buf: [u8; MAX_ENCODED_LEN - 1],
    len: usize,
    overflowed: bool,
}

impl Deframer {
    pub const fn new() -> Self {
        Self {
            buf: [0; MAX_ENCODED_LEN - 1],
            len: 0,
            overflowed: false,
        }
    }

    /// Consumes `input` up to and including the next delimiter and returns the decoded frame, or
    /// returns `None` once `input` is exhausted mid-frame. The partial frame is kept for the next
    /// call.
    pub fn push(&mut self, input: &mut &[u8]) -> Option<Result<&[u8], DecodeError>> {
        loop {
            let (&byte, rest) = input.split_first()?;
            *input = rest;
            if byte != 0 {
                match self.buf.get_mut(self.len) {
                    Some(slot) if !self.overflowed => {
                        *slot = byte;
                        self.len += 1;
                    }
                    _ => self.overflowed = true,
                }
                continue;
            }
            let len = core::mem::take(&mut self.len);
            if core::mem::take(&mut self.overflowed) {
                return Some(Err(DecodeError::Oversized));
            }
            if len == 0 {
                continue;
            }
            let Some(encoded) = self.buf.get_mut(..len) else {
                return Some(Err(DecodeError::Oversized));
            };
            return Some(match cobs::decode_in_place(encoded) {
                Ok(n) => encoded.get(..n).ok_or(DecodeError::BadCobs),
                Err(e) => Err(e),
            });
        }
    }

    /// Discards any partial frame, e.g. after the transport reconnects.
    pub fn reset(&mut self) {
        self.len = 0;
        self.overflowed = false;
    }
}

impl Default for Deframer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec::Vec;

    use super::*;
    use crate::MAX_CSI_LEN;
    use crate::message::{Csi, NodeId, RxMeta, TimeProbe};

    const HEADER: Header = Header {
        node: NodeId([1, 2, 3, 4, 5, 6]),
        epoch: 1,
        seq: 2,
    };

    fn decode_all(deframer: &mut Deframer, mut input: &[u8]) -> Vec<Result<u32, DecodeError>> {
        let mut seqs = Vec::new();
        while let Some(frame) = deframer.push(&mut input) {
            seqs.push(frame.and_then(|f| frame::decode(f).map(|(h, _)| h.seq)));
        }
        seqs
    }

    fn time_probe(seq: u32) -> Vec<u8> {
        let header = Header { seq, ..HEADER };
        Encoder::new()
            .encode(&header, &Message::TimeProbe(TimeProbe::default()))
            .unwrap()
            .to_vec()
    }

    #[test]
    fn skips_empty_frames_and_resyncs_after_garbage() {
        let mut stream = b"\x00\x00boot garbage\x00".to_vec();
        stream.extend(time_probe(7));
        stream.extend(time_probe(8));
        let results = decode_all(&mut Deframer::new(), &stream);
        assert_eq!(results.len(), 3);
        assert!(results[0].is_err());
        assert_eq!(results[1..], [Ok(7), Ok(8)]);
    }

    #[test]
    fn frames_split_across_reads() {
        let stream = [time_probe(1), time_probe(2)].concat();
        let mut deframer = Deframer::new();
        let mut seqs = Vec::new();
        for chunk in stream.chunks(3) {
            seqs.extend(decode_all(&mut deframer, chunk));
        }
        assert_eq!(seqs, [Ok(1), Ok(2)]);
    }

    #[test]
    fn oversized_frame_is_reported_once_then_recovers() {
        let mut stream = std::vec![0xAB; MAX_ENCODED_LEN + 10];
        stream.push(0);
        stream.extend(time_probe(3));
        assert_eq!(
            decode_all(&mut Deframer::new(), &stream),
            [Err(DecodeError::Oversized), Ok(3)]
        );
    }

    #[test]
    fn largest_frame_fits() {
        let data = [0x5A; MAX_CSI_LEN];
        let message = Message::Csi(Csi {
            capture_seq: u32::MAX,
            node_time_us: u64::MAX,
            rx_timestamp_us: u32::MAX,
            source_mac: [0xFF; 6],
            probe: None,
            first_word_invalid: false,
            rx: RxMeta::default(),
            data: &data,
        });
        let mut encoder = Encoder::new();
        let encoded = encoder.encode(&HEADER, &message).unwrap().to_vec();
        assert!(encoded.len() <= MAX_ENCODED_LEN);
        let mut deframer = Deframer::new();
        let mut input = encoded.as_slice();
        let frame = deframer.push(&mut input).unwrap().unwrap();
        assert_eq!(frame::decode(frame), Ok((HEADER, message)));
    }
}
