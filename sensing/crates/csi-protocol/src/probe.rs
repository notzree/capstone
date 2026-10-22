//! Payload a sensing transmitter puts in each over-air frame. Receivers read it in the CSI callback
//! and attach it to the CSI record, which ties each measurement to the probe that produced it.
//!
//! ```text
//! offset  size  field
//! 0       4     magic "CSIp"
//! 4       1     version (= PROBE_VERSION)
//! 5       6     tx: transmitter NodeId
//! 11      4     epoch: transmitter boot epoch
//! 15      4     schedule: id of the schedule the probe was sent under; 0 = unscheduled
//! 19      4     seq: transmitter's probe sequence within the epoch, wrapping
//! ```
//!
//! Bytes after the probe are padding and are ignored.

use crate::message::NodeId;
use crate::wire::{Reader, Writer};
use crate::{DecodeError, EncodeError};

pub const PROBE_MAGIC: [u8; 4] = *b"CSIp";
/// Versioned separately from messages: transmitters and receivers flash independently, and the
/// probe layout has not changed.
pub const PROBE_VERSION: u8 = 1;
pub const PROBE_LEN: usize = 23;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Probe {
    pub tx: NodeId,
    pub epoch: u32,
    pub schedule: u32,
    pub seq: u32,
}

impl Probe {
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut w = Writer::new(out);
        w.bytes(&PROBE_MAGIC)?;
        w.u8(PROBE_VERSION)?;
        self.write_fields(&mut w)?;
        Ok(w.len())
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader::new(bytes);
        if r.array()? != PROBE_MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let version = r.u8()?;
        if version != PROBE_VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        Self::read_fields(&mut r)
    }

    /// Finds the first decodable probe anywhere in a received frame body.
    pub fn find(payload: &[u8]) -> Option<Self> {
        payload
            .windows(PROBE_MAGIC.len())
            .enumerate()
            .filter(|(_, window)| *window == PROBE_MAGIC)
            .find_map(|(at, _)| Self::decode(payload.get(at..)?).ok())
    }

    pub(crate) fn write_fields(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        w.bytes(&self.tx.0)?;
        w.u32(self.epoch)?;
        w.u32(self.schedule)?;
        w.u32(self.seq)
    }

    pub(crate) fn read_fields(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            tx: NodeId(r.array()?),
            epoch: r.u32()?,
            schedule: r.u32()?,
            seq: r.u32()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROBE: Probe = Probe {
        tx: NodeId([0xa0, 0xb7, 0x65, 0xdc, 0x01, 0x68]),
        epoch: 7,
        schedule: 3,
        seq: 99,
    };

    #[test]
    fn roundtrip_with_padding() {
        let mut buf = [0xEE; 40];
        assert_eq!(PROBE.encode(&mut buf), Ok(PROBE_LEN));
        assert_eq!(Probe::decode(&buf), Ok(PROBE));
    }

    #[test]
    fn found_after_other_bytes() {
        let mut buf = [0u8; 64];
        buf[..5].copy_from_slice(b"xCSIp");
        PROBE.encode(&mut buf[17..]).unwrap();
        assert_eq!(Probe::find(&buf), Some(PROBE));
        assert_eq!(Probe::find(&buf[..30]), None);
    }

    #[test]
    fn rejects_wrong_magic_and_version() {
        let mut buf = [0u8; PROBE_LEN];
        PROBE.encode(&mut buf).unwrap();
        buf[4] = PROBE_VERSION + 1;
        assert_eq!(
            Probe::decode(&buf),
            Err(DecodeError::UnsupportedVersion(PROBE_VERSION + 1))
        );
        buf[0] = b'X';
        assert_eq!(Probe::decode(&buf), Err(DecodeError::BadMagic));
        assert_eq!(Probe::decode(&buf[..3]), Err(DecodeError::Truncated));
    }
}
