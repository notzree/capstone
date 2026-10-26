use std::fs::{self, File};
use std::path::Path;

use arrow_array::{Array, BinaryArray, ListArray, RecordBatch, StringArray, UInt64Array};
use arrow_ipc::reader::StreamReader;
use csi_host::export::{self, Format};
use csi_host::manifest::SourceInfo;
use csi_host::session::{SessionWriter, WriterOptions};
use csi_host::{Record, RecordKind};
use csi_protocol::{
    Chip, Csi, CsiConfig, Header, Hello, Message, NodeId, Probe, Roles, RxMeta, frame,
};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

fn record(seq: u32, time: u64, message: Message<'_>) -> Record {
    let mut payload = [0; 700];
    let len = frame::encode(
        &Header {
            node: NodeId([1; 6]),
            epoch: 7,
            seq,
        },
        &message,
        &mut payload,
    )
    .unwrap();
    Record {
        mono_ns: time,
        wall_ns: 1_700_000_000_000_000_000 + time,
        source: 0,
        kind: RecordKind::Frame,
        payload: payload[..len].to_vec(),
    }
}

fn fixture(dir: &Path) {
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
    let sources = vec![SourceInfo {
        name: "fixture".into(),
        kind: "test".into(),
        details: serde_json::Value::Null,
    }];
    let mut writer = SessionWriter::create(dir, sources, WriterOptions::default()).unwrap();
    writer.write(&record(0, 0, Message::Hello(hello))).unwrap();
    let bytes: Vec<u8> = (0..=255).collect();
    for i in 0..4 {
        // One unresolved sequence gap must be finalized in the completed report.
        let seq = [0, 2, 3, 4][i];
        let node_time = 1_000_000 + i as u64 * 6_000_000;
        let csi = Csi {
            capture_seq: seq,
            node_time_us: node_time,
            rx_timestamp_us: (node_time - if i < 3 { 100_000 } else { 18_000_000 }) as u32,
            source_mac: [2; 6],
            probe: Some(Probe {
                tx: NodeId([2; 6]),
                epoch: 8,
                schedule: 9,
                seq,
            }),
            first_word_invalid: true,
            rx: RxMeta {
                sig_mode: 1,
                ..Default::default()
            },
            data: &bytes,
        };
        writer
            .write(&record(
                seq + 1,
                (i as u64 + 1) * 6_000_000_000,
                Message::Csi(csi),
            ))
            .unwrap();
    }
    // A layout cannot be inferred from byte count or another receiver's Hello.
    writer
        .write(&record(
            6,
            31_000_000_000,
            Message::Hello(Hello {
                chip: Chip::Esp32S3,
                ..hello
            }),
        ))
        .unwrap();
    let csi = Csi {
        capture_seq: 5,
        node_time_us: 20_000_000,
        rx_timestamp_us: 2_000_000,
        source_mac: [2; 6],
        probe: None,
        first_word_invalid: false,
        rx: RxMeta {
            sig_mode: 1,
            ..Default::default()
        },
        data: &bytes,
    };
    writer
        .write(&record(7, 32_000_000_000, Message::Csi(csi)))
        .unwrap();
    let mut bad = record(8, 33_000_000_000, Message::Hello(hello));
    bad.payload[20] ^= 1;
    writer.write(&bad).unwrap();
    writer.finish().unwrap();
}

fn read(dir: &Path, format: Format) -> Vec<RecordBatch> {
    match format {
        Format::Arrow => StreamReader::try_new(File::open(dir.join("csi.arrow")).unwrap(), None)
            .unwrap()
            .map(Result::unwrap)
            .collect(),
        Format::Parquet => {
            let builder = ParquetRecordBatchReaderBuilder::try_new(
                File::open(dir.join("csi.parquet")).unwrap(),
            )
            .unwrap();
            assert_eq!(builder.schema().metadata()["csi.schema"], "csi-export/1");
            let schema = builder.schema().clone();
            // Rust's Parquet batch iterator strips schema-level metadata by design.
            builder
                .build()
                .unwrap()
                .map(|b| {
                    RecordBatch::try_new(schema.clone(), b.unwrap().columns().to_vec()).unwrap()
                })
                .collect()
        }
    }
}

#[test]
fn formats_preserve_raw_values_masks_unknown_layouts_and_clock_discontinuity() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("raw");
    fixture(&input);
    let original = fs::read(input.join("segment-000000.bin")).unwrap();
    for (name, format) in [("arrow", Format::Arrow), ("parquet", Format::Parquet)] {
        let out = tmp.path().join(name);
        let summary = export::session(&input, &out, format).unwrap();
        assert_eq!(
            (
                summary.csi_rows,
                summary.decoded_rows,
                summary.invalid_frames
            ),
            (5, 4, 1)
        );
        let batches = read(&out, format);
        assert_eq!(batches.iter().map(|b| b.num_rows()).sum::<usize>(), 5);
        let mut observed = Vec::new();
        for batch in &batches {
            assert_eq!(batch.schema().metadata()["csi.schema"], "csi-export/1");
            let indices = batch
                .column_by_name("record_index")
                .unwrap()
                .as_any()
                .downcast_ref::<UInt64Array>()
                .unwrap();
            let raw = batch
                .column_by_name("native_csi")
                .unwrap()
                .as_any()
                .downcast_ref::<BinaryArray>()
                .unwrap();
            let status = batch
                .column_by_name("layout_status")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let real = batch
                .column_by_name("real")
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            for i in 0..batch.num_rows() {
                assert_eq!(raw.value(i), (0..=255).collect::<Vec<u8>>());
                if status.value(i) == "decoded" {
                    assert_eq!(real.value(i).len(), 128);
                } else {
                    assert_eq!(status.value(i), "unsupported_chip");
                    assert!(real.is_null(i));
                }
                observed.push((
                    indices.value(i),
                    batch.column_by_name("radio_run").unwrap().is_null(i),
                    batch.column_by_name("mapped_time_us").unwrap().is_null(i),
                ));
            }
        }
        observed.sort();
        // First capture has no causal fit; pre-reset capture maps, reset cannot reuse old map.
        assert!(observed[0].2);
        assert!(!observed[2].2);
        assert!(observed[3].2);
        assert!(observed[4].1);
        let meta: serde_json::Value =
            serde_json::from_slice(&fs::read(out.join("export.json")).unwrap()).unwrap();
        assert_eq!(meta["complete"], true);
        assert_eq!(
            meta["health"]["nodes"][0]["epochs"][0]["captures_missing"],
            1
        );
        assert!(export::session(&input, &out, format).is_err());
    }
    assert_eq!(
        fs::read(input.join("segment-000000.bin")).unwrap(),
        original
    );
}

#[test]
fn corrupt_incomplete_and_inside_session_exports_are_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("raw");
    fixture(&input);
    assert!(export::session(&input, &input.join("derived"), Format::Arrow).is_err());
    let path = input.join("segment-000000.bin");
    let mut bytes = fs::read(&path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 1;
    fs::write(path, bytes).unwrap();
    let output = tmp.path().join("bad");
    assert!(export::session(&input, &output, Format::Arrow).is_err());
    assert!(!output.exists(), "failed export is removed");
    let path = input.join("manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    manifest["complete"] = false.into();
    fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(export::session(&input, &output, Format::Arrow).is_err());
    assert!(!output.exists());
}
