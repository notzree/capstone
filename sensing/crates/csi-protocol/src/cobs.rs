//! Consistent Overhead Byte Stuffing: removes every zero byte so zero can delimit frames.

use crate::{DecodeError, EncodeError};

/// Upper bound on [`encode`] output for `n` input bytes.
pub const fn max_encoded_len(n: usize) -> usize {
    n + n / 254 + 1
}

/// Encodes `input` into `out` without a trailing delimiter; returns the encoded length.
pub fn encode(input: &[u8], out: &mut [u8]) -> Result<usize, EncodeError> {
    let mut code_at = 0;
    let mut len = 1;
    let mut code: u8 = 1;
    for (i, &byte) in input.iter().enumerate() {
        if byte != 0 {
            *out.get_mut(len).ok_or(EncodeError::BufferTooSmall)? = byte;
            len += 1;
            code += 1;
        }
        let more = i + 1 < input.len();
        if byte == 0 || (code == 0xFF && more) {
            *out.get_mut(code_at).ok_or(EncodeError::BufferTooSmall)? = code;
            code_at = len;
            len += 1;
            code = 1;
        }
    }
    *out.get_mut(code_at).ok_or(EncodeError::BufferTooSmall)? = code;
    Ok(len)
}

/// Decodes `buf` (without its delimiter) in place; returns the decoded length.
pub fn decode_in_place(buf: &mut [u8]) -> Result<usize, DecodeError> {
    let mut read = 0;
    let mut write = 0;
    while let Some(&code) = buf.get(read) {
        let start = read + 1;
        let end = start
            + usize::from(code)
                .checked_sub(1)
                .ok_or(DecodeError::BadCobs)?;
        let block = buf.get(start..end).ok_or(DecodeError::BadCobs)?;
        if block.contains(&0) {
            return Err(DecodeError::BadCobs);
        }
        buf.copy_within(start..end, write);
        write += end - start;
        read = end;
        if code != 0xFF && read < buf.len() {
            *buf.get_mut(write).ok_or(DecodeError::BadCobs)? = 0;
            write += 1;
        }
    }
    Ok(write)
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::vec;
    use std::vec::Vec;

    use super::*;

    fn roundtrip(input: &[u8], expected: &[u8]) {
        let mut out = vec![0; max_encoded_len(input.len())];
        let n = encode(input, &mut out).unwrap();
        assert_eq!(&out[..n], expected);
        let n = decode_in_place(&mut out[..n]).unwrap();
        assert_eq!(&out[..n], input);
    }

    #[test]
    fn reference_vectors() {
        roundtrip(&[], &[0x01]);
        roundtrip(&[0x00], &[0x01, 0x01]);
        roundtrip(&[0x00, 0x00], &[0x01, 0x01, 0x01]);
        roundtrip(&[0x00, 0x11, 0x00], &[0x01, 0x02, 0x11, 0x01]);
        roundtrip(&[0x11, 0x22, 0x00, 0x33], &[0x03, 0x11, 0x22, 0x02, 0x33]);
        roundtrip(&[0x11, 0x22, 0x33, 0x44], &[0x05, 0x11, 0x22, 0x33, 0x44]);
        roundtrip(&[0x11, 0x00, 0x00, 0x00], &[0x02, 0x11, 0x01, 0x01, 0x01]);
    }

    #[test]
    fn long_runs() {
        let run: Vec<u8> = (1..=0xFE).collect();
        let mut expected = vec![0xFF];
        expected.extend(&run);
        roundtrip(&run, &expected);

        let run: Vec<u8> = (1..=0xFF).collect();
        let mut expected = vec![0xFF];
        expected.extend(1..=0xFE);
        expected.extend([0x02, 0xFF]);
        roundtrip(&run, &expected);
    }

    #[test]
    fn rejects_malformed() {
        assert_eq!(decode_in_place(&mut [0x00]), Err(DecodeError::BadCobs));
        assert_eq!(
            decode_in_place(&mut [0x03, 0x11]),
            Err(DecodeError::BadCobs)
        );
        assert_eq!(
            decode_in_place(&mut [0x03, 0x11, 0x00]),
            Err(DecodeError::BadCobs)
        );
    }

    #[test]
    fn reports_small_buffer() {
        assert_eq!(
            encode(&[1, 2, 3], &mut [0; 3]),
            Err(EncodeError::BufferTooSmall)
        );
        assert_eq!(encode(&[], &mut []), Err(EncodeError::BufferTooSmall));
    }
}
