//! Cross-node clock alignment from shared broadcast probes (reference-broadcast
//! synchronization).
//!
//! Every receiver that captured one probe (transmitter, transmitter epoch, sequence) captured it at
//! the same instant, to within detection jitter. Pairing receivers' radio timestamps for the same
//! probes gives each receiver clock an affine map onto one reference timeline. Raw values are never
//! changed: mappings are separate records, refitted over a sliding window.
//!
//! - A receiver clock is a node epoch, split again if its radio timer restarts. A reboot therefore
//!   always starts a new mapping, never bridged to the old one.
//! - The 32-bit `rx_timestamp_us` is unwrapped against `node_time_us`, which counts the same
//!   crystal from an earlier origin and is stamped a little later, in the CSI callback.
//! - The reference is chosen deterministically; when it disappears, a receiver that is still mapped
//!   takes over with its current mapping, so the timeline continues. Receivers that share no
//!   probes with the reference are chained through ones that do, along a maximum spanning tree of
//!   shared-probe counts.
//! - When every receiver reboots within a short time, the only link between the old clocks and
//!   the new ones is the brief overlap between them, so the timeline's rate can shift slightly
//!   there. Receivers capturing at the same time still agree with each other; that agreement is
//!   what the mapping is for.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

use csi_protocol::{Csi, Header, NodeId, frame};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::IoContext;
use crate::{Error, Record, RecordKind};

pub const SCHEMA: &str = "csi-clock-fit/1";
/// File name inside a session directory.
pub const CLOCK_FIT_FILE: &str = "clock-fit.jsonl";

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FitOptions {
    /// Probes that arrived longer ago than this (host time) leave the fit.
    pub window_ns: u64,
    /// How often every receiver is refitted.
    pub step_ns: u64,
    /// Fewest shared probes for a fit between two receivers.
    pub min_pairs: usize,
    /// A jump of more than this in a receiver's node-minus-radio clock difference means its radio
    /// timer restarted.
    pub radio_reset_us: u64,
    /// Residuals beyond this many robust standard deviations are excluded as outliers.
    pub outlier_sigmas: f64,
}

impl Default for FitOptions {
    fn default() -> Self {
        Self {
            window_ns: 60_000_000_000,
            step_ns: 10_000_000_000,
            min_pairs: 20,
            radio_reset_us: 250_000,
            outlier_sigmas: 5.0,
        }
    }
}

mod node_id_text {
    use csi_protocol::NodeId;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(id: &NodeId, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(id)
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<NodeId, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

/// One receiver clock: a node epoch, and which radio-timer run within it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ClockKey {
    #[serde(with = "node_id_text")]
    pub node: NodeId,
    pub epoch: u32,
    /// Radio-timer run identity. Live cache eviction advances a session-wide
    /// generation, so readmission cannot reuse a key whose continuity was lost.
    pub radio: u64,
}

impl std::fmt::Display for ClockKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} epoch {:08x}", self.node, self.epoch)?;
        if self.radio > 0 {
            write!(f, " radio run {}", self.radio)?;
        }
        Ok(())
    }
}

/// `timeline_us = mapped_pivot_us + slope · (x_us - pivot_us)` for a receiver's unwrapped radio
/// time `x_us`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Affine {
    pub pivot_us: u64,
    pub mapped_pivot_us: f64,
    pub slope: f64,
}

impl Affine {
    fn identity(at: u64) -> Self {
        Self {
            pivot_us: at,
            mapped_pivot_us: at as f64,
            slope: 1.0,
        }
    }

    pub fn apply(&self, x_us: f64) -> f64 {
        self.mapped_pivot_us + self.slope * (x_us - self.pivot_us as f64)
    }

    /// `outer(inner(x))`, where `inner` maps into the clock `outer` maps from.
    fn then(&self, outer: &Self) -> Self {
        Self {
            pivot_us: self.pivot_us,
            mapped_pivot_us: outer.apply(self.mapped_pivot_us),
            slope: outer.slope * self.slope,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Residuals {
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

/// A receiver clock's map onto the reference timeline, as of one fit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mapping {
    pub receiver: ClockKey,
    /// Increases each time this receiver is refitted; live mode uses a session-wide sequence.
    pub version: u64,
    /// Increments only if every mapped receiver disappeared at once and the timeline restarted.
    pub timeline: u32,
    pub fitted_at_mono_ns: u64,
    /// Host arrival times bounding the probes the fit considered.
    pub window_mono_ns: [u64; 2],
    /// The receiver this one was fitted against; `None` for the reference.
    pub parent: Option<ClockKey>,
    pub hops: u32,
    pub map: Affine,
    pub drift_ppm: f64,
    /// Timeline minus receiver time at the pivot.
    pub offset_us: f64,
    pub pairs: u64,
    pub outliers: u64,
    /// Absolute residuals of the fit against the parent, µs; `None` for the reference.
    pub residual_us: Option<Residuals>,
}

impl Mapping {
    fn new(receiver: ClockKey, map: Affine) -> Self {
        Self {
            receiver,
            version: 0,
            timeline: 0,
            fitted_at_mono_ns: 0,
            window_mono_ns: [0, 0],
            parent: None,
            hops: 0,
            map,
            drift_ppm: (map.slope - 1.0) * 1e6,
            offset_us: map.mapped_pivot_us - map.pivot_us as f64,
            pairs: 0,
            outliers: 0,
            residual_us: None,
        }
    }
}

/// The reference timeline changing hands or restarting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Handover {
    pub at_mono_ns: u64,
    pub reference: ClockKey,
    pub timeline: u32,
    /// The timeline restarted because no mapped receiver remained to carry it.
    pub restarted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClockFit {
    pub options: FitOptions,
    /// Every receiver clock that reported CSI with a probe, mapped or not.
    pub clocks: BTreeSet<ClockKey>,
    pub mappings: Vec<Mapping>,
    pub handovers: Vec<Handover>,
    /// CSI records whose radio timestamp could not be unwrapped against node time.
    pub unwrap_rejects: u64,
    pub radio_restarts: u64,
}

impl ClockFit {
    /// The latest mapping of every receiver clock.
    pub fn latest(&self) -> BTreeMap<ClockKey, &Mapping> {
        self.mappings.iter().map(|m| (m.receiver, m)).collect()
    }

    pub fn summary(&self) -> ClockSummary {
        let receivers = self
            .clocks
            .iter()
            .map(|key| {
                let fits: Vec<&Mapping> = self
                    .mappings
                    .iter()
                    .filter(|m| m.receiver == *key)
                    .collect();
                let drifts = fits.iter().map(|m| m.drift_ppm);
                ReceiverClock {
                    clock: key.to_string(),
                    fits: fits.len(),
                    latest: fits.last().map(|m| (*m).clone()),
                    drift_ppm_range: (!fits.is_empty()).then(|| {
                        [
                            drifts.clone().fold(f64::MAX, f64::min),
                            drifts.fold(f64::MIN, f64::max),
                        ]
                    }),
                    worst_p99_us: fits
                        .iter()
                        .filter_map(|m| m.residual_us.map(|r| r.p99))
                        .reduce(f64::max),
                }
            })
            .collect();
        ClockSummary {
            window_s: self.options.window_ns as f64 / 1e9,
            step_s: self.options.step_ns as f64 / 1e9,
            reference: self.handovers.first().map(|h| h.reference.to_string()),
            handovers: self.handovers.len().saturating_sub(1),
            timeline_restarts: self.handovers.iter().filter(|h| h.restarted).count(),
            unwrap_rejects: self.unwrap_rejects,
            radio_restarts: self.radio_restarts,
            receivers,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ClockSummary {
    pub window_s: f64,
    pub step_s: f64,
    /// The first reference; later handovers continue its timeline.
    pub reference: Option<String>,
    pub handovers: usize,
    pub timeline_restarts: usize,
    pub unwrap_rejects: u64,
    pub radio_restarts: u64,
    pub receivers: Vec<ReceiverClock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceiverClock {
    pub clock: String,
    pub fits: usize,
    pub latest: Option<Mapping>,
    /// Lowest and highest drift over this clock's fits.
    pub drift_ppm_range: Option<[f64; 2]>,
    pub worst_p99_us: Option<f64>,
}

type ProbeKey = (NodeId, u32, u32);

struct Sample {
    probe: ProbeKey,
    radio_us: u64,
    mono_ns: u64,
}

struct RadioRun {
    radio: u64,
    last_offset_us: u64,
}

pub struct ClockFitter {
    options: FitOptions,
    samples: BTreeMap<ClockKey, VecDeque<Sample>>,
    runs: BTreeMap<(NodeId, u32), RadioRun>,
    next_fit_ns: Option<u64>,
    last_mono_ns: u64,
    fitted_through_ns: Option<u64>,
    reference: Option<(ClockKey, Affine)>,
    timeline: u32,
    current: BTreeMap<ClockKey, Mapping>,
    versions: BTreeMap<ClockKey, u64>,
    fit: ClockFit,
    live_capacity: Option<usize>,
    retained_samples: usize,
    live_version: u64,
    radio_generation: u64,
    pub capacity_rejects: u64,
}

/// Unwraps a 32-bit radio timestamp against the node's 64-bit callback time. Both count µs of
/// the same crystal; node time started earlier and is read later, so the true difference is small
/// and positive and its low 32 bits give it exactly.
pub fn unwrap_radio_us(rx_timestamp_us: u32, node_time_us: u64) -> Option<(u64, u64)> {
    let offset = u64::from((node_time_us as u32).wrapping_sub(rx_timestamp_us));
    node_time_us
        .checked_sub(offset)
        .map(|radio_us| (radio_us, offset))
}

impl ClockFitter {
    pub fn new(options: FitOptions) -> Self {
        Self {
            options,
            samples: BTreeMap::new(),
            runs: BTreeMap::new(),
            next_fit_ns: None,
            last_mono_ns: 0,
            fitted_through_ns: None,
            reference: None,
            timeline: 0,
            current: BTreeMap::new(),
            versions: BTreeMap::new(),
            fit: ClockFit {
                options,
                clocks: BTreeSet::new(),
                mappings: Vec::new(),
                handovers: Vec::new(),
                unwrap_rejects: 0,
                radio_restarts: 0,
            },
            live_capacity: None,
            retained_samples: 0,
            live_version: 0,
            radio_generation: 0,
            capacity_rejects: 0,
        }
    }

    /// Bounded live mode. Call `take_updates` regularly to persist and release fit history.
    /// At most 256 receiver clocks and `sample_capacity` samples are admitted at once.
    pub fn live(options: FitOptions, sample_capacity: usize) -> Self {
        let mut fitter = Self::new(options);
        fitter.live_capacity = Some(sample_capacity);
        fitter
    }

    pub fn buffered_samples(&self) -> usize {
        self.retained_samples
    }

    pub fn has_updates(&self) -> bool {
        !self.fit.mappings.is_empty() || !self.fit.handovers.is_empty()
    }

    pub fn refit_now(&mut self, mono_ns: u64) {
        self.last_mono_ns = self.last_mono_ns.max(mono_ns);
        self.refit(self.last_mono_ns);
    }

    pub fn current_mappings(&self) -> &BTreeMap<ClockKey, Mapping> {
        &self.current
    }

    /// Map the node clock used by TimeEcho through its radio origin onto the fitted timeline.
    pub fn map_node_time(&self, node: NodeId, epoch: u32, node_us: u64) -> Option<(u32, f64)> {
        let run = self.runs.get(&(node, epoch))?;
        let mapping = self.current.get(&ClockKey {
            node,
            epoch,
            radio: run.radio,
        })?;
        let radio = node_us.checked_sub(run.last_offset_us)?;
        Some((mapping.timeline, mapping.map.apply(radio as f64)))
    }

    /// Refit even during silence, so old clocks expire and no stale fit appears current.
    pub fn advance(&mut self, mono_ns: u64) {
        self.last_mono_ns = self.last_mono_ns.max(mono_ns);
        if self.next_fit_ns.is_some_and(|t| mono_ns >= t) {
            self.refit(mono_ns);
            self.next_fit_ns = Some(mono_ns.saturating_add(self.options.step_ns.max(1)));
        }
    }

    /// Drain only new records; current mappings and the sliding-window samples remain available.
    pub fn take_updates(&mut self) -> ClockFit {
        ClockFit {
            options: self.options,
            clocks: self.fit.clocks.clone(),
            mappings: std::mem::take(&mut self.fit.mappings),
            handovers: std::mem::take(&mut self.fit.handovers),
            unwrap_rejects: self.fit.unwrap_rejects,
            radio_restarts: self.fit.radio_restarts,
        }
    }

    /// Feeds a recorded record; frames that are not decodable CSI with a probe are ignored.
    pub fn push(&mut self, record: &Record) {
        if record.kind == RecordKind::Frame
            && let Ok((header, csi_protocol::Message::Csi(csi))) = frame::decode(&record.payload)
        {
            self.push_csi(record.mono_ns, &header, &csi);
        }
    }

    /// Feeds one CSI record received at host time `mono_ns`. Duplicates of a probe already seen by
    /// the same receiver clock are ignored.
    pub fn push_csi(&mut self, mono_ns: u64, header: &Header, csi: &Csi<'_>) {
        self.last_mono_ns = self.last_mono_ns.max(mono_ns);
        let step_ns = self.options.step_ns.max(1);
        let next = *self
            .next_fit_ns
            .get_or_insert(mono_ns.saturating_add(step_ns));
        if mono_ns >= next {
            if self.live_capacity.is_some() {
                self.advance(mono_ns);
            } else {
                let mut at = next;
                while at <= mono_ns {
                    self.refit(at);
                    let Some(next) = at.checked_add(step_ns) else {
                        break;
                    };
                    at = next;
                }
                self.next_fit_ns = Some(at);
            }
        }

        let Some(probe) = csi.probe else {
            return;
        };
        let Some((radio_us, offset)) = unwrap_radio_us(csi.rx_timestamp_us, csi.node_time_us)
        else {
            self.fit.unwrap_rejects += 1;
            return;
        };
        if let Some(capacity) = self.live_capacity
            && (self.retained_samples >= capacity
                || (!self.runs.contains_key(&(header.node, header.epoch))
                    && self.runs.len() >= 256))
        {
            self.capacity_rejects += 1;
            return;
        }
        let run = self
            .runs
            .entry((header.node, header.epoch))
            .or_insert(RadioRun {
                radio: self.radio_generation,
                last_offset_us: offset,
            });
        if offset.abs_diff(run.last_offset_us) > self.options.radio_reset_us {
            if self.live_capacity.is_some() {
                self.radio_generation += 1;
                run.radio = self.radio_generation;
            } else {
                run.radio += 1;
            }
            self.fit.radio_restarts += 1;
        }
        run.last_offset_us = offset;
        let key = ClockKey {
            node: header.node,
            epoch: header.epoch,
            radio: run.radio,
        };
        if self.live_capacity.is_some()
            && !self.samples.contains_key(&key)
            && self.samples.len() >= 256
        {
            self.capacity_rejects += 1;
            return;
        }
        self.fit.clocks.insert(key);
        self.samples.entry(key).or_default().push_back(Sample {
            probe: (probe.tx, probe.epoch, probe.seq),
            radio_us,
            mono_ns,
        });
        self.retained_samples += 1;
    }

    pub fn finish(mut self) -> ClockFit {
        if self.fitted_through_ns.is_none_or(|t| t < self.last_mono_ns) {
            self.refit(self.last_mono_ns);
        }
        self.fit
    }

    fn refit(&mut self, at_ns: u64) {
        self.fitted_through_ns = Some(at_ns);
        let start_ns = at_ns.saturating_sub(self.options.window_ns);
        for samples in self.samples.values_mut() {
            while samples.front().is_some_and(|s| s.mono_ns < start_ns) {
                samples.pop_front();
                self.retained_samples -= 1;
            }
        }
        self.samples.retain(|_, s| !s.is_empty());
        if self.live_capacity.is_some() {
            let runs_before = self.runs.len();
            self.runs.retain(|&(node, epoch), _| {
                self.samples
                    .keys()
                    .any(|k| k.node == node && k.epoch == epoch)
            });
            if self.runs.len() < runs_before {
                // No tombstone cache is required: a fresh generation separates all
                // future admissions from expired clocks. Without an overlapping mapped
                // receiver, reference selection will explicitly restart the timeline.
                self.radio_generation += 1;
            }
            self.versions.retain(|k, _| self.samples.contains_key(k));
            self.fit.clocks.retain(|k| self.samples.contains_key(k));
        }
        let clocks: Vec<ClockKey> = self.samples.keys().copied().collect();
        if clocks.is_empty() {
            self.current.clear();
            return;
        }

        let mut heard: BTreeMap<ProbeKey, Vec<(usize, u64)>> = BTreeMap::new();
        for (i, samples) in self.samples.values().enumerate() {
            for s in samples.iter().filter(|s| s.mono_ns <= at_ns) {
                let receivers = heard.entry(s.probe).or_default();
                if !receivers.iter().any(|&(j, _)| j == i) {
                    receivers.push((i, s.radio_us));
                }
            }
        }
        let mut shared: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        for receivers in heard.values() {
            for (a, &(i, _)) in receivers.iter().enumerate() {
                for &(j, _) in &receivers[a + 1..] {
                    *shared.entry((i.min(j), i.max(j))).or_default() += 1;
                }
            }
        }
        let min_pairs = self.options.min_pairs;
        let mut neighbours: Vec<BTreeMap<usize, usize>> = vec![BTreeMap::new(); clocks.len()];
        for (&(i, j), &n) in &shared {
            if n >= min_pairs {
                neighbours[i].insert(j, n);
                neighbours[j].insert(i, n);
            }
        }
        // Most partners, then most shared probes, then lowest key.
        let rank = |i: usize| {
            let n = &neighbours[i];
            (
                std::cmp::Reverse(n.len()),
                std::cmp::Reverse(n.values().sum::<usize>()),
                clocks[i],
            )
        };

        let reference = match self.reference {
            Some((key, map)) if clocks.contains(&key) => (key, map),
            previous => {
                let carried = (0..clocks.len())
                    .filter(|&i| {
                        self.current
                            .get(&clocks[i])
                            .is_some_and(|m| m.timeline == self.timeline)
                    })
                    .min_by_key(|&i| rank(i));
                let (key, map, restarted) = match carried {
                    Some(i) => (clocks[i], self.current[&clocks[i]].map, false),
                    None => {
                        let i = (0..clocks.len()).min_by_key(|&i| rank(i)).unwrap_or(0);
                        let first = self.samples[&clocks[i]].front().map_or(0, |s| s.radio_us);
                        if previous.is_some() {
                            self.timeline += 1;
                        }
                        (clocks[i], Affine::identity(first), previous.is_some())
                    }
                };
                self.fit.handovers.push(Handover {
                    at_mono_ns: at_ns,
                    reference: key,
                    timeline: self.timeline,
                    restarted,
                });
                (key, map)
            }
        };
        self.reference = Some(reference);
        let root = clocks.iter().position(|&k| k == reference.0).unwrap_or(0);

        // Grow a maximum spanning tree by shared-probe count (Prim's algorithm), so every clock
        // reaches the reference through the longest overlaps available. Ties go to fewer hops,
        // then lower keys.
        let mut mapped: BTreeMap<usize, Mapping> = BTreeMap::new();
        mapped.insert(root, Mapping::new(reference.0, reference.1));
        let mut failed: BTreeSet<(usize, usize)> = BTreeSet::new();
        loop {
            let (tree, dead_ends) = (&mapped, &failed);
            let best = tree
                .iter()
                .flat_map(|(&p, parent)| {
                    neighbours[p]
                        .iter()
                        .filter(move |&(c, _)| {
                            !tree.contains_key(c) && !dead_ends.contains(&(p, *c))
                        })
                        .map(move |(&c, &n)| (n, parent.hops, c, p))
                })
                .min_by_key(|&(n, hops, c, p)| (std::cmp::Reverse(n), hops, clocks[c], clocks[p]));
            let Some((_, _, c, p)) = best else {
                break;
            };
            let pairs: Vec<(u64, u64)> = heard
                .values()
                .filter_map(|r| {
                    let x = r.iter().find(|&&(i, _)| i == c)?.1;
                    let y = r.iter().find(|&&(i, _)| i == p)?.1;
                    Some((x, y))
                })
                .collect();
            let Some(line) = fit_line(&pairs, &self.options) else {
                failed.insert((p, c));
                continue;
            };
            let parent = &mapped[&p];
            let mapping = Mapping {
                parent: Some(parent.receiver),
                hops: parent.hops + 1,
                pairs: line.inliers as u64,
                outliers: (pairs.len() - line.inliers) as u64,
                residual_us: Some(line.residuals),
                ..Mapping::new(clocks[c], line.map.then(&parent.map))
            };
            mapped.insert(c, mapping);
        }

        self.current.clear();
        for (_, mut mapping) in mapped {
            if self.live_capacity.is_some() {
                // A session-wide sequence stays monotonic even if an idle clock expires from
                // the bounded caches and later returns with the same key.
                self.live_version += 1;
                mapping.version = self.live_version;
            } else {
                let version = self.versions.entry(mapping.receiver).or_default();
                *version += 1;
                mapping.version = *version;
            }
            mapping.timeline = self.timeline;
            mapping.fitted_at_mono_ns = at_ns;
            mapping.window_mono_ns = [start_ns, at_ns];
            self.current.insert(mapping.receiver, mapping.clone());
            self.fit.mappings.push(mapping);
        }
    }
}

struct Line {
    map: Affine,
    inliers: usize,
    residuals: Residuals,
}

/// Robust least-squares fit of `y` against `x`: refits without residuals beyond
/// `outlier_sigmas` robust standard deviations until the inlier set stops changing.
fn fit_line(pairs: &[(u64, u64)], options: &FitOptions) -> Option<Line> {
    if pairs.len() < options.min_pairs.max(2) {
        return None;
    }
    let n = pairs.len() as u128;
    let pivot_x = (pairs.iter().map(|p| u128::from(p.0)).sum::<u128>() / n) as u64;
    let pivot_y = (pairs.iter().map(|p| u128::from(p.1)).sum::<u128>() / n) as u64;
    let points: Vec<(f64, f64)> = pairs
        .iter()
        .map(|&(x, y)| {
            (
                (x as i64 - pivot_x as i64) as f64,
                (y as i64 - pivot_y as i64) as f64,
            )
        })
        .collect();
    let mut inlier = vec![true; points.len()];
    let mut line = (0.0, 1.0);
    for _ in 0..10 {
        let used: Vec<(f64, f64)> = points
            .iter()
            .zip(&inlier)
            .filter_map(|(p, &keep)| keep.then_some(*p))
            .collect();
        if used.len() < options.min_pairs.max(2) {
            return None;
        }
        let m = used.len() as f64;
        let (mx, my) = used
            .iter()
            .fold((0.0, 0.0), |(a, b), &(x, y)| (a + x / m, b + y / m));
        let (sxx, sxy) = used.iter().fold((0.0, 0.0), |(a, b), &(x, y)| {
            (a + (x - mx) * (x - mx), b + (x - mx) * (y - my))
        });
        if sxx <= 0.0 {
            return None;
        }
        let slope = sxy / sxx;
        line = (my - slope * mx, slope);
        let residuals: Vec<f64> = points
            .iter()
            .map(|&(x, y)| y - (line.0 + line.1 * x))
            .collect();
        let mut kept: Vec<f64> = residuals
            .iter()
            .zip(&inlier)
            .filter_map(|(r, &keep)| keep.then_some(*r))
            .collect();
        let median = quantile(&mut kept, 0.5);
        let mut deviations: Vec<f64> = kept.iter().map(|r| (r - median).abs()).collect();
        let sigma = 1.4826 * quantile(&mut deviations, 0.5);
        let limit = (options.outlier_sigmas * sigma).max(1.0);
        let next: Vec<bool> = residuals
            .iter()
            .map(|r| (r - median).abs() <= limit)
            .collect();
        if next == inlier {
            break;
        }
        inlier = next;
    }
    let mut absolute: Vec<f64> = points
        .iter()
        .zip(&inlier)
        .filter_map(|(&(x, y), &keep)| keep.then_some((y - (line.0 + line.1 * x)).abs()))
        .collect();
    let inliers = absolute.len();
    Some(Line {
        map: Affine {
            pivot_us: pivot_x,
            mapped_pivot_us: pivot_y as f64 + line.0,
            slope: line.1,
        },
        inliers,
        residuals: Residuals {
            p50: quantile(&mut absolute, 0.50),
            p95: quantile(&mut absolute, 0.95),
            p99: quantile(&mut absolute, 0.99),
            max: quantile(&mut absolute, 1.0),
        },
    })
}

/// Nearest-rank quantile; sorts `values`.
fn quantile(values: &mut [f64], q: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let rank = ((q * values.len() as f64).ceil() as usize).clamp(1, values.len());
    values[rank - 1]
}

/// One line of `clock-fit.jsonl`: a header, then handovers and mappings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum FileLine {
    Header {
        schema: String,
        session_id: Uuid,
        fitter: String,
        options: FitOptions,
        clocks: BTreeSet<ClockKey>,
        unwrap_rejects: u64,
        radio_restarts: u64,
    },
    Handover(Handover),
    Mapping(Mapping),
    /// Live append-only checkpoints update cumulative counts and include unmapped clocks.
    Checkpoint {
        clocks: BTreeSet<ClockKey>,
        unwrap_rejects: u64,
        radio_restarts: u64,
    },
}

/// Incremental clock history writer for a live session. History stays on disk, not in RAM.
pub struct ClockFitWriter {
    path: std::path::PathBuf,
    file: std::io::BufWriter<fs::File>,
}

impl ClockFitWriter {
    pub fn create(dir: &Path, session_id: Uuid, options: FitOptions) -> Result<Self, Error> {
        let empty = ClockFitter::new(options).finish();
        empty.write(dir, session_id)?;
        let path = dir.join(CLOCK_FIT_FILE);
        let file = OpenOptions::new().append(true).open(&path).at(&path)?;
        Ok(Self {
            path,
            file: std::io::BufWriter::new(file),
        })
    }

    pub fn append(&mut self, fit: ClockFit) -> Result<(), Error> {
        let checkpoint = FileLine::Checkpoint {
            clocks: fit.clocks,
            unwrap_rejects: fit.unwrap_rejects,
            radio_restarts: fit.radio_restarts,
        };
        for line in fit
            .handovers
            .into_iter()
            .map(FileLine::Handover)
            .chain(fit.mappings.into_iter().map(FileLine::Mapping))
            .chain(std::iter::once(checkpoint))
        {
            serde_json::to_writer(&mut self.file, &line).map_err(|source| Error::Manifest {
                path: self.path.clone(),
                source,
            })?;
            self.file.write_all(b"\n").at(&self.path)?;
        }
        self.file.flush().at(&self.path)
    }

    pub fn sync(&mut self) -> Result<(), Error> {
        self.file.flush().at(&self.path)?;
        self.file.get_ref().sync_all().at(&self.path)
    }
}

impl ClockFit {
    /// Writes `clock-fit.jsonl` into a session directory; fails if one is already there.
    pub fn write(&self, dir: &Path, session_id: Uuid) -> Result<(), Error> {
        let path = dir.join(CLOCK_FIT_FILE);
        let mut text = Vec::new();
        let header = FileLine::Header {
            schema: SCHEMA.into(),
            session_id,
            fitter: concat!("csi-host ", env!("CARGO_PKG_VERSION")).into(),
            options: self.options,
            clocks: self.clocks.clone(),
            unwrap_rejects: self.unwrap_rejects,
            radio_restarts: self.radio_restarts,
        };
        let lines = std::iter::once(header)
            .chain(self.handovers.iter().cloned().map(FileLine::Handover))
            .chain(self.mappings.iter().cloned().map(FileLine::Mapping));
        for line in lines {
            serde_json::to_writer(&mut text, &line).map_err(|source| Error::Manifest {
                path: path.clone(),
                source,
            })?;
            text.push(b'\n');
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .at(&path)?;
        file.write_all(&text).at(&path)?;
        file.sync_all().at(&path)
    }

    /// Reads a session's `clock-fit.jsonl`, if any, checking it belongs to `session_id`.
    pub fn read(dir: &Path, session_id: Uuid) -> Result<Option<Self>, Error> {
        let path = dir.join(CLOCK_FIT_FILE);
        if !path.exists() {
            return Ok(None);
        }
        let format_error = |reason: String| Error::Format {
            path: path.clone(),
            reason,
        };
        let mut fit = None;
        for (number, line) in BufReader::new(fs::File::open(&path).at(&path)?)
            .lines()
            .enumerate()
        {
            let line = line.at(&path)?;
            let parsed: FileLine = serde_json::from_str(&line)
                .map_err(|e| format_error(format!("line {}: {e}", number + 1)))?;
            match (parsed, &mut fit) {
                (
                    FileLine::Header {
                        schema,
                        session_id: id,
                        options,
                        clocks,
                        unwrap_rejects,
                        radio_restarts,
                        ..
                    },
                    None,
                ) => {
                    if schema != SCHEMA || id != session_id {
                        return Err(format_error(format!(
                            "{schema} for session {id}, not {SCHEMA} for {session_id}"
                        )));
                    }
                    fit = Some(Self {
                        options,
                        clocks,
                        mappings: Vec::new(),
                        handovers: Vec::new(),
                        unwrap_rejects,
                        radio_restarts,
                    });
                }
                (FileLine::Handover(h), Some(fit)) => fit.handovers.push(h),
                (FileLine::Mapping(m), Some(fit)) => {
                    fit.clocks.insert(m.receiver);
                    fit.mappings.push(m);
                }
                (
                    FileLine::Checkpoint {
                        clocks,
                        unwrap_rejects,
                        radio_restarts,
                    },
                    Some(fit),
                ) => {
                    fit.clocks.extend(clocks);
                    fit.unwrap_rejects = unwrap_rejects;
                    fit.radio_restarts = radio_restarts;
                }
                _ => return Err(format_error(format!("line {}: out of place", number + 1))),
            }
        }
        fit.map(Some).ok_or_else(|| format_error("empty".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwrap_is_exact_across_the_32_bit_wrap() {
        let radio = (1u64 << 32) + 1234;
        let node = radio + 150_000;
        assert_eq!(unwrap_radio_us(radio as u32, node), Some((radio, 150_000)));
        let radio = (1u64 << 32) - 3;
        assert_eq!(unwrap_radio_us(radio as u32, radio + 7), Some((radio, 7)));
        assert_eq!(unwrap_radio_us(10, 5), None);
    }

    #[test]
    fn line_fit_ignores_outliers() {
        let options = FitOptions::default();
        let mut pairs: Vec<(u64, u64)> = (0..200u64)
            .map(|i| {
                let x = 1_000_000_000 + i * 10_000;
                let y = 5_000_000 + x + x / 100_000 + (i % 3);
                (x, y)
            })
            .collect();
        pairs[17].1 += 5_000;
        pairs[90].1 -= 9_000;
        let line = fit_line(&pairs, &options).unwrap();
        assert_eq!(line.inliers, 198);
        assert!(
            (line.map.slope - 1.00001).abs() < 1e-7,
            "{}",
            line.map.slope
        );
        assert!(line.residuals.max < 2.0);
    }

    #[test]
    fn composition_matches_applying_both_maps() {
        let inner = Affine {
            pivot_us: 4_000_000_000,
            mapped_pivot_us: 3_999_123_456.25,
            slope: 1.000_012,
        };
        let outer = Affine {
            pivot_us: 3_999_000_000,
            mapped_pivot_us: 7_000_000_000.5,
            slope: 0.999_985,
        };
        let both = inner.then(&outer);
        for x in [3_990_000_000.0, 4_000_000_000.0, 4_100_000_000.0] {
            assert!((both.apply(x) - outer.apply(inner.apply(x))).abs() < 1e-3);
        }
    }
}

#[cfg(test)]
mod regression_tests {
    use super::*;
    use csi_protocol::{Probe, RxMeta};

    #[test]
    fn regression_idle_eviction_never_reuses_reference_clock_identity() {
        let options = FitOptions {
            window_ns: 1_000_000_000,
            step_ns: 100_000_000,
            ..FitOptions::default()
        };
        let mut fitter = ClockFitter::live(options, 100);
        let header = Header {
            node: NodeId([2; 6]),
            epoch: 1,
            seq: 0,
        };
        let csi = |node_time_us, rx_timestamp_us| Csi {
            capture_seq: 0,
            node_time_us,
            rx_timestamp_us,
            source_mac: [3; 6],
            probe: Some(Probe {
                tx: NodeId([3; 6]),
                epoch: 1,
                schedule: 0,
                seq: 0,
            }),
            first_word_invalid: false,
            rx: RxMeta::default(),
            data: &[],
        };
        fitter.push_csi(0, &header, &csi(9_100_000, 9_000_000));
        fitter.refit_now(0);
        let mut before = fitter.current_mappings().values().next().unwrap().clone();
        fitter.take_updates();
        for cycle in 1..=3 {
            let expired_at = cycle * 3_000_000_000 - 1_000_000_000;
            fitter.advance(expired_at);
            assert!(fitter.current_mappings().is_empty());
            assert_eq!(fitter.buffered_samples(), 0);
            fitter.push_csi(
                cycle * 3_000_000_000,
                &header,
                &csi(12_000_000 * cycle, 100_000),
            );
            fitter.refit_now(cycle * 3_000_000_000);
            let after = fitter.current_mappings().values().next().unwrap();
            assert_ne!(
                after.timeline, before.timeline,
                "lost continuity must start a new timeline"
            );
            assert_ne!(
                after.receiver, before.receiver,
                "evicted clock keys must not alias earlier runs"
            );
            before = after.clone();
            let update = fitter.take_updates();
            assert!(update.handovers.iter().any(|h| h.restarted));
        }
    }

    #[test]
    fn regression_idle_readmission_rejoins_a_surviving_receivers_timeline() {
        let options = FitOptions {
            window_ns: 1_000_000_000,
            step_ns: 100_000_000,
            ..FitOptions::default()
        };
        let mut fitter = ClockFitter::live(options, 1000);
        let a = NodeId([2; 6]);
        let b = NodeId([4; 6]);
        let feed = |fitter: &mut ClockFitter, node, mono_ns, seq, node_time_us, radio_us| {
            fitter.push_csi(
                mono_ns,
                &Header {
                    node,
                    epoch: 1,
                    seq,
                },
                &Csi {
                    capture_seq: seq,
                    node_time_us,
                    rx_timestamp_us: radio_us,
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
                },
            );
        };
        for i in 0..20 {
            let t = i * 1000;
            feed(
                &mut fitter,
                a,
                u64::from(t) * 1000,
                i,
                u64::from(9_100_000 + t),
                9_000_000 + t,
            );
            feed(
                &mut fitter,
                b,
                u64::from(t) * 1000,
                i,
                u64::from(8_100_000 + t),
                8_000_000 + t,
            );
        }
        fitter.refit_now(20_000_000);
        let original = fitter
            .current_mappings()
            .values()
            .find(|m| m.receiver.node == a)
            .unwrap()
            .clone();
        for i in 1..=21 {
            let us = i * 100_000;
            feed(
                &mut fitter,
                b,
                u64::from(us) * 1000,
                100 + i,
                u64::from(8_100_000 + us),
                8_000_000 + us,
            );
        }
        fitter.refit_now(2_100_000_000);
        assert!(fitter.current_mappings().keys().all(|key| key.node == b));
        for i in 0..20 {
            let delta = i * 1000;
            let mono = 2_200_000_000 + u64::from(delta) * 1000;
            feed(
                &mut fitter,
                a,
                mono,
                1000 + i,
                u64::from(12_000_000 + delta),
                100_000 + delta,
            );
            feed(
                &mut fitter,
                b,
                mono,
                1000 + i,
                u64::from(10_300_000 + delta),
                10_200_000 + delta,
            );
        }
        fitter.refit_now(2_230_000_000);
        let latest = fitter.current_mappings();
        let returned = latest.values().find(|m| m.receiver.node == a).unwrap();
        let survivor = latest.values().find(|m| m.receiver.node == b).unwrap();
        assert_ne!(returned.receiver, original.receiver);
        assert_eq!(returned.timeline, original.timeline);
        assert_eq!(returned.timeline, survivor.timeline);
        assert!((returned.map.apply(119_000.0) - survivor.map.apply(10_219_000.0)).abs() < 1.0);
    }
}
