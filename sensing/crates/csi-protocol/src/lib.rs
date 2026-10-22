//! Versioned wire contract shared by sensing nodes and the host.
//!
//! Layers, innermost first:
//!
//! - [`message`]: a 16-byte header plus a kind-specific body.
//! - [`frame`]: a message plus its CRC-32; one frame per datagram.
//! - [`stream`]: COBS-encoded frames separated by zero bytes, for UART and TCP.
//! - [`probe`]: the payload sensing transmitters put in over-air frames, which receivers attach to
//!   each CSI record.
//!
//! All integers are little-endian. Decoding never allocates or panics on malformed input; every
//! failure is a [`DecodeError`] and the caller drops that frame.
//!
//! [`VERSION`] changes whenever an existing layout changes. Encoders emit `VERSION`; decoders accept
//! [`MIN_VERSION`]`..=VERSION` and decode older layouts into the current types, with fields an old
//! version lacks set to `None`. New message kinds can be added without a version change: older
//! receivers report them as [`DecodeError::UnknownKind`] and skip the frame. Hosts must be updated
//! before nodes.
//!
//! Must build without `std` or a required allocator (`cargo check-nostd`).
#![no_std]
#![cfg_attr(
    not(test),
    deny(
        clippy::indexing_slicing,
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic
    )
)]

pub mod cobs;
pub mod crc;
mod error;
pub mod frame;
pub mod message;
pub mod probe;
pub mod stream;
mod wire;

pub use error::{DecodeError, EncodeError};
pub use message::{
    Chip, Counters, Csi, CsiConfig, Header, Hello, Kind, Message, NodeId, ParseNodeIdError, Roles,
    RxGain, RxMeta, TimeEcho, TimeProbe,
};
pub use probe::Probe;

/// Version 2 added receiver gain to [`RxMeta`].
pub const VERSION: u8 = 2;

pub const MIN_VERSION: u8 = 1;

/// Upper bound on native CSI bytes per record. Classic ESP32 reports at most 612; the rest is
/// headroom for other ESP32-family layouts.
pub const MAX_CSI_LEN: usize = 1024;

pub const MAX_MESSAGE_LEN: usize = message::HEADER_LEN + message::CSI_FIXED_LEN + MAX_CSI_LEN;

pub const MAX_FRAME_LEN: usize = MAX_MESSAGE_LEN + frame::CRC_LEN;
