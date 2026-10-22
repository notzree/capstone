use csi_host::clock::ClockFit;
use csi_host::live::{
    self,
    config::{HubConfig, SourceConfig, Transport},
};
use csi_host::session::{SessionReader, WriterOptions};
use csi_host::{RecordKind, StreamError};
use csi_protocol::{Csi, Header, Message, NodeId, Probe, RxMeta, TimeEcho, frame};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::time::{sleep, sleep_until, timeout};

fn config(sources: Vec<SourceConfig>) -> HubConfig {
    HubConfig {
        sources,
        stale_ms: 150,
        reconnect_ms: 30,
        echo_interval_ms: 100,
        status_interval_ms: 100,
        clock_step_ms: 200,
        clock_window_ms: 2000,
        ..HubConfig::default()
    }
}
fn encode(header: Header, message: Message<'_>) -> Vec<u8> {
    let mut out = vec![0; csi_protocol::MAX_FRAME_LEN];
    let n = frame::encode(&header, &message, &mut out).unwrap();
    out.truncate(n);
    out
}
fn cobs(raw: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; csi_protocol::stream::MAX_ENCODED_LEN];
    let n = csi_protocol::cobs::encode(raw, &mut bytes).unwrap();
    bytes[n] = 0;
    bytes.truncate(n + 1);
    bytes
}
struct Node {
    index: u8,
    epoch: u32,
    seq: u32,
}
impl Node {
    fn header(&mut self) -> Header {
        let h = Header {
            node: NodeId([2, 0, 0, 0, 0, self.index]),
            epoch: self.epoch,
            seq: self.seq,
        };
        self.seq += 1;
        h
    }
    fn node_us(&self, t: u64) -> u64 {
        1_000_000
            + u64::from(self.index) * 100_000
            + (t as f64 * (1.0 + f64::from(self.index) * 10e-6)) as u64
    }
    fn csi(&mut self, seq: u32) -> Vec<u8> {
        let radio = self.node_us(u64::from(seq) * 1000) - 50_000;
        encode(
            self.header(),
            Message::Csi(Csi {
                capture_seq: seq,
                node_time_us: radio + 50_000,
                rx_timestamp_us: radio as u32,
                source_mac: [3; 6],
                probe: Some(Probe {
                    tx: NodeId([3; 6]),
                    epoch: 1,
                    schedule: 0,
                    seq,
                }),
                first_word_invalid: false,
                rx: RxMeta::default(),
                data: &[0x55; 384],
            }),
        )
    }
    fn echo(&mut self, request: &[u8], elapsed_us: u64) -> Option<Vec<u8>> {
        let (_, Message::TimeProbe(p)) = frame::decode(request).ok()? else {
            return None;
        };
        let t = self.node_us(elapsed_us);
        Some(encode(
            self.header(),
            Message::TimeEcho(TimeEcho {
                probe_id: p.probe_id,
                host_time: p.host_time,
                node_rx_us: t,
                node_tx_us: t,
            }),
        ))
    }
}
async fn wait_status(dir: &Path, predicate: impl Fn(&serde_json::Value) -> bool) {
    timeout(Duration::from_secs(6), async {
        loop {
            if let Ok(b) = std::fs::read(dir.join(live::STATUS_FILE))
                && let Ok(s) = serde_json::from_slice(&b)
                && predicate(&s)
            {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "status timed out: {}",
            std::fs::read_to_string(dir.join(live::STATUS_FILE)).unwrap_or_default()
        )
    });
}
async fn udp(index: usize) -> (UdpSocket, SourceConfig) {
    let s = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let config = SourceConfig {
        name: format!("rx-{index}"),
        transport: Transport::Udp {
            bind: "127.0.0.1:0".parse().unwrap(),
            peer: s.local_addr().unwrap(),
        },
    };
    (s, config)
}
fn verify(dir: &Path, expected: usize) -> usize {
    let mut reader = SessionReader::open(dir).unwrap();
    assert!(reader.manifest().complete);
    let id = reader.manifest().session_id;
    let mut csi = 0;
    let mut total = 0;
    for r in reader.by_ref() {
        let r = r.unwrap();
        total += 1;
        if matches!(frame::decode(&r.payload), Ok((_, Message::Csi(_)))) {
            csi += 1
        }
    }
    assert_eq!(csi, expected);
    assert!(
        reader
            .checks()
            .iter()
            .all(|c| matches!(c.integrity, csi_host::session::Integrity::Verified))
    );
    let fit = ClockFit::read(dir, id).unwrap().unwrap();
    assert_eq!(fit.latest().len(), 3);
    assert!(
        fit.latest()
            .values()
            .all(|m| m.residual_us.is_none_or(|r| r.p99 < 50.0))
    );
    let reference = fit.handovers.first().unwrap().reference.node.0[5];
    for m in fit.latest().values() {
        let receiver = m.receiver.node.0[5];
        let expected = ((1.0 + f64::from(reference) * 10e-6) / (1.0 + f64::from(receiver) * 10e-6)
            - 1.0)
            * 1e6;
        assert!((m.drift_ppm - expected).abs() < 1.0, "{m:?}");
    }
    total
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn udp_3000_csi_per_second_records_before_lossy_delivery_and_maps_host_time() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let mut sockets = Vec::new();
    let mut sources = Vec::new();
    for i in 0..3 {
        let (s, c) = udp(i).await;
        sockets.push(s);
        sources.push(c)
    }
    let (subscriber, _stalled, drops) = live::subscription(1).unwrap();
    let hub = live::start(
        config(sources),
        &dir,
        WriterOptions::default(),
        vec![subscriber],
    )
    .await
    .unwrap();
    let start = Instant::now() + Duration::from_millis(100);
    let mut tasks = Vec::new();
    for (i, socket) in sockets.into_iter().enumerate() {
        let dest = hub.addresses[i].unwrap();
        tasks.push(tokio::spawn(async move {
            let mut node=Node{index:i as u8,epoch:1,seq:0};let mut next=0;let mut buf=[0;1024];
            while next<3000 {
                tokio::select! {
                    _=sleep_until((start+Duration::from_micros(u64::from(next)*1000)).into())=>{
                        for _ in 0..10 {socket.send_to(&node.csi(next),dest).await.unwrap();next+=1;}
                    }
                    result=socket.recv_from(&mut buf)=>{
                        let(n,_)=result.unwrap();
                        if let Some(reply)=node.echo(&buf[..n],start.elapsed().as_micros() as u64) {socket.send_to(&reply,dest).await.unwrap();}
                    }
                }
            }
        }));
    }
    for task in tasks {
        task.await.unwrap()
    }
    wait_status(&dir, |s| {
        s["records"].as_u64().unwrap() >= 9000 && !s["host_mapping"].is_null()
    })
    .await;
    let status = timeout(Duration::from_secs(10), hub.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(status.records as usize, verify(&dir, 9000));
    assert_eq!(
        drops.load(std::sync::atomic::Ordering::Relaxed),
        status.records - 1
    );
    assert!(status.max_queue_depth <= status.queue_capacity);
    assert!(status.max_clock_samples <= status.clock_sample_capacity);
    assert_eq!(status.clock_capacity_rejects, 0);
    assert!(status.echoes_accepted > 3);
    let host_records = std::fs::read_to_string(dir.join("host-clock.jsonl")).unwrap();
    assert!(!host_records.is_empty());
    for line in host_records.lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        assert_eq!(record["schema"], "csi-host-clock/1");
        assert_eq!(record["session_id"], status.session_id.to_string());
        assert!(record["mapping"]["version"].as_u64().unwrap() > 0);
    }
    let host = status.host_mapping.unwrap();
    assert!(host.residual_max_us < 10_000.0, "{host:?}");
    println!(
        "UDP: 9000 CSI / 3 s, {} total, peak queue {}, samples {}, host residual {:.1} us",
        status.records, status.max_queue_depth, status.max_clock_samples, host.residual_max_us
    );
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn udp_stale_restart_foreign_bad_frames_and_unsolicited_echo() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let (socket, source) = udp(0).await;
    let hub = live::start(config(vec![source]), &dir, WriterOptions::default(), vec![])
        .await
        .unwrap();
    let dest = hub.addresses[0].unwrap();
    let mut node = Node {
        index: 0,
        epoch: 1,
        seq: 0,
    };
    for seq in 0..30 {
        socket.send_to(&node.csi(seq), dest).await.unwrap();
    }
    let bad = vec![0xAB; 30];
    socket.send_to(&bad, dest).await.unwrap();
    socket
        .send_to(&vec![0x55; csi_protocol::MAX_FRAME_LEN + 100], dest)
        .await
        .unwrap();
    let foreign = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    foreign.send_to(&node.csi(30), dest).await.unwrap();
    wait_status(&dir, |s| {
        s["sources"][0]["state"] == "stale"
            && s["health"]["nodes"][0]["epochs"][0]["links"][0]["stale"] == true
    })
    .await;
    node.epoch = 2;
    node.seq = 0;
    for seq in 30..60 {
        socket.send_to(&node.csi(seq), dest).await.unwrap();
    }
    socket
        .send_to(
            &encode(node.header(), Message::TimeEcho(TimeEcho::default())),
            dest,
        )
        .await
        .unwrap();
    wait_status(&dir, |s| s["sources"][0]["epoch"] == 2).await;
    let status = hub.shutdown().await.unwrap();
    let s = &status.sources[0];
    assert_eq!(
        (
            s.foreign_datagrams,
            s.decode_errors,
            s.framing_errors,
            s.unmatched_echoes
        ),
        (1, 1, 1, 1)
    );
    assert!(s.stale_transitions >= 1);
    assert_eq!(s.epoch_changes, 1);
    assert_eq!(status.echoes_accepted, 0);
    let records: Vec<_> = SessionReader::open(&dir)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert!(records.iter().any(|r| r.payload == bad));
    assert!(
        records
            .iter()
            .any(|r| r.kind == RecordKind::StreamError
                && r.payload == [StreamError::Oversized as u8])
    );
    let fit = ClockFit::read(&dir, status.session_id).unwrap().unwrap();
    assert!(fit.clocks.iter().any(|k| k.epoch == 1));
    assert!(fit.clocks.iter().any(|k| k.epoch == 2));
    let events = std::fs::read_to_string(dir.join("hub-events.jsonl")).unwrap();
    assert!(events.contains("source_stale") && events.contains("node_epoch"));
}
#[cfg(unix)]
mod serial {
    use super::*;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_serial::{SerialPort, SerialStream};
    fn pty() -> (SerialStream, PathBuf) {
        let (master, mut slave) = SerialStream::pair().unwrap();
        slave.set_exclusive(false).unwrap();
        let path = slave.name().unwrap().into();
        drop(slave);
        (master, path)
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn pty_3000_csi_per_second_with_echo_and_clock_fit() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("session");
        let mut sources = Vec::new();
        let mut masters = Vec::new();
        for i in 0..3 {
            let (master, path) = pty();
            masters.push(master);
            sources.push(SourceConfig {
                name: format!("serial-{i}"),
                transport: Transport::Serial { path, baud: 0 },
            })
        }
        let hub = live::start(config(sources), &dir, WriterOptions::default(), vec![])
            .await
            .unwrap();
        wait_status(&dir, |s| {
            s["sources"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["connects"] == 1)
        })
        .await;
        let start = Instant::now() + Duration::from_millis(50);
        let mut tasks = Vec::new();
        for (i, mut master) in masters.into_iter().enumerate() {
            tasks.push(tokio::spawn(async move {
                let mut node=Node{index:i as u8,epoch:1,seq:0};let mut next=0;let mut buf=[0;4096];let mut deframer=csi_protocol::stream::Deframer::new();
                while next<3000 {
                    tokio::select! {
                        _=sleep_until((start+Duration::from_micros(u64::from(next)*1000)).into())=>{
                            let mut bytes=Vec::new();for _ in 0..10 {bytes.extend(cobs(&node.csi(next)));next+=1;}master.write_all(&bytes).await.unwrap();
                        }
                        result=master.read(&mut buf)=>{
                            let n=result.unwrap();assert!(n>0);let mut input=&buf[..n];
                            while let Some(Ok(request))=deframer.push(&mut input) {
                                if let Some(reply)=node.echo(request,start.elapsed().as_micros() as u64) {master.write_all(&cobs(&reply)).await.unwrap();}
                            }
                        }
                    }
                }
                master
            }));
        }
        let mut keep_open = Vec::new();
        for task in tasks {
            keep_open.push(task.await.unwrap());
        }
        wait_status(&dir, |s| s["records"].as_u64().unwrap() >= 9000).await;
        let status = hub.shutdown().await.unwrap();
        assert_eq!(status.records as usize, verify(&dir, 9000));
        assert_eq!(status.clock_capacity_rejects, 0);
        assert!(status.echoes_accepted > 3);
        assert_eq!(
            status
                .sources
                .iter()
                .map(|s| s.framing_errors + s.decode_errors)
                .sum::<u64>(),
            0
        );
        println!(
            "PTY: 9000 CSI / 3 s, {} total, peak queue {}, samples {}",
            status.records, status.max_queue_depth, status.max_clock_samples
        );
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unplug_reconnect_resets_partial_frame_and_tracks_epoch() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("session");
        let alias = tmp.path().join("node-port");
        let (mut master, path) = pty();
        std::os::unix::fs::symlink(path, &alias).unwrap();
        let source = SourceConfig {
            name: "receiver".into(),
            transport: Transport::Serial {
                path: alias.clone(),
                baud: 0,
            },
        };
        let hub = live::start(config(vec![source]), &dir, WriterOptions::default(), vec![])
            .await
            .unwrap();
        wait_status(&dir, |s| s["sources"][0]["connects"] == 1).await;
        let mut node = Node {
            index: 0,
            epoch: 1,
            seq: 0,
        };
        master.write_all(&cobs(&node.csi(0))).await.unwrap();
        master.write_all(&[0x55, 0x55, 0x55]).await.unwrap();
        wait_status(&dir, |s| s["sources"][0]["valid_frames"] == 1).await;
        drop(master);
        wait_status(&dir, |s| s["sources"][0]["state"] == "disconnected").await;
        let (mut master, path) = pty();
        std::fs::remove_file(&alias).unwrap();
        std::os::unix::fs::symlink(path, &alias).unwrap();
        wait_status(&dir, |s| s["sources"][0]["connects"].as_u64().unwrap() >= 2).await;
        node.epoch = 2;
        node.seq = 0;
        master.write_all(&[2, 0]).await.unwrap();
        let frame = cobs(&node.csi(1));
        master.write_all(&frame[..7]).await.unwrap();
        sleep(Duration::from_millis(10)).await;
        master.write_all(&frame[7..]).await.unwrap();
        wait_status(&dir, |s| s["sources"][0]["valid_frames"] == 2).await;
        let status = hub.shutdown().await.unwrap();
        assert!(status.sources[0].disconnects >= 1);
        assert_eq!(status.sources[0].epoch_changes, 1);
        assert_eq!(status.sources[0].framing_errors, 1);
        let frames: Vec<_> = SessionReader::open(&dir)
            .unwrap()
            .map(Result::unwrap)
            .filter(|r| r.kind == RecordKind::Frame)
            .collect();
        assert_eq!(frames.len(), 2);
        assert_eq!(frame::decode(&frames[0].payload).unwrap().0.epoch, 1);
        assert_eq!(frame::decode(&frames[1].payload).unwrap().0.epoch, 2);
    }
}
#[test]
fn config_validates_toml_and_limits() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("hub.toml");
    std::fs::write(&path, "queue_capcity=3\n").unwrap();
    assert!(HubConfig::load(&path).is_err());
    let mut c = config(vec![SourceConfig {
        name: "a".into(),
        transport: Transport::Udp {
            bind: "127.0.0.1:0".parse().unwrap(),
            peer: "127.0.0.1:1234".parse().unwrap(),
        },
    }]);
    std::fs::write(&path, toml::to_string(&c).unwrap()).unwrap();
    HubConfig::load(&path).unwrap();
    c.sources.push(c.sources[0].clone());
    assert!(c.validate().is_err());
    c.sources.pop();
    c.queue_capacity = 0;
    assert!(c.validate().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn recorder_failure_is_propagated_and_never_seals_session() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let (socket, source) = udp(0).await;
    let hub = live::start(config(vec![source]), &dir, WriterOptions::default(), vec![])
        .await
        .unwrap();
    wait_status(&dir, |s| s["sources"][0]["connects"] == 1).await;
    std::fs::create_dir(dir.join("segment-000000.bin")).unwrap();
    let mut node = Node {
        index: 0,
        epoch: 1,
        seq: 0,
    };
    socket
        .send_to(&node.csi(0), hub.addresses[0].unwrap())
        .await
        .unwrap();
    let error = timeout(Duration::from_secs(3), hub.wait())
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.to_string().contains("segment-000000.bin"), "{error}");
    assert!(!csi_host::manifest::Manifest::load(&dir).unwrap().complete);
}

#[tokio::test]
async fn bind_failure_creates_no_session() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let (socket, mut source) = udp(0).await;
    let address = socket.local_addr().unwrap();
    source.transport = Transport::Udp {
        bind: address,
        peer: address,
    };
    assert!(
        live::start(config(vec![source]), &dir, WriterOptions::default(), vec![])
            .await
            .is_err()
    );
    assert!(!dir.exists());
}
