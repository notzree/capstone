use core::fmt;

/// Why bytes could not be decoded. Every variant means "drop this frame and continue".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DecodeError {
    /// Fewer bytes than the layout requires.
    Truncated,
    /// More bytes than the layout requires.
    TrailingBytes,
    UnsupportedVersion(u8),
    UnknownKind(u8),
    /// A value this version does not define: an unknown enum value, a reserved bit, or nonzero
    /// bytes in an absent optional block.
    InvalidField(&'static str),
    /// A CSI payload longer than [`MAX_CSI_LEN`](crate::MAX_CSI_LEN).
    CsiTooLong(u16),
    BadCrc,
    BadCobs,
    /// A stream frame exceeded the maximum encoded length before its delimiter.
    Oversized,
    BadMagic,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated => f.write_str("message is shorter than its layout"),
            Self::TrailingBytes => f.write_str("message is longer than its layout"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported wire version {v}"),
            Self::UnknownKind(k) => write!(f, "unknown message kind {k:#04x}"),
            Self::InvalidField(field) => write!(f, "invalid {field}"),
            Self::CsiTooLong(n) => write!(f, "CSI payload of {n} bytes exceeds the maximum"),
            Self::BadCrc => f.write_str("CRC mismatch"),
            Self::BadCobs => f.write_str("malformed COBS encoding"),
            Self::Oversized => f.write_str("stream frame exceeds the maximum length"),
            Self::BadMagic => f.write_str("missing probe magic"),
        }
    }
}

impl core::error::Error for DecodeError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EncodeError {
    BufferTooSmall,
    /// A CSI payload longer than [`MAX_CSI_LEN`](crate::MAX_CSI_LEN).
    CsiTooLong(usize),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooSmall => f.write_str("output buffer too small"),
            Self::CsiTooLong(n) => write!(f, "CSI payload of {n} bytes exceeds the maximum"),
        }
    }
}

impl core::error::Error for EncodeError {}
