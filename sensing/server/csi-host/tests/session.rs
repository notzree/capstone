use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use csi_host::manifest::{Manifest, Rotation, SourceInfo};
use csi_host::session::{Integrity, SegmentCheck, SessionReader, SessionWriter, WriterOptions};
use csi_host::{Error, Record, RecordKind, StreamError};

fn sources(n: usize) -> Vec<SourceInfo> {
    (0..n)
        .map(|i| SourceInfo {
            name: format!("test-{i}"),
            kind: "test".into(),
            details: serde_json::Value::Null,
        })
        .collect()
}

fn records(n: u64) -> Vec<Record> {
    (0..n)
        .map(|i| match i % 7 {
            6 => Record::stream_error(i * 1_000_000, 10 + i, 1, StreamError::BadCobs),
            _ => Record {
                mono_ns: i * 1_000_000,
                wall_ns: 1_800_000_000_000_000_000 + i,
                source: (i % 2) as u16,
                kind: RecordKind::Frame,
                payload: (0..(i % 300) as u8).collect(),
            },
        })
        .collect()
}

fn write(dir: &Path, records: &[Record], rotation: Rotation) -> SessionWriter {
    let options = WriterOptions {
        rotation,
        ..WriterOptions::default()
    };
    let mut writer = SessionWriter::create(dir, sources(2), options).unwrap();
    for record in records {
        writer.write(record).unwrap();
    }
    writer
}

fn read(dir: &Path) -> (Vec<Record>, Vec<SegmentCheck>) {
    let mut reader = SessionReader::open(dir).unwrap();
    let records = reader.by_ref().map(Result::unwrap).collect();
    (records, reader.checks().to_vec())
}

fn integrities(checks: &[SegmentCheck]) -> Vec<Integrity> {
    checks.iter().map(|c| c.integrity.clone()).collect()
}

#[test]
fn roundtrip_with_size_rotation() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let input = records(1000);
    let rotation = Rotation {
        max_bytes: 20_000,
        ..Rotation::default()
    };
    let manifest = write(&dir, &input, rotation).finish().unwrap();
    assert!(manifest.complete);
    assert!(manifest.segments.len() > 5);
    for (i, segment) in manifest.segments.iter().enumerate() {
        assert_eq!(segment.file, format!("segment-{i:06}.bin"));
        assert!(segment.bytes <= rotation.max_bytes);
        assert_eq!(
            fs::metadata(dir.join(&segment.file)).unwrap().len(),
            segment.bytes
        );
    }
    assert_eq!(Manifest::load(&dir).unwrap(), manifest);

    let (output, checks) = read(&dir);
    assert_eq!(output, input);
    assert!(
        checks
            .iter()
            .all(|c| c.integrity == Integrity::Verified && c.sealed)
    );
}

#[test]
fn duration_rotation_uses_record_time() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let rotation = Rotation {
        max_duration_ns: 100_000_000,
        ..Rotation::default()
    };
    let manifest = write(&dir, &records(1000), rotation).finish().unwrap();
    assert_eq!(manifest.segments.len(), 10);
    for segment in &manifest.segments {
        let span = segment.last_mono_ns.unwrap() - segment.first_mono_ns.unwrap();
        assert!(span < rotation.max_duration_ns);
    }
}

#[test]
fn unfinished_session_keeps_its_records_and_reports_a_torn_tail() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let input = records(500);
    let rotation = Rotation {
        max_bytes: 30_000,
        ..Rotation::default()
    };
    drop(write(&dir, &input, rotation));

    let manifest = Manifest::load(&dir).unwrap();
    assert!(!manifest.complete);
    let (output, checks) = read(&dir);
    assert_eq!(output, input);
    let last = checks.last().unwrap();
    assert!(!last.sealed);
    assert_eq!(last.integrity, Integrity::Unsealed);
    assert_eq!(checks.len(), manifest.segments.len() + 1);

    let mut file = OpenOptions::new()
        .append(true)
        .open(dir.join(&last.file))
        .unwrap();
    file.write_all(&[7; 10]).unwrap();
    let (output, checks) = read(&dir);
    assert_eq!(output, input);
    assert!(matches!(
        checks.last().unwrap().integrity,
        Integrity::Torn { bytes: 10, .. }
    ));
}

#[test]
fn tampering_and_missing_segments_are_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let rotation = Rotation {
        max_bytes: 20_000,
        ..Rotation::default()
    };
    let manifest = write(&dir, &records(300), rotation).finish().unwrap();
    let first = dir.join(&manifest.segments[0].file);
    let mut bytes = fs::read(&first).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    fs::write(&first, bytes).unwrap();
    fs::remove_file(dir.join(&manifest.segments[1].file)).unwrap();

    let (_, checks) = read(&dir);
    let found = integrities(&checks);
    assert_eq!(found[0], Integrity::Mismatch);
    assert_eq!(found[1], Integrity::Missing);
    assert!(found[2..].iter().all(|i| *i == Integrity::Verified));
}

#[test]
fn corrupt_record_header_stops_that_segment_only() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let rotation = Rotation {
        max_bytes: 20_000,
        ..Rotation::default()
    };
    let manifest = write(&dir, &records(300), rotation).finish().unwrap();
    let first = dir.join(&manifest.segments[0].file);
    let mut bytes = fs::read(&first).unwrap();
    bytes[8 + 23] = 1;
    fs::write(&first, bytes).unwrap();

    let (output, checks) = read(&dir);
    assert!(matches!(
        checks[0].integrity,
        Integrity::Corrupt { offset: 8, .. }
    ));
    assert_eq!(checks[0].records, 0);
    let later: u64 = manifest.segments[1..].iter().map(|s| s.records).sum();
    assert_eq!(output.len() as u64, later);
}

#[test]
fn unknown_record_kinds_pass_through() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("session");
    let record = Record {
        mono_ns: 1,
        wall_ns: 2,
        source: 0,
        kind: RecordKind::Unknown(9),
        payload: vec![1, 2, 3],
    };
    write(&dir, std::slice::from_ref(&record), Rotation::default())
        .finish()
        .unwrap();
    assert_eq!(read(&dir).0, [record]);
}

#[test]
fn refuses_existing_directories_and_unknown_sources() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(matches!(
        SessionWriter::create(tmp.path(), sources(1), WriterOptions::default()),
        Err(Error::Exists(_))
    ));
    let mut writer =
        SessionWriter::create(&tmp.path().join("s"), sources(1), WriterOptions::default()).unwrap();
    let mut record = records(1).remove(0);
    record.source = 1;
    assert!(matches!(writer.write(&record), Err(Error::Config(_))));
}
