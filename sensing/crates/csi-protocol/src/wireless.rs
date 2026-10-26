//! Bounded wireless carrier: probe identity + optional turn tag + original frames.
//! Records retain their own CRC, node/epoch/sequence and native capture timestamps.
//! The carrier CRC covers the complete batch. No nested carriers or foreign identities.
#![allow(
    clippy::indexing_slicing,
    reason = "All indexing follows explicit fixed-header/length bounds"
)]

use crate::{DecodeError, EncodeError, Header, Probe, crc::crc32, frame};

pub const MAGIC: &[u8; 4] = b"CSIb";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 55;
pub const MAX_LEN: usize = 1200;
pub const MAX_RECORDS: usize = 16;
pub const MAX_TURN_LEN: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Carrier {
    pub probe: Probe,
    /// False for an export-only carrier: receiving it must not create a sensing observation.
    pub sensing: bool,
    pub sent_node_us: u64,
    pub batch_seq: u32,
    /// Destination on the configured hub; relays do not rewrite observation identities.
    pub hub_port: u16,
    /// Gateway forwarding queue/send losses, separate from this node's own export drops.
    pub relay_drops: u64,
}

pub struct Builder<'a> {
    out: &'a mut [u8],
    len: usize,
    count: u8,
    carrier: Carrier,
}

impl<'a> Builder<'a> {
    pub fn new(out: &'a mut [u8], carrier: Carrier, turn: &[u8]) -> Result<Self, EncodeError> {
        if turn.len() > MAX_TURN_LEN || out.len() < HEADER_LEN + turn.len() + 4 {
            return Err(EncodeError::BufferTooSmall);
        }
        carrier.probe.encode(out)?;
        out[23..27].copy_from_slice(MAGIC);
        out[27] = VERSION;
        out[28] = u8::from(carrier.sensing);
        out[29] = turn.len() as u8;
        out[30] = 0;
        out[31..39].copy_from_slice(&carrier.sent_node_us.to_le_bytes());
        out[39..43].copy_from_slice(&carrier.batch_seq.to_le_bytes());
        out[43..45].copy_from_slice(&carrier.hub_port.to_le_bytes());
        out[45..47].copy_from_slice(&0u16.to_le_bytes());
        out[47..55].copy_from_slice(&carrier.relay_drops.to_le_bytes());
        out[HEADER_LEN..HEADER_LEN + turn.len()].copy_from_slice(turn);
        Ok(Self {
            out,
            len: HEADER_LEN + turn.len(),
            count: 0,
            carrier,
        })
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<(), EncodeError> {
        let Ok((header, message)) = frame::decode(bytes) else {
            return Err(EncodeError::BufferTooSmall);
        };
        if header.node != self.carrier.probe.tx
            || header.epoch != self.carrier.probe.epoch
            || message.kind().is_host_to_node()
            || self.count as usize >= MAX_RECORDS
            || self.len + 2 + bytes.len() + 4 > self.out.len().min(MAX_LEN)
        {
            return Err(EncodeError::BufferTooSmall);
        }
        self.out[self.len..self.len + 2].copy_from_slice(&(bytes.len() as u16).to_le_bytes());
        self.len += 2;
        self.out[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
        self.count += 1;
        Ok(())
    }

    pub fn count(&self) -> usize {
        self.count as usize
    }

    pub fn finish(mut self) -> usize {
        self.out[30] = self.count;
        self.out[45..47].copy_from_slice(&(self.len as u16).to_le_bytes());
        let crc = crc32(&self.out[..self.len]);
        self.out[self.len..self.len + 4].copy_from_slice(&crc.to_le_bytes());
        self.len += 4;
        self.len
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Batch<'a> {
    pub carrier: Carrier,
    pub turn: &'a [u8],
    pub count: usize,
    records: &'a [u8],
}

impl<'a> Batch<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, DecodeError> {
        let carrier = peek(bytes)?;
        let end = usize::from(u16::from_le_bytes([bytes[45], bytes[46]]));
        let turn_len = bytes[29] as usize;
        if end + 4 != bytes.len() || bytes.len() > MAX_LEN || end < HEADER_LEN + turn_len {
            return Err(DecodeError::InvalidField("wireless length"));
        }
        if crc32(&bytes[..end])
            != u32::from_le_bytes([bytes[end], bytes[end + 1], bytes[end + 2], bytes[end + 3]])
        {
            return Err(DecodeError::BadCrc);
        }
        let batch = Self {
            carrier,
            turn: &bytes[HEADER_LEN..HEADER_LEN + turn_len],
            count: bytes[30] as usize,
            records: &bytes[HEADER_LEN + turn_len..end],
        };
        let mut count = 0;
        for bytes in batch.frames() {
            let (header, message) = frame::decode(bytes)?;
            if header.node != carrier.probe.tx
                || header.epoch != carrier.probe.epoch
                || message.kind().is_host_to_node()
            {
                return Err(DecodeError::InvalidField("wireless record identity"));
            }
            count += 1;
        }
        // Validate lengths separately: Frames intentionally never exposes a partial record.
        let consumed: usize = batch.frames().map(|b| 2 + b.len()).sum();
        if count != batch.count || consumed != batch.records.len() {
            return Err(DecodeError::InvalidField("wireless record count"));
        }
        Ok(batch)
    }

    pub fn frames(&self) -> Frames<'a> {
        Frames(self.records)
    }
}

/// Read only the fixed routing/header fields. Does not establish CRC validity.
pub fn peek(bytes: &[u8]) -> Result<Carrier, DecodeError> {
    if bytes.len() < HEADER_LEN + 4 {
        return Err(DecodeError::Truncated);
    }
    let probe = Probe::decode(bytes)?;
    if &bytes[23..27] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    if bytes[27] != VERSION {
        return Err(DecodeError::UnsupportedVersion(bytes[27]));
    }
    if bytes[28] > 1 || bytes[29] as usize > MAX_TURN_LEN || bytes[30] as usize > MAX_RECORDS {
        return Err(DecodeError::InvalidField("wireless flags/count"));
    }
    Ok(Carrier {
        probe,
        sensing: bytes[28] == 1,
        sent_node_us: u64::from_le_bytes(
            bytes[31..39]
                .try_into()
                .map_err(|_| DecodeError::Truncated)?,
        ),
        batch_seq: u32::from_le_bytes(
            bytes[39..43]
                .try_into()
                .map_err(|_| DecodeError::Truncated)?,
        ),
        hub_port: u16::from_le_bytes([bytes[43], bytes[44]]),
        relay_drops: u64::from_le_bytes(
            bytes[47..55]
                .try_into()
                .map_err(|_| DecodeError::Truncated)?,
        ),
    })
}

pub struct Frames<'a>(&'a [u8]);
impl<'a> Iterator for Frames<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<Self::Item> {
        let prefix = self.0.get(..2)?;
        let n = u16::from_le_bytes([prefix[0], prefix[1]]) as usize;
        let bytes = self.0.get(2..2 + n)?;
        self.0 = self.0.get(2 + n..)?;
        Some(bytes)
    }
}

/// CSI callbacks may see raw ESP-NOW vendor element separators in long packets.
/// Only inspect the compact header at the first probe, never decode batch bodies here.
pub fn sensing_probe(payload: &[u8]) -> Option<Probe> {
    let at = payload
        .windows(4)
        .position(|w| w == crate::probe::PROBE_MAGIC)?;
    let bytes = payload.get(at..)?;
    let probe = Probe::decode(bytes).ok()?;
    if bytes.get(23..27) == Some(MAGIC.as_slice()) {
        return peek(bytes).ok().filter(|h| h.sensing).map(|h| h.probe);
    }
    Some(probe) // Legacy probe-only frames retain their existing compatibility.
}

pub fn identity(header: Header) -> Probe {
    Probe {
        tx: header.node,
        epoch: header.epoch,
        schedule: 0,
        seq: 0,
    }
}

/// Attach a scheduled sensing identity without changing original records.
pub fn retag(bytes: &mut [u8], carrier: Carrier, turn: &[u8]) -> Result<(), EncodeError> {
    let previous = Batch::decode(bytes).map_err(|_| EncodeError::BufferTooSmall)?;
    if previous.turn.len() != turn.len()
        || previous.carrier.probe.tx != carrier.probe.tx
        || previous.carrier.probe.epoch != carrier.probe.epoch
    {
        return Err(EncodeError::BufferTooSmall);
    }
    carrier.probe.encode(bytes)?;
    bytes[28] = u8::from(carrier.sensing);
    bytes[31..39].copy_from_slice(&carrier.sent_node_us.to_le_bytes());
    bytes[39..43].copy_from_slice(&carrier.batch_seq.to_le_bytes());
    bytes[43..45].copy_from_slice(&carrier.hub_port.to_le_bytes());
    bytes[47..55].copy_from_slice(&carrier.relay_drops.to_le_bytes());
    bytes[HEADER_LEN..HEADER_LEN + turn.len()].copy_from_slice(turn);
    let end = bytes.len() - 4;
    let crc = crc32(&bytes[..end]);
    bytes[end..].copy_from_slice(&crc.to_le_bytes());
    Ok(())
}
