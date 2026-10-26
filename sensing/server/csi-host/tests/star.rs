#![cfg(unix)]
use csi_host::{
    RecordKind,
    session::SessionReader,
    star::{Command, StarController, StartOptions},
};
use csi_node::star::{Action, Observation, Receiver, Transmitter};
use csi_protocol::{
    Csi, Header, Message, NodeId, RxMeta,
    star::{Body, Identity, Packet},
    stream,
};
use std::{
    fs,
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const RX: Identity = Identity {
    mac: NodeId([2; 6]),
    epoch: 1,
};
const TX: Identity = Identity {
    mac: NodeId([4; 6]),
    epoch: 2,
};
async fn simulator(mut serial: tokio::io::DuplexStream, starts: Arc<AtomicUsize>) {
    let mut r = Receiver::new(RX, 1, 6, [1; 8], 7, 0);
    let mut t = Transmitter::new(TX, 1, 6, [2; 8]);
    r.tick(0);
    while r.pop().is_some() {}
    r.air_input(
        Packet {
            kit: 1,
            sender: TX,
            destination: RX,
            correlation: 0,
            body: Body::DiscoverAck {
                nonce: 7,
                round: 0,
                info: t.info,
            },
        },
        TX.mac.0,
        9999,
    );
    r.tick(10000);
    while r.pop().is_some() {}
    let clock = std::time::Instant::now();
    let mut ticker = tokio::time::interval(Duration::from_millis(2));
    let mut d = stream::Deframer::new();
    let mut bytes = [0; 4096];
    let mut seq = 0;
    let mut capture = 0;
    loop {
        let now = 10000 + clock.elapsed().as_millis() as u64;
        tokio::select! {
            result=serial.read(&mut bytes)=>{let Ok(n)=result else{return};if n==0{return}let mut input=&bytes[..n];while let Some(Ok(raw))=d.push(&mut input){if let Ok(p)=Packet::decode(raw){if matches!(p.body,Body::Start(_)){starts.fetch_add(1,Ordering::Relaxed);}r.host(p,now);}}},
            _=ticker.tick()=>{}
        }
        // Periodic capability and live TX status snapshots.
        if seq % 25 == 0 {
            let hello = Message::Hello(csi_protocol::Hello {
                chip: csi_protocol::Chip::Esp32,
                roles: csi_protocol::Roles {
                    tx: false,
                    rx: true,
                },
                firmware_version: [0, 4, 0],
                firmware_commit: [1; 8],
                channel: 6,
                secondary_channel: 0,
                csi_config: csi_protocol::CsiConfig {
                    lltf: true,
                    htltf: true,
                    ..Default::default()
                },
                pool_capacity: 32,
            });
            let bytes = stream::Encoder::new()
                .encode(
                    &Header {
                        node: RX.mac,
                        epoch: RX.epoch,
                        seq: 0,
                    },
                    &hello,
                )
                .unwrap()
                .to_vec();
            if serial.write_all(&bytes).await.is_err() {
                return;
            }
            for body in [
                Body::RxReady {
                    token: 0,
                    info: r.info(),
                },
                Body::NodeSeen(t.info),
            ] {
                let p = Packet {
                    kit: 1,
                    sender: RX,
                    destination: Identity::BROADCAST,
                    correlation: 0,
                    body,
                };
                let mut b = [0; 254];
                let n = p.encode_stream(&mut b).unwrap();
                if serial.write_all(&b[..n]).await.is_err() {
                    return;
                }
            }
        }
        r.tick(now);
        while let Some(action) = r.pop() {
            match action {
                Action::Usb(p) => {
                    let mut b = [0; 254];
                    let n = p.encode_stream(&mut b).unwrap();
                    if serial.write_all(&b[..n]).await.is_err() {
                        return;
                    }
                }
                Action::Air(p) => {
                    if let Some((probe, tag)) = t.grant(p, RX.mac.0, now * 1000, now * 1000) {
                        if r.observe(probe, tag, TX.mac.0, capture, now) == Observation::Current {
                            let native = [0x42; 256];
                            let msg = Message::Csi(Csi {
                                capture_seq: capture,
                                node_time_us: now * 1000,
                                rx_timestamp_us: now as u32 * 1000,
                                source_mac: TX.mac.0,
                                probe: Some(probe),
                                first_word_invalid: true,
                                rx: RxMeta {
                                    sig_mode: 1,
                                    mcs: 0,
                                    channel: 6,
                                    ..Default::default()
                                },
                                data: &native,
                            });
                            let encoded = stream::Encoder::new()
                                .encode(
                                    &Header {
                                        node: RX.mac,
                                        epoch: RX.epoch,
                                        seq: capture,
                                    },
                                    &msg,
                                )
                                .unwrap()
                                .to_vec();
                            if serial.write_all(&encoded).await.is_err() {
                                return;
                            }
                            capture += 1;
                        }
                    } else if let Some(body) = t.command(p, RX.mac.0, now) {
                        r.air_input(
                            Packet {
                                kit: 1,
                                sender: TX,
                                destination: RX,
                                correlation: p.correlation,
                                body,
                            },
                            TX.mac.0,
                            now,
                        );
                    }
                }
            }
        }
        if r.needs_drain() {
            r.drain_complete(
                u64::from(capture),
                capture,
                u64::from(capture),
                u64::from(capture),
                0,
            );
        }
        seq += 1;
    }
}
async fn ready() -> (StarController, Arc<AtomicUsize>) {
    let (a, b) = tokio::io::duplex(65536);
    let starts = Arc::new(AtomicUsize::new(0));
    tokio::spawn(simulator(b, starts.clone()));
    let c = StarController::connect(a).await.unwrap();
    for _ in 0..100 {
        let s = c.request(Command::Status).await.unwrap();
        if s["attached"] == true && s["state"] == "Ready" && s["nodes"][0]["ready"] == true {
            return (c, starts);
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("simulator never ready")
}
async fn wait_finished(c: &StarController) -> serde_json::Value {
    for _ in 0..200 {
        let s = c.request(Command::Status).await.unwrap();
        if !s["last_run"].is_null() && s["run"].is_null() && s["finishing"] == false {
            return s["last_run"].clone();
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("run not finished")
}
#[tokio::test]
async fn no_sink_or_overwrite_never_arms_radio_and_file_has_control_watermark() {
    let (c, starts) = ready().await;
    assert!(c.request(Command::Start(Default::default())).await.is_err());
    assert_eq!(starts.load(Ordering::Relaxed), 0);
    let root = tempfile::tempdir().unwrap();
    let out = root.path().join("trial");
    fs::create_dir(&out).unwrap();
    assert!(
        c.request(Command::Start(StartOptions {
            out: Some(out.clone()),
            ..Default::default()
        }))
        .await
        .is_err()
    );
    assert_eq!(starts.load(Ordering::Relaxed), 0);
    let out = root.path().join("good");
    c.request(Command::Start(StartOptions {
        out: Some(out.clone()),
        duration: 1,
        ..Default::default()
    }))
    .await
    .unwrap();
    let final_status = wait_finished(&c).await;
    assert_eq!(final_status["stop_confirmed"], true);
    let trial: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("trial.json")).unwrap()).unwrap();
    assert_eq!(trial["complete"], true);
    assert!(trial["plan"].is_null());
    let mut reader = SessionReader::open(&out.join("raw")).unwrap();
    assert_eq!(reader.manifest().sources.len(), 1);
    let mut controls = 0;
    let mut csi = 0;
    for record in &mut reader {
        let record = record.unwrap();
        if record.kind == RecordKind::StarControl {
            controls += 1;
        }
        if record.kind == RecordKind::Frame {
            csi += 1;
        }
    }
    assert!(controls > 0 && csi >= 8);
    c.request(Command::Shutdown).await.unwrap();
}
#[tokio::test]
async fn stream_only_requires_schema_ack_creates_no_files_and_primary_disconnect_stops() {
    let (c, starts) = ready().await;
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("data.sock");
    let cloned = c.clone();
    let path = socket.clone();
    let start = tokio::spawn(async move {
        cloned
            .request(Command::Start(StartOptions {
                socket: Some(path),
                duration: 10,
                ..Default::default()
            }))
            .await
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(starts.load(Ordering::Relaxed), 0);
    let client = tokio::task::spawn_blocking(move || {
        let mut s = std::os::unix::net::UnixStream::connect(socket).unwrap();
        let mut reader =
            arrow_ipc::reader::StreamReader::try_new(s.try_clone().unwrap(), None).unwrap();
        assert_eq!(
            reader.schema().metadata()["csi.primary_handshake"],
            "CSI-READY/1"
        );
        s.write_all(b"CSI-READY/1\n").unwrap();
        let first = reader.next().unwrap().unwrap();
        assert!(first.num_rows() > 0);
    });
    start.await.unwrap().unwrap();
    client.await.unwrap();
    let final_status = wait_finished(&c).await;
    assert_eq!(final_status["stop_confirmed"], true);
    assert_eq!(starts.load(Ordering::Relaxed), 1);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    c.request(Command::Shutdown).await.unwrap();
}
#[tokio::test]
async fn file_and_stream_native_rows_match_and_subscriber_failure_preserves_recording() {
    let (c, _) = ready().await;
    let root = tempfile::tempdir().unwrap();
    let socket = root.path().join("data.sock");
    let out = root.path().join("trial");
    let cloned = c.clone();
    let s = socket.clone();
    let o = out.clone();
    let start = tokio::spawn(async move {
        cloned
            .request(Command::Start(StartOptions {
                out: Some(o),
                socket: Some(s),
                duration: 1,
                ..Default::default()
            }))
            .await
    });
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let client = tokio::task::spawn_blocking(move || {
        let mut s = std::os::unix::net::UnixStream::connect(socket).unwrap();
        let reader =
            arrow_ipc::reader::StreamReader::try_new(s.try_clone().unwrap(), None).unwrap();
        s.write_all(b"CSI-READY/1\n").unwrap();
        reader.map(|b| b.unwrap().num_rows()).sum::<usize>()
    });
    start.await.unwrap().unwrap();
    wait_finished(&c).await;
    let live = client.await.unwrap();
    let file = FileReader::rows(&out.join("export/csi.arrow"));
    assert!(live >= 8);
    assert_eq!(live, file);
    c.request(Command::Shutdown).await.unwrap();
}
struct FileReader;
impl FileReader {
    fn rows(path: &std::path::Path) -> usize {
        arrow_ipc::reader::StreamReader::try_new(fs::File::open(path).unwrap(), None)
            .unwrap()
            .map(|b| b.unwrap().num_rows())
            .sum()
    }
}

#[tokio::test]
async fn labelled_star_bundle_preserves_one_source_two_participants_and_validates_activities() {
    let (c, _) = ready().await;
    let root = tempfile::tempdir().unwrap();
    let plan = root.path().join("plan.json");
    let geometry = root.path().join("geometry.json");
    let out = root.path().join("labelled");
    fs::write(
        root.path().join("image.bin"),
        b"test artifact; not physical firmware",
    )
    .unwrap();
    fs::write(root.path().join("build.json"), b"{}").unwrap();
    let nodes = serde_json::json!([
        {"id":RX.mac.to_string(),"source":"star-rx","mode":"rx","requested_tx_hz":0,"channel":6,"image":"image.bin","build_manifest":"build.json"},
        {"id":TX.mac.to_string(),"source":"star-rx","mode":"tx","requested_tx_hz":10,"channel":6,"image":"image.bin","build_manifest":"build.json"}
    ]);
    fs::write(&plan,serde_json::json!({"schema":"csi-trial-plan/2","trial_id":"fixture","day_id":"day-fixture","participant_id":"fixture","split_group":"fixture-day","room":"fixture","geometry_revision":"r1","label_uncertainty_ms":1,"activities":["walking"],"nodes":nodes}).to_string()).unwrap();
    fs::write(&geometry,serde_json::json!({"schema":"csi-geometry/1","room":"fixture","revision":"r1","origin":"synthetic test fixture; not a real survey","units":"m","nodes":[{"id":RX.mac.to_string(),"x":0,"y":0,"z":0,"tier":"floor"},{"id":TX.mac.to_string(),"x":1,"y":0,"z":0,"tier":"floor"}]}).to_string()).unwrap();
    c.request(Command::Start(StartOptions {
        out: Some(out.clone()),
        plan: Some(plan),
        geometry: Some(geometry),
        duration: 10,
        ..Default::default()
    }))
    .await
    .unwrap();
    assert!(
        c.request(Command::Begin {
            activity: "undeclared".into(),
            position: None
        })
        .await
        .is_err()
    );
    c.request(Command::Begin {
        activity: "walking".into(),
        position: None,
    })
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    c.request(Command::End).await.unwrap();
    let status = c.request(Command::Stop).await.unwrap();
    assert_eq!(status["stop_confirmed"], true);
    let trial: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("trial.json")).unwrap()).unwrap();
    assert_eq!(trial["schema"], "csi-trial/2");
    assert_eq!(trial["complete"], true);
    let quality: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("quality.json")).unwrap()).unwrap();
    assert_eq!(
        quality["source_identity_checks"].as_array().unwrap().len(),
        1
    );
    assert_eq!(quality["expected_links"].as_array().unwrap().len(), 1);
    c.request(Command::Shutdown).await.unwrap();
}

#[tokio::test]
async fn repeated_start_stop_in_one_attachment_needs_no_rediscovery() {
    let (c, starts) = ready().await;
    let root = tempfile::tempdir().unwrap();
    for i in 0..3 {
        c.request(Command::Start(StartOptions {
            out: Some(root.path().join(format!("run-{i}"))),
            duration: 10,
            ..Default::default()
        }))
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let status = c.request(Command::Stop).await.unwrap();
        assert_eq!(status["stop_confirmed"], true);
    }
    assert_eq!(starts.load(Ordering::Relaxed), 3);
    c.request(Command::Shutdown).await.unwrap();
}
