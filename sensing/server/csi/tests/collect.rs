use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::UdpSocket;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

fn setup(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let plan = root.join("plan.json");
    let geometry = root.join("geometry.json");
    let config = root.join("hub.toml");
    fs::write(
        root.join("image.bin"),
        b"test artifact, not physical firmware",
    )
    .unwrap();
    fs::write(root.join("build.json"), b"{\"test\":true}").unwrap();
    fs::write(
        &config,
        "[[sources]]\nname='a'\nkind='udp'\nbind='127.0.0.1:0'\npeer='127.0.0.1:9'\n",
    )
    .unwrap();
    fs::write(&geometry, json!({"schema":"csi-geometry/1","room":"lab","revision":"r1","units":"m","origin":"test only",
        "nodes":[{"id":"02:00:00:00:00:01","x":0,"y":0,"z":0,"tier":"floor"}]}).to_string()).unwrap();
    fs::write(&plan, json!({"schema":"csi-trial-plan/1","trial_id":"test-01","day_id":"day-1","participant_id":null,
        "split_group":"day-1","room":"lab","geometry_revision":"r1","label_uncertainty_ms":1,
        "activities":["empty","walking"],"nodes":[{"id":"02:00:00:00:00:01","source":"a","mode":"rx",
            "requested_tx_hz":0,"channel":6,"image":"image.bin","build_manifest":"build.json"}]}).to_string()).unwrap();
    (plan, config, geometry)
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn collection_stops_with_stdin_open_and_preserves_provenance_and_unknown_quality() {
    let tmp = tempfile::tempdir().unwrap();
    let (plan, config, geometry) = setup(tmp.path());
    let out = tmp.path().join("trial");
    let mut child = Command::new(env!("CARGO_BIN_EXE_csi"))
        .arg("collect")
        .arg("--plan")
        .arg(&plan)
        .arg("--config")
        .arg(&config)
        .arg("--geometry")
        .arg(&geometry)
        .arg("--out")
        .arg(&out)
        .args(["--seconds", "0.25"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(b"begin walking\nnote test collection\n")
        .unwrap();
    stdin.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() > deadline {
            child.kill().unwrap();
            panic!("deadline shutdown blocked on stdin");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    drop(stdin);
    let metadata = read(&out.join("trial.json"));
    assert_eq!(metadata["complete"], true);
    assert_eq!(metadata["stop_reason"], "deadline");
    assert_eq!(read(&out.join("raw/manifest.json"))["complete"], true);
    assert_eq!(read(&out.join("export/export.json"))["complete"], true);
    assert_eq!(read(&out.join("split.json"))["assignment"], "unassigned");
    assert_eq!(
        metadata["session_id"],
        read(&out.join("quality.json"))["session_id"]
    );
    assert_eq!(
        read(&out.join("quality.json"))["source_identity_checks"][0]["matches"],
        false
    );
    assert_eq!(read(&out.join("quality.json"))["summary"]["csi_rows"], 0);
    assert_eq!(
        fs::read(out.join("provenance/node-0-image.bin")).unwrap(),
        fs::read(tmp.path().join("image.bin")).unwrap()
    );
    let labels: Vec<Value> = fs::read_to_string(out.join("labels.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(labels[3]["truncated"], true);
    assert_eq!(labels[4]["type"], "stop");
}

#[test]
fn invalid_or_existing_trial_does_not_replace_files_or_start_raw_capture() {
    let tmp = tempfile::tempdir().unwrap();
    let (plan, config, geometry) = setup(tmp.path());
    let out = tmp.path().join("trial");
    let mut bad = read(&plan);
    bad["geometry_revision"] = "wrong".into();
    fs::write(&plan, bad.to_string()).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_csi"))
        .arg("collect")
        .arg("--plan")
        .arg(&plan)
        .arg("--config")
        .arg(&config)
        .arg("--geometry")
        .arg(&geometry)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!out.exists());
    fs::create_dir(&out).unwrap();
    fs::write(out.join("keep"), b"original").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_csi"))
        .arg("collect")
        .arg("--plan")
        .arg(&plan)
        .arg("--config")
        .arg(&config)
        .arg("--geometry")
        .arg(&geometry)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(fs::read(out.join("keep")).unwrap(), b"original");
    assert!(!out.join("raw").exists());
}

#[test]
fn labelled_udp_csi_exports_with_final_gaps_validity_and_counter_window_quality() {
    use csi_protocol::{
        Chip, Counters, Csi, CsiConfig, Header, Hello, Message, NodeId, Probe, Roles, RxMeta, frame,
    };
    let tmp = tempfile::tempdir().unwrap();
    let (plan, config, geometry) = setup(tmp.path());
    let a = UdpSocket::bind("127.0.0.1:0").unwrap();
    let b = UdpSocket::bind("127.0.0.1:0").unwrap();
    fs::write(&config, format!("[[sources]]\nname='a'\nkind='udp'\nbind='127.0.0.1:0'\npeer='{}'\n[[sources]]\nname='b'\nkind='udp'\nbind='127.0.0.1:0'\npeer='{}'\n", a.local_addr().unwrap(), b.local_addr().unwrap())).unwrap();
    let mut p = read(&plan);
    p["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"02:00:00:00:00:02","source":"b","mode":"tx",
        "requested_tx_hz":50,"channel":6,"image":"image.bin","build_manifest":"build.json"}));
    fs::write(&plan, p.to_string()).unwrap();
    let mut g = read(&geometry);
    g["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":"02:00:00:00:00:02","x":1,"y":0,"z":0,"tier":"floor"}));
    fs::write(&geometry, g.to_string()).unwrap();
    let out = tmp.path().join("trial");
    let mut child = Command::new(env!("CARGO_BIN_EXE_csi"))
        .arg("collect")
        .arg("--plan")
        .arg(&plan)
        .arg("--config")
        .arg(&config)
        .arg("--geometry")
        .arg(&geometry)
        .arg("--out")
        .arg(&out)
        .args(["--seconds", "5"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut destinations = Vec::new();
    loop {
        let mut line = String::new();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "collector exited before prompt"
        );
        if let Some((_, addr)) = line.trim().split_once("UDP listening on ") {
            destinations.push(addr.to_string());
        }
        if line.starts_with("Commands:") {
            break;
        }
    }
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"begin walking\n").unwrap();
    stdin.flush().unwrap();
    loop {
        let mut line = String::new();
        assert!(stdout.read_line(&mut line).unwrap() > 0);
        if line.starts_with("Activity:") {
            break;
        }
    }
    let nodes = [NodeId([2, 0, 0, 0, 0, 1]), NodeId([2, 0, 0, 0, 0, 2])];
    let send = |socket: &UdpSocket, source: usize, seq, message| {
        let mut bytes = [0; csi_protocol::MAX_FRAME_LEN];
        let n = frame::encode(
            &Header {
                node: nodes[source],
                epoch: 1,
                seq,
            },
            &message,
            &mut bytes,
        )
        .unwrap();
        socket.send_to(&bytes[..n], &destinations[source]).unwrap();
    };
    let hello = Hello {
        chip: Chip::Esp32,
        roles: Roles {
            tx: false,
            rx: true,
        },
        firmware_version: [0, 2, 2],
        firmware_commit: [0; 8],
        channel: 6,
        secondary_channel: 0,
        pool_capacity: 32,
        csi_config: CsiConfig {
            lltf: true,
            htltf: true,
            ..Default::default()
        },
    };
    send(&a, 0, 0, Message::Hello(hello));
    send(
        &b,
        1,
        0,
        Message::Hello(Hello {
            roles: Roles {
                tx: true,
                rx: false,
            },
            ..hello
        }),
    );
    send(
        &a,
        0,
        1,
        Message::Counters(Counters {
            node_time_us: 900_000,
            pool_drops: 100,
            export_drops: 200,
            ..Default::default()
        }),
    );
    for seq in [0, 2, 3, 4] {
        send(
            &a,
            0,
            seq + 2,
            Message::Csi(Csi {
                capture_seq: seq,
                node_time_us: 1_000_000 + u64::from(seq) * 20_000,
                rx_timestamp_us: 900_000 + seq * 20_000,
                source_mac: nodes[1].0,
                probe: Some(Probe {
                    tx: nodes[1],
                    epoch: 1,
                    schedule: 0,
                    seq,
                }),
                first_word_invalid: true,
                rx: RxMeta {
                    sig_mode: 1,
                    channel: 6,
                    ..Default::default()
                },
                data: &[1; 256],
            }),
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    send(
        &a,
        0,
        7,
        Message::Counters(Counters {
            node_time_us: 2_000_000,
            pool_drops: 103,
            export_drops: 202,
            ..Default::default()
        }),
    );
    // Wait for the recorder's own status rather than guessing when UDP has drained.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let ready = fs::read(out.join("raw/hub-status.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .is_some_and(|s| s["records"].as_u64().is_some_and(|n| n >= 8));
        if ready {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "recorder did not receive test frames"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    stdin.write_all(b"end\nstop\n").unwrap();
    stdin.flush().unwrap();
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let q = read(&out.join("quality.json"));
    assert_eq!(q["missing_links"], json!([]));
    assert_eq!(q["summary"]["csi_rows"], 4);
    assert_eq!(q["summary"]["decoded_rows"], 4);
    let link = q["summary"]["links"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(link["stored_samples"], 512);
    assert_eq!(link["valid_samples"], 428);
    assert_eq!(link["gain_records"], 0);
    let counters = q["summary"]["counter_windows"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(counters["delta"]["pool_drops"], 3);
    assert_eq!(counters["delta"]["export_drops"], 2);
    assert_eq!(q["health"]["nodes"][0]["epochs"][0]["captures_missing"], 1);
    assert!(
        q["node_configuration_checks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["roles_and_channel_match"] == true)
    );
    let events: Vec<Value> = fs::read_to_string(out.join("labels.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(events[2]["truncated"], false);
}
