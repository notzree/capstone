use crate::{DecodeError, EncodeError};

pub(crate) struct Writer<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, len: 0 }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn bytes(&mut self, src: &[u8]) -> Result<(), EncodeError> {
        let end = self
            .len
            .checked_add(src.len())
            .ok_or(EncodeError::BufferTooSmall)?;
        self.buf
            .get_mut(self.len..end)
            .ok_or(EncodeError::BufferTooSmall)?
            .copy_from_slice(src);
        self.len = end;
        Ok(())
    }

    pub(crate) fn u8(&mut self, v: u8) -> Result<(), EncodeError> {
        self.bytes(&[v])
    }

    pub(crate) fn i8(&mut self, v: i8) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }

    pub(crate) fn u16(&mut self, v: u16) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }

    pub(crate) fn u32(&mut self, v: u32) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }

    pub(crate) fn u64(&mut self, v: u64) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }
}

pub(crate) struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let (head, rest) = self.buf.split_at_checked(n).ok_or(DecodeError::Truncated)?;
        self.buf = rest;
        Ok(head)
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (head, rest) = self
            .buf
            .split_first_chunk::<N>()
            .ok_or(DecodeError::Truncated)?;
        self.buf = rest;
        Ok(*head)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, DecodeError> {
        self.array().map(u8::from_le_bytes)
    }

    pub(crate) fn i8(&mut self) -> Result<i8, DecodeError> {
        self.array().map(i8::from_le_bytes)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, DecodeError> {
        self.array().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        self.array().map(u32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Result<u64, DecodeError> {
        self.array().map(u64::from_le_bytes)
    }

    pub(crate) fn finish(self) -> Result<(), DecodeError> {
        if self.buf.is_empty() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes)
        }
    }
}

/// Packs `flags[i]` into bit `i`.
pub(crate) fn pack_flags<const N: usize>(flags: [bool; N]) -> u8 {
    flags
        .iter()
        .enumerate()
        .fold(0, |byte, (bit, &set)| byte | (u8::from(set) << bit))
}

/// Inverse of [`pack_flags`]; bits at or above `N` are reserved and must be zero.
pub(crate) fn unpack_flags<const N: usize>(
    byte: u8,
    field: &'static str,
) -> Result<[bool; N], DecodeError> {
    if N < 8 && byte >> N != 0 {
        return Err(DecodeError::InvalidField(field));
    }
    Ok(core::array::from_fn(|bit| byte & (1 << bit) != 0))
}
