//! Pins the exact bytes of each message kind.
//!
//! `golden/v1/` holds frames written by protocol version 1. They are never regenerated: decoding
//! them proves current decoders still read version 1. `golden/v2/` and `golden/probe.bin` pin the
//! current encoding. If one of those changes on purpose, bump the version, move the old files into
//! a frozen directory like `v1/`, and regenerate with `CSI_PROTOCOL_BLESS=1 cargo test -p
//! csi-protocol`.

use std::fs;
use std::path::PathBuf;

use csi_protocol::{
    Chip, Counters, Csi, CsiConfig, Header, Hello, MAX_FRAME_LEN, Message, NodeId, Probe, Roles,
    RxGain, RxMeta, TimeEcho, TimeProbe, VERSION, frame, probe, stream,
};

const HEADER: Header = Header {
    node: NodeId([0xa0, 0xb7, 0x65, 0xdd, 0xe9, 0x3c]),
    epoch: 0x5eed_c0de,
    seq: 1234,
};

const PROBE: Probe = Probe {
    tx: NodeId([0xa0, 0xb7, 0x65, 0xdc, 0x01, 0x68]),
    epoch: 0x0bad_f00d,
    schedule: 2,
    seq: 70559,
};

const RX: RxMeta = RxMeta {
    rssi: -71,
    noise_floor: -96,
    rate: 11,
    sig_mode: 1,
    mcs: 0,
    cwb: 1,
    stbc: 0,
    channel: 11,
    secondary_channel: 2,
    smoothing: true,
    not_sounding: true,
    aggregation: false,
    fec_coding: false,
    sgi: false,
    ampdu_cnt: 0,
    ant: 0,
    rx_state: 0,
    sig_len: 47,
    gain: None,
};

const GAIN: RxGain = RxGain { agc: 36, fft: -2 };

fn csi_data() -> Vec<u8> {
    (0..384u32).map(|i| (i * 37 % 251) as u8).collect()
}

fn hello() -> Message<'static> {
    Message::Hello(Hello {
        chip: Chip::Esp32,
        roles: Roles {
            tx: false,
            rx: true,
        },
        firmware_version: [0, 1, 0],
        firmware_commit: *b"\x8d\x3f\x01\x2a\xbe\xef\x00\x07",
        channel: 11,
        secondary_channel: 2,
        csi_config: CsiConfig {
            lltf: true,
            htltf: true,
            stbc_htltf2: true,
            ltf_merge: false,
            channel_filter: false,
            manu_scale: false,
            shift: 0,
        },
        pool_capacity: 32,
    })
}

fn csi_with_probe(data: &[u8], gain: Option<RxGain>) -> Message<'_> {
    Message::Csi(Csi {
        capture_seq: 12470,
        node_time_us: 1_095_148_477,
        rx_timestamp_us: 1_095_148_400,
        source_mac: [0x1a, 0, 0, 0, 0, 0],
        probe: Some(PROBE),
        first_word_invalid: true,
        rx: RxMeta { gain, ..RX },
        data,
    })
}

fn csi_unattributed() -> Message<'static> {
    Message::Csi(Csi {
        capture_seq: 1,
        node_time_us: 2,
        rx_timestamp_us: 3,
        source_mac: [0x02, 0x11, 0x22, 0x33, 0x44, 0x55],
        probe: None,
        first_word_invalid: false,
        rx: RxMeta {
            sig_mode: 0,
            cwb: 0,
            secondary_channel: 0,
            ..RX
        },
        data: &[0x10, 0xf0, 0x00, 0x7f],
    })
}

fn counters() -> Message<'static> {
    Message::Counters(Counters {
        node_time_us: 180_000_000,
        tx_requested: 18_000,
        tx_accepted: 17_990,
        csi_callbacks: 16_400,
        csi_filtered: 30,
        csi_captured: 16_370,
        pool_drops: 12,
        export_drops: 3,
        export_bytes: 7_595_000,
        pool_high_water: 31,
    })
}

fn time_probe() -> Message<'static> {
    Message::TimeProbe(TimeProbe {
        probe_id: 77,
        host_time: 1_789_681_068_137_487_200,
    })
}

fn time_echo() -> Message<'static> {
    Message::TimeEcho(TimeEcho {
        probe_id: 77,
        host_time: 1_789_681_068_137_487_200,
        node_rx_us: 1_095_150_000,
        node_tx_us: 1_095_150_042,
    })
}

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn read(name: &str) -> Vec<u8> {
    fs::read(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Compares `bytes` with the current-encoding fixture `name`, or writes it when blessing.
fn check(name: &str, bytes: &[u8]) {
    let path = path(name);
    if std::env::var_os("CSI_PROTOCOL_BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        return;
    }
    let expected = fs::read(&path).unwrap_or_else(|e| {
        panic!(
            "{}: {e}; run with CSI_PROTOCOL_BLESS=1 to create it",
            path.display()
        )
    });
    assert!(
        bytes == expected,
        "{name} changed; wire changes need a version bump and re-bless"
    );
}

fn check_frame(name: &str, message: Message<'_>) {
    let mut buf = [0; MAX_FRAME_LEN];
    let n = frame::encode(&HEADER, &message, &mut buf).unwrap();
    assert_eq!(buf[0], VERSION);
    check(name, &buf[..n]);
    assert_eq!(frame::decode(&buf[..n]), Ok((HEADER, message)));
}

fn deframe(bytes: &[u8]) -> Vec<u8> {
    let mut deframer = stream::Deframer::new();
    let mut input = bytes;
    let frame = deframer.push(&mut input).unwrap().unwrap().to_vec();
    assert!(input.is_empty());
    frame
}

#[test]
fn version_1_frames_still_decode() {
    let data = csi_data();
    let cases = [
        ("v1/hello.bin", hello()),
        ("v1/csi.bin", csi_with_probe(&data, None)),
        ("v1/csi-unattributed.bin", csi_unattributed()),
        ("v1/counters.bin", counters()),
        ("v1/time-probe.bin", time_probe()),
        ("v1/time-echo.bin", time_echo()),
    ];
    for (name, message) in cases {
        let bytes = read(name);
        assert_eq!(bytes[0], 1, "{name}");
        assert_eq!(frame::decode(&bytes), Ok((HEADER, message)), "{name}");
    }
    let frame = deframe(&read("v1/csi.stream.bin"));
    assert_eq!(
        frame::decode(&frame),
        Ok((HEADER, csi_with_probe(&data, None)))
    );
}

#[test]
fn hello_v2() {
    check_frame("v2/hello.bin", hello());
}

#[test]
fn csi_with_gain_v2() {
    let data = csi_data();
    check_frame("v2/csi.bin", csi_with_probe(&data, Some(GAIN)));
}

#[test]
fn csi_without_gain_v2() {
    check_frame("v2/csi-unattributed.bin", csi_unattributed());
}

#[test]
fn counters_v2() {
    check_frame("v2/counters.bin", counters());
}

#[test]
fn time_probe_v2() {
    check_frame("v2/time-probe.bin", time_probe());
}

#[test]
fn time_echo_v2() {
    check_frame("v2/time-echo.bin", time_echo());
}

#[test]
fn csi_stream_frame_v2() {
    let data = csi_data();
    let message = csi_with_probe(&data, Some(GAIN));
    let mut encoder = stream::Encoder::new();
    let encoded = encoder.encode(&HEADER, &message).unwrap();
    check("v2/csi.stream.bin", encoded);
    assert_eq!(frame::decode(&deframe(encoded)), Ok((HEADER, message)));
}

#[test]
fn over_air_probe() {
    let mut buf = [0; probe::PROBE_LEN];
    PROBE.encode(&mut buf).unwrap();
    check("probe.bin", &buf);
    assert_eq!(Probe::decode(&buf), Ok(PROBE));
}
