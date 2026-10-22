use std::fs;
use std::path::Path;
use std::process::Command;

fn csi(args: &[&str]) -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_csi"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "csi {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn record_inspect_and_rerecord() {
    let tmp = tempfile::tempdir().unwrap();
    let original = tmp.path().join("original");
    let copy = tmp.path().join("copy");
    csi(&[
        "record",
        "--out",
        path(&original),
        "--segment-seconds",
        "1",
        "synthetic",
        "--seconds",
        "4",
        "--receivers",
        "2",
        "--rate",
        "200",
        "--faults",
        "typical",
        "--duplicate",
        "0.01",
    ]);

    let json: serde_json::Value = serde_json::from_str(&csi(&[
        "inspect",
        path(&original),
        "--json",
        "--events",
        "5",
    ]))
    .unwrap();
    assert_eq!(json["manifest"]["complete"], true);
    let segments = json["segments"].as_array().unwrap();
    assert!(segments.len() >= 4);
    assert!(
        segments
            .iter()
            .all(|s| s["integrity"]["status"] == "verified")
    );
    assert_eq!(json["report"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(json["report"]["events"].as_array().unwrap().len(), 5);

    let truth: serde_json::Value =
        serde_json::from_slice(&fs::read(original.join("synthetic-truth.json")).unwrap()).unwrap();
    let duplicated: u64 = truth
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["duplicated"].as_u64().unwrap())
        .sum();
    assert!(duplicated > 0);
    assert_eq!(
        json["report"]["event_counts"]["duplicate_frame"],
        duplicated
    );

    let text = csi(&["inspect", path(&original)]);
    assert!(text.contains("segments: ") && text.contains("verified"));
    assert!(text.contains("link from 02:54:58:00:00:00"));

    csi(&[
        "replay",
        path(&original),
        "--speed",
        "max",
        "--record",
        path(&copy),
    ]);
    for segment in segments {
        let file = segment["file"].as_str().unwrap();
        assert_eq!(
            fs::read(original.join(file)).unwrap(),
            fs::read(copy.join(file)).unwrap(),
            "{file}"
        );
    }
    let copy_json: serde_json::Value =
        serde_json::from_str(&csi(&["inspect", path(&copy), "--json"])).unwrap();
    assert_eq!(
        copy_json["manifest"]["derived_from"]["session_id"],
        json["manifest"]["session_id"]
    );

    // Reboots can leave a clock sharing no probes with the rest; those are reported unmapped.
    let receivers = json["report"]["clock"]["receivers"].as_array().unwrap();
    assert!(receivers.iter().filter(|r| !r["latest"].is_null()).count() >= 2);
    let mappings = |dir: &Path| -> Vec<String> {
        fs::read_to_string(dir.join("clock-fit.jsonl"))
            .unwrap()
            .lines()
            .skip(1)
            .map(str::to_owned)
            .collect()
    };
    assert!(!mappings(&original).is_empty());
    assert_eq!(
        mappings(&original),
        mappings(&copy),
        "replay re-derives the same clock fit"
    );
    assert!(csi(&["inspect", path(&original)]).contains("clock fit (window 60 s"));
}

#[test]
fn geometry_is_copied_checked_and_carried_through_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let survey = tmp.path().join("survey.json");
    fs::write(
        &survey,
        r#"{"schema": "csi-geometry/1", "room": "lab", "revision": "r1", "units": "m",
            "origin": "door corner", "nodes": [
              {"id": "02:52:58:00:00:00", "x": 0, "y": 0, "z": 0.3, "tier": "floor"},
              {"id": "02:99:00:00:00:01", "x": 2, "y": 0, "z": 2.2, "tier": "upper"}]}"#,
    )
    .unwrap();
    let original = tmp.path().join("original");
    csi(&[
        "record",
        "--out",
        path(&original),
        "--geometry",
        path(&survey),
        "synthetic",
        "--seconds",
        "1",
        "--receivers",
        "2",
    ]);
    let json: serde_json::Value =
        serde_json::from_str(&csi(&["inspect", path(&original), "--json"])).unwrap();
    let geometry = &json["geometry"];
    assert_eq!(geometry["status"], "ok");
    assert_eq!(geometry["revision"], "r1");
    assert_eq!(
        geometry["missing"],
        serde_json::json!(["02:52:58:00:00:01", "02:54:58:00:00:00"])
    );
    assert_eq!(
        geometry["unobserved"],
        serde_json::json!(["02:99:00:00:00:01"])
    );
    assert!(
        csi(&["inspect", path(&original)])
            .contains("WARNING: 2 nodes in the session are not in the geometry")
    );

    let copy = tmp.path().join("copy");
    csi(&[
        "replay",
        path(&original),
        "--speed",
        "max",
        "--record",
        path(&copy),
    ]);
    let copy_json: serde_json::Value =
        serde_json::from_str(&csi(&["inspect", path(&copy), "--json"])).unwrap();
    assert_eq!(
        copy_json["manifest"]["geometry"],
        json["manifest"]["geometry"]
    );
    assert_eq!(
        fs::read(copy.join("geometry.json")).unwrap(),
        fs::read(&survey).unwrap()
    );
}

#[test]
fn rejects_bad_arguments() {
    let tmp = tempfile::tempdir().unwrap();
    let status = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_csi"))
            .args(args)
            .output()
            .unwrap()
            .status
    };
    let out = tmp.path().join("s");
    assert!(!status(&["record", "--out", path(&out), "synthetic", "--rate", "0"]).success());
    assert!(!status(&["replay", path(tmp.path()), "--speed", "0"]).success());
    assert!(!status(&["inspect", path(tmp.path())]).success());
}

#[test]
fn rejects_unrepresentable_clock_intervals_without_replacing_the_fit() {
    let tmp = tempfile::tempdir().unwrap();
    let session = tmp.path().join("session");
    csi(&["record", "--out", path(&session), "synthetic"]);
    let fit = session.join("clock-fit.jsonl");
    let original = fs::read(&fit).unwrap();
    for (flag, value) in [
        ("--clock-step-s", "0.0000000001"),
        ("--clock-step-s", "inf"),
        ("--clock-window-s", "inf"),
        ("--clock-window-s", "NaN"),
        ("--clock-window-s", "1e100"),
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_csi"))
            .args(["fit-clocks", path(&session), "--force", flag, value])
            .output()
            .unwrap();
        assert!(!output.status.success(), "accepted {flag} {value}");
        assert_eq!(fs::read(&fit).unwrap(), original);
    }
}

#[test]
fn hub_duration_seals_session_and_archives_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("hub.toml");
    fs::write(&config, format!("reconnect_ms = 20\n[[sources]]\nname = 'unplugged'\nkind = 'serial'\npath = '{}'\nbaud = 921600\n", tmp.path().join("absent-port").display())).unwrap();
    let out = tmp.path().join("session");
    let text = csi(&[
        "hub",
        "--config",
        path(&config),
        "--out",
        path(&out),
        "--seconds",
        "0.1",
    ]);
    assert!(text.contains("session sealed"));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["complete"], true);
    assert!(out.join("hub-config.toml").exists());
    assert!(out.join("clock-fit.jsonl").exists());
    assert!(out.join("hub-events.jsonl").exists());
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("hub-status.json")).unwrap()).unwrap();
    assert_eq!(status["complete"], true);
    assert_eq!(status["sources"][0]["state"], "disconnected");
}

#[cfg(unix)]
#[test]
fn hub_sigterm_drains_and_seals() {
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("hub.toml");
    fs::write(
        &config,
        format!(
            "[[sources]]\nname = 'unplugged'\nkind = 'serial'\npath = '{}'\nbaud = 921600\n",
            tmp.path().join("absent-port").display()
        ),
    )
    .unwrap();
    let out = tmp.path().join("session");
    let mut child = Command::new(env!("CARGO_BIN_EXE_csi"))
        .args(["hub", "--config", path(&config), "--out", path(&out)])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    stdout.read_line(&mut line).unwrap();
    assert!(line.contains("hub recording"));
    assert!(
        Command::new("kill")
            .args(["-TERM", &child.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(out.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest["complete"], true);
}
