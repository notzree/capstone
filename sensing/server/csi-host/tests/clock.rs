use std::collections::BTreeMap;

use csi_host::clock::{ClockFit, ClockFitter, ClockKey, FitOptions, Mapping};
use csi_host::synthetic::{ClockTruth, Faults, Synthetic, SyntheticConfig, Truth};
use csi_host::{Record, RecordKind};
use csi_protocol::{Message, NodeId, frame};

struct Run {
    fit: ClockFit,
    truth: Vec<Truth>,
    nodes: Vec<NodeId>,
    records: Vec<Record>,
    seconds: f64,
}

/// Worst cases over a whole fit history.
#[derive(Debug)]
struct Errors {
    /// Disagreement between two receivers' mapped times for the same instant, while both were
    /// capturing within a fit's window.
    alignment_us: f64,
    /// Relative drift error between two receivers that overlapped at least 20 s in a window.
    drift_ppm: f64,
    /// Change in a receiver's mapped time between consecutive fits, at the earlier fit's time.
    jump_us: f64,
    residual_p99_us: f64,
}

impl Errors {
    fn assert_within_bars(&self) {
        assert!(self.drift_ppm < 1.0, "{self:?}");
        assert!(self.residual_p99_us < 50.0, "{self:?}");
        assert!(self.alignment_us < 20.0, "{self:?}");
        assert!(self.jump_us < 20.0, "{self:?}");
    }
}

fn run(config: &SyntheticConfig, options: FitOptions) -> Run {
    let mut synthetic = Synthetic::new(config).unwrap();
    let mut fitter = ClockFitter::new(options);
    let mut records = Vec::new();
    for record in synthetic.by_ref() {
        fitter.push(&record);
        records.push(record);
    }
    let nodes = Synthetic::sources(config)
        .iter()
        .map(|s| s.details["node"].as_str().unwrap().parse().unwrap())
        .collect();
    Run {
        fit: fitter.finish(),
        truth: synthetic.truth(),
        nodes,
        records,
        seconds: config.seconds,
    }
}

impl Run {
    fn epochs(&self, key: &ClockKey) -> (&[ClockTruth], usize) {
        let i = self.nodes.iter().position(|n| *n == key.node).unwrap();
        let clocks = &self.truth[i].clocks;
        (
            clocks,
            clocks.iter().position(|c| c.epoch == key.epoch).unwrap(),
        )
    }

    fn clock(&self, key: &ClockKey) -> ClockTruth {
        let (clocks, k) = self.epochs(key);
        clocks[k]
    }

    /// Host time (ns) during which a receiver clock was capturing.
    fn alive(&self, key: &ClockKey) -> (f64, f64) {
        let (clocks, k) = self.epochs(key);
        let start = if k == 0 {
            0.0
        } else {
            clocks[k].boot_ns as f64 + 0.3e9
        };
        let end = clocks
            .get(k + 1)
            .map_or(self.seconds * 1e9, |next| next.boot_ns as f64);
        (start, end)
    }

    fn mapped_at(&self, m: &Mapping, t_ns: f64) -> f64 {
        m.map.apply(self.clock(&m.receiver).radio_us(t_ns))
    }

    fn errors(&self) -> Errors {
        let mut errors = Errors {
            alignment_us: 0.0,
            drift_ppm: 0.0,
            jump_us: 0.0,
            residual_p99_us: 0.0,
        };
        let mut fits: BTreeMap<u64, Vec<&Mapping>> = BTreeMap::new();
        for m in &self.fit.mappings {
            fits.entry(m.fitted_at_mono_ns).or_default().push(m);
            errors.residual_p99_us = errors
                .residual_p99_us
                .max(m.residual_us.map_or(0.0, |r| r.p99));
        }
        for mappings in fits.values() {
            for (i, a) in mappings.iter().enumerate() {
                for b in &mappings[i + 1..] {
                    let (sa, ea) = self.alive(&a.receiver);
                    let (sb, eb) = self.alive(&b.receiver);
                    let [w0, w1] = a.window_mono_ns.map(|t| t as f64);
                    let (from, to) = (sa.max(sb).max(w0), ea.min(eb).min(w1));
                    if to <= from {
                        continue;
                    }
                    for t in [from, to] {
                        let gap = (self.mapped_at(a, t) - self.mapped_at(b, t)).abs();
                        errors.alignment_us = errors.alignment_us.max(gap);
                    }
                    if to - from >= 20e9 {
                        let rate = |m: &Mapping| {
                            m.map.slope * (1.0 + self.clock(&m.receiver).drift_ppm * 1e-6)
                        };
                        let ppm = (rate(a) / rate(b) - 1.0).abs() * 1e6;
                        errors.drift_ppm = errors.drift_ppm.max(ppm);
                    }
                }
            }
        }
        let mut previous: BTreeMap<ClockKey, &Mapping> = BTreeMap::new();
        for mappings in fits.values() {
            for m in mappings {
                if let Some(p) = previous.insert(m.receiver, m) {
                    let t = p.fitted_at_mono_ns as f64;
                    let (start, end) = self.alive(&m.receiver);
                    if (start..=end).contains(&t) {
                        let jump = (self.mapped_at(p, t) - self.mapped_at(m, t)).abs();
                        errors.jump_us = errors.jump_us.max(jump);
                    }
                }
            }
        }
        errors
    }
}

fn config() -> SyntheticConfig {
    SyntheticConfig {
        seed: 21,
        seconds: 120.0,
        receivers: 3,
        transmitters: 2,
        rate_hz: 200.0,
        csi_len: 64,
        ..SyntheticConfig::default()
    }
}

#[test]
fn recovers_each_receivers_offset_and_drift() {
    let run = run(&config(), FitOptions::default());
    assert_eq!(run.fit.clocks.len(), 3);
    assert_eq!(run.fit.latest().len(), 3);
    assert_eq!(run.fit.handovers.len(), 1);
    run.errors().assert_within_bars();
    assert!(run.fit.mappings.iter().all(|m| m.outliers == 0));

    // With one reference throughout, every mapping must also match it directly.
    let reference = run.clock(&run.fit.handovers[0].reference);
    for m in &run.fit.mappings {
        let receiver = run.clock(&m.receiver);
        let expected =
            ((1.0 + reference.drift_ppm * 1e-6) / (1.0 + receiver.drift_ppm * 1e-6) - 1.0) * 1e6;
        assert!((m.drift_ppm - expected).abs() < 0.1, "{m:?}");
        let pivot_ns = receiver.host_ns(m.map.pivot_us as f64);
        assert!(
            (m.map.mapped_pivot_us - reference.radio_us(pivot_ns)).abs() < 2.0,
            "{m:?}"
        );
    }
}

#[test]
fn reboots_start_new_mappings_and_the_timeline_carries_on() {
    // Long enough that a rebooted reference's old samples leave the 60 s window, forcing a
    // handover, wherever its reboot lands.
    let config = SyntheticConfig {
        seconds: 240.0,
        faults: Faults::typical(),
        ..config()
    };
    let run = run(&config, FitOptions::default());
    let keys: Vec<ClockKey> = run.fit.latest().keys().copied().collect();
    for node in &run.nodes {
        let epochs = keys.iter().filter(|k| k.node == *node).count();
        assert_eq!(epochs, 2, "{node} should have a mapping for each epoch");
    }
    assert!(
        run.fit.handovers.len() >= 2,
        "the reference rebooted, so it must hand over"
    );
    assert!(run.fit.handovers.iter().all(|h| !h.restarted));
    assert!(run.fit.mappings.iter().all(|m| m.timeline == 0));
    run.errors().assert_within_bars();
}

#[test]
fn unwraps_the_32_bit_radio_timer() {
    let config = SyntheticConfig {
        seconds: 30.0,
        receivers: 2,
        transmitters: 1,
        initial_uptime_s: Some(4_294.0),
        ..config()
    };
    let run = run(&config, FitOptions::default());
    let mut last: Vec<Option<u32>> = vec![None; run.nodes.len()];
    let mut wraps = 0;
    for record in &run.records {
        if record.kind != RecordKind::Frame {
            continue;
        }
        if let Ok((_, Message::Csi(csi))) = frame::decode(&record.payload) {
            let slot = &mut last[usize::from(record.source)];
            if slot.is_some_and(|prev| {
                csi.rx_timestamp_us < prev && prev - csi.rx_timestamp_us > 1 << 31
            }) {
                wraps += 1;
            }
            *slot = Some(csi.rx_timestamp_us);
        }
    }
    assert_eq!(wraps, 2, "each receiver's radio timer should wrap once");
    run.errors().assert_within_bars();
    assert!(run.fit.mappings.iter().all(|m| m.outliers == 0));
    assert_eq!(run.fit.unwrap_rejects, 0);
}

#[test]
fn chains_receivers_that_share_no_probes_with_the_reference() {
    let config = SyntheticConfig {
        receivers: 4,
        transmitters: 3,
        hears: Some(vec![vec![0], vec![0, 1], vec![1, 2], vec![2]]),
        ..config()
    };
    let run = run(&config, FitOptions::default());
    let latest = run.fit.latest();
    assert_eq!(latest.len(), 4);
    assert!(latest.values().any(|m| m.hops >= 2), "{latest:?}");
    run.errors().assert_within_bars();
}

#[test]
fn fit_is_deterministic_and_survives_its_file() {
    let a = run(&config(), FitOptions::default()).fit;
    let b = run(&config(), FitOptions::default()).fit;
    assert_eq!(a, b);
    let tmp = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::now_v7();
    a.write(tmp.path(), id).unwrap();
    assert_eq!(ClockFit::read(tmp.path(), id).unwrap(), Some(a.clone()));
    assert!(ClockFit::read(tmp.path(), uuid::Uuid::now_v7()).is_err());
    assert!(a.write(tmp.path(), id).is_err(), "never overwrites");
}

#[test]
fn candidate_windows_meet_the_bars() {
    let config = SyntheticConfig {
        faults: Faults::typical(),
        ..config()
    };
    for window_s in [30, 60] {
        let options = FitOptions {
            window_ns: window_s * 1_000_000_000,
            ..FitOptions::default()
        };
        let run = run(&config, options);
        let errors = run.errors();
        let mut pairs: Vec<u64> = run
            .fit
            .mappings
            .iter()
            .filter(|m| m.parent.is_some())
            .map(|m| m.pairs)
            .collect();
        pairs.sort();
        println!(
            "window {window_s} s: {errors:?}, median pairs per fit {}",
            pairs[pairs.len() / 2]
        );
        errors.assert_within_bars();
    }
}

#[test]
fn live_fit_caps_samples_streams_history_and_expires_idle_clocks() {
    use csi_host::clock::ClockFitWriter;
    let options = FitOptions {
        window_ns: 1_000_000_000,
        step_ns: 100_000_000,
        ..FitOptions::default()
    };
    let config = SyntheticConfig {
        seconds: 8.0,
        rate_hz: 100.0,
        ..config()
    };
    let tmp = tempfile::tempdir().unwrap();
    let id = uuid::Uuid::now_v7();
    let mut file = ClockFitWriter::create(tmp.path(), id, options).unwrap();
    // Deliberately too small, proving overload is counted while memory remains capped.
    let mut fitter = ClockFitter::live(options, 100);
    for record in Synthetic::new(&config).unwrap() {
        fitter.push(&record);
        assert!(fitter.buffered_samples() <= 100);
        if fitter.has_updates() {
            file.append(fitter.take_updates()).unwrap();
        }
    }
    assert!(fitter.capacity_rejects > 0);
    fitter.advance(20_000_000_000);
    assert_eq!(fitter.buffered_samples(), 0);
    assert!(fitter.current_mappings().is_empty());
    file.append(fitter.take_updates()).unwrap();
    file.sync().unwrap();
    let fit = ClockFit::read(tmp.path(), id).unwrap().unwrap();
    assert!(fit.mappings.len() > 20);
    let mut last = BTreeMap::new();
    for m in fit.mappings {
        if let Some(previous) = last.insert(m.receiver, m.version) {
            assert!(m.version > previous);
        }
    }
}
