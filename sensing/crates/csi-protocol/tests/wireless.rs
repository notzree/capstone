use csi_protocol::{
    Header, Message, NodeId, Probe, TimeEcho, crc, frame,
    wireless::{self, Batch, Builder, Carrier, HEADER_LEN, MAX_LEN},
};
use proptest::prelude::*;

fn carrier() -> Carrier {
    Carrier {
        probe: Probe {
            tx: NodeId([2; 6]),
            epoch: 7,
            schedule: 1,
            seq: 99,
        },
        sensing: true,
        sent_node_us: 123,
        batch_seq: 4,
        hub_port: 9001,
        relay_drops: 3,
    }
}
fn echo(node: NodeId, seq: u32) -> Vec<u8> {
    let mut out = vec![0; csi_protocol::MAX_FRAME_LEN];
    let n = frame::encode(
        &Header {
            node,
            epoch: 7,
            seq,
        },
        &Message::TimeEcho(TimeEcho::default()),
        &mut out,
    )
    .unwrap();
    out.truncate(n);
    out
}

#[test]
fn batches_keep_original_frames_and_retagging_does_not_change_readings() {
    let mut out = [0; MAX_LEN];
    let c = carrier();
    let a = echo(c.probe.tx, 1);
    let b = echo(c.probe.tx, 2);
    let mut encoder = Builder::new(&mut out, c, &[1; 14]).unwrap();
    encoder.push(&a).unwrap();
    encoder.push(&b).unwrap();
    let len = encoder.finish();
    let decoded = Batch::decode(&out[..len]).unwrap();
    assert_eq!(decoded.carrier, c);
    assert_eq!(
        decoded.frames().collect::<Vec<_>>(),
        vec![a.as_slice(), b.as_slice()]
    );
    assert_eq!(wireless::sensing_probe(&out[..len]), Some(c.probe));
    let changed = Carrier {
        sensing: false,
        sent_node_us: 777,
        ..c
    };
    wireless::retag(&mut out[..len], changed, &[3; 14]).unwrap();
    assert!(wireless::sensing_probe(&out[..len]).is_none());
    assert_eq!(
        Batch::decode(&out[..len])
            .unwrap()
            .frames()
            .collect::<Vec<_>>(),
        vec![a.as_slice(), b.as_slice()]
    );
    assert_eq!(Batch::decode(&out[..len]).unwrap().carrier, changed);
    let mut legacy = [0; 64];
    c.probe.encode(&mut legacy).unwrap();
    assert_eq!(wireless::sensing_probe(&legacy), Some(c.probe));
    for cut in 0..len {
        assert!(Batch::decode(&out[..cut]).is_err());
    }
    for i in 0..len {
        let mut bad = out;
        bad[i] ^= 1;
        assert!(Batch::decode(&bad[..len]).is_err());
    }
}

#[test]
fn limits_and_inner_identity_are_checked_before_any_records_are_exposed() {
    let c = carrier();
    let a = echo(c.probe.tx, 1);
    let mut out = [0; MAX_LEN];
    let mut builder = Builder::new(&mut out, c, &[]).unwrap();
    assert!(builder.push(&echo(NodeId([3; 6]), 1)).is_err());
    for _ in 0..16 {
        builder.push(&a).unwrap();
    }
    assert!(builder.push(&a).is_err());
    let n = builder.finish();
    assert_eq!(Batch::decode(&out[..n]).unwrap().count, 16);
    out[HEADER_LEN + 2 + 20] ^= 1;
    let check = crc::crc32(&out[..n - 4]);
    out[n - 4..n].copy_from_slice(&check.to_le_bytes());
    assert!(
        Batch::decode(&out[..n]).is_err(),
        "valid carrier CRC cannot hide an invalid inner frame"
    );
    let mut tiny = [0; 32];
    assert!(Builder::new(&mut tiny, c, &[]).is_err());
}

proptest! {
    #[test]
    fn arbitrary_wireless_data_never_panics(bytes in proptest::collection::vec(any::<u8>(),0..1800)) {
        let _=Batch::decode(&bytes);let _=wireless::peek(&bytes);let _=wireless::sensing_probe(&bytes);
    }
}
