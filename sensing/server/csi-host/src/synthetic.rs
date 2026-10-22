//! Deterministic synthetic sensing nodes for exercising transport, recording and health
//! accounting. The CSI bytes are noise: a synthetic session says nothing about sensing.
//!
//! Each receiver node hears every transmitter's probes and exports frames to the host over its own
//! source, with injected faults at each layer. [`Truth`] records what was injected and what an
//! observer of the resulting records can detect.

use std::collections::{BTreeMap, VecDeque};

use csi_protocol::{
    Chip, Counters, Csi, CsiConfig, Header, Hello, MAX_CSI_LEN, MAX_FRAME_LEN, Message, NodeId,
    Probe, Roles, RxGain, RxMeta, TimeEcho, frame,
};
use serde::{Deserialize, Serialize};

use crate::manifest::SourceInfo;
use crate::{Error, Record, RecordKind, StreamError};

const SECOND_NS: u64 = 1_000_000_000;
const REBOOT_DOWN_NS: u64 = 300_000_000;
const HELLO_INTERVAL_NS: u64 = 10 * SECOND_NS;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyntheticConfig {
    pub seed: u64,
    pub seconds: f64,
    pub receivers: u16,
    pub transmitters: u16,
    /// Probes per second from each transmitter.
    pub rate_hz: f64,
    pub csi_len: u16,
    /// Wall-clock time of the session start; fixed so output is reproducible.
    pub start_wall_ns: u64,
    pub faults: Faults,
    /// How long each receiver had been up when the session started, within a second; random
    /// between 1 and 61 s when unset. About 4,294 s puts the 32-bit radio timer near its wrap.
    pub initial_uptime_s: Option<f64>,
    /// Transmitter indices each receiver hears; every receiver hears every transmitter when
    /// unset.
    pub hears: Option<Vec<Vec<u16>>>,
}

impl Default for SyntheticConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            seconds: 10.0,
            receivers: 1,
            transmitters: 1,
            rate_hz: 100.0,
            csi_len: 384,
            start_wall_ns: 1_789_000_000_000_000_000,
            faults: Faults::default(),
            initial_uptime_s: None,
            hears: None,
        }
    }
}

/// Fault rates are probabilities per event. Outages and reboots are spread evenly through the
/// session with jitter, so they do not overlap when the session is long enough.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Faults {
    /// A probe the receiver never hears.
    pub air_loss: f64,
    /// An accepted capture dropped by a full capture pool.
    pub pool_drop: f64,
    /// An encoded frame dropped by the node's export queue.
    pub export_drop: f64,
    /// Per delivered frame, mutually exclusive: a flipped byte, a cut-short frame, an extra copy,
    /// or arriving after the next clean frame.
    pub corrupt: f64,
    pub truncate: f64,
    pub duplicate: f64,
    pub reorder: f64,
    /// An extra stream framing error alongside a delivered frame.
    pub stream_error: f64,
    /// Transport outages per receiver, each losing every frame sent during it.
    pub outages: u32,
    pub outage_ms: f64,
    /// Receiver reboots: a new epoch, clocks and counters reset, silent while booting.
    pub reboots: u32,
    /// A receiver gain change on one link before an accepted capture.
    pub gain_step: f64,
    /// A capture whose gain the firmware could not read.
    pub gain_invalid: f64,
}

impl Faults {
    pub fn typical() -> Self {
        Self {
            gain_step: 0.002,
            gain_invalid: 0.01,
            air_loss: 0.05,
            pool_drop: 0.002,
            export_drop: 0.001,
            corrupt: 0.0005,
            truncate: 0.0005,
            duplicate: 0.001,
            reorder: 0.001,
            stream_error: 0.0005,
            outages: 1,
            outage_ms: 500.0,
            reboots: 1,
        }
    }
}

/// Per receiver: what was injected, and what an observer of the records can detect.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Truth {
    pub reboots: u64,
    pub air_losses: u64,
    pub pool_drops: u64,
    pub export_drops: u64,
    pub outage_losses: u64,
    pub corrupted: u64,
    pub truncated: u64,
    pub duplicated: u64,
    pub reordered: u64,
    pub stream_errors: u64,
    /// Lost frames, captures and probes that fall between values the observer received in the
    /// same epoch; losses before the first or after the last arrival are invisible to anyone.
    pub frames_missing: u64,
    pub captures_missing: u64,
    pub probes_missing: u64,
    /// Pool and export drops as of the latest delivered `Counters` frame of each epoch, summed.
    pub reported_pool_drops: u64,
    pub reported_export_drops: u64,
    pub gain_steps: u64,
    /// Gain changes between consecutive valid-gain records of a link, in capture-time order,
    /// among decodable delivered records.
    pub gain_steps_observable: u64,
    /// Decodable delivered CSI records with a valid gain.
    pub gain_valid_records: u64,
    /// The receiver's clocks, one entry per epoch.
    pub clocks: Vec<ClockTruth>,
}

/// One receiver epoch's clocks. Node time is `(t_ns - boot_ns) · (1 + drift_ppm·1e-6) / 1000` µs
/// for host time `t_ns`; the radio timer reads node time minus `radio_start_us`, plus a few µs of
/// detection jitter, truncated to 32 bits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct ClockTruth {
    pub epoch: u32,
    pub boot_ns: i64,
    pub drift_ppm: f64,
    pub radio_start_us: u64,
}

impl ClockTruth {
    /// Host time, ns, at which this epoch's unwrapped radio timer read `radio_us`.
    pub fn host_ns(&self, radio_us: f64) -> f64 {
        self.boot_ns as f64
            + (radio_us + self.radio_start_us as f64) * 1000.0 / (1.0 + self.drift_ppm * 1e-6)
    }

    /// This epoch's unwrapped radio timer, µs, at host time `t_ns`.
    pub fn radio_us(&self, t_ns: f64) -> f64 {
        (t_ns - self.boot_ns as f64) * (1.0 + self.drift_ppm * 1e-6) / 1000.0
            - self.radio_start_us as f64
    }
}

pub struct Synthetic {
    receivers: Vec<Receiver>,
    peeked: Vec<Option<Record>>,
}

impl Synthetic {
    pub fn new(config: &SyntheticConfig) -> Result<Self, Error> {
        config.validate()?;
        Ok(Self {
            receivers: (0..config.receivers)
                .map(|i| Receiver::new(config, i))
                .collect(),
            peeked: vec![None; usize::from(config.receivers)],
        })
    }

    pub fn sources(config: &SyntheticConfig) -> Vec<SourceInfo> {
        (0..config.receivers)
            .map(|i| SourceInfo {
                name: format!("synthetic-rx-{i}"),
                kind: "synthetic".into(),
                details: serde_json::json!({
                    "receiver": i,
                    "node": node_id(b'R', i).to_string(),
                    "config": config,
                }),
            })
            .collect()
    }

    /// Complete once the iterator is exhausted.
    pub fn truth(&self) -> Vec<Truth> {
        self.receivers.iter().map(|r| r.truth.clone()).collect()
    }
}

impl Iterator for Synthetic {
    type Item = Record;

    fn next(&mut self) -> Option<Record> {
        for (slot, receiver) in self.peeked.iter_mut().zip(&mut self.receivers) {
            if slot.is_none() {
                *slot = receiver.next_record();
            }
        }
        let (index, _) = self
            .peeked
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.as_ref().map(|r| (i, r.mono_ns)))
            .min_by_key(|&(i, mono_ns)| (mono_ns, i))?;
        self.peeked[index].take()
    }
}

impl SyntheticConfig {
    fn validate(&self) -> Result<(), Error> {
        let f = &self.faults;
        let probabilities = [
            f.air_loss,
            f.pool_drop,
            f.export_drop,
            f.corrupt,
            f.truncate,
            f.duplicate,
            f.reorder,
            f.stream_error,
            f.gain_step,
            f.gain_invalid,
        ];
        let problem = if self.receivers == 0 || self.transmitters == 0 {
            Some("need at least one receiver and one transmitter")
        } else if !(self.seconds.is_finite() && self.seconds > 0.0) {
            Some("seconds must be positive")
        } else if !(self.rate_hz.is_finite() && self.rate_hz > 0.0 && self.rate_hz <= 100_000.0) {
            Some("rate must be in (0, 100000] Hz")
        } else if usize::from(self.csi_len) > MAX_CSI_LEN {
            Some("csi_len exceeds MAX_CSI_LEN")
        } else if !probabilities.iter().all(|p| (0.0..=1.0).contains(p))
            || f.corrupt + f.truncate + f.duplicate + f.reorder > 1.0
        {
            Some(
                "fault probabilities must be in [0, 1], and the per-frame ones must sum to at most 1",
            )
        } else if self
            .initial_uptime_s
            .is_some_and(|u| !(u.is_finite() && u >= 1.0))
        {
            Some("initial_uptime_s must be at least 1")
        } else if self.hears.as_ref().is_some_and(|h| {
            h.len() != usize::from(self.receivers)
                || h.iter().flatten().any(|&tx| tx >= self.transmitters)
        }) {
            Some("hears needs one list per receiver of valid transmitter indices")
        } else if !(f.outage_ms.is_finite() && f.outage_ms >= 0.0) {
            Some("outage_ms must be non-negative")
        } else {
            None
        };
        problem.map_or(Ok(()), |p| Err(Error::Config(p.into())))
    }
}

/// SplitMix64: small, fast and fully determined by its seed.
#[derive(Debug, Clone)]
struct Rng(u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        p > 0.0 && self.unit() < p
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    fn nonzero_u32(&mut self) -> u32 {
        loop {
            let v = self.next_u64() as u32;
            if v != 0 {
                return v;
            }
        }
    }

    fn fill(&mut self, out: &mut [u8]) {
        for chunk in out.chunks_mut(8) {
            chunk.copy_from_slice(&self.next_u64().to_le_bytes()[..chunk.len()]);
        }
    }
}

fn stream_rng(seed: u64, stream: u64) -> Rng {
    Rng(Rng(seed ^ stream.wrapping_mul(0xD1B5_4A32_D192_ED03)).next_u64())
}

fn node_id(role: u8, index: u16) -> NodeId {
    let [hi, lo] = index.to_be_bytes();
    NodeId([0x02, role, b'X', 0, hi, lo])
}

const HELLO: Hello = Hello {
    chip: Chip::Esp32,
    roles: Roles {
        tx: false,
        rx: true,
    },
    firmware_version: [0, 0, 0],
    firmware_commit: [0; 8],
    channel: 11,
    secondary_channel: 2,
    csi_config: CsiConfig {
        lltf: true,
        htltf: true,
        stbc_htltf2: true,
        ltf_merge: false,
        channel_filter: false,
        manu_scale: false,
        shift: 0,
    },
    pool_capacity: 32,
};

const RX: RxMeta = RxMeta {
    rssi: -65,
    noise_floor: -96,
    rate: 11,
    sig_mode: 1,
    mcs: 0,
    cwb: 1,
    stbc: 0,
    channel: 11,
    secondary_channel: 2,
    smoothing: true,
    not_sounding: true,
    aggregation: false,
    fec_coding: false,
    sgi: false,
    ampdu_cnt: 0,
    ant: 0,
    rx_state: 0,
    sig_len: 47,
    gain: None,
};

/// A transmitter's probe schedule. Every receiver runs an identical copy, so all of them hear the
/// same probes at the same times.
struct Tx {
    node: NodeId,
    epoch: u32,
    seq: u32,
    next_ns: u64,
    period_ns: f64,
    rng: Rng,
}

impl Tx {
    fn new(seed: u64, index: u16, rate_hz: f64) -> Self {
        let mut rng = stream_rng(seed, 0x5458_0000 | u64::from(index));
        let period_ns = 1e9 / rate_hz;
        Self {
            node: node_id(b'T', index),
            epoch: rng.nonzero_u32(),
            seq: 0,
            next_ns: (rng.unit() * period_ns) as u64,
            period_ns,
            rng,
        }
    }

    fn advance(&mut self) {
        self.seq = self.seq.wrapping_add(1);
        let jitter = (self.rng.unit() - 0.5) * 0.1 * self.period_ns;
        self.next_ns += (self.period_ns + jitter).max(1.0) as u64;
    }
}

/// Values an observer received or lost, in the order the observer saw them.
#[derive(Default)]
struct LossBook {
    first: Option<u32>,
    max: Option<u32>,
    lost: Vec<u32>,
}

impl LossBook {
    fn arrived(&mut self, seq: u32) {
        self.first.get_or_insert(seq);
        self.max = Some(self.max.map_or(seq, |m| m.max(seq)));
    }

    fn observable(&self) -> u64 {
        match (self.first, self.max) {
            (Some(first), Some(max)) => {
                self.lost.iter().filter(|&&s| first < s && s < max).count() as u64
            }
            _ => 0,
        }
    }
}

/// A link's latest record in capture-time order, as the observer saw it.
#[derive(Default)]
struct LinkBook {
    last_capture_us: Option<u64>,
    last_gain: Option<RxGain>,
}

#[derive(Default)]
struct Books {
    frames: LossBook,
    captures: LossBook,
    probes: BTreeMap<u16, LossBook>,
    links: BTreeMap<u16, LinkBook>,
    counters: Option<Counters>,
}

#[derive(Clone, Copy, Default)]
struct Meta {
    frame_seq: u32,
    capture: Option<u32>,
    probe: Option<(u16, u32)>,
    counters: Option<Counters>,
    /// The unwrapped radio capture time of a CSI record.
    capture_us: u64,
    gain: Option<RxGain>,
}

enum Event {
    Counters,
    Hello,
    Echo,
    Reboot,
    Probe(usize),
}

struct Receiver {
    source: u16,
    node: NodeId,
    rng: Rng,
    faults: Faults,
    csi_len: usize,
    start_wall_ns: u64,
    end_ns: u64,
    /// Every transmitter's schedule, heard or not, so all receivers see identical probes.
    txs: Vec<Tx>,
    hears: Vec<bool>,
    /// Current receiver gain for each transmitter's link.
    gains: Vec<RxGain>,
    epoch: u32,
    boot_ns: i64,
    up_ns: u64,
    drift: f64,
    radio_start_us: u64,
    /// CSI callbacks run one at a time, so their node times never go backwards.
    last_callback_us: u64,
    frame_seq: u32,
    capture_seq: u32,
    counters: Counters,
    next_counters_ns: u64,
    next_hello_ns: u64,
    next_echo_ns: u64,
    echo_id: u32,
    reboots: VecDeque<u64>,
    outages: Vec<(u64, u64)>,
    last_arrival_ns: u64,
    queue: VecDeque<Record>,
    held: Option<(Record, Meta)>,
    books: Books,
    truth: Truth,
    buf: Vec<u8>,
    done: bool,
}

impl Receiver {
    fn new(config: &SyntheticConfig, index: u16) -> Self {
        let mut rng = stream_rng(config.seed, 0x5258_0000 | u64::from(index));
        let faults = config.faults;
        let end_ns = (config.seconds * 1e9) as u64;

        let disruptions = faults.outages + faults.reboots;
        let mut is_reboot: Vec<bool> = (0..disruptions).map(|i| i < faults.reboots).collect();
        for i in (1..is_reboot.len()).rev() {
            is_reboot.swap(i, rng.below(i as u64 + 1) as usize);
        }
        let slot = end_ns as f64 / f64::from(disruptions + 1);
        let mut reboots = VecDeque::new();
        let mut outages = Vec::new();
        for (i, reboot) in is_reboot.into_iter().enumerate() {
            let at = ((i as f64 + 1.0 + (rng.unit() - 0.5) * 0.2) * slot) as u64;
            if reboot {
                reboots.push_back(at);
            } else {
                outages.push((at, at + (faults.outage_ms * 1e6) as u64));
            }
        }
        let gains = (0..config.transmitters)
            .map(|_| RxGain {
                agc: 20 + rng.below(30) as u8,
                fft: rng.below(9) as i8 - 4,
            })
            .collect();
        let hears = (0..config.transmitters)
            .map(|tx| {
                config
                    .hears
                    .as_ref()
                    .is_none_or(|h| h[usize::from(index)].contains(&tx))
            })
            .collect();
        let uptime_s = config.initial_uptime_s.unwrap_or(1.0 + rng.unit() * 60.0) + rng.unit();
        let epoch = rng.nonzero_u32();
        let clock = ClockTruth {
            epoch,
            boot_ns: -((uptime_s * 1e9) as i64),
            drift_ppm: (rng.unit() - 0.5) * 40.0,
            radio_start_us: 100_000 + rng.below(150_000),
        };

        Self {
            source: index,
            node: node_id(b'R', index),
            epoch,
            boot_ns: clock.boot_ns,
            up_ns: 0,
            drift: clock.drift_ppm * 1e-6,
            radio_start_us: clock.radio_start_us,
            last_callback_us: 0,
            hears,
            next_counters_ns: rng.below(SECOND_NS),
            next_echo_ns: rng.below(SECOND_NS),
            next_hello_ns: 0,
            rng,
            faults,
            csi_len: usize::from(config.csi_len),
            start_wall_ns: config.start_wall_ns,
            end_ns,
            gains,
            txs: (0..config.transmitters)
                .map(|t| Tx::new(config.seed, t, config.rate_hz))
                .collect(),
            frame_seq: 0,
            capture_seq: 0,
            counters: Counters::default(),
            echo_id: 0,
            reboots,
            outages,
            last_arrival_ns: 0,
            queue: VecDeque::new(),
            held: None,
            books: Books::default(),
            truth: Truth {
                clocks: vec![clock],
                ..Truth::default()
            },
            buf: vec![0; MAX_FRAME_LEN],
            done: false,
        }
    }

    fn next_record(&mut self) -> Option<Record> {
        while self.queue.is_empty() && !self.done {
            self.step();
        }
        self.queue.pop_front()
    }

    fn node_time_us(&self, t: u64) -> u64 {
        self.node_time_at(t as f64) as u64
    }

    fn node_time_at(&self, t_ns: f64) -> f64 {
        (t_ns - self.boot_ns as f64) * (1.0 + self.drift) / 1000.0
    }

    fn next_event(&self) -> (u64, Event) {
        let mut best = (self.next_counters_ns, Event::Counters);
        for candidate in [
            (self.next_hello_ns, Event::Hello),
            (self.next_echo_ns, Event::Echo),
        ] {
            if candidate.0 < best.0 {
                best = candidate;
            }
        }
        if let Some(&at) = self.reboots.front().filter(|&&at| at < best.0) {
            best = (at, Event::Reboot);
        }
        for (i, tx) in self.txs.iter().enumerate() {
            if tx.next_ns < best.0 {
                best = (tx.next_ns, Event::Probe(i));
            }
        }
        best
    }

    fn step(&mut self) {
        let (t, event) = self.next_event();
        if t >= self.end_ns {
            self.release_held();
            self.close_epoch();
            self.done = true;
            return;
        }
        match event {
            Event::Reboot => self.reboot(t),
            Event::Probe(i) => {
                let tx = &mut self.txs[i];
                let probe = Probe {
                    tx: tx.node,
                    epoch: tx.epoch,
                    schedule: 0,
                    seq: tx.seq,
                };
                tx.advance();
                if t >= self.up_ns && self.hears[i] {
                    self.on_probe(t, i as u16, probe);
                }
            }
            Event::Counters => {
                self.next_counters_ns = t + SECOND_NS;
                let counters = Counters {
                    node_time_us: self.node_time_us(t),
                    ..self.counters
                };
                let meta = Meta {
                    counters: Some(counters),
                    ..Meta::default()
                };
                self.emit(t, &Message::Counters(counters), meta);
            }
            Event::Hello => {
                self.next_hello_ns = t + HELLO_INTERVAL_NS;
                self.emit(t, &Message::Hello(HELLO), Meta::default());
            }
            Event::Echo => {
                self.next_echo_ns = t + SECOND_NS;
                let received = t + SECOND_NS / 1000 + self.rng.below(SECOND_NS / 1000);
                let node_rx_us = self.node_time_us(received);
                let echo = TimeEcho {
                    probe_id: self.echo_id,
                    host_time: t,
                    node_rx_us,
                    node_tx_us: node_rx_us + 50,
                };
                self.echo_id = self.echo_id.wrapping_add(1);
                self.emit(received + 50_000, &Message::TimeEcho(echo), Meta::default());
            }
        }
    }

    fn reboot(&mut self, t: u64) {
        self.reboots.pop_front();
        self.release_held();
        self.close_epoch();
        self.truth.reboots += 1;
        self.epoch = self.rng.nonzero_u32();
        self.boot_ns = t as i64;
        self.radio_start_us = 100_000 + self.rng.below(150_000);
        self.last_callback_us = 0;
        self.truth.clocks.push(ClockTruth {
            epoch: self.epoch,
            boot_ns: self.boot_ns,
            drift_ppm: self.drift * 1e6,
            radio_start_us: self.radio_start_us,
        });
        self.up_ns = t + REBOOT_DOWN_NS;
        self.frame_seq = 0;
        self.capture_seq = 0;
        self.counters = Counters::default();
        self.next_hello_ns = self.up_ns;
        self.next_counters_ns = self.up_ns + SECOND_NS;
        self.next_echo_ns = self.up_ns + SECOND_NS / 2;
    }

    fn on_probe(&mut self, t: u64, tx: u16, probe: Probe) {
        if self.rng.chance(self.faults.air_loss) {
            self.truth.air_losses += 1;
            self.books
                .probes
                .entry(tx)
                .or_default()
                .lost
                .push(probe.seq);
            return;
        }
        self.counters.csi_callbacks += 1;
        self.counters.csi_captured += 1;
        let capture = self.capture_seq;
        self.capture_seq = capture.wrapping_add(1);
        if self.rng.chance(self.faults.pool_drop) {
            self.counters.pool_drops += 1;
            self.truth.pool_drops += 1;
            self.books.captures.lost.push(capture);
            self.books
                .probes
                .entry(tx)
                .or_default()
                .lost
                .push(probe.seq);
            return;
        }
        if self.rng.chance(self.faults.gain_step) {
            self.step_gain(usize::from(tx));
        }
        let gain =
            (!self.rng.chance(self.faults.gain_invalid)).then_some(self.gains[usize::from(tx)]);
        let mut data = vec![0; self.csi_len];
        self.rng.fill(&mut data);
        let jitter_ns = (self.rng.unit() + self.rng.unit() - 1.0) * 5000.0;
        let radio_us = self.node_time_at(t as f64 + jitter_ns) as u64 - self.radio_start_us;
        let latency_ns = 20_000 + self.rng.below(200_000);
        let node_time_us = self
            .node_time_us(t + latency_ns)
            .max(self.last_callback_us + 5);
        self.last_callback_us = node_time_us;
        let csi = Csi {
            capture_seq: capture,
            node_time_us,
            rx_timestamp_us: radio_us as u32,
            source_mac: probe.tx.0,
            probe: Some(probe),
            first_word_invalid: true,
            rx: RxMeta {
                rssi: -60 - self.rng.below(15) as i8,
                gain,
                ..RX
            },
            data: &data,
        };
        let meta = Meta {
            capture: Some(capture),
            probe: Some((tx, probe.seq)),
            capture_us: radio_us,
            gain,
            ..Meta::default()
        };
        self.emit(t + 20_000, &Message::Csi(csi), meta);
    }

    /// Changes one link's gain by an AGC step of 1–3 or, a quarter of the time, an FFT step of 1.
    fn step_gain(&mut self, tx: usize) {
        let rng = &mut self.rng;
        let gain = &mut self.gains[tx];
        if rng.below(4) == 0 {
            gain.fft = match gain.fft {
                8.. => gain.fft - 1,
                ..=-8 => gain.fft + 1,
                fft if rng.chance(0.5) => fft + 1,
                fft => fft - 1,
            };
        } else {
            let delta = 1 + rng.below(3) as u8;
            let up = gain.agc < delta || (gain.agc + delta <= 63 && rng.chance(0.5));
            gain.agc = if up {
                gain.agc + delta
            } else {
                gain.agc - delta
            };
        }
        self.truth.gain_steps += 1;
    }

    fn emit(&mut self, send_ns: u64, message: &Message<'_>, meta: Meta) {
        let seq = self.frame_seq;
        self.frame_seq = seq.wrapping_add(1);
        let meta = Meta {
            frame_seq: seq,
            ..meta
        };
        let header = Header {
            node: self.node,
            epoch: self.epoch,
            seq,
        };
        let len = frame::encode(&header, message, &mut self.buf)
            .expect("synthetic messages fit in a frame");
        let f = self.faults;

        if self.rng.chance(f.export_drop) {
            self.counters.export_drops += 1;
            self.truth.export_drops += 1;
            self.lose(meta);
            return;
        }
        self.counters.export_bytes += len as u64;
        if self
            .outages
            .iter()
            .any(|&(from, to)| (from..to).contains(&send_ns))
        {
            self.truth.outage_losses += 1;
            self.lose(meta);
            return;
        }

        let arrival = self
            .last_arrival_ns
            .max(send_ns + 2_000_000 + self.rng.below(1_000_000));
        self.last_arrival_ns = arrival;
        let wall_ns = self.start_wall_ns + arrival;
        if self.rng.chance(f.stream_error) {
            self.truth.stream_errors += 1;
            self.queue.push_back(Record::stream_error(
                arrival,
                wall_ns,
                self.source,
                StreamError::BadCobs,
            ));
        }
        let mut record = Record {
            mono_ns: arrival,
            wall_ns,
            source: self.source,
            kind: RecordKind::Frame,
            payload: self.buf[..len].to_vec(),
        };

        let roll = self.rng.unit();
        let corrupt = f.corrupt;
        let truncate = corrupt + f.truncate;
        let duplicate = truncate + f.duplicate;
        let reorder = duplicate + f.reorder;
        if roll < truncate {
            if roll < corrupt {
                let at = self.rng.below(len as u64) as usize;
                record.payload[at] ^= 1 + self.rng.below(255) as u8;
                self.truth.corrupted += 1;
            } else {
                record.payload.truncate(self.rng.below(len as u64) as usize);
                self.truth.truncated += 1;
            }
            self.lose(meta);
            self.queue.push_back(record);
        } else if roll < duplicate {
            self.truth.duplicated += 1;
            self.deliver(record.clone(), meta);
            self.queue.push_back(record);
        } else if roll < reorder && self.held.is_none() && self.books.frames.first.is_some() {
            self.held = Some((record, meta));
        } else {
            self.deliver(record, meta);
        }
    }

    /// Queues a clean frame, then any held frame, which therefore arrives late.
    fn deliver(&mut self, record: Record, meta: Meta) {
        let mono_ns = record.mono_ns;
        self.arrive(meta);
        self.queue.push_back(record);
        if let Some((mut held, held_meta)) = self.held.take() {
            held.mono_ns = mono_ns;
            held.wall_ns = self.start_wall_ns + mono_ns;
            self.arrive(held_meta);
            self.queue.push_back(held);
            self.truth.reordered += 1;
        }
    }

    /// Queues a held frame in order, e.g. before a reboot.
    fn release_held(&mut self) {
        if let Some((mut held, meta)) = self.held.take() {
            held.mono_ns = self.last_arrival_ns;
            held.wall_ns = self.start_wall_ns + self.last_arrival_ns;
            self.arrive(meta);
            self.queue.push_back(held);
        }
    }

    fn arrive(&mut self, meta: Meta) {
        self.books.frames.arrived(meta.frame_seq);
        if let Some(capture) = meta.capture {
            self.books.captures.arrived(capture);
        }
        if let Some((tx, seq)) = meta.probe {
            self.books.probes.entry(tx).or_default().arrived(seq);
            if meta.gain.is_some() {
                self.truth.gain_valid_records += 1;
            }
            let link = self.books.links.entry(tx).or_default();
            if link
                .last_capture_us
                .is_none_or(|last| meta.capture_us > last)
            {
                if let Some(gain) = meta.gain {
                    if link.last_gain.is_some_and(|last| last != gain) {
                        self.truth.gain_steps_observable += 1;
                    }
                    link.last_gain = Some(gain);
                }
                link.last_capture_us = Some(meta.capture_us);
            }
        }
        if let Some(counters) = meta.counters
            && self
                .books
                .counters
                .is_none_or(|c| counters.node_time_us >= c.node_time_us)
        {
            self.books.counters = Some(counters);
        }
    }

    fn lose(&mut self, meta: Meta) {
        self.books.frames.lost.push(meta.frame_seq);
        if let Some(capture) = meta.capture {
            self.books.captures.lost.push(capture);
        }
        if let Some((tx, seq)) = meta.probe {
            self.books.probes.entry(tx).or_default().lost.push(seq);
        }
    }

    fn close_epoch(&mut self) {
        let books = std::mem::take(&mut self.books);
        self.truth.frames_missing += books.frames.observable();
        self.truth.captures_missing += books.captures.observable();
        self.truth.probes_missing += books.probes.values().map(LossBook::observable).sum::<u64>();
        if let Some(counters) = books.counters {
            self.truth.reported_pool_drops += counters.pool_drops;
            self.truth.reported_export_drops += counters.export_drops;
        }
    }
}
