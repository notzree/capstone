//! `fixtures/m2-session` was recorded by the M2 (protocol v1) code before protocol v2 existed.

use std::fs;
use std::path::Path;

use csi_host::inspect::{EpochReport, HealthEvent, InspectOptions, Inspector};
use csi_host::manifest::DerivedFrom;
use csi_host::session::{Integrity, SessionReader, SessionWriter, WriterOptions};
use csi_host::synthetic::{SyntheticConfig, Truth};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/m2-session");

#[test]
fn m2_session_inspects_with_the_counts_it_was_recorded_with() {
    let dir = Path::new(FIXTURE);
    let mut reader = SessionReader::open(dir).unwrap();
    assert_eq!(reader.manifest().protocol_version, 1);
    let sources = reader.manifest().sources.clone();
    let mut inspector = Inspector::new(InspectOptions {
        max_events: usize::MAX,
        ..InspectOptions::default()
    });
    for record in reader.by_ref() {
        inspector.push(&record.unwrap());
    }
    assert!(
        reader
            .checks()
            .iter()
            .all(|c| c.integrity == Integrity::Verified)
    );
    let report = inspector.finish();

    let truth: Vec<Truth> =
        serde_json::from_slice(&fs::read(dir.join("synthetic-truth.json")).unwrap()).unwrap();
    let config: SyntheticConfig =
        serde_json::from_value(sources[0].details["config"].clone()).unwrap();
    assert_eq!(truth.len(), sources.len());
    for ((t, info), source) in truth.iter().zip(&sources).zip(&report.sources) {
        let node = info.details["node"].as_str().unwrap();
        assert_eq!(source.versions.keys().copied().collect::<Vec<_>>(), [1]);
        assert_eq!(
            source.decode_errors.values().sum::<u64>(),
            t.corrupted + t.truncated
        );
        let epochs = &report.nodes.iter().find(|n| n.node == node).unwrap().epochs;
        let sum = |f: &dyn Fn(&EpochReport) -> u64| epochs.iter().map(f).sum::<u64>();
        assert_eq!(epochs.len() as u64, t.reboots + 1);
        assert_eq!(sum(&|e| e.frames_missing), t.frames_missing);
        assert_eq!(sum(&|e| e.frames_duplicate), t.duplicated);
        assert_eq!(sum(&|e| e.frames_late), t.reordered);
        assert_eq!(sum(&|e| e.captures_missing), t.captures_missing);
        assert_eq!(
            sum(&|e| e.links.iter().map(|l| l.probes_missing).sum()),
            t.probes_missing
        );
        assert_eq!(
            sum(&|e| e.counters.map_or(0, |c| c.pool_drops)),
            t.reported_pool_drops
        );
        assert_eq!(sum(&|e| e.links.iter().map(|l| l.gain_valid).sum()), 0);
        let silences = report
            .events
            .iter()
            .filter(|e| matches!(e, HealthEvent::Silence { node: n, .. } if n == node))
            .count() as u64;
        assert_eq!(
            silences,
            u64::from(config.faults.outages + config.faults.reboots)
        );
    }
}

#[test]
fn m2_session_rerecords_byte_identically() {
    let reader = SessionReader::open(Path::new(FIXTURE)).unwrap();
    let original = reader.manifest().clone();
    let tmp = tempfile::tempdir().unwrap();
    let copy_dir = tmp.path().join("copy");
    let options = WriterOptions {
        rotation: original.rotation,
        derived_from: Some(DerivedFrom {
            session_id: original.session_id,
            how: "replay".into(),
        }),
        ..WriterOptions::default()
    };
    let mut writer = SessionWriter::create(&copy_dir, original.sources.clone(), options).unwrap();
    for record in reader {
        writer.write(&record.unwrap()).unwrap();
    }
    let copy = writer.finish().unwrap();
    assert_eq!(copy.segments, original.segments);
    for segment in &original.segments {
        assert_eq!(
            fs::read(Path::new(FIXTURE).join(&segment.file)).unwrap(),
            fs::read(copy_dir.join(&segment.file)).unwrap()
        );
    }
}
