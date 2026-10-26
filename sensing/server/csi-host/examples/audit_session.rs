//! Read-only physical bench audit, using the normal session and protocol decoders.
//! Run from sensing/: cargo run -p csi-host --example audit_session -- SESSION
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    path::Path,
};

use csi_host::{
    RecordKind,
    inspect::{InspectOptions, Inspector},
    seq::SeqTracker,
    session::SessionReader,
};
use csi_protocol::{Counters, Message, NodeId, frame};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[derive(Default)]
struct Node {
    roles: Option<Value>,
    sources: BTreeSet<u16>,
    first: Option<Counters>,
    last: Option<Counters>,
    snapshots: u64,
}

#[derive(Default)]
struct Link {
    records: u64,
    first_seq: Option<u32>,
    last_seq: Option<u32>,
    first_node_us: Option<u64>,
    last_node_us: Option<u64>,
    probes: SeqTracker,
    layouts: BTreeMap<String, u64>,
    raw: BTreeSet<[u8; 32]>,
    samples: Vec<Value>,
    intervals_us: Vec<f64>,
    rssi_dbm: Vec<f64>,
    noise_floor_dbm: Vec<f64>,
    timestamp_regressions: u64,
}

// Exact nearest-rank quantiles for this bounded bench audit, rather than the
// streaming histogram used by the long-running health inspector.
fn distribution(mut samples: Vec<f64>) -> Value {
    if samples.is_empty() {
        return Value::Null;
    }
    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    let mean = samples.iter().sum::<f64>() / n as f64;
    let quantile = |p: f64| samples[((p * n as f64).ceil() as usize).saturating_sub(1)];
    let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64;
    json!({"count":n,"min":samples[0],"mean":mean,"p50":quantile(0.5),
        "p95":quantile(0.95),"p99":quantile(0.99),"max":samples[n-1],
        "stddev":variance.sqrt()})
}

fn counters(c: Counters) -> Value {
    json!({
        "node_time_us": c.node_time_us,
        "tx_requested": c.tx_requested, "tx_accepted": c.tx_accepted,
        "csi_callbacks": c.csi_callbacks, "csi_filtered": c.csi_filtered,
        "csi_captured": c.csi_captured, "pool_drops": c.pool_drops,
        "export_drops": c.export_drops, "export_bytes": c.export_bytes,
        "pool_high_water": c.pool_high_water,
    })
}

// Counter deltas cover only the two report instants, not the entire host recording.
fn deltas(first: Counters, last: Counters) -> Value {
    let first = counters(first);
    let last = counters(last);
    let mut out = serde_json::Map::new();
    for (key, value) in last.as_object().unwrap() {
        if key == "pool_high_water" {
            continue; // A high-water mark is not an accumulating counter.
        }
        out.insert(
            key.clone(),
            match value
                .as_u64()
                .unwrap()
                .checked_sub(first[key].as_u64().unwrap())
            {
                Some(delta) => json!(delta),
                None => Value::Null,
            },
        );
    }
    Value::Object(out)
}

fn run() -> Result<(), Box<dyn Error>> {
    let dir = std::env::args()
        .nth(1)
        .ok_or("usage: audit_session SESSION")?;
    let mut reader = SessionReader::open(Path::new(&dir))?;
    let mut inspector = Inspector::new(InspectOptions {
        max_events: 0,
        ..Default::default()
    });
    let mut nodes = BTreeMap::<(NodeId, u32), Node>::new();
    // Separate receiver boots, transmitter boots, and schedule generations.
    let mut links = BTreeMap::<(NodeId, u32, NodeId, u32, u32), Link>::new();
    let mut errors = BTreeMap::<String, u64>::new();
    let mut error_samples = Vec::<Value>::new();
    let mut startup_errors = 0u64;
    let mut later_errors = 0u64;
    let mut note_error = |error: String, mono_ns: u64, source: u16| {
        *errors.entry(error.clone()).or_default() += 1;
        if mono_ns <= 1_000_000_000 {
            startup_errors += 1;
        } else {
            later_errors += 1;
        }
        if error_samples.len() < 32 {
            error_samples.push(json!({"host_mono_ns":mono_ns,"source":source,"error":error}));
        }
    };
    let mut self_captures = 0u64;
    let mut mac_probe_mismatch = 0u64;
    let mut unattributed_csi = 0u64;
    let mut host_to_node_exports = 0u64;
    for item in reader.by_ref() {
        let record = item?;
        inspector.push(&record);
        let observations = match record.observations() {
            Ok(observations) => observations,
            Err(error) => {
                note_error(format!("batch:{error:?}"), record.mono_ns, record.source);
                continue;
            }
        };
        for record in observations {
            if record.kind != RecordKind::Frame {
                note_error(
                    format!("{:?}:{:?}", record.kind, record.payload),
                    record.mono_ns,
                    record.source,
                );
                continue;
            }
            let (header, message) = match frame::decode(&record.payload) {
                Ok(decoded) => decoded,
                Err(error) => {
                    note_error(format!("decode:{error:?}"), record.mono_ns, record.source);
                    continue;
                }
            };
            let node = nodes.entry((header.node, header.epoch)).or_default();
            node.sources.insert(record.source);
            match message {
                Message::Hello(hello) => {
                    node.roles = Some(json!({"tx": hello.roles.tx, "rx": hello.roles.rx}))
                }
                Message::Counters(c) => {
                    node.first.get_or_insert(c);
                    node.last = Some(c);
                    node.snapshots += 1;
                }
                Message::TimeProbe(_) => host_to_node_exports += 1,
                Message::TimeEcho(_) => {}
                Message::Csi(c) => {
                    if c.source_mac == header.node.0 {
                        self_captures += 1;
                    }
                    let Some(probe) = c.probe else {
                        unattributed_csi += 1;
                        continue;
                    };
                    if probe.tx.0 != c.source_mac {
                        mac_probe_mismatch += 1;
                    }
                    let link = links
                        .entry((
                            header.node,
                            header.epoch,
                            probe.tx,
                            probe.epoch,
                            probe.schedule,
                        ))
                        .or_default();
                    link.records += 1;
                    link.first_seq.get_or_insert(probe.seq);
                    link.last_seq = Some(probe.seq);
                    link.first_node_us.get_or_insert(c.node_time_us);
                    if let Some(previous) = link.last_node_us {
                        if let Some(interval) = c.node_time_us.checked_sub(previous) {
                            link.intervals_us.push(interval as f64);
                        } else {
                            link.timestamp_regressions += 1;
                        }
                    }
                    link.last_node_us = Some(c.node_time_us);
                    link.rssi_dbm.push(f64::from(c.rx.rssi));
                    link.noise_floor_dbm.push(f64::from(c.rx.noise_floor));
                    link.probes.push(probe.seq, record.mono_ns);
                    link.raw.insert(Sha256::digest(c.data).into());
                    let layout = format!(
                        "sig_mode={} mcs={} cwb={} sgi={} stbc={} channel={} len={} first_word_invalid={} gain={:?}",
                        c.rx.sig_mode,
                        c.rx.mcs,
                        c.rx.cwb,
                        c.rx.sgi,
                        c.rx.stbc,
                        c.rx.channel,
                        c.data.len(),
                        c.first_word_invalid,
                        c.rx.gain
                    );
                    *link.layouts.entry(layout).or_default() += 1;
                    // Bounded diagnostic samples retain timestamps for external schedule analysis.
                    if link.samples.len() < 32 {
                        link.samples.push(json!({"host_mono_ns":record.mono_ns,"node_time_us":c.node_time_us,
                        "radio_time_us":c.rx_timestamp_us,"capture_seq":c.capture_seq,"probe_seq":probe.seq}));
                    }
                }
            }
        }
    }
    let links: Vec<_> = links.into_iter().map(|((rx, rx_epoch, tx, tx_epoch, schedule), mut link)| {
        link.probes.finish();
        let span_us = link.last_node_us.unwrap_or(0).saturating_sub(link.first_node_us.unwrap_or(0));
        json!({"rx":rx.to_string(),"rx_epoch":rx_epoch,"tx":tx.to_string(),"tx_epoch":tx_epoch,"schedule":schedule,
            "records":link.records,"distinct_raw_csi_sha256":link.raw.len(),"layouts":link.layouts,
            "first_probe_seq":link.first_seq,"last_probe_seq":link.last_seq,
            "first_node_time_us":link.first_node_us,"last_node_time_us":link.last_node_us,
            "rate_hz": if span_us > 0 { Some((link.records.saturating_sub(1)) as f64 * 1e6 / span_us as f64) } else { None },
            "probes_unique":link.probes.received,"probes_missing":link.probes.missing,
            "probes_duplicate":link.probes.duplicates,"probes_late":link.probes.late,"probes_stale":link.probes.stale,
            "interarrival_node_us":distribution(link.intervals_us),
            "rssi_dbm":distribution(link.rssi_dbm),"noise_floor_dbm":distribution(link.noise_floor_dbm),
            "node_timestamp_regressions":link.timestamp_regressions,
            "timestamp_samples_first_32":link.samples})
    }).collect();
    let nodes: Vec<_> = nodes.into_iter().map(|((node, epoch), n)| json!({
        "node":node.to_string(),"epoch":epoch,"roles":n.roles,"sources":n.sources,"counter_snapshots":n.snapshots,
        "first_counters":n.first.map(counters),"last_counters":n.last.map(counters),
        "counter_delta": n.first.zip(n.last).map(|(first,last)|deltas(first,last))
    })).collect();
    let output = json!({"session":dir,"nodes":nodes,"links":links,"stream_or_decode_errors":errors,
        "stream_or_decode_error_samples_first_32":error_samples,
        "stream_or_decode_errors_first_second":startup_errors,"stream_or_decode_errors_after_first_second":later_errors,
        "self_captures":self_captures,"probe_source_mac_mismatches":mac_probe_mismatch,
        "unattributed_csi":unattributed_csi,"host_to_node_exported_frames":host_to_node_exports,
        "segment_checks":reader.checks(),"health":inspector.finish(),
        "notes":["Probe missing counts cover observed sequence interiors only, not recording boundaries.",
        "Counter deltas cover first-to-last reports only; null means a counter decreased.",
        "Interarrival uses receiver node timestamps; exact nearest-rank quantiles and population standard deviation include observed missing-probe gaps. Negative deltas are counted separately.",
        "CSI uniqueness compares SHA-256 digests of native bytes, including any invalid first word.",
        "Per-link timestamp samples retain only the first 32 records; they do not prove collision freedom."]});
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("audit failed: {error}");
        std::process::exit(1);
    }
}
