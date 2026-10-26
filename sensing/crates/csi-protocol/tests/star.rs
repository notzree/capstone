use csi_protocol::{DecodeError, NodeId, crc::crc32, star::*};
use proptest::prelude::*;
fn packet(body: Body) -> Packet {
    Packet {
        kit: 0x11223344,
        sender: Identity {
            mac: NodeId([2, 3, 4, 5, 6, 7]),
            epoch: 0xaabbccdd,
        },
        destination: Identity::BROADCAST,
        correlation: 7,
        body,
    }
}
#[test]
fn pinned_header_golden_and_strict_errors() {
    let p = packet(Body::Discover {
        nonce: 9,
        round: 0,
        channel: 6,
    });
    let mut buf = [0; MAX_LEN];
    let n = p.encode(&mut buf).unwrap();
    assert_eq!(n, 46);
    assert_eq!(&buf[..42],b"CSIs\x01\x01\x06\x00\x44\x33\x22\x11\x02\x03\x04\x05\x06\x07\xdd\xcc\xbb\xaa\xff\xff\xff\xff\xff\xff\0\0\0\0\x07\0\0\0\x09\0\0\0\0\x06");
    assert_eq!(Packet::decode(&buf[..n]), Ok(p));
    for len in 0..n {
        assert!(Packet::decode(&buf[..len]).is_err());
    }
    for at in 8..n {
        let mut bad = buf;
        bad[at] ^= 1;
        assert!(Packet::decode(&bad[..n]).is_err());
    }
    let mut bad = buf;
    bad[4] = 2;
    assert_eq!(
        Packet::decode(&bad[..n]),
        Err(DecodeError::UnsupportedVersion(2))
    );
    bad = buf;
    bad[5] = 0x81;
    assert_eq!(
        Packet::decode(&bad[..n]),
        Err(DecodeError::UnknownKind(0x81))
    );
    assert!(Packet::decode(&buf[..n + 1]).is_err());
    // Body length must match the selected kind, even with a valid CRC.
    bad = buf;
    bad[5] = Kind::GetStatus as u8;
    let c = crc32(&bad[..n - 4]);
    bad[n - 4..n].copy_from_slice(&c.to_le_bytes());
    assert!(Packet::decode(&bad[..n]).is_err());
}
#[test]
fn all_kinds_roundtrip_within_espnow_v1_and_stream_bound() {
    let info = NodeInfo::default();
    let s = Start {
        attachment: 1,
        run: 2,
        schedule: 3,
        rate: 10,
        duration_ms: 1000,
        lease_ms: 3000,
        count: 8,
        nodes: [Identity::BROADCAST; 8],
    };
    let bodies = [
        Body::Discover {
            nonce: 1,
            round: 2,
            channel: 6,
        },
        Body::DiscoverAck {
            nonce: 1,
            round: 2,
            info,
        },
        Body::Start(s),
        Body::CommandAck {
            run: 2,
            command: Kind::Start,
            result: ResultCode::Ok,
            state: State::Armed,
            last_probe: 4,
            attempts: 6,
        },
        Body::Grant {
            run: 2,
            token: 4,
            schedule: 3,
        },
        Body::Keepalive {
            run: 2,
            lease_ms: 3000,
        },
        Body::Stop {
            attachment: 1,
            run: 2,
        },
        Body::StatusRequest,
        Body::StatusReply(info),
        Body::Attach { token: 1 },
        Body::GetRoster,
        Body::GetStatus,
        Body::HostHeartbeat { token: 1 },
        Body::RxReady { token: 1, info },
        Body::NodeSeen(info),
        Body::RosterSnapshot {
            index: 0,
            total: 1,
            info,
        },
        Body::DiscoveryComplete {
            nonce: 1,
            count: 1,
            rejected: 0,
        },
        Body::CommandResult {
            command: Kind::Start,
            result: ResultCode::Ok,
            missing: 0,
        },
        Body::RunState {
            run: 2,
            schedule: 3,
            state: State::Measuring,
            selected: 1,
            acked: 1,
            reason: ResultCode::Ok,
        },
        Body::DrainComplete {
            run: 2,
            capture: 3,
            export: 4,
            captured: 5,
            exported: 6,
            dropped: 0,
            selected: 1,
            confirmed: 1,
        },
        Body::GrantOutcome {
            run: 2,
            token: 4,
            tx: Identity::default(),
            probe_seq: 8,
            capture_seq: 9,
            result: ResultCode::Ok,
        },
    ];
    for body in bodies {
        let p = packet(body);
        let mut raw = [0; MAX_LEN];
        let n = p.encode(&mut raw).unwrap();
        assert!(n <= 250);
        assert_eq!(Packet::decode(&raw[..n]), Ok(p));
        let mut framed = [0; 254];
        let n = p.encode_stream(&mut framed).unwrap();
        let mut d = csi_protocol::stream::Deframer::new();
        let mut input = &framed[..n];
        assert_eq!(Packet::decode(d.push(&mut input).unwrap().unwrap()), Ok(p));
    }
}
proptest! {
    #[test]
    fn malformed_input_never_panics(bytes in proptest::collection::vec(any::<u8>(),0..1200)){let _=Packet::decode(&bytes);let _=ProbeTag::find(&bytes);}
    #[test]
    fn random_identity_and_token_roundtrip(kit in any::<u32>(),epoch in any::<u32>(),run in any::<u64>(),token in any::<u32>()){
        let mut p=packet(Body::Grant{run,token,schedule:3});p.kit=kit;p.sender.epoch=epoch;let mut raw=[0;MAX_LEN];let n=p.encode(&mut raw).unwrap();prop_assert_eq!(Packet::decode(&raw[..n]),Ok(p));
    }
}
