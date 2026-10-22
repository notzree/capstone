use csi_host::inspect::{EpochReport, HealthEvent, InspectOptions, Inspector, Report};
use csi_host::synthetic::{Faults, Synthetic, SyntheticConfig, Truth};

fn inspect(config: &SyntheticConfig) -> (Report, Vec<Truth>, Vec<String>) {
    let mut synthetic = Synthetic::new(config).unwrap();
    let mut inspector = Inspector::new(InspectOptions {
        max_events: usize::MAX,
        ..InspectOptions::default()
    });
    for record in synthetic.by_ref() {
        inspector.push(&record);
    }
    let nodes = Synthetic::sources(config)
        .iter()
        .map(|s| s.details["node"].as_str().unwrap().to_owned())
        .collect();
    (inspector.finish(), synthetic.truth(), nodes)
}

fn silences(report: &Report, node: &str) -> u64 {
    report
        .events
        .iter()
        .filter(|e| matches!(e, HealthEvent::Silence { node: n, .. } if n == node))
        .count() as u64
}

/// Asserts the report detects exactly what the synthetic source injected.
fn assert_detected_exactly(config: &SyntheticConfig) -> Vec<Truth> {
    let (report, truth, nodes) = inspect(config);
    for ((t, node), source) in truth.iter().zip(&nodes).zip(&report.sources) {
        let context = format!("seed {} node {node}", config.seed);
        assert_eq!(
            source.decode_errors.values().sum::<u64>(),
            t.corrupted + t.truncated,
            "{context}"
        );
        assert_eq!(
            source.stream_errors.values().sum::<u64>(),
            t.stream_errors,
            "{context}"
        );

        let epochs = &report
            .nodes
            .iter()
            .find(|n| &n.node == node)
            .unwrap()
            .epochs;
        let sum = |f: &dyn Fn(&EpochReport) -> u64| epochs.iter().map(f).sum::<u64>();
        assert_eq!(epochs.len() as u64, t.reboots + 1, "{context}");
        assert_eq!(sum(&|e| e.frames_missing), t.frames_missing, "{context}");
        assert_eq!(sum(&|e| e.frames_duplicate), t.duplicated, "{context}");
        assert_eq!(sum(&|e| e.frames_late), t.reordered, "{context}");
        assert_eq!(sum(&|e| e.frames_stale + e.clock_backwards), 0, "{context}");
        assert_eq!(
            sum(&|e| e.captures_missing),
            t.captures_missing,
            "{context}"
        );
        assert_eq!(
            sum(&|e| e.links.iter().map(|l| l.probes_missing).sum()),
            t.probes_missing,
            "{context}"
        );
        assert_eq!(
            sum(&|e| e.counters.map_or(0, |c| c.pool_drops)),
            t.reported_pool_drops,
            "{context}"
        );
        assert_eq!(
            sum(&|e| e.counters.map_or(0, |c| c.export_drops)),
            t.reported_export_drops,
            "{context}"
        );
        assert_eq!(
            sum(&|e| e.links.iter().map(|l| l.gain_steps).sum()),
            t.gain_steps_observable,
            "{context}"
        );
        assert_eq!(
            sum(&|e| e.links.iter().map(|l| l.gain_valid).sum()),
            t.gain_valid_records,
            "{context}"
        );
        assert_eq!(
            silences(&report, node),
            u64::from(config.faults.outages + config.faults.reboots),
            "{context}"
        );
    }
    truth
}

#[test]
fn clean_session_reports_configured_rates_and_no_losses() {
    let config = SyntheticConfig {
        seconds: 5.0,
        receivers: 2,
        transmitters: 3,
        rate_hz: 500.0,
        ..SyntheticConfig::default()
    };
    let (report, _, nodes) = inspect(&config);
    assert_eq!(report.event_counts.get("node_epoch"), Some(&2));
    assert_eq!(report.event_counts.len(), 1, "{:?}", report.event_counts);
    for (source, node) in report.sources.iter().zip(&nodes) {
        assert!(source.decode_errors.is_empty() && source.stream_errors.is_empty());
        let node = report.nodes.iter().find(|n| &n.node == node).unwrap();
        let [epoch] = node.epochs.as_slice() else {
            panic!("expected one epoch")
        };
        assert_eq!(epoch.frames_missing + epoch.captures_missing, 0);
        assert_eq!(epoch.links.len(), 3);
        for link in &epoch.links {
            assert_eq!(link.probes_missing, 0);
            let rate = link.rate_hz.unwrap();
            assert!((rate - 500.0).abs() < 5.0, "rate {rate}");
            let interval = link.interval_us.unwrap();
            assert!((1950..=2050).contains(&interval.p50), "{interval:?}");
            assert!(interval.max < 2200);
            assert_eq!(link.layouts.len(), 1);
            assert_eq!(link.layouts[0].layout.len, 384);
            assert_eq!(link.gain_valid_fraction, 1.0);
            assert_eq!(link.gain_steps, 0);
        }
        assert!(epoch.min_rtt_us.unwrap() > 2000.0);
        assert_eq!(epoch.hello.as_ref().unwrap().chip, "Esp32");
    }
    let clock = &report.clock;
    assert_eq!(clock.receivers.len(), 2);
    assert_eq!(clock.unwrap_rejects, 0);
    for receiver in &clock.receivers {
        let latest = receiver.latest.as_ref().unwrap();
        assert!(
            latest.residual_us.is_none_or(|r| r.p99 < 50.0),
            "{receiver:?}"
        );
    }
}

#[test]
fn injected_faults_are_detected_exactly() {
    let configs = [
        SyntheticConfig {
            seed: 3,
            seconds: 12.0,
            receivers: 2,
            transmitters: 2,
            rate_hz: 300.0,
            faults: Faults::typical(),
            ..SyntheticConfig::default()
        },
        SyntheticConfig {
            seed: 11,
            seconds: 10.0,
            receivers: 1,
            transmitters: 1,
            rate_hz: 1000.0,
            faults: Faults {
                air_loss: 0.2,
                pool_drop: 0.02,
                export_drop: 0.01,
                corrupt: 0.005,
                truncate: 0.005,
                duplicate: 0.01,
                reorder: 0.01,
                stream_error: 0.005,
                outages: 2,
                outage_ms: 400.0,
                reboots: 2,
                gain_step: 0.01,
                gain_invalid: 0.02,
            },
            ..SyntheticConfig::default()
        },
    ];
    for config in configs {
        for t in assert_detected_exactly(&config) {
            assert!(
                t.reordered > 0 && t.duplicated > 0 && t.corrupted > 0,
                "{t:?}"
            );
            assert!(t.gain_steps_observable > 0 && t.gain_steps >= t.gain_steps_observable);
        }
    }
}

#[test]
fn fault_accounting_holds_across_seeds() {
    for seed in 100..140 {
        assert_detected_exactly(&SyntheticConfig {
            seed,
            seconds: 3.0,
            receivers: 2,
            transmitters: 2,
            rate_hz: 250.0,
            faults: Faults {
                air_loss: 0.3,
                pool_drop: 0.05,
                export_drop: 0.03,
                corrupt: 0.02,
                truncate: 0.02,
                duplicate: 0.03,
                reorder: 0.05,
                stream_error: 0.01,
                outages: 1,
                outage_ms: 300.0,
                reboots: 1,
                gain_step: 0.05,
                gain_invalid: 0.05,
            },
            ..SyntheticConfig::default()
        });
    }
}

#[test]
fn regression_radio_reset_keeps_gain_and_interval_accounting_running() {
    use csi_host::{Record, RecordKind};
    use csi_protocol::{Csi, Header, Message, NodeId, Probe, RxGain, RxMeta, frame};
    let mut inspector = Inspector::new(InspectOptions::default());
    // Sequence 2 is a pre-reset record arriving late after two new-run records.
    for (arrival, (seq, node_time_us, rx_timestamp_us, agc)) in [
        (0, 10_000_000, 9_000_000, 1),
        (1, 10_010_000, 9_010_000, 1),
        (3, 10_020_000, 100_000, 2),
        (4, 10_040_000, 120_000, 3),
        (2, 10_015_000, 9_015_000, 99),
    ]
    .into_iter()
    .enumerate()
    {
        let mut payload = vec![0; csi_protocol::MAX_FRAME_LEN];
        let n = frame::encode(
            &Header {
                node: NodeId([2; 6]),
                epoch: 1,
                seq,
            },
            &Message::Csi(Csi {
                capture_seq: seq,
                node_time_us,
                rx_timestamp_us,
                source_mac: [3; 6],
                probe: Some(Probe {
                    tx: NodeId([3; 6]),
                    epoch: 1,
                    schedule: 0,
                    seq,
                }),
                first_word_invalid: false,
                rx: RxMeta {
                    gain: Some(RxGain { agc, fft: 0 }),
                    ..RxMeta::default()
                },
                data: &[],
            }),
            &mut payload,
        )
        .unwrap();
        payload.truncate(n);
        inspector.push(&Record {
            mono_ns: arrival as u64 * 20_000_000,
            wall_ns: 0,
            source: 0,
            kind: RecordKind::Frame,
            payload,
        });
    }
    let report = inspector.finish();
    let epoch = &report.nodes[0].epochs[0];
    let link = &epoch.links[0];
    assert_eq!(link.gain_steps, 2);
    assert_eq!(link.radio_restarts, 1);
    assert!(
        (link.rate_hz.unwrap() - 2e6 / 30_000.0).abs() < 1e-6,
        "{link:?}"
    );
    assert_eq!(link.interval_us.unwrap().max, 20_000);
    assert_eq!(epoch.frames_late, 1);
    assert_eq!(epoch.clock_backwards, 0);
}
