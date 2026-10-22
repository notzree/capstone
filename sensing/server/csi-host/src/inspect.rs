//! Per-source, per-node and per-link health from a record stream. Fed one record at a time so the
//! same accounting can run on a recording or a live hub.

use std::collections::BTreeMap;

use csi_protocol::{Counters, Csi, DecodeError, Header, Hello, Message, NodeId, RxGain, frame};
use serde::Serialize;

use crate::clock::{ClockFitter, ClockSummary, FitOptions, unwrap_radio_us};
use crate::histogram::Histogram;
use crate::seq::{Arrival, SeqTracker};
use crate::{Record, RecordKind, StreamError};

#[derive(Debug, Clone, Copy)]
pub struct InspectOptions {
    /// Silences and link gaps longer than this are reported.
    pub gap_threshold_ns: u64,
    /// Events beyond this many are counted but not kept.
    pub max_events: usize,
    pub clock: FitOptions,
}

impl Default for InspectOptions {
    fn default() -> Self {
        Self {
            gap_threshold_ns: 100_000_000,
            max_events: 1000,
            clock: FitOptions::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HealthEvent {
    /// First frame of a node epoch; `previous` is set when this is a reboot.
    NodeEpoch {
        mono_ns: u64,
        node: String,
        epoch: u32,
        previous: Option<u32>,
    },
    /// No frames from a node for longer than the gap threshold.
    Silence {
        mono_ns: u64,
        node: String,
        silent_ms: f64,
    },
    FramesMissing {
        mono_ns: u64,
        node: String,
        epoch: u32,
        first_seq: u32,
        count: u64,
    },
    DuplicateFrame {
        mono_ns: u64,
        node: String,
        epoch: u32,
        seq: u32,
    },
    LateFrame {
        mono_ns: u64,
        node: String,
        epoch: u32,
        seq: u32,
    },
    /// An in-order CSI record's node time went backwards without an epoch change.
    ClockBackwards {
        mono_ns: u64,
        node: String,
        epoch: u32,
    },
    /// A link reported a CSI layout different from the ones it reported before.
    LayoutChange {
        mono_ns: u64,
        node: String,
        tx: String,
        layout: Layout,
    },
    DecodeError {
        mono_ns: u64,
        source: u16,
        error: &'static str,
    },
    StreamError {
        mono_ns: u64,
        source: u16,
        error: &'static str,
    },
}

impl HealthEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::NodeEpoch { .. } => "node_epoch",
            Self::Silence { .. } => "silence",
            Self::FramesMissing { .. } => "frames_missing",
            Self::DuplicateFrame { .. } => "duplicate_frame",
            Self::LateFrame { .. } => "late_frame",
            Self::ClockBackwards { .. } => "clock_backwards",
            Self::LayoutChange { .. } => "layout_change",
            Self::DecodeError { .. } => "decode_error",
            Self::StreamError { .. } => "stream_error",
        }
    }

    pub fn mono_ns(&self) -> u64 {
        match self {
            Self::NodeEpoch { mono_ns, .. }
            | Self::Silence { mono_ns, .. }
            | Self::FramesMissing { mono_ns, .. }
            | Self::DuplicateFrame { mono_ns, .. }
            | Self::LateFrame { mono_ns, .. }
            | Self::ClockBackwards { mono_ns, .. }
            | Self::LayoutChange { mono_ns, .. }
            | Self::DecodeError { mono_ns, .. }
            | Self::StreamError { mono_ns, .. } => *mono_ns,
        }
    }
}

/// The record properties that determine how CSI bytes map to subcarriers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Layout {
    pub sig_mode: u8,
    pub cwb: u8,
    pub secondary_channel: u8,
    pub stbc: u8,
    pub len: u16,
    pub first_word_invalid: bool,
}

impl Layout {
    fn of(csi: &Csi<'_>) -> Self {
        Self {
            sig_mode: csi.rx.sig_mode,
            cwb: csi.rx.cwb,
            secondary_channel: csi.rx.secondary_channel,
            stbc: csi.rx.stbc,
            len: csi.data.len() as u16,
            first_word_invalid: csi.first_word_invalid,
        }
    }
}

pub fn decode_error_name(error: DecodeError) -> &'static str {
    match error {
        DecodeError::Truncated => "truncated",
        DecodeError::TrailingBytes => "trailing_bytes",
        DecodeError::UnsupportedVersion(_) => "unsupported_version",
        DecodeError::UnknownKind(_) => "unknown_kind",
        DecodeError::InvalidField(_) => "invalid_field",
        DecodeError::CsiTooLong(_) => "csi_too_long",
        DecodeError::BadCrc => "bad_crc",
        DecodeError::BadCobs => "bad_cobs",
        DecodeError::Oversized => "oversized",
        DecodeError::BadMagic => "bad_magic",
    }
}

#[derive(Default)]
struct SourceStats {
    records: u64,
    frames: u64,
    versions: BTreeMap<u8, u64>,
    decode_errors: BTreeMap<&'static str, u64>,
    stream_errors: BTreeMap<&'static str, u64>,
    unknown_records: u64,
}

#[derive(Default)]
struct LinkStats {
    records: u64,
    last_host_mono_ns: u64,
    probes: BTreeMap<u32, SeqTracker>,
    /// Unwrapped radio capture times, which unlike callback-time `node_time_us` carry no
    /// callback latency.
    last_capture_us: Option<u64>,
    last_node_us: Option<u64>,
    last_radio_offset_us: Option<u64>,
    /// Sum of capture intervals within radio runs; no interval bridges a restart.
    capture_span_us: u64,
    radio_restarts: u64,
    intervals: Histogram,
    long_gaps: u64,
    layouts: BTreeMap<Layout, u64>,
    gain_valid: u64,
    gain_steps: u64,
    last_gain: Option<RxGain>,
}

struct EpochStats {
    epoch: u32,
    first_mono_ns: u64,
    last_mono_ns: u64,
    frames: u64,
    kinds: BTreeMap<&'static str, u64>,
    seq: SeqTracker,
    captures: SeqTracker,
    hello: Option<Hello>,
    counters: Option<Counters>,
    last_csi_node_us: Option<u64>,
    clock_backwards: u64,
    time_echoes: u64,
    min_rtt_ns: Option<u64>,
    links: BTreeMap<NodeId, LinkStats>,
    unattributed: u64,
}

#[derive(Default)]
struct NodeStats {
    last_arrival_ns: Option<u64>,
    epochs: Vec<EpochStats>,
}

pub struct Inspector {
    options: InspectOptions,
    records: u64,
    first_mono_ns: Option<u64>,
    last_mono_ns: u64,
    sources: BTreeMap<u16, SourceStats>,
    nodes: BTreeMap<NodeId, NodeStats>,
    events: Vec<HealthEvent>,
    event_counts: BTreeMap<&'static str, u64>,
    clock: ClockFitter,
    live: bool,
    pub history_evictions: u64,
}

impl Inspector {
    pub fn new(options: InspectOptions) -> Self {
        Self {
            clock: ClockFitter::new(options.clock),
            options,
            records: 0,
            first_mono_ns: None,
            last_mono_ns: 0,
            sources: BTreeMap::new(),
            nodes: BTreeMap::new(),
            events: Vec::new(),
            event_counts: BTreeMap::new(),
            live: false,
            history_evictions: 0,
        }
    }

    /// Live reports keep at most 256 nodes, two epochs/node, 64 links/epoch,
    /// two transmitter epochs/link and 16 layouts/link. Raw recordings retain everything.
    pub fn live(options: InspectOptions) -> Self {
        Self {
            live: true,
            ..Self::new(options)
        }
    }

    pub fn advance_live(&mut self, mono_ns: u64) {
        if self.live {
            self.last_mono_ns = self.last_mono_ns.max(mono_ns);
        }
    }

    pub fn take_events(&mut self) -> Vec<HealthEvent> {
        std::mem::take(&mut self.events)
    }

    fn event(&mut self, event: HealthEvent) {
        *self.event_counts.entry(event.name()).or_default() += 1;
        if self.events.len() < self.options.max_events {
            self.events.push(event);
        }
    }

    pub fn push(&mut self, record: &Record) {
        self.records += 1;
        self.first_mono_ns.get_or_insert(record.mono_ns);
        self.last_mono_ns = self.last_mono_ns.max(record.mono_ns);
        let source = self.sources.entry(record.source).or_default();
        source.records += 1;
        match record.kind {
            RecordKind::Frame => match frame::decode(&record.payload) {
                Ok((header, message)) => {
                    source.frames += 1;
                    *source.versions.entry(record.payload[0]).or_default() += 1;
                    self.on_message(record.mono_ns, header, &message);
                }
                Err(e) => {
                    let error = decode_error_name(e);
                    *source.decode_errors.entry(error).or_default() += 1;
                    self.event(HealthEvent::DecodeError {
                        mono_ns: record.mono_ns,
                        source: record.source,
                        error,
                    });
                }
            },
            RecordKind::StreamError => {
                let error = record
                    .payload
                    .first()
                    .and_then(|&code| StreamError::from_code(code))
                    .map_or("unknown", StreamError::name);
                *source.stream_errors.entry(error).or_default() += 1;
                self.event(HealthEvent::StreamError {
                    mono_ns: record.mono_ns,
                    source: record.source,
                    error,
                });
            }
            RecordKind::Unknown(_) => source.unknown_records += 1,
        }
    }

    fn on_message(&mut self, mono_ns: u64, header: Header, message: &Message<'_>) {
        let threshold_ns = self.options.gap_threshold_ns;
        let name = header.node.to_string();
        let mut events = Vec::new();
        if self.live && !self.nodes.contains_key(&header.node) && self.nodes.len() >= 256 {
            self.history_evictions += 1;
            return;
        }
        let node = self.nodes.entry(header.node).or_default();

        if let Some(previous) = node.last_arrival_ns {
            let silent = mono_ns.saturating_sub(previous);
            if silent > threshold_ns {
                events.push(HealthEvent::Silence {
                    mono_ns,
                    node: name.clone(),
                    silent_ms: silent as f64 / 1e6,
                });
            }
        }
        node.last_arrival_ns = Some(node.last_arrival_ns.map_or(mono_ns, |p| p.max(mono_ns)));

        let index = match node.epochs.iter().position(|e| e.epoch == header.epoch) {
            Some(index) => index,
            None => {
                events.push(HealthEvent::NodeEpoch {
                    mono_ns,
                    node: name.clone(),
                    epoch: header.epoch,
                    previous: node.epochs.last().map(|e| e.epoch),
                });
                if self.live && node.epochs.len() >= 2 {
                    node.epochs.remove(0);
                    self.history_evictions += 1;
                }
                node.epochs.push(EpochStats::new(header.epoch, mono_ns));
                node.epochs.len() - 1
            }
        };
        let epoch = &mut node.epochs[index];
        epoch.last_mono_ns = epoch.last_mono_ns.max(mono_ns);

        let arrival = epoch.seq.push(header.seq, mono_ns);
        match arrival {
            Arrival::Duplicate => events.push(HealthEvent::DuplicateFrame {
                mono_ns,
                node: name.clone(),
                epoch: header.epoch,
                seq: header.seq,
            }),
            Arrival::Late => events.push(HealthEvent::LateFrame {
                mono_ns,
                node: name.clone(),
                epoch: header.epoch,
                seq: header.seq,
            }),
            _ => {}
        }
        for missing in epoch.seq.take_finalized() {
            events.push(HealthEvent::FramesMissing {
                mono_ns: missing.seen_at,
                node: name.clone(),
                epoch: header.epoch,
                first_seq: missing.first,
                count: missing.count,
            });
        }

        if arrival.is_new() {
            epoch.frames += 1;
            *epoch.kinds.entry(kind_name(message)).or_default() += 1;
            if self.live
                && let Message::Csi(csi) = message
                && let Some(probe) = csi.probe
            {
                if !epoch.links.contains_key(&probe.tx) && epoch.links.len() >= 64 {
                    self.history_evictions += 1;
                    return;
                }
                if let Some(link) = epoch.links.get_mut(&probe.tx) {
                    if !link.probes.contains_key(&probe.epoch) && link.probes.len() >= 2 {
                        let key = *link.probes.keys().next().unwrap();
                        link.probes.remove(&key);
                        self.history_evictions += 1;
                    }
                    let layout = Layout::of(csi);
                    if !link.layouts.contains_key(&layout) && link.layouts.len() >= 16 {
                        let key = *link.layouts.keys().next().unwrap();
                        link.layouts.remove(&key);
                        self.history_evictions += 1;
                    }
                }
            }
            epoch.on_message(mono_ns, arrival, message, &self.options, &name, &mut events);
            if !self.live
                && let Message::Csi(csi) = message
            {
                self.clock.push_csi(mono_ns, &header, csi);
            }
        }

        for event in events {
            self.event(event);
        }
    }

    pub fn finish(mut self) -> Report {
        let clock = std::mem::replace(&mut self.clock, ClockFitter::new(self.options.clock))
            .finish()
            .summary();
        self.finalize();
        self.snapshot(clock)
    }

    /// Close pending gaps after all inputs have drained. Also used before publishing
    /// a completed live status; repeated calls do not recount gaps or events.
    pub fn finalize(&mut self) {
        let mut events = Vec::new();
        for (node, stats) in &mut self.nodes {
            for epoch in &mut stats.epochs {
                epoch.seq.finish();
                for missing in epoch.seq.take_finalized() {
                    events.push(HealthEvent::FramesMissing {
                        mono_ns: missing.seen_at,
                        node: node.to_string(),
                        epoch: epoch.epoch,
                        first_seq: missing.first,
                        count: missing.count,
                    });
                }
                epoch.captures.finish();
                epoch.captures.take_finalized();
                for link in epoch.links.values_mut() {
                    for tracker in link.probes.values_mut() {
                        tracker.finish();
                        tracker.take_finalized();
                    }
                }
            }
        }
        for event in events {
            self.event(event);
        }
        self.events.sort_by_key(HealthEvent::mono_ns);
    }

    /// Snapshot without finalizing open sequence gaps; those may still arrive late.
    pub fn snapshot(&self, clock: ClockSummary) -> Report {
        let events_omitted = self
            .event_counts
            .values()
            .sum::<u64>()
            .saturating_sub(self.events.len() as u64);

        let first = self.first_mono_ns.unwrap_or(0);
        let seconds = |ns: u64| ns.saturating_sub(first) as f64 / 1e9;
        Report {
            records: self.records,
            span_s: seconds(self.last_mono_ns),
            sources: self
                .sources
                .iter()
                .map(|(&index, s)| SourceReport {
                    index,
                    records: s.records,
                    frames: s.frames,
                    versions: s.versions.clone(),
                    decode_errors: s.decode_errors.clone(),
                    stream_errors: s.stream_errors.clone(),
                    unknown_records: s.unknown_records,
                })
                .collect(),
            nodes: self
                .nodes
                .iter()
                .map(|(node, stats)| NodeReport {
                    node: node.to_string(),
                    epochs: stats
                        .epochs
                        .iter()
                        .map(|e| e.report(&seconds, &self.options, self.last_mono_ns))
                        .collect(),
                })
                .collect(),
            event_counts: self.event_counts.clone(),
            events_omitted,
            events: self.events.clone(),
            clock,
        }
    }
}

fn kind_name(message: &Message<'_>) -> &'static str {
    match message {
        Message::Hello(_) => "hello",
        Message::Csi(_) => "csi",
        Message::Counters(_) => "counters",
        Message::TimeEcho(_) => "time_echo",
        Message::TimeProbe(_) => "time_probe",
    }
}

impl EpochStats {
    fn new(epoch: u32, mono_ns: u64) -> Self {
        Self {
            epoch,
            first_mono_ns: mono_ns,
            last_mono_ns: mono_ns,
            frames: 0,
            kinds: BTreeMap::new(),
            seq: SeqTracker::default(),
            captures: SeqTracker::default(),
            hello: None,
            counters: None,
            last_csi_node_us: None,
            clock_backwards: 0,
            time_echoes: 0,
            min_rtt_ns: None,
            links: BTreeMap::new(),
            unattributed: 0,
        }
    }

    fn on_message(
        &mut self,
        mono_ns: u64,
        arrival: Arrival,
        message: &Message<'_>,
        options: &InspectOptions,
        node: &str,
        events: &mut Vec<HealthEvent>,
    ) {
        let threshold_ns = options.gap_threshold_ns;
        match message {
            Message::Hello(hello) => self.hello = Some(*hello),
            Message::Counters(counters) => {
                if self
                    .counters
                    .is_none_or(|c| counters.node_time_us >= c.node_time_us)
                {
                    self.counters = Some(*counters);
                }
            }
            Message::TimeEcho(echo) => {
                self.time_echoes += 1;
                if let Some(rtt) = echo
                    .node_tx_us
                    .checked_sub(echo.node_rx_us)
                    .and_then(|t| t.checked_mul(1000))
                    .and_then(|t| mono_ns.checked_sub(echo.host_time)?.checked_sub(t))
                {
                    self.min_rtt_ns = Some(self.min_rtt_ns.map_or(rtt, |m| m.min(rtt)));
                }
            }
            Message::TimeProbe(_) => {}
            Message::Csi(csi) => {
                if arrival != Arrival::Late {
                    if self
                        .last_csi_node_us
                        .is_some_and(|last| csi.node_time_us < last)
                    {
                        self.clock_backwards += 1;
                        events.push(HealthEvent::ClockBackwards {
                            mono_ns,
                            node: node.to_owned(),
                            epoch: self.epoch,
                        });
                    }
                    self.last_csi_node_us = Some(csi.node_time_us);
                }
                self.captures.push(csi.capture_seq, mono_ns);
                self.captures.take_finalized();
                let Some(probe) = csi.probe else {
                    self.unattributed += 1;
                    return;
                };
                let link = self.links.entry(probe.tx).or_default();
                if let Some(layout) = link.push(
                    csi,
                    probe.epoch,
                    probe.seq,
                    threshold_ns / 1000,
                    mono_ns,
                    options.clock.radio_reset_us,
                ) {
                    events.push(HealthEvent::LayoutChange {
                        mono_ns,
                        node: node.to_owned(),
                        tx: probe.tx.to_string(),
                        layout,
                    });
                }
            }
        }
    }

    fn report(
        &self,
        seconds: &dyn Fn(u64) -> f64,
        options: &InspectOptions,
        now_ns: u64,
    ) -> EpochReport {
        EpochReport {
            epoch: self.epoch,
            first_s: seconds(self.first_mono_ns),
            last_s: seconds(self.last_mono_ns),
            frames: self.frames,
            kinds: self.kinds.clone(),
            frames_missing: self.seq.missing,
            frames_duplicate: self.seq.duplicates,
            frames_late: self.seq.late,
            frames_stale: self.seq.stale,
            captures_missing: self.captures.missing,
            hello: self.hello.as_ref().map(HelloReport::from),
            counters: self.counters.as_ref().map(CountersReport::from),
            clock_backwards: self.clock_backwards,
            time_echoes: self.time_echoes,
            min_rtt_us: self.min_rtt_ns.map(|ns| ns as f64 / 1e3),
            unattributed_csi: self.unattributed,
            links: self
                .links
                .iter()
                .map(|(tx, link)| link.report(tx, options, now_ns))
                .collect(),
        }
    }
}

impl LinkStats {
    /// Returns the layout if it differs from every layout this link reported before.
    fn push(
        &mut self,
        csi: &Csi<'_>,
        tx_epoch: u32,
        probe_seq: u32,
        threshold_us: u64,
        mono_ns: u64,
        radio_reset_us: u64,
    ) -> Option<Layout> {
        self.records += 1;
        self.last_host_mono_ns = self.last_host_mono_ns.max(mono_ns);
        let tracker = self.probes.entry(tx_epoch).or_default();
        tracker.push(probe_seq, csi.node_time_us);
        tracker.take_finalized();
        let gain = csi.rx.gain;
        if gain.is_some() {
            self.gain_valid += 1;
        }
        let unwrapped = unwrap_radio_us(csi.rx_timestamp_us, csi.node_time_us);
        let capture_us = unwrapped.map_or(csi.node_time_us, |(radio_us, _)| radio_us);
        let offset = unwrapped.map(|(_, offset)| offset);
        // Node time orders captures across radio resets. A late pre-reset frame must
        // not change the new run's origin, interval baseline or gain baseline.
        let in_order = self.last_node_us.is_none_or(|last| csi.node_time_us > last);
        if in_order {
            let restarted = self
                .last_radio_offset_us
                .zip(offset)
                .is_some_and(|(last, now)| last.abs_diff(now) > radio_reset_us)
                || self.last_capture_us.is_some_and(|last| capture_us < last);
            if restarted {
                self.radio_restarts += 1;
            } else if let Some(last) = self.last_capture_us
                && capture_us > last
            {
                let interval = capture_us - last;
                self.capture_span_us += interval;
                self.intervals.record(interval);
                if interval > threshold_us {
                    self.long_gaps += 1;
                }
            }
            self.last_node_us = Some(csi.node_time_us);
            self.last_radio_offset_us = offset;
            self.last_capture_us = Some(capture_us);
            if let Some(gain) = gain {
                if self.last_gain.is_some_and(|last| last != gain) {
                    self.gain_steps += 1;
                }
                self.last_gain = Some(gain);
            }
        }
        let layout = Layout::of(csi);
        let count = self.layouts.entry(layout).or_default();
        *count += 1;
        let new = *count == 1;
        (new && self.layouts.len() > 1).then_some(layout)
    }

    fn report(&self, tx: &NodeId, options: &InspectOptions, now_ns: u64) -> LinkReport {
        let sum = |f: fn(&SeqTracker) -> u64| self.probes.values().map(f).sum::<u64>();
        let span_us = self.capture_span_us;
        LinkReport {
            tx: tx.to_string(),
            last_host_mono_ns: self.last_host_mono_ns,
            stale: now_ns.saturating_sub(self.last_host_mono_ns) >= options.gap_threshold_ns,
            records: self.records,
            radio_restarts: self.radio_restarts,
            rate_hz: (span_us > 0).then(|| self.intervals.total() as f64 * 1e6 / span_us as f64),
            gain_valid: self.gain_valid,
            gain_valid_fraction: self.gain_valid as f64 / self.records as f64,
            gain_steps: self.gain_steps,
            gain_steps_per_minute: (span_us > 0)
                .then(|| self.gain_steps as f64 * 60e6 / span_us as f64),
            probes_missing: sum(|t| t.missing),
            probes_duplicate: sum(|t| t.duplicates),
            probes_late: sum(|t| t.late),
            tx_epochs: self.probes.len(),
            interval_us: (self.intervals.total() > 0).then(|| Percentiles {
                p50: self.intervals.quantile(0.50).unwrap_or(0),
                p95: self.intervals.quantile(0.95).unwrap_or(0),
                p99: self.intervals.quantile(0.99).unwrap_or(0),
                max: self.intervals.max(),
            }),
            gaps_over_threshold: self.long_gaps,
            gap_threshold_ms: options.gap_threshold_ns as f64 / 1e6,
            layouts: self
                .layouts
                .iter()
                .map(|(&layout, &records)| LayoutCount { layout, records })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub records: u64,
    pub span_s: f64,
    pub sources: Vec<SourceReport>,
    pub nodes: Vec<NodeReport>,
    pub event_counts: BTreeMap<&'static str, u64>,
    pub events: Vec<HealthEvent>,
    pub events_omitted: u64,
    /// Shared-probe clock alignment.
    pub clock: ClockSummary,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceReport {
    pub index: u16,
    pub records: u64,
    pub frames: u64,
    /// Decoded frames by protocol version.
    pub versions: BTreeMap<u8, u64>,
    pub decode_errors: BTreeMap<&'static str, u64>,
    pub stream_errors: BTreeMap<&'static str, u64>,
    pub unknown_records: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct NodeReport {
    pub node: String,
    pub epochs: Vec<EpochReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EpochReport {
    pub epoch: u32,
    pub first_s: f64,
    pub last_s: f64,
    /// Distinct frames received (duplicates excluded).
    pub frames: u64,
    pub kinds: BTreeMap<&'static str, u64>,
    pub frames_missing: u64,
    pub frames_duplicate: u64,
    pub frames_late: u64,
    pub frames_stale: u64,
    pub captures_missing: u64,
    pub hello: Option<HelloReport>,
    /// The node's own counters, from its latest report in this epoch.
    pub counters: Option<CountersReport>,
    pub clock_backwards: u64,
    pub time_echoes: u64,
    pub min_rtt_us: Option<f64>,
    pub unattributed_csi: u64,
    pub links: Vec<LinkReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkReport {
    pub tx: String,
    pub last_host_mono_ns: u64,
    pub stale: bool,
    pub records: u64,
    /// Radio-origin changes observed on captures in node-time order.
    pub radio_restarts: u64,
    /// Records per second of capture time.
    pub rate_hz: Option<f64>,
    /// Records carrying a valid receiver gain.
    pub gain_valid: u64,
    pub gain_valid_fraction: f64,
    /// Gain changes between consecutive valid-gain records in node-time order.
    pub gain_steps: u64,
    pub gain_steps_per_minute: Option<f64>,
    pub probes_missing: u64,
    pub probes_duplicate: u64,
    pub probes_late: u64,
    pub tx_epochs: usize,
    /// Capture-time spacing between consecutive records.
    pub interval_us: Option<Percentiles>,
    pub gaps_over_threshold: u64,
    pub gap_threshold_ms: f64,
    pub layouts: Vec<LayoutCount>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Percentiles {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
    pub max: u64,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct LayoutCount {
    #[serde(flatten)]
    pub layout: Layout,
    pub records: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HelloReport {
    pub chip: String,
    pub tx: bool,
    pub rx: bool,
    pub firmware_version: String,
    pub firmware_commit: String,
    pub channel: u8,
    pub secondary_channel: u8,
    pub csi_config: String,
    pub pool_capacity: u16,
}

impl From<&Hello> for HelloReport {
    fn from(h: &Hello) -> Self {
        let [major, minor, patch] = h.firmware_version;
        let c = &h.csi_config;
        let flags: Vec<&str> = [
            (c.lltf, "lltf"),
            (c.htltf, "htltf"),
            (c.stbc_htltf2, "stbc_htltf2"),
            (c.ltf_merge, "ltf_merge"),
            (c.channel_filter, "channel_filter"),
            (c.manu_scale, "manu_scale"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect();
        Self {
            chip: format!("{:?}", h.chip),
            tx: h.roles.tx,
            rx: h.roles.rx,
            firmware_version: format!("{major}.{minor}.{patch}"),
            firmware_commit: h
                .firmware_commit
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
            channel: h.channel,
            secondary_channel: h.secondary_channel,
            csi_config: format!("{} shift {}", flags.join("+"), c.shift),
            pool_capacity: h.pool_capacity,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct CountersReport {
    pub node_time_us: u64,
    pub tx_requested: u64,
    pub tx_accepted: u64,
    pub csi_callbacks: u64,
    pub csi_filtered: u64,
    pub csi_captured: u64,
    pub pool_drops: u64,
    pub export_drops: u64,
    pub export_bytes: u64,
    pub pool_high_water: u16,
}

impl From<&Counters> for CountersReport {
    fn from(c: &Counters) -> Self {
        Self {
            node_time_us: c.node_time_us,
            tx_requested: c.tx_requested,
            tx_accepted: c.tx_accepted,
            csi_callbacks: c.csi_callbacks,
            csi_filtered: c.csi_filtered,
            csi_captured: c.csi_captured,
            pool_drops: c.pool_drops,
            export_drops: c.export_drops,
            export_bytes: c.export_bytes,
            pool_high_water: c.pool_high_water,
        }
    }
}
