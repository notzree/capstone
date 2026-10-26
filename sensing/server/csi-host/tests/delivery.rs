#![cfg(unix)]
use std::fs::{self, File};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, atomic::AtomicU64};
use std::thread;

use arrow_ipc::reader::StreamReader;
use csi_host::delivery::Publisher;
use csi_host::export::{self, Format};
use csi_host::session::{SessionWriter, WriterOptions};
use csi_host::synthetic::{Synthetic, SyntheticConfig};
use tokio::sync::mpsc;

#[test]
fn live_stream_matches_offline_native_values_and_same_python_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let raw = tmp.path().join("raw");
    let out = tmp.path().join("out");
    let socket = tmp.path().join("csi.sock");
    let config = SyntheticConfig {
        seconds: 1.0,
        receivers: 2,
        transmitters: 1,
        rate_hz: 100.0,
        ..Default::default()
    };
    let mut writer =
        SessionWriter::create(&raw, Synthetic::sources(&config), WriterOptions::default()).unwrap();
    let publisher = Publisher::bind(&socket).unwrap();
    assert!(
        Publisher::bind(&socket).is_err(),
        "an existing socket must not be replaced"
    );
    let (tx, rx) = mpsc::channel(8192);
    let worker = publisher
        .spawn(
            rx,
            Arc::new(AtomicU64::new(0)),
            writer.manifest().clone(),
            None,
            raw.clone(),
        )
        .unwrap();
    let stream = UnixStream::connect(&socket).unwrap();
    let reader = StreamReader::try_new(stream, None).unwrap(); // schema acknowledges acceptance
    let client = thread::spawn(move || reader.map(Result::unwrap).collect::<Vec<_>>());
    for record in Synthetic::new(&config).unwrap() {
        writer.write(&record).unwrap();
        tx.blocking_send(Arc::new(record)).unwrap();
    }
    writer.finish().unwrap();
    drop(tx);
    worker.join().unwrap().unwrap();
    let live = client.join().unwrap();
    export::session(&raw, &out, Format::Arrow).unwrap();
    let offline: Vec<_> = StreamReader::try_new(File::open(out.join("csi.arrow")).unwrap(), None)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    let values = |batches: &[arrow_array::RecordBatch]| {
        let mut rows = Vec::new();
        for batch in batches {
            assert_eq!(batch.schema().metadata()["csi.schema"], "csi-export/1");
            let indices = batch
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
            for i in 0..batch.num_rows() {
                rows.push((indices.value(i), raw.value(i).to_vec()));
            }
        }
        rows.sort();
        rows
    };
    assert_eq!(values(&live), values(&offline));
    assert_eq!(values(&live).len(), 198);
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(raw.join("arrow-status.json")).unwrap()).unwrap();
    assert_eq!(status["client_batch_drops"], 0);
    assert_eq!(status["client_write_failures"], 0);
    assert_eq!(status["complete"], true);
    assert!(!socket.exists());
}

#[test]
fn stalled_client_drops_batches_and_raw_session_still_seals() {
    let tmp = tempfile::tempdir().unwrap();
    let raw = tmp.path().join("raw");
    let socket = tmp.path().join("csi.sock");
    let config = SyntheticConfig {
        seconds: 1.0,
        receivers: 16,
        transmitters: 1,
        rate_hz: 500.0,
        ..Default::default()
    };
    let mut writer =
        SessionWriter::create(&raw, Synthetic::sources(&config), WriterOptions::default()).unwrap();
    let (tx, rx) = mpsc::channel(8192);
    let worker = Publisher::bind(&socket)
        .unwrap()
        .spawn(
            rx,
            Arc::new(AtomicU64::new(0)),
            writer.manifest().clone(),
            None,
            raw.clone(),
        )
        .unwrap();
    let stream = UnixStream::connect(&socket).unwrap();
    let _stalled = StreamReader::try_new(stream, None).unwrap(); // consume schema only
    let mut total = 0;
    for record in Synthetic::new(&config).unwrap() {
        writer.write(&record).unwrap();
        tx.blocking_send(Arc::new(record)).unwrap();
        total += 1;
    }
    let manifest = writer.finish().unwrap();
    assert!(manifest.complete);
    assert_eq!(
        manifest.segments.iter().map(|s| s.records).sum::<u64>(),
        total
    );
    drop(tx);
    worker.join().unwrap().unwrap();
    let status: serde_json::Value =
        serde_json::from_slice(&fs::read(raw.join("arrow-status.json")).unwrap()).unwrap();
    assert_eq!(status["summary"]["csi_rows"], 7984);
    assert!(
        status["client_batch_drops"].as_u64().unwrap() > 0,
        "{status}"
    );
    assert_eq!(status["input_record_drops"], 0);
    assert!(status["client_write_failures"].as_u64().unwrap() >= 1);
}
