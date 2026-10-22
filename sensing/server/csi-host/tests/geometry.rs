use std::fs;
use std::path::Path;

use csi_host::Error;
use csi_host::geometry::{GEOMETRY_FILE, Geometry, Tier};
use csi_host::manifest::{Manifest, SourceInfo};
use csi_host::session::{SessionReader, SessionWriter, WriterOptions};
use sha2::{Digest, Sha256};

const VALID: &str = r#"{
  "schema": "csi-geometry/1",
  "room": "bedroom-a",
  "revision": "2026-10-20a",
  "units": "m",
  "origin": "south-west floor corner; x east, y north, z up",
  "nodes": [
    { "id": "02:52:58:00:00:00", "x": 0.0, "y": 0.4, "z": 0.3, "tier": "floor",
      "antenna": { "azimuth_deg": 0.0, "elevation_deg": 0.0 }, "notes": "behind bed" },
    { "id": "02:54:58:00:00:00", "x": 3.2, "y": 2.9, "z": 2.3, "tier": "upper" }
  ]
}
"#;

fn session_with(dir: &Path, geometry: &Path) -> Result<Manifest, Error> {
    let sources = vec![SourceInfo {
        name: "test".into(),
        kind: "test".into(),
        details: serde_json::Value::Null,
    }];
    let options = WriterOptions {
        geometry: Some(geometry.to_owned()),
        ..WriterOptions::default()
    };
    SessionWriter::create(dir, sources, options)?.finish()
}

#[test]
fn valid_geometry_parses() {
    let geometry = Geometry::parse(VALID.as_bytes()).unwrap();
    assert_eq!(geometry.nodes.len(), 2);
    assert_eq!(geometry.nodes[1].tier, Tier::Upper);
    assert!(geometry.nodes[1].antenna.is_none());
    let ids: Vec<String> = geometry
        .node_ids()
        .iter()
        .map(|id| id.to_string())
        .collect();
    assert_eq!(ids, ["02:52:58:00:00:00", "02:54:58:00:00:00"]);
}

#[test]
fn invalid_geometry_is_rejected() {
    let cases = [
        ("\"csi-geometry/1\"", "\"csi-geometry/2\""),
        ("\"units\": \"m\"", "\"units\": \"cm\""),
        ("\"room\": \"bedroom-a\"", "\"room\": \" \""),
        ("\"02:54:58:00:00:00\"", "\"02:52:58:00:00:00\""),
        ("\"02:54:58:00:00:00\"", "\"02-54-58-00-00-00\""),
        ("\"tier\": \"upper\"", "\"tier\": \"ceiling\""),
        ("\"x\": 3.2", "\"x\": 3.2, \"height\": 1"),
        ("\"z\": 2.3", "\"z\": 1e999"),
    ];
    for (from, to) in cases {
        let text = VALID.replacen(from, to, 1);
        assert_ne!(text, VALID, "{to}");
        assert!(
            Geometry::parse(text.as_bytes()).is_err(),
            "{to} was accepted"
        );
    }
    let empty = VALID.replace(
        &VALID[VALID.find("\"nodes\"").unwrap()..VALID.rfind('}').unwrap()],
        "\"nodes\": []\n",
    );
    assert!(Geometry::parse(empty.as_bytes()).is_err());
}

#[test]
fn session_copies_the_file_and_references_its_revision_and_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("survey.json");
    fs::write(&source, VALID).unwrap();
    let dir = tmp.path().join("session");
    let manifest = session_with(&dir, &source).unwrap();

    assert_eq!(fs::read(dir.join(GEOMETRY_FILE)).unwrap(), VALID.as_bytes());
    let reference = manifest.geometry.unwrap();
    assert_eq!(reference.file, GEOMETRY_FILE);
    assert_eq!(reference.room, "bedroom-a");
    assert_eq!(reference.revision, "2026-10-20a");
    let expected: String = Sha256::digest(VALID)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(reference.sha256, expected);

    let reader = SessionReader::open(&dir).unwrap();
    assert_eq!(
        reader.geometry().unwrap(),
        Some(Geometry::parse(VALID.as_bytes()).unwrap())
    );
}

#[test]
fn edited_geometry_no_longer_matches_the_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("survey.json");
    fs::write(&source, VALID).unwrap();
    let dir = tmp.path().join("session");
    session_with(&dir, &source).unwrap();
    fs::write(dir.join(GEOMETRY_FILE), VALID.replace("3.2", "3.3")).unwrap();
    let error = SessionReader::open(&dir).unwrap().geometry().unwrap_err();
    assert!(error.to_string().contains("hash"), "{error}");
}

#[test]
fn invalid_geometry_creates_no_session() {
    let tmp = tempfile::tempdir().unwrap();
    let source = tmp.path().join("survey.json");
    fs::write(&source, VALID.replace("\"m\"", "\"ft\"")).unwrap();
    let dir = tmp.path().join("session");
    assert!(matches!(
        session_with(&dir, &source),
        Err(Error::Format { .. })
    ));
    assert!(!dir.exists());
}

#[test]
fn sessions_without_geometry_report_none() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/m2-session");
    let reader = SessionReader::open(&fixture).unwrap();
    assert!(reader.manifest().geometry.is_none());
    assert_eq!(reader.geometry().unwrap(), None);
}
