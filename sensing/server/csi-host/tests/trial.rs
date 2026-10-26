use csi_host::geometry::Geometry;
use csi_host::live::config::{HubConfig, SourceConfig, Transport};
use csi_host::trial::{Labels, Mode, Node, Plan, Position, Stamp};
use std::fs;

fn plan() -> Plan {
    Plan {
        schema: "csi-trial-plan/1".into(),
        trial_id: "walk-01".into(),
        day_id: "day-a".into(),
        participant_id: Some("p01".into()),
        split_group: "p01-day-a".into(),
        room: "lab".into(),
        geometry_revision: "r1".into(),
        label_uncertainty_ms: 10,
        activities: vec!["empty".into(), "walking".into()],
        positions: vec![Position {
            id: "centre".into(),
            x: 1.0,
            y: 2.0,
            z: 0.0,
            azimuth_deg: Some(90.0),
        }],
        nodes: vec![Node {
            id: "02:00:00:00:00:01".into(),
            source: "a".into(),
            mode: Mode::Independent,
            requested_tx_hz: 50.0,
            channel: 6,
            image: "a.bin".into(),
            build_manifest: "a.json".into(),
        }],
        notes: String::new(),
    }
}

#[test]
fn validates_survey_source_identity_and_declared_rates() {
    let config = HubConfig {
        sources: vec![SourceConfig {
            name: "a".into(),
            transport: Transport::Udp {
                bind: "127.0.0.1:0".parse().unwrap(),
                peer: "127.0.0.1:9".parse().unwrap(),
            },
        }],
        ..Default::default()
    };
    let geometry = Geometry::parse(br#"{"schema":"csi-geometry/1","room":"lab","revision":"r1","units":"m","origin":"test","nodes":[{"id":"02:00:00:00:00:01","x":0,"y":0,"z":0,"tier":"floor"}]}"#).unwrap();
    let p = plan();
    p.validate(&config, &geometry).unwrap();
    for bad in [
        Plan {
            geometry_revision: "old".into(),
            ..p.clone()
        },
        Plan {
            label_uncertainty_ms: 0,
            ..p.clone()
        },
        Plan {
            activities: vec!["walking around".into()],
            ..p.clone()
        },
        Plan {
            nodes: vec![Node {
                source: "unknown".into(),
                ..p.nodes[0].clone()
            }],
            ..p.clone()
        },
        Plan {
            nodes: vec![Node {
                id: "02:00:00:00:00:02".into(),
                ..p.nodes[0].clone()
            }],
            ..p.clone()
        },
        Plan {
            nodes: vec![Node {
                mode: Mode::Rx,
                ..p.nodes[0].clone()
            }],
            ..p.clone()
        },
    ] {
        assert!(bad.validate(&config, &geometry).is_err());
    }
}

#[test]
fn labels_switch_close_truncated_and_reject_invalid_without_fabricating_events() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("labels.jsonl");
    let p = plan();
    let mut labels = Labels::create(&path, uuid::Uuid::now_v7(), &p).unwrap();
    let t = |mono_ns| Stamp {
        mono_ns,
        wall_ns: 99,
    }; // Wall time need not advance monotonically.
    labels.begin("empty", None, t(10)).unwrap();
    labels.begin("walking", Some("centre"), t(20)).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(labels.begin("fall", None, t(25)).is_err());
    assert!(labels.begin("walking", Some("unknown"), t(25)).is_err());
    assert!(labels.end(t(19), "operator", false).is_err());
    assert_eq!(fs::read(&path).unwrap(), before);
    labels.note("changed direction", t(30)).unwrap();
    labels.finish(t(40), "deadline").unwrap();
    assert!(labels.finish(t(41), "deadline").is_err());
    assert!(labels.note("after stop", t(42)).is_err());
    assert!(Labels::create(&path, uuid::Uuid::now_v7(), &p).is_err());
    let rows: Vec<serde_json::Value> = fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(rows.len(), 7);
    assert_eq!(rows[2]["type"], "end");
    assert_eq!(rows[2]["reason"], "switch");
    assert_eq!(rows[2]["truncated"], false);
    assert_eq!(rows[3]["position"]["azimuth_deg"], 90.0);
    assert_eq!(rows[5]["truncated"], true);
    assert_eq!(rows[6]["reason"], "deadline");
}

#[test]
fn star_plan_separates_one_physical_rx_source_from_remote_radio_participants() {
    let config = HubConfig {
        sources: vec![SourceConfig {
            name: "star-rx".into(),
            transport: Transport::Serial {
                path: "/dev/fake-star".into(),
                baud: 921600,
            },
        }],
        ..Default::default()
    };
    let geometry=Geometry::parse(br#"{"schema":"csi-geometry/1","room":"lab","revision":"r1","units":"m","origin":"test only","nodes":[{"id":"02:00:00:00:00:01","x":0,"y":0,"z":0,"tier":"floor"},{"id":"02:00:00:00:00:02","x":1,"y":0,"z":0,"tier":"floor"}]}"#).unwrap();
    let mut p = plan();
    p.schema = "csi-trial-plan/2".into();
    p.nodes[0].source = "star-rx".into();
    p.nodes[0].mode = Mode::Rx;
    p.nodes[0].requested_tx_hz = 0.0;
    p.nodes.push(Node {
        id: "02:00:00:00:00:02".into(),
        source: "star-rx".into(),
        mode: Mode::Tx,
        requested_tx_hz: 10.0,
        ..p.nodes[0].clone()
    });
    p.validate(&config, &geometry).unwrap();
    let mut old = p.clone();
    old.schema = "csi-trial-plan/1".into();
    assert!(old.validate(&config, &geometry).is_err());
    p.nodes[1].mode = Mode::Rx;
    p.nodes[1].requested_tx_hz = 0.0;
    assert!(p.validate(&config, &geometry).is_err());
}
