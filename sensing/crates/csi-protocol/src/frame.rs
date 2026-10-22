//! A frame is one encoded message followed by its CRC-32 ([`crate::crc`]), little-endian.
//! Datagram transports carry exactly one frame per datagram; byte streams wrap frames with
//! [`crate::stream`].

use crate::crc::crc32;
use crate::message::{self, Header, Message};
use crate::{DecodeError, EncodeError};

pub const CRC_LEN: usize = 4;

/// Encodes a message and its CRC into `out`; returns the frame length.
pub fn encode(
    header: &Header,
    message: &Message<'_>,
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    let len = message::encode(header, message, out)?;
    let (body, rest) = out
        .split_at_mut_checked(len)
        .ok_or(EncodeError::BufferTooSmall)?;
    rest.first_chunk_mut::<CRC_LEN>()
        .ok_or(EncodeError::BufferTooSmall)?
        .copy_from_slice(&crc32(body).to_le_bytes());
    Ok(len + CRC_LEN)
}

/// Verifies the CRC, then decodes the message.
pub fn decode(frame: &[u8]) -> Result<(Header, Message<'_>), DecodeError> {
    let (body, crc) = frame
        .split_last_chunk::<CRC_LEN>()
        .ok_or(DecodeError::Truncated)?;
    if crc32(body) != u32::from_le_bytes(*crc) {
        return Err(DecodeError::BadCrc);
    }
    message::decode(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_FRAME_LEN;
    use crate::message::{NodeId, TimeProbe};

    const HEADER: Header = Header {
        node: NodeId([1, 2, 3, 4, 5, 6]),
        epoch: 1,
        seq: 2,
    };

    #[test]
    fn roundtrip_and_crc_check() {
        let message = Message::TimeProbe(TimeProbe {
            probe_id: 5,
            host_time: 6,
        });
        let mut buf = [0; MAX_FRAME_LEN];
        let n = encode(&HEADER, &message, &mut buf).unwrap();
        assert_eq!(decode(&buf[..n]), Ok((HEADER, message)));

        buf[3] ^= 0x01;
        assert_eq!(decode(&buf[..n]), Err(DecodeError::BadCrc));
        assert_eq!(decode(&buf[..3]), Err(DecodeError::Truncated));
    }

    #[test]
    fn needs_room_for_crc() {
        let message = Message::TimeProbe(TimeProbe::default());
        let mut buf = [0; 28 + CRC_LEN - 1];
        assert_eq!(
            encode(&HEADER, &message, &mut buf),
            Err(EncodeError::BufferTooSmall)
        );
    }
}
