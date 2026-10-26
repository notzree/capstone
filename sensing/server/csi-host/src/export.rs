//! Offline Python boundary: bounded Arrow batches, raw bytes and explicit provenance.
use std::collections::{BTreeMap, HashMap};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use arrow_array::builder::{BooleanBuilder, Int8Builder, Int16Builder, ListBuilder, StringBuilder};
use arrow_array::*;
use arrow_ipc::writer::StreamWriter;
use arrow_schema::{Field, Schema};
use csi_protocol::{Counters, Csi, Header, Hello, Message, NodeId, frame};
use parquet::arrow::ArrowWriter;
use parquet::file::properties::WriterProperties;
use serde::Serialize;
use serde_json::json;

use crate::clock::{ClockFitter, FitOptions, unwrap_radio_us};
use crate::error::IoContext;
use crate::inspect::{InspectOptions, Inspector};
use crate::layout::{self, Samples};
use crate::manifest::Manifest;
use crate::seq::{Arrival, SeqTracker};
use crate::session::{Integrity, SessionReader};
use crate::{Error, Record, RecordKind};

pub const SCHEMA: &str = "csi-export/1";
pub const BATCH_ROWS: usize = 1024;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Arrow,
    Parquet,
}

impl Format {
    fn file(self) -> &'static str {
        match self {
            Self::Arrow => "csi.arrow",
            Self::Parquet => "csi.parquet",
        }
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Summary {
    pub records: u64,
    pub csi_rows: u64,
    pub decoded_rows: u64,
    pub invalid_frames: u64,
    pub non_frame_records: u64,
    pub layouts: BTreeMap<String, u64>,
    pub links: BTreeMap<String, LinkQuality>,
    pub counter_windows: BTreeMap<String, CounterWindow>,
}

#[derive(Debug, Serialize)]
pub struct CounterWindow {
    pub node: String,
    pub epoch: u32,
    pub snapshots: u64,
    pub first_host_mono_ns: u64,
    pub last_host_mono_ns: u64,
    pub first_node_time_us: u64,
    pub last_node_time_us: u64,
    pub first: BTreeMap<&'static str, u64>,
    pub last: BTreeMap<&'static str, u64>,
    /// Only the observed interval. Null for a single snapshot or counter rollback.
    pub delta: Option<BTreeMap<&'static str, u64>>,
    pub counter_rollback: bool,
}

fn counter_values(c: Counters) -> BTreeMap<&'static str, u64> {
    BTreeMap::from([
        ("tx_requested", c.tx_requested),
        ("tx_accepted", c.tx_accepted),
        ("csi_callbacks", c.csi_callbacks),
        ("csi_filtered", c.csi_filtered),
        ("csi_captured", c.csi_captured),
        ("pool_drops", c.pool_drops),
        ("export_drops", c.export_drops),
        ("export_bytes", c.export_bytes),
    ])
}

#[derive(Debug, Default, Serialize)]
pub struct LinkQuality {
    pub receiver: String,
    pub receiver_epoch: u32,
    pub transmitter: Option<String>,
    pub transmitter_epoch: Option<u32>,
    pub schedule: Option<u32>,
    pub records: u64,
    pub decoded_records: u64,
    pub gain_records: u64,
    pub mapped_records: u64,
    pub stored_samples: u64,
    pub valid_samples: u64,
    pub first_host_mono_ns: u64,
    pub last_host_mono_ns: u64,
}

// Removing an unsuccessful new export cannot remove a pre-existing directory:
// create_dir reserves it before constructing this guard. export.json is written last.
struct OutputDir(Option<PathBuf>);
impl Drop for OutputDir {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[derive(Clone)]
pub(crate) struct Row {
    record_index: u64,
    source_index: u16,
    protocol_version: u8,
    receiver: String,
    receiver_epoch: u32,
    frame_seq: u32,
    transmitter: Option<String>,
    transmitter_epoch: Option<u32>,
    schedule: Option<u32>,
    probe_seq: Option<u32>,
    source_mac: String,
    capture_seq: u32,
    host_mono_ns: u64,
    host_wall_ns: u64,
    node_time_us: u64,
    rx_timestamp_us: u32,
    radio_time_us: Option<u64>,
    radio_run: Option<u64>,
    timeline: Option<u32>,
    mapping_version: Option<u64>,
    mapped_time_us: Option<f64>,
    fit_p99_us: Option<f64>,
    mapping_hops: Option<u32>,
    frame_arrival: &'static str,
    capture_arrival: &'static str,
    probe_arrival: Option<&'static str>,
    rssi_dbm: i8,
    noise_floor_dbm: i8,
    sig_mode: u8,
    mcs: u8,
    cwb: u8,
    stbc: u8,
    channel: u8,
    secondary_channel: u8,
    rx_state: u8,
    gain_agc: Option<u8>,
    gain_fft: Option<i8>,
    first_word_invalid: bool,
    hello_json: Option<String>,
    rx_metadata_json: String,
    layout_id: String,
    layout_status: String,
    native_csi: Vec<u8>,
    samples: Option<Samples>,
}

type Link = (NodeId, u32, NodeId, u32, u32);

/// State also used by the live publisher, so replay and live do not invent
/// different layout, sequence or clock semantics.
pub(crate) struct Encoder {
    next_index: u64,
    hellos: BTreeMap<(NodeId, u32), Hello>,
    frames: BTreeMap<(NodeId, u32), SeqTracker>,
    captures: BTreeMap<(NodeId, u32), SeqTracker>,
    probes: BTreeMap<Link, SeqTracker>,
    pub(crate) clocks: ClockFitter,
    pub(crate) summary: Summary,
    live: bool,
    pub(crate) cache_evictions: u64,
}

impl Encoder {
    pub(crate) fn new() -> Self {
        Self {
            next_index: 0,
            hellos: BTreeMap::new(),
            frames: BTreeMap::new(),
            captures: BTreeMap::new(),
            probes: BTreeMap::new(),
            clocks: ClockFitter::new(FitOptions::default()),
            summary: Summary::default(),
            live: false,
            cache_evictions: 0,
        }
    }

    pub(crate) fn live() -> Self {
        Self {
            clocks: ClockFitter::live(FitOptions::default(), 300_000),
            live: true,
            ..Self::new()
        }
    }

    pub(crate) fn push(&mut self, record: &Record) -> Vec<Row> {
        self.summary.records += 1;
        let observations = match record.observations() {
            Ok(observations) => observations,
            Err(_) => {
                self.summary.invalid_frames += 1;
                self.next_index += 1;
                return Vec::new();
            }
        };
        let mut rows = Vec::new();
        let before = self.next_index;
        for observation in observations {
            let index = self.next_index;
            self.next_index += 1;
            if let Some(row) = self.push_observation(&observation, index) {
                rows.push(row);
            }
        }
        if self.next_index == before {
            self.next_index += 1;
        }
        rows
    }

    fn push_observation(&mut self, record: &Record, index: u64) -> Option<Row> {
        self.clocks.advance(record.mono_ns);
        if self.live {
            self.clocks.take_updates();
        }
        if record.kind != RecordKind::Frame {
            self.summary.non_frame_records += 1;
            return None;
        }
        let Ok((header, message)) = frame::decode(&record.payload) else {
            self.summary.invalid_frames += 1;
            return None;
        };
        // Host-to-node frames do not consume the receiver's frame sequence.
        if message.kind().is_host_to_node() {
            return None;
        }
        if self.live
            && !self.frames.contains_key(&(header.node, header.epoch))
            && self.frames.len() >= 256
        {
            self.cache_evictions += self.frames.len() as u64;
            self.frames.clear();
            self.captures.clear();
            self.hellos.clear();
            self.probes.clear();
            self.summary.counter_windows.clear();
        }
        let arrival = self
            .frames
            .entry((header.node, header.epoch))
            .or_default()
            .push(header.seq, record.mono_ns);
        // Sequence finalization details live in Inspector's completed report;
        // drain events here to keep the per-row annotation trackers bounded.
        self.frames
            .get_mut(&(header.node, header.epoch))
            .unwrap()
            .take_finalized();
        match message {
            Message::Hello(hello) => {
                self.hellos.insert((header.node, header.epoch), hello);
                None
            }
            Message::Csi(csi) => Some(self.csi(index, record, header, csi, arrival)),
            Message::Counters(c) => {
                let window = self
                    .summary
                    .counter_windows
                    .entry(format!("{}/{}", header.node, header.epoch))
                    .or_insert_with(|| CounterWindow {
                        node: header.node.to_string(),
                        epoch: header.epoch,
                        snapshots: 0,
                        first_host_mono_ns: record.mono_ns,
                        last_host_mono_ns: record.mono_ns,
                        first_node_time_us: c.node_time_us,
                        last_node_time_us: c.node_time_us,
                        first: counter_values(c),
                        last: counter_values(c),
                        delta: None,
                        counter_rollback: false,
                    });
                window.snapshots += 1;
                if c.node_time_us >= window.last_node_time_us {
                    let next = counter_values(c);
                    window.counter_rollback |= next.iter().any(|(k, v)| *v < window.last[k]);
                    window.last = next;
                    window.last_host_mono_ns = record.mono_ns;
                    window.last_node_time_us = c.node_time_us;
                    if !window.counter_rollback
                        && window.last_node_time_us > window.first_node_time_us
                    {
                        window.delta = window
                            .last
                            .iter()
                            .map(|(&k, &v)| v.checked_sub(window.first[k]).map(|d| (k, d)))
                            .collect();
                    } else {
                        window.delta = None;
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn csi(
        &mut self,
        index: u64,
        record: &Record,
        header: Header,
        csi: Csi<'_>,
        arrival: Arrival,
    ) -> Row {
        let capture_tracker = self
            .captures
            .entry((header.node, header.epoch))
            .or_default();
        let capture_arrival = capture_tracker.push(csi.capture_seq, record.mono_ns);
        capture_tracker.take_finalized();
        let probe_arrival = csi.probe.map(|p| {
            let link = (header.node, header.epoch, p.tx, p.epoch, p.schedule);
            if self.live && !self.probes.contains_key(&link) && self.probes.len() >= 4096 {
                self.cache_evictions += self.probes.len() as u64;
                self.probes.clear();
            }
            let tracker = self.probes.entry(link).or_default();
            let arrival = tracker.push(p.seq, record.mono_ns);
            tracker.take_finalized();
            arrival_name(arrival)
        });
        // Do not let duplicate/stale/out-of-order captures mutate radio-run identity.
        let fresh = matches!(
            capture_arrival,
            Arrival::First | Arrival::InOrder | Arrival::Gap(_)
        );
        if fresh {
            self.clocks.push_csi(record.mono_ns, &header, &csi);
        }
        let key = (fresh && csi.probe.is_some())
            .then(|| self.clocks.capture_clock(&header, &csi))
            .flatten();
        let radio_time_us = unwrap_radio_us(csi.rx_timestamp_us, csi.node_time_us).map(|p| p.0);
        let mapping = key.and_then(|k| self.clocks.current_mappings().get(&k));
        let hello = self.hellos.get(&(header.node, header.epoch));
        let decoded = layout::decode(hello, &csi);
        let status = decoded.as_ref().map_or_else(|s| *s, |_| "decoded");
        let layout_id = format!(
            "{}:{:?}:{:?}:phy{}:bw{}:sc{}:stbc{}:len{}",
            layout::DECODER,
            hello.map(|h| h.chip),
            hello.map(|h| h.csi_config),
            csi.rx.sig_mode,
            csi.rx.cwb,
            csi.rx.secondary_channel,
            csi.rx.stbc,
            csi.data.len()
        );
        self.summary.csi_rows += 1;
        self.summary.decoded_rows += u64::from(decoded.is_ok());
        *self.summary.layouts.entry(status.into()).or_default() += 1;
        let link_key = format!(
            "{}/{}/{:?}/{:?}/{:?}",
            header.node,
            header.epoch,
            csi.probe.map(|p| p.tx.to_string()),
            csi.probe.map(|p| p.epoch),
            csi.probe.map(|p| p.schedule)
        );
        if self.live
            && !self.summary.links.contains_key(&link_key)
            && self.summary.links.len() >= 4096
        {
            self.cache_evictions += self.summary.links.len() as u64;
            self.summary.links.clear();
        }
        let quality = self
            .summary
            .links
            .entry(link_key)
            .or_insert_with(|| LinkQuality {
                receiver: header.node.to_string(),
                receiver_epoch: header.epoch,
                transmitter: csi.probe.map(|p| p.tx.to_string()),
                transmitter_epoch: csi.probe.map(|p| p.epoch),
                schedule: csi.probe.map(|p| p.schedule),
                first_host_mono_ns: record.mono_ns,
                ..Default::default()
            });
        quality.records += 1;
        quality.decoded_records += u64::from(decoded.is_ok());
        quality.gain_records += u64::from(csi.rx.gain.is_some());
        quality.mapped_records += u64::from(mapping.is_some() && radio_time_us.is_some());
        if let Ok(samples) = &decoded {
            quality.stored_samples += samples.valid.len() as u64;
            quality.valid_samples += samples.valid.iter().filter(|&&v| v).count() as u64;
        }
        quality.first_host_mono_ns = quality.first_host_mono_ns.min(record.mono_ns);
        quality.last_host_mono_ns = quality.last_host_mono_ns.max(record.mono_ns);
        let r = csi.rx;
        Row {
            record_index: index,
            source_index: record.source,
            protocol_version: record.payload[0],
            receiver: header.node.to_string(),
            receiver_epoch: header.epoch,
            frame_seq: header.seq,
            transmitter: csi.probe.map(|p| p.tx.to_string()),
            transmitter_epoch: csi.probe.map(|p| p.epoch),
            schedule: csi.probe.map(|p| p.schedule),
            probe_seq: csi.probe.map(|p| p.seq),
            source_mac: NodeId(csi.source_mac).to_string(),
            capture_seq: csi.capture_seq,
            host_mono_ns: record.mono_ns,
            host_wall_ns: record.wall_ns,
            node_time_us: csi.node_time_us,
            rx_timestamp_us: csi.rx_timestamp_us,
            radio_time_us,
            radio_run: key.map(|k| k.radio),
            timeline: mapping.map(|m| m.timeline),
            mapping_version: mapping.map(|m| m.version),
            mapped_time_us: mapping.and_then(|m| radio_time_us.map(|t| m.map.apply(t as f64))),
            fit_p99_us: mapping.and_then(|m| m.residual_us.map(|r| r.p99)),
            mapping_hops: mapping.map(|m| m.hops),
            frame_arrival: arrival_name(arrival),
            capture_arrival: arrival_name(capture_arrival),
            probe_arrival,
            rssi_dbm: r.rssi,
            noise_floor_dbm: r.noise_floor,
            sig_mode: r.sig_mode,
            mcs: r.mcs,
            cwb: r.cwb,
            stbc: r.stbc,
            channel: r.channel,
            secondary_channel: r.secondary_channel,
            rx_state: r.rx_state,
            gain_agc: r.gain.map(|g| g.agc),
            gain_fft: r.gain.map(|g| g.fft),
            first_word_invalid: csi.first_word_invalid,
            hello_json: hello.map(hello_json),
            rx_metadata_json:
                json!({"rate":r.rate,"smoothing":r.smoothing,"not_sounding":r.not_sounding,
                "aggregation":r.aggregation,"fec_coding":r.fec_coding,"sgi":r.sgi,
                "ampdu_cnt":r.ampdu_cnt,"ant":r.ant,"sig_len":r.sig_len})
                .to_string(),
            layout_id,
            layout_status: status.into(),
            native_csi: csi.data.to_vec(),
            samples: decoded.ok(),
        }
    }
}

fn arrival_name(arrival: Arrival) -> &'static str {
    match arrival {
        Arrival::First => "first",
        Arrival::InOrder => "in_order",
        Arrival::Gap(_) => "gap",
        Arrival::Late => "late",
        Arrival::Duplicate => "duplicate",
        Arrival::Stale => "stale",
    }
}

fn hello_json(h: &Hello) -> String {
    let c = h.csi_config;
    json!({"chip":format!("{:?}",h.chip),"tx":h.roles.tx,"rx":h.roles.rx,
        "firmware_version":h.firmware_version,"firmware_commit":h.firmware_commit,
        "channel":h.channel,"secondary_channel":h.secondary_channel,"pool_capacity":h.pool_capacity,
        "csi_config":{"lltf":c.lltf,"htltf":c.htltf,"stbc_htltf2":c.stbc_htltf2,
        "ltf_merge":c.ltf_merge,"channel_filter":c.channel_filter,"manu_scale":c.manu_scale,"shift":c.shift}}).to_string()
}

pub(crate) fn metadata(manifest: &Manifest) -> HashMap<String, String> {
    HashMap::from([
        ("csi.schema".into(), SCHEMA.into()),
        ("csi.session_id".into(), manifest.session_id.to_string()),
        (
            "csi.manifest".into(),
            serde_json::to_string(manifest).expect("finite manifest"),
        ),
        ("csi.decoder".into(), layout::DECODER.into()),
        (
            "csi.clock_provenance".into(),
            "causal_recomputed_default_fit; relative timeline us; not host/wall time; no backfill"
                .into(),
        ),
        (
            "csi.clock_options".into(),
            serde_json::to_string(&FitOptions::default()).unwrap(),
        ),
    ])
}

pub(crate) fn batch(rows: &[Row], metadata: HashMap<String, String>) -> Result<RecordBatch, Error> {
    let mut columns: Vec<(&str, bool, ArrayRef)> = Vec::new();
    macro_rules! col {
        ($name:ident, StringArray, false) => {
            columns.push((
                stringify!($name),
                false,
                Arc::new(StringArray::from_iter_values(rows.iter().map(|r| &r.$name))),
            ));
        };
        ($name:ident, $array:ty, $nullable:expr) => {
            columns.push((
                stringify!($name),
                $nullable,
                Arc::new(<$array>::from_iter(rows.iter().map(|r| r.$name.clone()))),
            ));
        };
    }
    col!(record_index, UInt64Array, false);
    col!(source_index, UInt16Array, false);
    col!(protocol_version, UInt8Array, false);
    col!(receiver, StringArray, false);
    col!(receiver_epoch, UInt32Array, false);
    col!(frame_seq, UInt32Array, false);
    col!(transmitter, StringArray, true);
    col!(transmitter_epoch, UInt32Array, true);
    col!(schedule, UInt32Array, true);
    col!(probe_seq, UInt32Array, true);
    col!(source_mac, StringArray, false);
    col!(capture_seq, UInt32Array, false);
    col!(host_mono_ns, UInt64Array, false);
    col!(host_wall_ns, UInt64Array, false);
    col!(node_time_us, UInt64Array, false);
    col!(rx_timestamp_us, UInt32Array, false);
    col!(radio_time_us, UInt64Array, true);
    col!(radio_run, UInt64Array, true);
    col!(timeline, UInt32Array, true);
    col!(mapping_version, UInt64Array, true);
    col!(mapped_time_us, Float64Array, true);
    col!(fit_p99_us, Float64Array, true);
    col!(mapping_hops, UInt32Array, true);
    col!(frame_arrival, StringArray, false);
    col!(capture_arrival, StringArray, false);
    col!(probe_arrival, StringArray, true);
    col!(rssi_dbm, Int8Array, false);
    col!(noise_floor_dbm, Int8Array, false);
    col!(sig_mode, UInt8Array, false);
    col!(mcs, UInt8Array, false);
    col!(cwb, UInt8Array, false);
    col!(stbc, UInt8Array, false);
    col!(channel, UInt8Array, false);
    col!(secondary_channel, UInt8Array, false);
    col!(rx_state, UInt8Array, false);
    col!(gain_agc, UInt8Array, true);
    col!(gain_fft, Int8Array, true);
    col!(first_word_invalid, BooleanArray, false);
    col!(hello_json, StringArray, true);
    col!(rx_metadata_json, StringArray, false);
    col!(layout_id, StringArray, false);
    col!(layout_status, StringArray, false);
    columns.push((
        "native_csi",
        false,
        Arc::new(BinaryArray::from_iter_values(
            rows.iter().map(|r| r.native_csi.as_slice()),
        )),
    ));
    macro_rules! list {
        ($name:ident, $builder:ty) => {{
            let mut builder = ListBuilder::new(<$builder>::new());
            for row in rows {
                if let Some(s) = &row.samples {
                    for v in &s.$name {
                        builder.values().append_value(*v);
                    }
                    builder.append(true);
                } else {
                    builder.append(false);
                }
            }
            columns.push((stringify!($name), true, Arc::new(builder.finish())));
        }};
    }
    list!(real, Int8Builder);
    list!(imag, Int8Builder);
    list!(ltf, StringBuilder);
    list!(subcarrier, Int16Builder);
    list!(valid, BooleanBuilder);
    list!(pilot, BooleanBuilder);
    let fields: Vec<Field> = columns
        .iter()
        .map(|(n, nullable, a)| Field::new(*n, a.data_type().clone(), *nullable))
        .collect();
    let schema = Arc::new(Schema::new_with_metadata(fields, metadata));
    Ok(RecordBatch::try_new(
        schema,
        columns.into_iter().map(|(_, _, a)| a).collect(),
    )?)
}

enum Sink {
    Arrow(StreamWriter<File>),
    Parquet(ArrowWriter<File>),
}
impl Sink {
    fn write(&mut self, batch: &RecordBatch) -> Result<(), Error> {
        match self {
            Self::Arrow(w) => w.write(batch)?,
            Self::Parquet(w) => w.write(batch)?,
        }
        Ok(())
    }
    fn finish(self) -> Result<(), Error> {
        match self {
            Self::Arrow(mut w) => {
                w.finish()?;
                w.get_ref().sync_all().at(Path::new("csi.arrow"))?;
            }
            Self::Parquet(mut w) => {
                w.finish()?;
                w.inner().sync_all().at(Path::new("csi.parquet"))?;
            }
        }
        Ok(())
    }
}

/// Export only completed, hash-verified sessions. Invalid wire frames are retained
/// in raw storage and counted in the report, rather than invented as CSI rows.
pub fn session(input: &Path, output: &Path, format: Format) -> Result<Summary, Error> {
    if output.exists() {
        return Err(Error::Exists(output.to_owned()));
    }
    let mut reader = SessionReader::open(input)?;
    let manifest = reader.manifest().clone();
    if !manifest.complete {
        return Err(Error::Config("seal the session before exporting".into()));
    }
    let geometry = reader.geometry()?;
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let ancestor = parent
        .ancestors()
        .find(|p| p.exists())
        .unwrap_or(Path::new("."));
    if ancestor
        .canonicalize()
        .at(ancestor)?
        .starts_with(input.canonicalize().at(input)?)
    {
        return Err(Error::Config(
            "export outside the raw session directory".into(),
        ));
    }
    fs::create_dir_all(parent).at(parent)?;
    if parent
        .canonicalize()
        .at(parent)?
        .starts_with(input.canonicalize().at(input)?)
    {
        return Err(Error::Config(
            "export outside the raw session directory".into(),
        ));
    }
    fs::create_dir(output).at(output)?;
    let mut guard = OutputDir(Some(output.to_owned()));
    let mut metadata = metadata(&manifest);
    metadata.insert(
        "csi.geometry".into(),
        serde_json::to_string(&geometry).map_err(|e| Error::Config(e.to_string()))?,
    );
    let schema = batch(&[], metadata.clone())?.schema();
    let path = output.join(format.file());
    let file = File::create(&path).at(&path)?;
    let mut sink = match format {
        Format::Arrow => Sink::Arrow(StreamWriter::try_new(file, &schema)?),
        Format::Parquet => Sink::Parquet(ArrowWriter::try_new(
            file,
            schema,
            Some(
                WriterProperties::builder()
                    .set_max_row_group_row_count(Some(BATCH_ROWS))
                    .build(),
            ),
        )?),
    };
    let mut encoder = Encoder::new();
    let mut inspector = Inspector::new(InspectOptions {
        max_events: 1000,
        ..Default::default()
    });
    let mut rows = Vec::with_capacity(BATCH_ROWS);
    for item in reader.by_ref() {
        let record = item?;
        inspector.push(&record);
        for row in encoder.push(&record) {
            rows.push(row);
            if rows.len() == BATCH_ROWS {
                flush(&mut sink, &mut rows, &metadata)?;
            }
        }
    }
    if reader
        .checks()
        .iter()
        .any(|s| s.integrity != Integrity::Verified)
    {
        return Err(Error::Config(
            "session segment integrity check failed; export discarded".into(),
        ));
    }
    flush(&mut sink, &mut rows, &metadata)?;
    sink.finish()?;
    let fit = encoder.clocks.finish();
    fit.write(output, manifest.session_id)?;
    let report = inspector.finish();
    let document = json!({"schema":SCHEMA,"decoder":layout::DECODER,"data_file":format.file(),
        "manifest":manifest,"geometry":geometry,"summary":encoder.summary,
        "segments":reader.checks(),"health":report,
        "clock_provenance":"causal_recomputed_default_fit; no backfill; stored live host-clock mappings are not applied",
        "complete":true});
    let bytes = serde_json::to_vec_pretty(&document).map_err(|e| Error::Config(e.to_string()))?;
    fs::write(output.join("export.json"), bytes).at(output)?;
    guard.0 = None;
    Ok(encoder.summary)
}

fn flush(
    sink: &mut Sink,
    rows: &mut Vec<Row>,
    metadata: &HashMap<String, String>,
) -> Result<(), Error> {
    for batch in grouped_batches(rows, metadata)? {
        sink.write(&batch)?;
    }
    Ok(())
}

pub(crate) fn grouped_batches(
    rows: &mut Vec<Row>,
    metadata: &HashMap<String, String>,
) -> Result<Vec<RecordBatch>, Error> {
    // Homogeneous Arrow batches, bounded by the input block. record_index retains
    // the original order even when neighbouring links are grouped for delivery.
    let mut groups: BTreeMap<_, Vec<Row>> = BTreeMap::new();
    for row in rows.drain(..) {
        groups
            .entry((
                row.receiver.clone(),
                row.receiver_epoch,
                row.transmitter.clone(),
                row.transmitter_epoch,
                row.schedule,
                row.layout_id.clone(),
            ))
            .or_default()
            .push(row);
    }
    groups
        .values()
        .flat_map(|rows| {
            rows.chunks(BATCH_ROWS)
                .map(|chunk| batch(chunk, metadata.clone()))
        })
        .collect()
}

#[cfg(test)]
mod trial_quality_tests {
    use super::*;

    #[test]
    fn counter_windows_exclude_pretrial_totals_ignore_late_and_latch_rollback() {
        let mut encoder = Encoder::new();
        for (seq, node_us, drops) in [
            (0, 1_000_000, 100),
            (1, 3_000_000, 103),
            (2, 2_000_000, 0),
            (3, 4_000_000, 1),
            (4, 5_000_000, 200),
        ] {
            let mut bytes = vec![0; csi_protocol::MAX_FRAME_LEN];
            let len = frame::encode(
                &Header {
                    node: NodeId([2; 6]),
                    epoch: 1,
                    seq,
                },
                &Message::Counters(Counters {
                    node_time_us: node_us,
                    pool_drops: drops,
                    ..Default::default()
                }),
                &mut bytes,
            )
            .unwrap();
            bytes.truncate(len);
            encoder.push(&Record {
                mono_ns: u64::from(seq) * 1_000_000_000,
                wall_ns: 0,
                source: 0,
                kind: RecordKind::Frame,
                payload: bytes,
            });
            let window = encoder.summary.counter_windows.values().next().unwrap();
            match seq {
                0 => assert!(window.delta.is_none()),
                1 | 2 => assert_eq!(window.delta.as_ref().unwrap()["pool_drops"], 3),
                _ => {
                    assert!(window.delta.is_none());
                    assert!(window.counter_rollback);
                }
            }
        }
    }
}
