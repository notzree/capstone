use csi_protocol::{
    Chip, Counters, Csi, CsiConfig, DecodeError, Header, Hello, Kind, MAX_CSI_LEN, MAX_FRAME_LEN,
    Message, NodeId, Probe, Roles, RxGain, RxMeta, TimeEcho, TimeProbe, cobs, crc, frame, message,
    stream,
};
use proptest::collection::vec;
use proptest::prelude::*;

/// Owns what a [`Message`] borrows.
#[derive(Debug, Clone)]
enum Spec {
    Hello(Hello),
    Csi(Csi<'static>, Vec<u8>),
    Counters(Counters),
    TimeEcho(TimeEcho),
    TimeProbe(TimeProbe),
}

impl Spec {
    fn message(&self) -> Message<'_> {
        match self {
            Self::Hello(m) => Message::Hello(*m),
            Self::Csi(m, data) => Message::Csi(Csi { data, ..*m }),
            Self::Counters(m) => Message::Counters(*m),
            Self::TimeEcho(m) => Message::TimeEcho(*m),
            Self::TimeProbe(m) => Message::TimeProbe(*m),
        }
    }
}

fn node_id() -> impl Strategy<Value = NodeId> {
    any::<[u8; 6]>().prop_map(NodeId)
}

fn header() -> impl Strategy<Value = Header> {
    (node_id(), any::<u32>(), any::<u32>()).prop_map(|(node, epoch, seq)| Header {
        node,
        epoch,
        seq,
    })
}

fn probe() -> impl Strategy<Value = Probe> {
    (node_id(), any::<u32>(), any::<u32>(), any::<u32>()).prop_map(|(tx, epoch, schedule, seq)| {
        Probe {
            tx,
            epoch,
            schedule,
            seq,
        }
    })
}

fn rx_meta() -> impl Strategy<Value = RxMeta> {
    (
        (any::<i8>(), any::<i8>(), any::<[u8; 7]>()),
        any::<[bool; 5]>(),
        (any::<[u8; 3]>(), any::<u16>()),
        proptest::option::of(any::<(u8, i8)>()),
    )
        .prop_map(
            |(
                (rssi, noise_floor, [rate, sig_mode, mcs, cwb, stbc, channel, secondary_channel]),
                [smoothing, not_sounding, aggregation, fec_coding, sgi],
                ([ampdu_cnt, ant, rx_state], sig_len),
                gain,
            )| RxMeta {
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
                gain: gain.map(|(agc, fft)| RxGain { agc, fft }),
            },
        )
}

/// Re-encodes a version 2 frame of a gain-less message as version 1: version byte 1 and, for
/// `Csi`, without the two gain bytes that follow `sig_len`.
fn as_version_1(frame: &[u8]) -> Vec<u8> {
    let mut v1 = frame[..frame.len() - 4].to_vec();
    v1[0] = 1;
    if v1[1] == Kind::Csi as u8 {
        let gain_at = message::HEADER_LEN + 56;
        v1.drain(gain_at..gain_at + 2);
    }
    let crc = crc::crc32(&v1);
    v1.extend_from_slice(&crc.to_le_bytes());
    v1
}

fn without_gain(spec: Spec) -> Spec {
    match spec {
        Spec::Csi(mut csi, data) => {
            csi.rx.gain = None;
            Spec::Csi(csi, data)
        }
        other => other,
    }
}

fn hello() -> impl Strategy<Value = Hello> {
    (
        prop_oneof![Just(Chip::Esp32), Just(Chip::Esp32S3)],
        any::<[bool; 2]>(),
        any::<([u8; 3], [u8; 8], u8, u8)>(),
        any::<[bool; 6]>(),
        any::<(u8, u16)>(),
    )
        .prop_map(
            |(
                chip,
                [tx, rx],
                (firmware_version, firmware_commit, channel, secondary_channel),
                [
                    lltf,
                    htltf,
                    stbc_htltf2,
                    ltf_merge,
                    channel_filter,
                    manu_scale,
                ],
                (shift, pool_capacity),
            )| Hello {
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
                    shift,
                },
                pool_capacity,
            },
        )
}

fn csi() -> impl Strategy<Value = Spec> {
    (
        any::<(u32, u64, u32, [u8; 6])>(),
        proptest::option::of(probe()),
        any::<bool>(),
        rx_meta(),
        vec(any::<u8>(), 0..=MAX_CSI_LEN),
    )
        .prop_map(
            |(
                (capture_seq, node_time_us, rx_timestamp_us, source_mac),
                probe,
                first_word_invalid,
                rx,
                data,
            )| {
                let csi = Csi {
                    capture_seq,
                    node_time_us,
                    rx_timestamp_us,
                    source_mac,
                    probe,
                    first_word_invalid,
                    rx,
                    data: &[],
                };
                Spec::Csi(csi, data)
            },
        )
}

fn spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        hello().prop_map(Spec::Hello),
        csi(),
        any::<([u64; 9], u16)>().prop_map(|(c, pool_high_water)| Spec::Counters(Counters {
            node_time_us: c[0],
            tx_requested: c[1],
            tx_accepted: c[2],
            csi_callbacks: c[3],
            csi_filtered: c[4],
            csi_captured: c[5],
            pool_drops: c[6],
            export_drops: c[7],
            export_bytes: c[8],
            pool_high_water,
        })),
        any::<(u32, u64, u64, u64)>().prop_map(|(probe_id, host_time, node_rx_us, node_tx_us)| {
            Spec::TimeEcho(TimeEcho {
                probe_id,
                host_time,
                node_rx_us,
                node_tx_us,
            })
        }),
        any::<(u32, u64)>().prop_map(|(probe_id, host_time)| Spec::TimeProbe(TimeProbe {
            probe_id,
            host_time
        })),
    ]
}

proptest! {
    #[test]
    fn version_1_frames_decode_without_gain(header in header(), spec in spec().prop_map(without_gain)) {
        let message = spec.message();
        let mut buf = [0; MAX_FRAME_LEN];
        let n = frame::encode(&header, &message, &mut buf).unwrap();
        let v1 = as_version_1(&buf[..n]);
        prop_assert_eq!(frame::decode(&v1), Ok((header, message)));
    }

    #[test]
    fn frame_roundtrip(header in header(), spec in spec()) {
        let message = spec.message();
        let mut buf = [0; MAX_FRAME_LEN];
        let n = frame::encode(&header, &message, &mut buf).unwrap();
        prop_assert_eq!(frame::decode(&buf[..n]), Ok((header, message)));
    }

    #[test]
    fn any_bit_flip_fails_the_crc(header in header(), spec in spec(), bit in any::<prop::sample::Index>()) {
        let mut buf = [0; MAX_FRAME_LEN];
        let n = frame::encode(&header, &spec.message(), &mut buf).unwrap();
        let bit = bit.index(n * 8);
        buf[bit / 8] ^= 1 << (bit % 8);
        prop_assert_eq!(frame::decode(&buf[..n]), Err(DecodeError::BadCrc));
    }

    #[test]
    fn stream_roundtrip_with_arbitrary_chunking(
        frames in vec((header(), spec()), 1..6),
        garbage in vec(1u8..=255, 0..64),
        chunk in 1usize..700,
    ) {
        let mut encoder = stream::Encoder::new();
        let mut bytes = garbage.clone();
        if !garbage.is_empty() {
            bytes.push(0);
        }
        for (header, spec) in &frames {
            bytes.extend_from_slice(encoder.encode(header, &spec.message()).unwrap());
        }

        let mut deframer = stream::Deframer::new();
        let mut decoded = Vec::new();
        for mut input in bytes.chunks(chunk) {
            while let Some(frame) = deframer.push(&mut input) {
                decoded.push(frame.and_then(frame::decode).map(|(h, m)| (h, format!("{m:?}"))));
            }
        }
        if !garbage.is_empty() {
            prop_assert!(decoded.remove(0).is_err());
        }
        let expected: Vec<_> = frames.iter().map(|(h, s)| Ok((*h, format!("{:?}", s.message())))).collect();
        prop_assert_eq!(decoded, expected);
    }

    #[test]
    fn cobs_roundtrip(input in vec(any::<u8>(), 0..2000)) {
        let mut out = vec![0; cobs::max_encoded_len(input.len())];
        let n = cobs::encode(&input, &mut out).unwrap();
        prop_assert!(!out[..n].contains(&0));
        prop_assert_eq!(cobs::decode_in_place(&mut out[..n]), Ok(input.len()));
        prop_assert_eq!(&out[..input.len()], &input[..]);
    }

    #[test]
    fn probe_roundtrip(probe in probe(), padding in vec(any::<u8>(), 0..32)) {
        let mut buf = vec![0; csi_protocol::probe::PROBE_LEN];
        probe.encode(&mut buf).unwrap();
        buf.extend(padding);
        prop_assert_eq!(Probe::decode(&buf), Ok(probe));
        prop_assert_eq!(Probe::find(&buf), Some(probe));
    }

    #[test]
    fn decoders_accept_arbitrary_bytes_without_panicking(bytes in vec(any::<u8>(), 0..2500)) {
        let _ = message::decode(&bytes);
        let _ = frame::decode(&bytes);
        let _ = Probe::decode(&bytes);
        let _ = Probe::find(&bytes);
        let _ = cobs::decode_in_place(&mut bytes.clone());
        let mut deframer = stream::Deframer::new();
        let mut input = bytes.as_slice();
        while let Some(frame) = deframer.push(&mut input) {
            if let Ok(frame) = frame {
                let _ = frame::decode(frame);
            }
        }
    }

    #[test]
    fn decoders_accept_mutated_valid_frames(
        header in header(),
        spec in spec(),
        edits in vec((any::<prop::sample::Index>(), any::<u8>()), 1..8),
    ) {
        let mut buf = [0; MAX_FRAME_LEN];
        let n = message::encode(&header, &spec.message(), &mut buf).unwrap();
        for (at, value) in edits {
            buf[at.index(n)] = value;
        }
        let _ = message::decode(&buf[..n]);
    }
}
