//! Live ingest. Async source tasks -> bounded recorder queue -> dedicated recorder thread ->
//! bounded, lossy subscriber queues. The recorder never waits for a subscriber.
//!
//! Raw records are written before health/clock processing or live delivery. On normal shutdown,
//! source tasks stop reading, finish handing off acquired records, then the recorder drains and
//! seals. A recorder/source failure leaves the manifest incomplete.
pub mod config;
mod source;
pub mod time_sync;

use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use csi_protocol::{Message, frame};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinSet;
use tokio::time::{MissedTickBehavior, interval};

use crate::clock::{ClockFit, ClockFitWriter, ClockFitter};
use crate::error::IoContext;
use crate::inspect::{InspectOptions, Inspector, Report};
use crate::session::{SessionWriter, WriterOptions};
use crate::{Error, Record, RecordKind};
use config::HubConfig;
use time_sync::{HostClock, HostMapping};

pub const STATUS_FILE: &str = "hub-status.json";

#[derive(Clone)]
struct ArrivalClock(Instant);
impl ArrivalClock {
    fn mono_ns(&self) -> u64 {
        self.0.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64
    }
}

enum Input {
    Record {
        record: Record,
        correlated_echo: bool,
    },
    Connection {
        source: u16,
        mono_ns: u64,
        connected: bool,
        error: Option<String>,
    },
    Foreign(u16),
    ProbeWriteError(u16),
    Tick(u64),
    Fatal(String),
}

pub struct Subscriber {
    tx: mpsc::Sender<Arc<Record>>,
    dropped: Arc<AtomicU64>,
}

pub type Subscription = (Subscriber, mpsc::Receiver<Arc<Record>>, Arc<AtomicU64>);

/// A receiver can be stalled or dropped without affecting acquisition. Keep the counter to
/// attribute loss to this subscriber, distinct from node/transport loss.
pub fn subscription(capacity: usize) -> Result<Subscription, Error> {
    if !(1..=65_536).contains(&capacity) {
        return Err(Error::Config(
            "subscriber capacity must be 1..=65536".into(),
        ));
    }
    let (tx, rx) = mpsc::channel(capacity);
    let dropped = Arc::new(AtomicU64::new(0));
    Ok((
        Subscriber {
            tx,
            dropped: dropped.clone(),
        },
        rx,
        dropped,
    ))
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceStatus {
    pub name: String,
    pub state: &'static str,
    pub records: u64,
    pub valid_frames: u64,
    pub decode_errors: u64,
    pub framing_errors: u64,
    pub foreign_datagrams: u64,
    pub probe_write_errors: u64,
    pub unmatched_echoes: u64,
    pub connects: u64,
    pub disconnects: u64,
    pub stale_transitions: u64,
    pub node: Option<String>,
    pub epoch: Option<u32>,
    pub epoch_changes: u64,
    pub last_valid_mono_ns: Option<u64>,
    pub last_error: Option<String>,
    connected_at_ns: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HubStatus {
    pub schema: &'static str,
    pub session_id: uuid::Uuid,
    pub complete: bool,
    pub mono_ns: u64,
    pub records: u64,
    pub sources: Vec<SourceStatus>,
    pub subscriber_drops: Vec<u64>,
    pub queue_capacity: usize,
    pub max_queue_depth: usize,
    pub clock_sample_capacity: usize,
    pub max_clock_samples: usize,
    pub clock_capacity_rejects: u64,
    pub health_history_evictions: u64,
    pub echoes_accepted: u64,
    pub echoes_unmapped: u64,
    pub echoes_rejected: u64,
    pub host_mapping: Option<HostMapping>,
    /// Bounded health view: cumulative source totals plus retained node epochs/links.
    pub health: Report,
}

pub struct RunningHub {
    stop: watch::Sender<bool>,
    done: Option<oneshot::Receiver<Result<HubStatus, Error>>>,
    /// Actual bound UDP addresses (including dynamically assigned ports); None for serial.
    pub addresses: Vec<Option<SocketAddr>>,
}

impl RunningHub {
    pub fn request_shutdown(&self) {
        let _ = self.stop.send(true);
    }

    pub async fn wait(mut self) -> Result<HubStatus, Error> {
        self.done
            .take()
            .unwrap()
            .await
            .map_err(|_| Error::Config("hub supervisor stopped unexpectedly".into()))?
    }

    pub async fn shutdown(self) -> Result<HubStatus, Error> {
        self.request_shutdown();
        self.wait().await
    }

    pub async fn wait_for_shutdown<F>(mut self, shutdown: F) -> Result<HubStatus, Error>
    where
        F: std::future::Future<Output = std::io::Result<()>>,
    {
        let mut done = self.done.take().unwrap();
        tokio::select! {
            result = &mut done => result.map_err(|_| Error::Config("hub supervisor stopped unexpectedly".into()))?,
            result = shutdown => {
                self.request_shutdown();
                let status = done.await.map_err(|_| Error::Config("hub supervisor stopped unexpectedly".into()))??;
                result.map_err(|source| Error::Io { path: PathBuf::from("signal"), source })?;
                Ok(status)
            }
        }
    }
}

impl Drop for RunningHub {
    fn drop(&mut self) {
        self.request_shutdown();
    }
}

pub async fn start(
    config: HubConfig,
    out: &Path,
    options: WriterOptions,
    subscribers: Vec<Subscriber>,
) -> Result<RunningHub, Error> {
    config.validate()?;
    if subscribers.len() > 64 {
        return Err(Error::Config("at most 64 live subscribers".into()));
    }
    let mut sources = Vec::new();
    let mut addresses = Vec::new();
    // Bind all UDP sockets before creating a session, so a bind failure creates no partial output.
    for entry in &config.sources {
        let source = source::prepare(entry.clone())
            .await
            .map_err(|source| Error::Io {
                path: out.to_owned(),
                source,
            })?;
        addresses.push(
            source
                .socket
                .as_ref()
                .map(|s| s.local_addr())
                .transpose()
                .map_err(|source| Error::Io {
                    path: out.to_owned(),
                    source,
                })?,
        );
        sources.push(source);
    }
    let writer = SessionWriter::create(out, config.source_info(), options)?;
    let clock_writer =
        ClockFitWriter::create(out, writer.manifest().session_id, config.clock_options())?;
    let config_text = toml::to_string_pretty(&config).map_err(|e| Error::Config(e.to_string()))?;
    fs::write(out.join("hub-config.toml"), config_text).at(out)?;
    let config = Arc::new(config);
    let (tx, rx) = mpsc::channel(config.queue_capacity);
    let (stop, stop_rx) = watch::channel(false);
    let (worker_done_tx, worker_done) = oneshot::channel();
    let (done_tx, done) = oneshot::channel();
    let clock = ArrivalClock(Instant::now());
    let dir = out.to_owned();
    let worker_config = config.clone();
    let worker_clock = clock.clone();
    std::thread::Builder::new()
        .name("csi-recorder".into())
        .spawn(move || {
            let result = Recorder::new(dir, worker_config, writer, clock_writer, subscribers)
                .and_then(|mut r| r.run(rx, worker_clock));
            let _ = worker_done_tx.send(result);
        })
        .map_err(|source| Error::Io {
            path: out.to_owned(),
            source,
        })?;
    let supervisor_stop = stop.clone();
    tokio::spawn(async move {
        let mut tasks = JoinSet::new();
        for (i, source) in sources.into_iter().enumerate() {
            tasks.spawn(source::run(
                source,
                i as u16,
                config.clone(),
                clock.clone(),
                tx.clone(),
                stop_rx.clone(),
            ));
        }
        let mut stop_rx = stop_rx;
        let mut worker_done = worker_done;
        let mut ticker = interval(Duration::from_millis(
            config.status_interval_ms.min(config.stale_ms).min(1000),
        ));
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut failure = None;
        let early_worker = loop {
            tokio::select! {
                _ = stop_rx.changed() => break None,
                result = &mut worker_done => break Some(result),
                result = tasks.join_next() => {
                    if !matches!(result, Some(Ok(Ok(())))) || !*stop_rx.borrow() {
                        failure = Some(format!("source task exited unexpectedly: {result:?}"));
                    }
                    break None;
                }
                _ = ticker.tick() => {
                    if tx.send(Input::Tick(clock.mono_ns())).await.is_err() { break None; }
                }
            }
        };
        let _ = supervisor_stop.send(true);
        while let Some(result) = tasks.join_next().await {
            if !matches!(result, Ok(Ok(()))) && failure.is_none() {
                failure = Some(format!("source task failed: {result:?}"));
            }
        }
        // No source can enqueue again. Dropping the final sender lets the worker drain to EOF.
        if failure.is_some() {
            tx.send(Input::Connection {
                source: 0,
                mono_ns: clock.mono_ns(),
                connected: false,
                error: failure.clone(),
            })
            .await
            .ok();
        }
        // Tell the worker that EOF is a failure rather than a clean seal.
        if let Some(reason) = &failure {
            tx.send(Input::Fatal(reason.clone())).await.ok();
        }
        drop(tx);
        let result = match early_worker {
            Some(result) => result,
            None => worker_done.await,
        }
        .map_err(|_| Error::Config("recorder thread panicked".into()))
        .and_then(|r| r);
        let _ = done_tx.send(result);
    });
    Ok(RunningHub {
        stop,
        done: Some(done),
        addresses,
    })
}

struct Recorder {
    dir: PathBuf,
    config: Arc<HubConfig>,
    writer: Option<SessionWriter>,
    clock_writer: ClockFitWriter,
    fitter: ClockFitter,
    inspector: Inspector,
    host: HostClock,
    subscribers: Vec<Subscriber>,
    events: BufWriter<File>,
    host_file: BufWriter<File>,
    status: HubStatus,
    last_status_ns: u64,
    handovers: usize,
    restarts: usize,
    reference: Option<String>,
}

fn json_line(file: &mut impl Write, value: &impl Serialize, path: &Path) -> Result<(), Error> {
    serde_json::to_writer(&mut *file, value).map_err(|source| Error::Manifest {
        path: path.to_owned(),
        source,
    })?;
    file.write_all(b"\n").at(path)
}

fn atomic_json(dir: &Path, name: &str, value: &impl Serialize) -> Result<(), Error> {
    let tmp = dir.join(format!("{name}.tmp"));
    let mut file = BufWriter::new(File::create(&tmp).at(&tmp)?);
    json_line(&mut file, value, &tmp)?;
    file.flush().at(&tmp)?;
    fs::rename(&tmp, dir.join(name)).at(dir)
}

impl Recorder {
    fn new(
        dir: PathBuf,
        config: Arc<HubConfig>,
        writer: SessionWriter,
        clock_writer: ClockFitWriter,
        subscribers: Vec<Subscriber>,
    ) -> Result<Self, Error> {
        let options = config.clock_options();
        let empty_clock = ClockFitter::new(options).finish().summary();
        let inspector = Inspector::live(InspectOptions {
            clock: options,
            gap_threshold_ns: config.stale_ms * 1_000_000,
            ..InspectOptions::default()
        });
        let status = HubStatus {
            schema: "csi-hub-status/1",
            session_id: writer.manifest().session_id,
            complete: false,
            mono_ns: 0,
            records: 0,
            sources: config
                .sources
                .iter()
                .map(|s| SourceStatus {
                    name: s.name.clone(),
                    state: "connecting",
                    records: 0,
                    valid_frames: 0,
                    decode_errors: 0,
                    framing_errors: 0,
                    foreign_datagrams: 0,
                    probe_write_errors: 0,
                    unmatched_echoes: 0,
                    connects: 0,
                    disconnects: 0,
                    stale_transitions: 0,
                    node: None,
                    epoch: None,
                    epoch_changes: 0,
                    last_valid_mono_ns: None,
                    last_error: None,
                    connected_at_ns: None,
                })
                .collect(),
            subscriber_drops: Vec::new(),
            queue_capacity: config.queue_capacity,
            max_queue_depth: 0,
            clock_sample_capacity: config.clock_sample_capacity,
            max_clock_samples: 0,
            clock_capacity_rejects: 0,
            health_history_evictions: 0,
            echoes_accepted: 0,
            echoes_unmapped: 0,
            echoes_rejected: 0,
            host_mapping: None,
            health: inspector.snapshot(empty_clock),
        };
        let events = BufWriter::new(File::create(dir.join("hub-events.jsonl")).at(&dir)?);
        let host_file = BufWriter::new(File::create(dir.join("host-clock.jsonl")).at(&dir)?);
        Ok(Self {
            fitter: ClockFitter::live(options, config.clock_sample_capacity),
            inspector,
            host: HostClock::default(),
            writer: Some(writer),
            clock_writer,
            subscribers,
            events,
            host_file,
            status,
            config,
            dir,
            last_status_ns: 0,
            handovers: 0,
            restarts: 0,
            reference: None,
        })
    }

    fn event(&mut self, value: serde_json::Value) -> Result<(), Error> {
        json_line(&mut self.events, &value, &self.dir)
    }

    fn persist_clock(&mut self) -> Result<(), Error> {
        if self.fitter.has_updates() {
            let updates = self.fitter.take_updates();
            for h in &updates.handovers {
                if self.reference.is_some() {
                    self.handovers += 1;
                } else {
                    self.reference = Some(h.reference.to_string());
                }
                self.restarts += usize::from(h.restarted);
            }
            self.clock_writer.append(updates)?;
        }
        Ok(())
    }

    fn snapshot(&mut self, now_ns: u64) -> Result<(), Error> {
        self.fitter.advance(now_ns);
        self.persist_clock()?;
        let mut summary = ClockFit {
            options: self.config.clock_options(),
            clocks: self.fitter.current_mappings().keys().copied().collect(),
            mappings: self.fitter.current_mappings().values().cloned().collect(),
            handovers: Vec::new(),
            unwrap_rejects: 0,
            radio_restarts: 0,
        }
        .summary();
        summary.reference = self.reference.clone();
        summary.handovers = self.handovers;
        summary.timeline_restarts = self.restarts;
        // Checkpoints also persist unwrap/restart counts when no new mappings were produced.
        let counters = self.fitter.take_updates();
        summary.unwrap_rejects = counters.unwrap_rejects;
        summary.radio_restarts = counters.radio_restarts;
        self.clock_writer.append(counters)?;
        self.inspector.advance_live(now_ns);
        self.status.health = self.inspector.snapshot(summary);
        self.status.health_history_evictions = self.inspector.history_evictions;
        self.status.clock_capacity_rejects = self.fitter.capacity_rejects;
        self.status.mono_ns = now_ns;
        self.status.subscriber_drops = self
            .subscribers
            .iter()
            .map(|s| s.dropped.load(Ordering::Relaxed))
            .collect();
        self.status.echoes_accepted = self.host.accepted;
        self.status.echoes_unmapped = self.host.unmapped;
        self.status.echoes_rejected = self.host.rejected;
        let timeline = self
            .fitter
            .current_mappings()
            .values()
            .next()
            .map(|m| m.timeline);
        self.status.host_mapping =
            self.host
                .mapping(now_ns, self.config.clock_options().window_ns, timeline);
        if let Some(mapping) = &self.status.host_mapping {
            json_line(
                &mut self.host_file,
                &serde_json::json!({
                    "schema": "csi-host-clock/1",
                    "session_id": self.status.session_id,
                    "mapping": mapping,
                }),
                &self.dir,
            )?;
        }
        for event in self.inspector.take_events() {
            json_line(&mut self.events, &event, &self.dir)?;
        }
        self.events.flush().at(&self.dir)?;
        self.host_file.flush().at(&self.dir)?;
        self.writer.as_mut().unwrap().flush()?;
        atomic_json(&self.dir, STATUS_FILE, &self.status)?;
        self.last_status_ns = now_ns;
        Ok(())
    }

    fn run(
        &mut self,
        mut rx: mpsc::Receiver<Input>,
        clock: ArrivalClock,
    ) -> Result<HubStatus, Error> {
        self.snapshot(0)?;
        let mut fatal = None;
        while let Some(input) = rx.blocking_recv() {
            self.status.max_queue_depth = self.status.max_queue_depth.max(rx.len());
            match input {
                Input::Record {
                    record,
                    correlated_echo,
                } => {
                    self.writer.as_mut().unwrap().write(&record)?;
                    self.status.records += 1;
                    self.inspector.push(&record);
                    self.fitter.push(&record);
                    self.status.max_clock_samples = self
                        .status
                        .max_clock_samples
                        .max(self.fitter.buffered_samples());
                    self.persist_clock()?;
                    let source = &mut self.status.sources[usize::from(record.source)];
                    source.records += 1;
                    if record.kind == RecordKind::StreamError {
                        source.framing_errors += 1;
                    }
                    if record.kind == RecordKind::Frame {
                        match frame::decode(&record.payload) {
                            Ok((header, message)) => {
                                source.valid_frames += 1;
                                source.last_valid_mono_ns = Some(record.mono_ns);
                                let prior_state = source.state;
                                source.state = "active";
                                let node = header.node.to_string();
                                let new_epoch = source.node.as_ref().is_some_and(|n| n != &node)
                                    || source.epoch.is_some_and(|e| e != header.epoch);
                                if new_epoch {
                                    source.epoch_changes += 1;
                                }
                                source.node = Some(node);
                                source.epoch = Some(header.epoch);
                                if let Message::TimeEcho(echo) = message {
                                    if correlated_echo {
                                        self.host.push(
                                            &header,
                                            echo,
                                            record.mono_ns,
                                            record.wall_ns,
                                            &self.fitter,
                                        );
                                    } else {
                                        source.unmatched_echoes += 1;
                                    }
                                }
                                if prior_state != "active" || new_epoch {
                                    let value = serde_json::json!({"type":"source_active", "mono_ns":record.mono_ns, "source":record.source, "node":source.node, "epoch":source.epoch, "new_epoch":new_epoch});
                                    self.event(value)?;
                                }
                            }
                            Err(_) => source.decode_errors += 1,
                        }
                    }
                    let record = Arc::new(record);
                    for subscriber in &self.subscribers {
                        if subscriber.tx.try_send(record.clone()).is_err() {
                            subscriber.dropped.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Input::Connection {
                    source,
                    mono_ns,
                    connected,
                    error,
                } => {
                    let s = &mut self.status.sources[usize::from(source)];
                    if connected {
                        s.connects += 1;
                        s.state = "awaiting_data";
                        s.connected_at_ns = Some(mono_ns);
                        s.last_valid_mono_ns = None;
                    } else {
                        s.disconnects += 1;
                        s.state = "disconnected";
                    }
                    s.last_error = error.clone();
                    self.event(serde_json::json!({"type":"source_connection", "source":source, "mono_ns":mono_ns,"connected":connected,"error":error}))?;
                }
                Input::Foreign(source) => {
                    self.status.sources[usize::from(source)].foreign_datagrams += 1
                }
                Input::ProbeWriteError(source) => {
                    self.status.sources[usize::from(source)].probe_write_errors += 1
                }
                Input::Fatal(reason) => fatal = Some(reason),
                Input::Tick(now) => {
                    for i in 0..self.status.sources.len() {
                        let s = &mut self.status.sources[i];
                        if matches!(s.state, "active" | "awaiting_data")
                            && s.last_valid_mono_ns
                                .or(s.connected_at_ns)
                                .is_some_and(|last| {
                                    now.saturating_sub(last) >= self.config.stale_ms * 1_000_000
                                })
                        {
                            s.state = "stale";
                            s.stale_transitions += 1;
                            self.event(
                                serde_json::json!({"type":"source_stale","source":i,"mono_ns":now}),
                            )?;
                        }
                    }
                    if now.saturating_sub(self.last_status_ns)
                        >= self.config.status_interval_ms * 1_000_000
                    {
                        self.snapshot(now)?;
                    }
                }
            }
        }
        let now = clock.mono_ns();
        self.fitter.refit_now(now);
        self.inspector.finalize();
        self.snapshot(now)?;
        self.clock_writer.sync()?;
        self.events.get_ref().sync_all().at(&self.dir)?;
        self.host_file.get_ref().sync_all().at(&self.dir)?;
        if let Some(reason) = fatal {
            return Err(Error::Config(reason));
        }
        // Seal only after derived files have flushed; failures above leave an incomplete session.
        self.writer.take().unwrap().finish()?;
        self.status.complete = true;
        atomic_json(&self.dir, STATUS_FILE, &self.status)?;
        tracing::info!(
            records = self.status.records,
            "hub stopped and session sealed"
        );
        Ok(self.status.clone())
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use crate::live::config::{SourceConfig, Transport};
    use crate::session::SessionReader;
    use csi_protocol::{Csi, Header, NodeId, Probe, RxMeta};

    #[test]
    fn regression_completed_status_finalizes_frames_captures_probes_and_events() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("session");
        let config = Arc::new(HubConfig {
            sources: vec![SourceConfig {
                name: "receiver".into(),
                transport: Transport::Serial {
                    path: "unused".into(),
                    baud: 921600,
                },
            }],
            ..HubConfig::default()
        });
        let writer =
            SessionWriter::create(&dir, config.source_info(), WriterOptions::default()).unwrap();
        let clock_file =
            ClockFitWriter::create(&dir, writer.manifest().session_id, config.clock_options())
                .unwrap();
        let mut recorder = Recorder::new(dir.clone(), config, writer, clock_file, vec![]).unwrap();
        let (tx, rx) = mpsc::channel(8);
        for seq in [0, 2, 3, 4] {
            let mut payload = vec![0; csi_protocol::MAX_FRAME_LEN];
            let n = frame::encode(
                &Header {
                    node: NodeId([2; 6]),
                    epoch: 1,
                    seq,
                },
                &Message::Csi(Csi {
                    capture_seq: seq,
                    node_time_us: 1_100_000 + u64::from(seq) * 1000,
                    rx_timestamp_us: 1_000_000 + seq * 1000,
                    source_mac: [3; 6],
                    probe: Some(Probe {
                        tx: NodeId([3; 6]),
                        epoch: 1,
                        schedule: 0,
                        seq,
                    }),
                    first_word_invalid: false,
                    rx: RxMeta::default(),
                    data: &[],
                }),
                &mut payload,
            )
            .unwrap();
            payload.truncate(n);
            tx.blocking_send(Input::Record {
                record: Record {
                    mono_ns: u64::from(seq) * 1_000_000,
                    wall_ns: 0,
                    source: 0,
                    kind: RecordKind::Frame,
                    payload,
                },
                correlated_echo: false,
            })
            .unwrap();
        }
        drop(tx);
        let status = recorder
            .run(rx, ArrivalClock(Instant::now() - Duration::from_millis(20)))
            .unwrap();
        assert!(status.complete);
        recorder.inspector.finalize();
        let repeated = recorder.inspector.snapshot(status.health.clock.clone());
        assert_eq!(repeated.event_counts, status.health.event_counts);
        assert!(
            repeated.events.is_empty(),
            "finalization must not re-emit events"
        );
        let epoch = &status.health.nodes[0].epochs[0];
        assert_eq!(
            (
                epoch.frames_missing,
                epoch.captures_missing,
                epoch.links[0].probes_missing
            ),
            (1, 1, 1)
        );
        let mut offline = Inspector::new(InspectOptions::default());
        for record in SessionReader::open(&dir).unwrap() {
            offline.push(&record.unwrap());
        }
        let offline = offline.finish();
        let offline_epoch = &offline.nodes[0].epochs[0];
        assert_eq!(epoch.frames_missing, offline_epoch.frames_missing);
        assert_eq!(epoch.captures_missing, offline_epoch.captures_missing);
        assert_eq!(
            epoch.links[0].probes_missing,
            offline_epoch.links[0].probes_missing
        );
        let written: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join(STATUS_FILE)).unwrap()).unwrap();
        assert_eq!(written["complete"], true);
        assert_eq!(
            written["health"]["nodes"][0]["epochs"][0]["frames_missing"],
            1
        );
        let events = fs::read_to_string(dir.join("hub-events.jsonl")).unwrap();
        assert!(events.lines().any(|line| {
            let event: serde_json::Value = serde_json::from_str(line).unwrap();
            event["type"] == "frames_missing" && event["count"] == 1
        }));
    }
}
