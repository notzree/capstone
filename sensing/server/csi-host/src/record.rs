use csi_protocol::DecodeError;

/// One unit received from a source, stamped with host time on arrival.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Host monotonic time since the session started, ns.
    pub mono_ns: u64,
    /// Host wall-clock time, ns since the Unix epoch.
    pub wall_ns: u64,
    /// Index into the session's sources.
    pub source: u16,
    pub kind: RecordKind,
    pub payload: Vec<u8>,
}

impl Record {
    pub fn stream_error(mono_ns: u64, wall_ns: u64, source: u16, error: StreamError) -> Self {
        Self {
            mono_ns,
            wall_ns,
            source,
            kind: RecordKind::StreamError,
            payload: vec![error as u8],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RecordKind {
    /// A `csi-protocol` frame exactly as received (message and CRC), whether or not it decodes.
    Frame,
    /// A byte-stream framing failure; the payload is one [`StreamError`] code.
    StreamError,
    /// Written by a newer recorder; readers count and skip it.
    Unknown(u8),
}

impl RecordKind {
    pub const fn code(self) -> u8 {
        match self {
            Self::Frame => 1,
            Self::StreamError => 2,
            Self::Unknown(code) => code,
        }
    }

    pub const fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Frame,
            2 => Self::StreamError,
            code => Self::Unknown(code),
        }
    }
}

/// A failure a stream deframer reports without producing frame bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum StreamError {
    Oversized = 1,
    BadCobs = 2,
}

impl StreamError {
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Oversized),
            2 => Some(Self::BadCobs),
            _ => None,
        }
    }

    pub const fn from_decode_error(error: DecodeError) -> Option<Self> {
        match error {
            DecodeError::Oversized => Some(Self::Oversized),
            DecodeError::BadCobs => Some(Self::BadCobs),
            _ => None,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Oversized => "oversized",
            Self::BadCobs => "bad_cobs",
        }
    }
}
