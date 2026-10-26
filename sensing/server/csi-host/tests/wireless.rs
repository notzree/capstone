use csi_host::{
    RecordKind, export,
    live::{
        self,
        config::{HubConfig, SourceConfig, Transport},
    },
    session::{SessionReader, WriterOptions},
};
use csi_protocol::{
    Chip, Csi, CsiConfig, Header, Hello, Message, NodeId, Probe, Roles, RxMeta, frame,
    wireless::{Builder, Carrier, MAX_LEN},
};
use std::fs;
use std::time::Duration;
use tokio::net::UdpSocket;

fn packet(node: NodeId, port: u16, sequence: u32) -> Vec<u8> {
    let mut out = [0; MAX_LEN];
    let c = Carrier {
        probe: Probe {
            tx: node,
            epoch: 7,
            schedule: 1,
            seq: sequence,
        },
        sensing: true,
        sent_node_us: 123,
        batch_seq: sequence,
        hub_port: port,
        relay_drops: 0,
    };
    let mut builder = Builder::new(&mut out, c, &[0; 14]).unwrap();
    let hello = Hello {
        chip: Chip::Esp32,
        roles: Roles { tx: true, rx: true },
        firmware_version: [0, 3, 0],
        firmware_commit: [0; 8],
        channel: 6,
        secondary_channel: 0,
        csi_config: CsiConfig {
            lltf: true,
            htltf: true,
            ..Default::default()
        },
        pool_capacity: 32,
    };
    let mut bytes = [0; csi_protocol::MAX_FRAME_LEN];
    let n = frame::encode(
        &Header {
            node,
            epoch: 7,
            seq: 0,
        },
        &Message::Hello(hello),
        &mut bytes,
    )
    .unwrap();
    builder.push(&bytes[..n]).unwrap();
    for seq in [0, 2, 3] {
        let csi = Csi {
            capture_seq: seq,
            node_time_us: 1_000_000 + u64::from(seq) * 20_000,
            rx_timestamp_us: 900_000 + seq * 20_000,
            source_mac: [3; 6],
            probe: Some(Probe {
                tx: NodeId([3; 6]),
                epoch: 8,
                schedule: 1,
                seq,
            }),
            first_word_invalid: true,
            rx: RxMeta {
                sig_mode: 1,
                ..Default::default()
            },
            data: &[0x55; 256],
        };
        let n = frame::encode(
            &Header {
                node,
                epoch: 7,
                seq: seq + 1,
            },
            &Message::Csi(csi),
            &mut bytes,
        )
        .unwrap();
        builder.push(&bytes[..n]).unwrap();
    }
    let len = builder.finish();
    out[..len].to_vec()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wireless_datagram_is_raw_preserved_expanded_and_echo_targets_expected_node() {
    let tmp = tempfile::tempdir().unwrap();
    let raw = tmp.path().join("raw");
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let node = NodeId([2; 6]);
    let config = HubConfig {
        sources: vec![SourceConfig {
            name: "wireless-a".into(),
            transport: Transport::Wireless {
                bind: "127.0.0.1:0".parse().unwrap(),
                node: node.to_string(),
            },
        }],
        echo_interval_ms: 50,
        status_interval_ms: 50,
        ..Default::default()
    };
    let (subscriber, input, drops) = live::subscription(32).unwrap();
    let hub = live::start(config, &raw, WriterOptions::default(), vec![subscriber])
        .await
        .unwrap();
    let socket = tmp.path().join("live.sock");
    let publisher = csi_host::delivery::Publisher::bind(&socket)
        .unwrap()
        .with_flush_interval_ms(5)
        .unwrap()
        .spawn(
            input,
            drops,
            csi_host::manifest::Manifest::load(&raw).unwrap(),
            None,
            raw.clone(),
        )
        .unwrap();
    let stream = std::os::unix::net::UnixStream::connect(&socket).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let live_reader = arrow_ipc::reader::StreamReader::try_new(stream, None).unwrap();
    let client = std::thread::spawn(move || live_reader.map(Result::unwrap).collect::<Vec<_>>());
    let dest = hub.addresses[0].unwrap();
    let bytes = packet(node, dest.port(), 0);
    peer.send_to(&packet(NodeId([9; 6]), dest.port(), 0), dest)
        .await
        .unwrap();
    peer.send_to(&bytes, dest).await.unwrap();
    let mut buf = [0; 1500];
    let (n, _) = tokio::time::timeout(Duration::from_secs(3), peer.recv_from(&mut buf))
        .await
        .unwrap()
        .unwrap();
    let (header, message) = frame::decode(&buf[..n]).unwrap();
    assert_eq!(header.node, node);
    let Message::TimeProbe(request) = message else {
        panic!("expected request");
    };
    let mut response = [0u8; MAX_LEN];
    let mut inner = [0u8; csi_protocol::MAX_FRAME_LEN];
    let n = frame::encode(
        &Header {
            node,
            epoch: 7,
            seq: 5,
        },
        &Message::TimeEcho(csi_protocol::TimeEcho {
            probe_id: request.probe_id,
            host_time: request.host_time,
            node_rx_us: 2_000_000,
            node_tx_us: 2_000_000,
        }),
        &mut inner,
    )
    .unwrap();
    let carrier = csi_protocol::wireless::Batch::decode(&bytes)
        .unwrap()
        .carrier;
    let mut builder = Builder::new(&mut response, carrier, &[]).unwrap();
    builder.push(&inner[..n]).unwrap();
    builder.push(&inner[..n]).unwrap();
    let n = builder.finish();
    peer.send_to(&response[..n], dest).await.unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let value: serde_json::Value =
                serde_json::from_slice(&fs::read(raw.join("hub-status.json")).unwrap()).unwrap();
            if value["records"].as_u64().unwrap() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let status = hub.shutdown().await.unwrap();
    publisher.join().unwrap().unwrap();
    let live_batches = client.join().unwrap();
    assert_eq!(live_batches.iter().map(|b| b.num_rows()).sum::<usize>(), 3);
    assert_eq!(
        status.echoes_accepted + status.echoes_unmapped + status.echoes_rejected,
        1,
        "one correlated request must not supply multiple clock samples from duplicate batch echoes"
    );
    assert_eq!(status.sources[0].unmatched_echoes, 1);
    assert_eq!(status.records, 2);
    assert_eq!(status.sources[0].foreign_datagrams, 1);
    let mut reader = SessionReader::open(&raw).unwrap();
    let records = reader.by_ref().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(records[0].kind, RecordKind::WirelessBatch);
    assert_eq!(records[0].payload, bytes);
    assert_eq!(records[0].observations().unwrap().count(), 4);
    assert_eq!(status.health.records, 2);
    assert_eq!(status.health.nodes[0].epochs[0].captures_missing, 1);
    export::session(&raw, &tmp.path().join("export"), export::Format::Arrow).unwrap();
    let report: serde_json::Value =
        serde_json::from_slice(&fs::read(tmp.path().join("export/export.json")).unwrap()).unwrap();
    assert_eq!(report["summary"]["records"], 2);
    assert_eq!(report["summary"]["csi_rows"], 3);
    assert_eq!(report["summary"]["decoded_rows"], 3);
    assert_eq!(status.health.sources[0].piggyback_batches, 2);
    let offline = arrow_ipc::reader::StreamReader::try_new(
        fs::File::open(tmp.path().join("export/csi.arrow")).unwrap(),
        None,
    )
    .unwrap()
    .map(Result::unwrap)
    .collect::<Vec<_>>();
    let values = |batches: &[arrow_array::RecordBatch]| {
        let mut values = Vec::new();
        for batch in batches {
            let index = batch
                .column_by_name("record_index")
                .unwrap()
                .as_any()
                .downcast_ref::<arrow_array::UInt64Array>()
                .unwrap();
            let raw = batch
                .column_by_name("native_csi")
                .unwrap()
                .as_any()
                .downcast_ref::<arrow_array::BinaryArray>()
                .unwrap();
            let host = batch
                .column_by_name("host_mono_ns")
                .unwrap()
                .as_any()
                .downcast_ref::<arrow_array::UInt64Array>()
                .unwrap();
            for i in 0..batch.num_rows() {
                values.push((index.value(i), host.value(i), raw.value(i).to_vec()));
            }
        }
        values.sort();
        values
    };
    assert_eq!(values(&live_batches), values(&offline));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn corrupt_batch_is_recorded_without_learning_a_reply_endpoint() {
    let tmp = tempfile::tempdir().unwrap();
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let node = NodeId([2; 6]);
    let config = HubConfig {
        sources: vec![SourceConfig {
            name: "wireless".into(),
            transport: Transport::Wireless {
                bind: "127.0.0.1:0".parse().unwrap(),
                node: node.to_string(),
            },
        }],
        echo_interval_ms: 20,
        ..Default::default()
    };
    let raw = tmp.path().join("raw");
    let hub = live::start(config, &raw, WriterOptions::default(), vec![])
        .await
        .unwrap();
    let dest = hub.addresses[0].unwrap();
    let mut bytes = packet(node, dest.port(), 0);
    *bytes.last_mut().unwrap() ^= 1;
    peer.send_to(&bytes, dest).await.unwrap();
    let mut input = [0u8; 1500];
    assert!(
        tokio::time::timeout(Duration::from_millis(150), peer.recv_from(&mut input))
            .await
            .is_err()
    );
    let status = hub.shutdown().await.unwrap();
    assert_eq!(status.records, 1);
    assert_eq!(status.sources[0].decode_errors, 1);
    assert_eq!(status.health.sources[0].decode_errors["bad_crc"], 1);
    assert_eq!(status.health.sources[0].wireless_batches, 0);
    let record = SessionReader::open(&raw).unwrap().next().unwrap().unwrap();
    assert_eq!(record.payload, bytes);
    assert!(record.observations().is_err());
}
