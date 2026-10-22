//! Node ↔ host messages: a fixed header followed by a kind-specific body.
//!
//! ```text
//! offset  size  field
//! 0       1     version (MIN_VERSION..=VERSION)
//! 1       1     kind
//! 2       6     node: the sensing node the message is from (node → host) or for (host → node)
//! 8       4     epoch: sender's boot epoch, random and nonzero per boot; 0 = unknown
//! 12      4     seq: sender's frame sequence within the epoch, wrapping
//! 16      ..    body
//! ```
//!
//! A sender assigns `seq` when it encodes a frame, so frames its export queue drops still consume
//! a number (and are counted in [`Counters::export_drops`]). Bodies have fixed layouts, documented
//! on each type, except [`Csi`], whose native CSI bytes are length-prefixed and end the message.
//!
//! The CSI byte layout is not sent separately: it follows from [`Hello::chip`] and
//! [`Hello::csi_config`] together with each record's [`RxMeta`] and length.
//!
//! Only `Csi` differs between versions; every other layout is the same in versions 1 and 2.

use core::fmt;

use crate::probe::Probe;
use crate::wire::{Reader, Writer, pack_flags, unpack_flags};
use crate::{DecodeError, EncodeError, MAX_CSI_LEN, MIN_VERSION, VERSION};

pub const HEADER_LEN: usize = 16;
pub(crate) const CSI_FIXED_LEN: usize = 60;

/// Message type. Kinds with the high bit set travel host → node; `0x81` (schedule) and `0x82`
/// (configuration) are reserved for schedule distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Kind {
    Hello = 0x01,
    Csi = 0x02,
    Counters = 0x03,
    TimeEcho = 0x04,
    TimeProbe = 0x80,
}

impl Kind {
    pub const fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            0x01 => Self::Hello,
            0x02 => Self::Csi,
            0x03 => Self::Counters,
            0x04 => Self::TimeEcho,
            0x80 => Self::TimeProbe,
            _ => return None,
        })
    }

    pub const fn is_host_to_node(self) -> bool {
        self as u8 & 0x80 != 0
    }
}

/// A sensing node's factory MAC address: stable across reflashing and independent of the MAC its
/// radio is configured to transmit with.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct NodeId(pub [u8; 6]);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [a, b, c, d, e, g] = self.0;
        write!(f, "{a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{g:02x}")
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({self})")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseNodeIdError;

impl fmt::Display for ParseNodeIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("expected a MAC address like a0:b7:65:dd:e9:3c")
    }
}

impl core::error::Error for ParseNodeIdError {}

impl core::str::FromStr for NodeId {
    type Err = ParseNodeIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut bytes = [0; 6];
        let mut parts = s.split(':');
        for byte in &mut bytes {
            let part = parts
                .next()
                .filter(|p| p.len() == 2)
                .ok_or(ParseNodeIdError)?;
            *byte = u8::from_str_radix(part, 16).map_err(|_| ParseNodeIdError)?;
        }
        match parts.next() {
            None => Ok(Self(bytes)),
            Some(_) => Err(ParseNodeIdError),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Header {
    pub node: NodeId,
    pub epoch: u32,
    pub seq: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Message<'a> {
    Hello(Hello),
    Csi(Csi<'a>),
    Counters(Counters),
    TimeEcho(TimeEcho),
    TimeProbe(TimeProbe),
}

impl Message<'_> {
    pub const fn kind(&self) -> Kind {
        match self {
            Self::Hello(_) => Kind::Hello,
            Self::Csi(_) => Kind::Csi,
            Self::Counters(_) => Kind::Counters,
            Self::TimeEcho(_) => Kind::TimeEcho,
            Self::TimeProbe(_) => Kind::TimeProbe,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Chip {
    Esp32 = 1,
    Esp32S3 = 2,
}

impl Chip {
    pub const fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Esp32),
            2 => Some(Self::Esp32S3),
            _ => None,
        }
    }
}

/// Sensing roles a node is running. Neither set means the node is idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Roles {
    pub tx: bool,
    pub rx: bool,
}

/// The ESP-IDF `wifi_csi_config_t` the receiver applied; it determines which LTFs a record holds
/// and whether the radio smoothed or rescaled them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CsiConfig {
    pub lltf: bool,
    pub htltf: bool,
    pub stbc_htltf2: bool,
    pub ltf_merge: bool,
    pub channel_filter: bool,
    pub manu_scale: bool,
    pub shift: u8,
}

/// Node identity and capture configuration. Sent at boot and repeated periodically so a host that
/// attaches mid-stream learns it; decoding other messages never requires it.
///
/// ```text
/// offset  size  field
/// 0       1     chip (1 = ESP32, 2 = ESP32-S3)
/// 1       1     roles: bit 0 tx, bit 1 rx
/// 2       3     firmware_version (major, minor, patch)
/// 5       8     firmware_commit: leading bytes of the git commit; zero if unknown
/// 13      1     channel (configured primary)
/// 14      1     secondary_channel (0 none, 1 above, 2 below)
/// 15      1     csi_config: bit 0 lltf, 1 htltf, 2 stbc_htltf2, 3 ltf_merge,
///                 4 channel_filter, 5 manu_scale
/// 16      1     csi_config.shift
/// 17      2     pool_capacity: capture pool slots
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Hello {
    pub chip: Chip,
    pub roles: Roles,
    pub firmware_version: [u8; 3],
    pub firmware_commit: [u8; 8],
    pub channel: u8,
    pub secondary_channel: u8,
    pub csi_config: CsiConfig,
    pub pool_capacity: u16,
}

impl Hello {
    fn write(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        let c = &self.csi_config;
        w.u8(self.chip as u8)?;
        w.u8(pack_flags([self.roles.tx, self.roles.rx]))?;
        w.bytes(&self.firmware_version)?;
        w.bytes(&self.firmware_commit)?;
        w.u8(self.channel)?;
        w.u8(self.secondary_channel)?;
        w.u8(pack_flags([
            c.lltf,
            c.htltf,
            c.stbc_htltf2,
            c.ltf_merge,
            c.channel_filter,
            c.manu_scale,
        ]))?;
        w.u8(c.shift)?;
        w.u16(self.pool_capacity)
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        let chip = Chip::from_u8(r.u8()?).ok_or(DecodeError::InvalidField("chip"))?;
        let [tx, rx] = unpack_flags(r.u8()?, "roles")?;
        let firmware_version = r.array()?;
        let firmware_commit = r.array()?;
        let channel = r.u8()?;
        let secondary_channel = r.u8()?;
        let [
            lltf,
            htltf,
            stbc_htltf2,
            ltf_merge,
            channel_filter,
            manu_scale,
        ] = unpack_flags(r.u8()?, "csi_config")?;
        Ok(Self {
            chip,
            roles: Roles { tx, rx },
            firmware_version,
            firmware_commit,
            channel,
            secondary_channel,
            csi_config: CsiConfig {
                lltf,
                htltf,
                stbc_htltf2,
                ltf_merge,
                channel_filter,
                manu_scale,
                shift: r.u8()?,
            },
            pool_capacity: r.u16()?,
        })
    }
}

/// Receiver gain applied to one frame, as Espressif's `esp_csi_gain_ctrl_get_rx_gain` reports it.
/// Without it a gain step is indistinguishable from a change in the channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RxGain {
    pub agc: u8,
    pub fft: i8,
}

/// Radio metadata for one received frame: the classic ESP32 / ESP32-S3 `wifi_pkt_rx_ctrl_t`
/// fields, copied as the radio reported them, plus the receiver gain where the firmware can read
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct RxMeta {
    pub rssi: i8,
    pub noise_floor: i8,
    pub rate: u8,
    pub sig_mode: u8,
    pub mcs: u8,
    pub cwb: u8,
    pub stbc: u8,
    pub channel: u8,
    pub secondary_channel: u8,
    pub smoothing: bool,
    pub not_sounding: bool,
    pub aggregation: bool,
    pub fec_coding: bool,
    pub sgi: bool,
    pub ampdu_cnt: u8,
    pub ant: u8,
    pub rx_state: u8,
    pub sig_len: u16,
    /// `None` when the firmware could not read it, and always for version 1 records.
    pub gain: Option<RxGain>,
}

impl RxMeta {
    fn write(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        w.i8(self.rssi)?;
        w.i8(self.noise_floor)?;
        w.u8(self.rate)?;
        w.u8(self.sig_mode)?;
        w.u8(self.mcs)?;
        w.u8(self.cwb)?;
        w.u8(self.stbc)?;
        w.u8(self.channel)?;
        w.u8(self.secondary_channel)?;
        w.u8(pack_flags([
            self.smoothing,
            self.not_sounding,
            self.aggregation,
            self.fec_coding,
            self.sgi,
            self.gain.is_some(),
        ]))?;
        w.u8(self.ampdu_cnt)?;
        w.u8(self.ant)?;
        w.u8(self.rx_state)?;
        w.u16(self.sig_len)?;
        let gain = self.gain.unwrap_or_default();
        w.u8(gain.agc)?;
        w.i8(gain.fft)
    }

    fn read(r: &mut Reader<'_>, version: u8) -> Result<Self, DecodeError> {
        let rssi = r.i8()?;
        let noise_floor = r.i8()?;
        let rate = r.u8()?;
        let sig_mode = r.u8()?;
        let mcs = r.u8()?;
        let cwb = r.u8()?;
        let stbc = r.u8()?;
        let channel = r.u8()?;
        let secondary_channel = r.u8()?;
        let flags = r.u8()?;
        let [
            smoothing,
            not_sounding,
            aggregation,
            fec_coding,
            sgi,
            has_gain,
        ] = if version == 1 {
            let [a, b, c, d, e] = unpack_flags(flags, "rx flags")?;
            [a, b, c, d, e, false]
        } else {
            unpack_flags(flags, "rx flags")?
        };
        let ampdu_cnt = r.u8()?;
        let ant = r.u8()?;
        let rx_state = r.u8()?;
        let sig_len = r.u16()?;
        let gain = if version == 1 {
            None
        } else {
            let gain = RxGain {
                agc: r.u8()?,
                fft: r.i8()?,
            };
            match (has_gain, gain == RxGain::default()) {
                (true, _) => Some(gain),
                (false, true) => None,
                (false, false) => return Err(DecodeError::InvalidField("absent gain")),
            }
        };
        Ok(Self {
            rssi,
            noise_floor,
            rate,
            sig_mode,
            mcs,
            cwb,
            stbc,
            channel,
            secondary_channel,
            smoothing,
            not_sounding,
            aggregation,
            fec_coding,
            sgi,
            ampdu_cnt,
            ant,
            rx_state,
            sig_len,
            gain,
        })
    }
}

/// One CSI measurement, with native bytes exactly as the radio produced them.
///
/// ```text
/// offset  size  field
/// 0       4     capture_seq
/// 4       8     node_time_us
/// 12      4     rx_timestamp_us
/// 16      6     source_mac
/// 22      1     flags: bit 0 probe present, bit 1 first_word_invalid
/// 23      18    probe fields (tx, epoch, schedule, seq); all zero when absent
/// 41      17    rx: rssi, noise_floor, rate, sig_mode, mcs, cwb, stbc, channel,
///                 secondary_channel (1 byte each); flags (bit 0 smoothing,
///                 1 not_sounding, 2 aggregation, 3 fec_coding, 4 sgi, 5 gain valid);
///                 ampdu_cnt, ant, rx_state (1 byte each); sig_len (2); agc_gain (u8),
///                 fft_gain (i8), both zero when gain is not valid
/// 58      2     data_len (≤ MAX_CSI_LEN)
/// 60      n     data
/// ```
///
/// Version 1 has no gain: its rx block is 15 bytes (flag bit 5 reserved, no gain bytes), so
/// `data_len` is at 56 and `data` at 58. It decodes with `rx.gain = None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Csi<'a> {
    /// Increments for every capture the source filter accepts, including captures the pool then
    /// drops, so gaps between exported records are pool drops.
    pub capture_seq: u32,
    /// Node monotonic time when the CSI callback ran, µs since boot.
    pub node_time_us: u64,
    /// The radio's own receive timestamp, µs, wrapping at 2³².
    pub rx_timestamp_us: u32,
    /// 802.11 transmitter address of the measured frame.
    pub source_mac: [u8; 6],
    /// The probe carried by the measured frame, if it had one.
    pub probe: Option<Probe>,
    /// The first four bytes of `data` are invalid (ESP-IDF `first_word_invalid`).
    pub first_word_invalid: bool,
    pub rx: RxMeta,
    pub data: &'a [u8],
}

impl<'a> Csi<'a> {
    fn write(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        let data_len = u16::try_from(self.data.len())
            .ok()
            .filter(|&n| usize::from(n) <= MAX_CSI_LEN)
            .ok_or(EncodeError::CsiTooLong(self.data.len()))?;
        w.u32(self.capture_seq)?;
        w.u64(self.node_time_us)?;
        w.u32(self.rx_timestamp_us)?;
        w.bytes(&self.source_mac)?;
        w.u8(pack_flags([self.probe.is_some(), self.first_word_invalid]))?;
        self.probe.unwrap_or_default().write_fields(w)?;
        self.rx.write(w)?;
        w.u16(data_len)?;
        w.bytes(self.data)
    }

    fn read(r: &mut Reader<'a>, version: u8) -> Result<Self, DecodeError> {
        let capture_seq = r.u32()?;
        let node_time_us = r.u64()?;
        let rx_timestamp_us = r.u32()?;
        let source_mac = r.array()?;
        let [has_probe, first_word_invalid] = unpack_flags(r.u8()?, "csi flags")?;
        let probe = Probe::read_fields(r)?;
        let probe = match (has_probe, probe == Probe::default()) {
            (true, _) => Some(probe),
            (false, true) => None,
            (false, false) => return Err(DecodeError::InvalidField("absent probe")),
        };
        let rx = RxMeta::read(r, version)?;
        let data_len = r.u16()?;
        if usize::from(data_len) > MAX_CSI_LEN {
            return Err(DecodeError::CsiTooLong(data_len));
        }
        Ok(Self {
            capture_seq,
            node_time_us,
            rx_timestamp_us,
            source_mac,
            probe,
            first_word_invalid,
            rx,
            data: r.take(usize::from(data_len))?,
        })
    }
}

/// Node health counters, cumulative since the start of the epoch so a lost report costs
/// resolution, not counts.
///
/// ```text
/// offset  size  field
/// 0       8     node_time_us
/// 8       8     tx_requested
/// 16      8     tx_accepted
/// 24      8     csi_callbacks
/// 32      8     csi_filtered
/// 40      8     csi_captured
/// 48      8     pool_drops
/// 56      8     export_drops
/// 64      8     export_bytes
/// 72      2     pool_high_water
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Counters {
    pub node_time_us: u64,
    /// Probe sends the schedule called for.
    pub tx_requested: u64,
    /// Probe sends the radio driver accepted. Acceptance is not proof that anyone received it.
    pub tx_accepted: u64,
    /// Every CSI callback, before source filtering.
    pub csi_callbacks: u64,
    /// Callbacks rejected by the source filter.
    pub csi_filtered: u64,
    /// Callbacks accepted by the source filter; equals the next `capture_seq`.
    pub csi_captured: u64,
    /// Accepted captures discarded because the capture pool was full.
    pub pool_drops: u64,
    /// Encoded frames discarded because the export queue was full or the write failed.
    pub export_drops: u64,
    /// Bytes handed to the export transport.
    pub export_bytes: u64,
    /// Highest capture pool occupancy seen.
    pub pool_high_water: u16,
}

impl Counters {
    fn write(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        w.u64(self.node_time_us)?;
        w.u64(self.tx_requested)?;
        w.u64(self.tx_accepted)?;
        w.u64(self.csi_callbacks)?;
        w.u64(self.csi_filtered)?;
        w.u64(self.csi_captured)?;
        w.u64(self.pool_drops)?;
        w.u64(self.export_drops)?;
        w.u64(self.export_bytes)?;
        w.u16(self.pool_high_water)
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            node_time_us: r.u64()?,
            tx_requested: r.u64()?,
            tx_accepted: r.u64()?,
            csi_callbacks: r.u64()?,
            csi_filtered: r.u64()?,
            csi_captured: r.u64()?,
            pool_drops: r.u64()?,
            export_drops: r.u64()?,
            export_bytes: r.u64()?,
            pool_high_water: r.u16()?,
        })
    }
}

/// Host → node clock probe. `host_time` is opaque to the node and echoed unchanged.
///
/// ```text
/// offset  size  field
/// 0       4     probe_id
/// 4       8     host_time
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TimeProbe {
    pub probe_id: u32,
    pub host_time: u64,
}

impl TimeProbe {
    fn write(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        w.u32(self.probe_id)?;
        w.u64(self.host_time)
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            probe_id: r.u32()?,
            host_time: r.u64()?,
        })
    }
}

/// Node → host reply to a [`TimeProbe`], stamped with the node's monotonic clock (the clock of
/// [`Csi::node_time_us`]) on receipt and on send.
///
/// ```text
/// offset  size  field
/// 0       4     probe_id
/// 4       8     host_time
/// 12      8     node_rx_us
/// 20      8     node_tx_us
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TimeEcho {
    pub probe_id: u32,
    pub host_time: u64,
    pub node_rx_us: u64,
    pub node_tx_us: u64,
}

impl TimeEcho {
    fn write(&self, w: &mut Writer<'_>) -> Result<(), EncodeError> {
        w.u32(self.probe_id)?;
        w.u64(self.host_time)?;
        w.u64(self.node_rx_us)?;
        w.u64(self.node_tx_us)
    }

    fn read(r: &mut Reader<'_>) -> Result<Self, DecodeError> {
        Ok(Self {
            probe_id: r.u32()?,
            host_time: r.u64()?,
            node_rx_us: r.u64()?,
            node_tx_us: r.u64()?,
        })
    }
}

/// Encodes header and body into `out`; returns the message length.
pub fn encode(
    header: &Header,
    message: &Message<'_>,
    out: &mut [u8],
) -> Result<usize, EncodeError> {
    let mut w = Writer::new(out);
    w.u8(VERSION)?;
    w.u8(message.kind() as u8)?;
    w.bytes(&header.node.0)?;
    w.u32(header.epoch)?;
    w.u32(header.seq)?;
    match message {
        Message::Hello(m) => m.write(&mut w)?,
        Message::Csi(m) => m.write(&mut w)?,
        Message::Counters(m) => m.write(&mut w)?,
        Message::TimeEcho(m) => m.write(&mut w)?,
        Message::TimeProbe(m) => m.write(&mut w)?,
    }
    Ok(w.len())
}

/// Decodes exactly one message of any supported version; trailing bytes are an error.
pub fn decode(bytes: &[u8]) -> Result<(Header, Message<'_>), DecodeError> {
    let mut r = Reader::new(bytes);
    let version = r.u8()?;
    if !(MIN_VERSION..=VERSION).contains(&version) {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let kind = r.u8()?;
    let kind = Kind::from_u8(kind).ok_or(DecodeError::UnknownKind(kind))?;
    let header = Header {
        node: NodeId(r.array()?),
        epoch: r.u32()?,
        seq: r.u32()?,
    };
    let message = match kind {
        Kind::Hello => Message::Hello(Hello::read(&mut r)?),
        Kind::Csi => Message::Csi(Csi::read(&mut r, version)?),
        Kind::Counters => Message::Counters(Counters::read(&mut r)?),
        Kind::TimeEcho => Message::TimeEcho(TimeEcho::read(&mut r)?),
        Kind::TimeProbe => Message::TimeProbe(TimeProbe::read(&mut r)?),
    };
    r.finish()?;
    Ok((header, message))
}

#[cfg(test)]
mod tests {
    extern crate std;
    use std::format;

    use super::*;
    use crate::MAX_MESSAGE_LEN;

    const HEADER: Header = Header {
        node: NodeId([0x24, 0x0a, 0xc4, 0x00, 0x00, 0x01]),
        epoch: 0x1234_5678,
        seq: 0x0000_002a,
    };

    const DATA: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

    fn csi(data: &[u8]) -> Csi<'_> {
        Csi {
            capture_seq: 9,
            node_time_us: 10,
            rx_timestamp_us: 11,
            source_mac: [0x1a, 0, 0, 0, 0, 0],
            probe: None,
            first_word_invalid: true,
            rx: RxMeta::default(),
            data,
        }
    }

    fn encoded(message: &Message<'_>) -> ([u8; MAX_MESSAGE_LEN], usize) {
        let mut buf = [0; MAX_MESSAGE_LEN];
        let n = encode(&HEADER, message, &mut buf).unwrap();
        (buf, n)
    }

    #[test]
    fn layout_matches_documentation() {
        let probe = Message::TimeProbe(TimeProbe {
            probe_id: 0x0102_0304,
            host_time: 0x1112_1314_1516_1718,
        });
        let (buf, n) = encoded(&probe);
        assert_eq!(
            &buf[..n],
            &[
                VERSION, 0x80, // version, kind
                0x24, 0x0a, 0xc4, 0x00, 0x00, 0x01, // node
                0x78, 0x56, 0x34, 0x12, // epoch
                0x2a, 0x00, 0x00, 0x00, // seq
                0x04, 0x03, 0x02, 0x01, // probe_id
                0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, // host_time
            ]
        );
    }

    #[test]
    fn body_lengths_match_documentation() {
        let hello = Message::Hello(Hello {
            chip: Chip::Esp32,
            roles: Roles::default(),
            firmware_version: [0; 3],
            firmware_commit: [0; 8],
            channel: 11,
            secondary_channel: 2,
            csi_config: CsiConfig::default(),
            pool_capacity: 0,
        });
        let cases = [
            (hello, 19),
            (Message::Csi(csi(&DATA)), CSI_FIXED_LEN + DATA.len()),
            (Message::Counters(Counters::default()), 74),
            (Message::TimeEcho(TimeEcho::default()), 28),
            (Message::TimeProbe(TimeProbe::default()), 12),
        ];
        for (message, body_len) in cases {
            assert_eq!(
                encoded(&message).1,
                HEADER_LEN + body_len,
                "{:?}",
                message.kind()
            );
        }
    }

    #[test]
    fn every_prefix_is_truncated_and_extra_bytes_are_rejected() {
        let (buf, n) = encoded(&Message::Csi(csi(&DATA)));
        assert!(decode(&buf[..n]).is_ok());
        for len in 0..n {
            assert_eq!(
                decode(&buf[..len]),
                Err(DecodeError::Truncated),
                "prefix {len}"
            );
        }
        assert_eq!(decode(&buf[..n + 1]), Err(DecodeError::TrailingBytes));
    }

    #[test]
    fn rejects_unknown_version_and_kind() {
        let (mut buf, n) = encoded(&Message::TimeProbe(TimeProbe::default()));
        buf[1] = 0x81;
        assert_eq!(decode(&buf[..n]), Err(DecodeError::UnknownKind(0x81)));
        for version in [MIN_VERSION - 1, VERSION + 1] {
            buf[0] = version;
            assert_eq!(
                decode(&buf[..n]),
                Err(DecodeError::UnsupportedVersion(version))
            );
        }
    }

    #[test]
    fn rejects_oversized_csi() {
        let data = [0u8; MAX_CSI_LEN + 1];
        let mut buf = [0u8; MAX_MESSAGE_LEN + 1];
        assert_eq!(
            encode(&HEADER, &Message::Csi(csi(&data)), &mut buf),
            Err(EncodeError::CsiTooLong(MAX_CSI_LEN + 1))
        );

        let (mut buf, n) = encoded(&Message::Csi(csi(&DATA)));
        let len_at = HEADER_LEN + CSI_FIXED_LEN - 2;
        buf[len_at..len_at + 2].copy_from_slice(&(MAX_CSI_LEN as u16 + 1).to_le_bytes());
        assert_eq!(
            decode(&buf[..n]),
            Err(DecodeError::CsiTooLong(MAX_CSI_LEN as u16 + 1))
        );
    }

    #[test]
    fn rejects_reserved_bits_and_stray_probe_bytes() {
        let (buf, n) = encoded(&Message::Csi(csi(&DATA)));
        let flags_at = HEADER_LEN + 22;

        let mut bad = buf;
        bad[flags_at] |= 0x80;
        assert_eq!(
            decode(&bad[..n]),
            Err(DecodeError::InvalidField("csi flags"))
        );

        let mut bad = buf;
        bad[flags_at + 1] = 1;
        assert_eq!(
            decode(&bad[..n]),
            Err(DecodeError::InvalidField("absent probe"))
        );

        let mut bad = buf;
        bad[HEADER_LEN + 50] = 0x40;
        assert_eq!(
            decode(&bad[..n]),
            Err(DecodeError::InvalidField("rx flags"))
        );

        let mut bad = buf;
        bad[HEADER_LEN + 56] = 1;
        assert_eq!(
            decode(&bad[..n]),
            Err(DecodeError::InvalidField("absent gain"))
        );
    }

    /// The version 1 encoding of a gain-less message: version byte 1, no gain bytes.
    fn as_version_1(v2: &[u8]) -> std::vec::Vec<u8> {
        let mut v1 = v2.to_vec();
        v1[0] = 1;
        if v1[1] == Kind::Csi as u8 {
            v1.drain(HEADER_LEN + 56..HEADER_LEN + 58);
        }
        v1
    }

    #[test]
    fn decodes_version_1_without_gain() {
        let message = Message::Csi(csi(&DATA));
        let (buf, n) = encoded(&message);
        let v1 = as_version_1(&buf[..n]);
        assert_eq!(v1.len(), n - 2);
        assert_eq!(decode(&v1), Ok((HEADER, message)));

        let mut bad = v1.clone();
        bad[HEADER_LEN + 50] |= 0x20;
        assert_eq!(decode(&bad), Err(DecodeError::InvalidField("rx flags")));

        let probe = Message::TimeProbe(TimeProbe::default());
        let (buf, n) = encoded(&probe);
        assert_eq!(decode(&as_version_1(&buf[..n])), Ok((HEADER, probe)));
    }

    #[test]
    fn gain_roundtrips() {
        let mut record = csi(&DATA);
        record.rx.gain = Some(RxGain { agc: 63, fft: -12 });
        let message = Message::Csi(record);
        let (buf, n) = encoded(&message);
        assert_eq!(buf[HEADER_LEN + 50] & 0x20, 0x20);
        assert_eq!(&buf[HEADER_LEN + 56..HEADER_LEN + 58], &[63, (-12i8) as u8]);
        assert_eq!(decode(&buf[..n]), Ok((HEADER, message)));
    }

    #[test]
    fn rejects_unknown_chip() {
        let hello = Message::Hello(Hello {
            chip: Chip::Esp32S3,
            roles: Roles { tx: true, rx: true },
            firmware_version: [0, 1, 0],
            firmware_commit: [0; 8],
            channel: 6,
            secondary_channel: 0,
            csi_config: CsiConfig::default(),
            pool_capacity: 16,
        });
        let (mut buf, n) = encoded(&hello);
        buf[HEADER_LEN] = 9;
        assert_eq!(decode(&buf[..n]), Err(DecodeError::InvalidField("chip")));
    }

    #[test]
    fn node_id_formats_and_parses_as_mac() {
        assert_eq!(format!("{}", HEADER.node), "24:0a:c4:00:00:01");
        assert_eq!("24:0a:c4:00:00:01".parse(), Ok(HEADER.node));
        assert_eq!("24:0A:C4:00:00:01".parse(), Ok(HEADER.node));
        for bad in [
            "",
            "24:0a:c4:00:00",
            "24:0a:c4:00:00:01:02",
            "24:0a:c4:00:00:1",
            "24:0a:c4:00:00:zz",
        ] {
            assert_eq!(bad.parse::<NodeId>(), Err(ParseNodeIdError), "{bad}");
        }
    }
}
