//! Host mapping from correlated request/echo exchanges. Network asymmetry remains uncertainty;
//! this does not turn one-way arrival timestamps into precise capture timestamps.
use std::collections::VecDeque;

use csi_protocol::{Header, TimeEcho};
use serde::Serialize;

use crate::clock::{Affine, ClockFitter};

#[derive(Debug, Clone, Serialize)]
pub struct HostMapping {
    pub timeline: u32,
    pub version: u64,
    pub fitted_at_mono_ns: u64,
    pub samples: usize,
    /// Maps reference timeline µs to host monotonic µs since session start.
    pub to_host_mono_us: Affine,
    /// Contemporary wall-clock anchor; wall time may jump independently of monotonic time.
    pub wall_anchor_mono_ns: u64,
    pub wall_anchor_ns: u64,
    pub min_rtt_us: f64,
    pub residual_max_us: f64,
    pub uncertainty_us: f64,
}

struct Sample {
    timeline: u32,
    timeline_us: f64,
    host_us: f64,
    received_ns: u64,
    wall_ns: u64,
    rtt_us: f64,
}

#[derive(Default)]
pub struct HostClock {
    samples: VecDeque<Sample>,
    version: u64,
    pub accepted: u64,
    pub unmapped: u64,
    pub rejected: u64,
}

impl HostClock {
    pub fn push(
        &mut self,
        header: &Header,
        echo: TimeEcho,
        received_ns: u64,
        wall_ns: u64,
        fitter: &ClockFitter,
    ) {
        let Some(turnaround_us) = echo.node_tx_us.checked_sub(echo.node_rx_us) else {
            self.rejected += 1;
            return;
        };
        let Some(elapsed_ns) = received_ns.checked_sub(echo.host_time) else {
            self.rejected += 1;
            return;
        };
        let Some(rtt_ns) = turnaround_us
            .checked_mul(1000)
            .and_then(|t| elapsed_ns.checked_sub(t))
        else {
            self.rejected += 1;
            return;
        };
        // Keep exchanges with at most one second of adjusted RTT.
        if rtt_ns > 1_000_000_000 {
            self.rejected += 1;
            return;
        }
        let node_mid = echo.node_rx_us + turnaround_us / 2;
        let Some((timeline, timeline_us)) =
            fitter.map_node_time(header.node, header.epoch, node_mid)
        else {
            self.unmapped += 1;
            return;
        };
        if self.samples.back().is_some_and(|s| s.timeline != timeline) {
            self.samples.clear();
        }
        self.samples.push_back(Sample {
            timeline,
            timeline_us,
            host_us: (echo.host_time as f64 + elapsed_ns as f64 / 2.0) / 1000.0,
            received_ns,
            wall_ns,
            rtt_us: rtt_ns as f64 / 1000.0,
        });
        while self.samples.len() > 32 {
            self.samples.pop_front();
        }
        self.accepted += 1;
    }

    pub fn mapping(
        &mut self,
        now_ns: u64,
        window_ns: u64,
        timeline: Option<u32>,
    ) -> Option<HostMapping> {
        self.samples.retain(|s| {
            now_ns.saturating_sub(s.received_ns) <= window_ns && Some(s.timeline) == timeline
        });
        let latest = self.samples.back()?;
        if self.samples.len() < 3 {
            return None;
        }
        // Use the lower-RTT half of the window to reduce export/queue latency bias.
        let mut samples: Vec<&Sample> = self.samples.iter().collect();
        samples.sort_by(|a, b| a.rtt_us.total_cmp(&b.rtt_us));
        samples.truncate(samples.len().div_ceil(2).max(3));
        let n = samples.len() as f64;
        let x0 = samples[0].timeline_us;
        let y0 = samples[0].host_us;
        let mx = samples
            .iter()
            .map(|s| (s.timeline_us - x0) / n)
            .sum::<f64>();
        let my = samples.iter().map(|s| (s.host_us - y0) / n).sum::<f64>();
        let xx = samples
            .iter()
            .map(|s| (s.timeline_us - x0 - mx).powi(2))
            .sum::<f64>();
        let xy = samples
            .iter()
            .map(|s| (s.timeline_us - x0 - mx) * (s.host_us - y0 - my))
            .sum::<f64>();
        if xx < 1.0 {
            return None;
        }
        let slope = xy / xx;
        if !slope.is_finite() || !(0.99..=1.01).contains(&slope) || x0 + mx < 0.0 {
            return None;
        }
        let pivot_us = (x0 + mx) as u64;
        let map = Affine {
            pivot_us,
            mapped_pivot_us: y0 + my + slope * (pivot_us as f64 - x0 - mx),
            slope,
        };
        let residual = samples
            .iter()
            .map(|s| (map.apply(s.timeline_us) - s.host_us).abs())
            .fold(0.0, f64::max);
        self.version += 1;
        Some(HostMapping {
            timeline: latest.timeline,
            version: self.version,
            fitted_at_mono_ns: now_ns,
            samples: samples.len(),
            to_host_mono_us: map,
            wall_anchor_mono_ns: latest.received_ns,
            wall_anchor_ns: latest.wall_ns,
            min_rtt_us: samples[0].rtt_us,
            residual_max_us: residual,
            uncertainty_us: samples.iter().map(|s| s.rtt_us / 2.0).fold(0.0, f64::max) + residual,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::FitOptions;
    use csi_protocol::{Csi, Header, NodeId, Probe, RxMeta};

    #[test]
    fn echo_fit_recovers_host_time_and_rejects_invalid_turnaround() {
        let mut fitter = ClockFitter::live(FitOptions::default(), 1000);
        let header = Header {
            node: NodeId([2; 6]),
            epoch: 1,
            seq: 0,
        };
        let node_us = |t: u64| 1_000_000 + (t as f64 / 1000.0 * 1.00002) as u64;
        for i in 0..200u64 {
            let t = i * 10_000_000;
            let node = node_us(t);
            fitter.push_csi(
                t,
                &header,
                &Csi {
                    capture_seq: i as u32,
                    node_time_us: node,
                    rx_timestamp_us: (node - 50_000) as u32,
                    source_mac: [3; 6],
                    probe: Some(Probe {
                        tx: NodeId([3; 6]),
                        epoch: 1,
                        schedule: 0,
                        seq: i as u32,
                    }),
                    first_word_invalid: false,
                    rx: RxMeta::default(),
                    data: &[],
                },
            );
        }
        fitter.refit_now(2_000_000_000);
        let mut host = HostClock::default();
        for i in 0..10u64 {
            let t = 500_000_000 + i * 100_000_000;
            host.push(
                &header,
                TimeEcho {
                    probe_id: i as u32,
                    host_time: t - 120_000,
                    node_rx_us: node_us(t) - 20,
                    node_tx_us: node_us(t) + 20,
                },
                t + 120_000,
                1_700_000_000_000_000_000 + t + 120_000,
                &fitter,
            );
        }
        let mapping = host
            .mapping(2_000_000_000, 60_000_000_000, Some(0))
            .unwrap();
        for t in [500_000_000, 1_400_000_000] {
            let radio = node_us(t) - 50_000;
            let error = (mapping.to_host_mono_us.apply(radio as f64) - t as f64 / 1000.0).abs();
            assert!(error < 1.0, "{error}, {mapping:?}");
        }
        assert!(mapping.residual_max_us < 1.0);
        assert_eq!(mapping.min_rtt_us, 200.0);
        host.push(
            &header,
            TimeEcho {
                node_rx_us: 100,
                node_tx_us: 99,
                ..TimeEcho::default()
            },
            1000,
            0,
            &fitter,
        );
        host.push(
            &header,
            TimeEcho {
                node_rx_us: 0,
                node_tx_us: u64::MAX,
                ..TimeEcho::default()
            },
            1000,
            0,
            &fitter,
        );
        assert_eq!(host.rejected, 2);
        assert!(
            host.mapping(100_000_000_000, 60_000_000_000, Some(0))
                .is_none()
        );
    }
}
