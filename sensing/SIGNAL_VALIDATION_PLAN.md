# Implementation plan: compare acquisition designs by signal preservation

Date: 2026-10-22. **Plan only; tests below are not implemented or passed.**

This turns [SIGNAL_VALIDATION.md](SIGNAL_VALIDATION.md) into implementation tasks. The intended result is a reproducible comparison showing where each design preserves brief channel changes, loses information, introduces artifacts or becomes too stale under load. It does not rank fall classifiers or establish physical room coverage.

Overall delivery priorities live in [PLAN.md](PLAN.md#next-steps-unblock-data-collection-and-tomography): the Python handoff and labelled collection workflow come first, with this validation work supporting them in parallel. The phases below order the validation tasks, not all project work.

## Designs and comparison rules

| Candidate | Available evidence | Planned use |
| --- | --- | --- |
| Fixed-role star | Two-board measurements in both directions | One transmitting node and one or more receiving nodes; one direction per physical reference trial. |
| Independent TX/RX | Two-board measurements | Nodes transmit periodically and receive peer CSI. Actual radio contention is represented by recorded traces or explicitly stated fault assumptions. |
| Packet-driven pair | Implemented and measured on two boards | Replay observed request-driven acquisition patterns; exercise the real `csi-node` policy in deterministic event simulations. |
| Coordinated N-node schedule | Not implemented | Optional ideal round-robin benchmark, clearly labelled as an ideal reference. It is not the current pair policy scaled by assumption. |

Use two separate result tracks:

1. **Measured-trace replay:** sample known continuous CSI signals at the native acquisition times and observed sequence gaps from the existing two-board trials. This estimates how each recorded acquisition pattern would preserve those signals; it is not an RF experiment or an estimate of fall accuracy.
2. **Explicit simulation:** simulate larger node counts, clocks, bounded queues, serial service and fault patterns. Publish every radio/collision/visibility assumption. Do not claim that an ideal scheduler is an implemented or measured winner.

Run comparisons under both equal per-transmitter target rate and equal aggregate probe budget. Report achieved per-link rate, total probes, actual encoded export bytes and useful valid samples. These are different fairness questions: the same target rate across more transmitters increases total load. A pair follower's transmission opportunities depend on received requests, so its conditional sequence-loss fraction must not stand alone as the score.

Use the same signal, event starts, visibility scenario and fault seeds across candidates where applicable. Preserve each measured trace/repetition separately; links from unrelated boots must not be presented as a simultaneous measured multi-link capture. Do not give additional mesh links independent information automatically: include shared/reciprocal and correlated-link scenarios. A physical visibility map remains unknown.

## Phase 1 — Audit native sampling time

**Files:** extend `server/csi-host/examples/audit_session.rs`; reuse the radio unwrap/identity logic in `server/csi-host/src/clock.rs` and existing inspection logic rather than creating a competing clock model. Add focused regression fixtures under `server/csi-host/tests/`.

Tasks:

- Retain the existing callback interval output and add native radio interval statistics per receiver, transmitter, boot and radio run.
- Distinguish wrap, late/out-of-order records and a radio restart. End intervals at discontinuities; report unmappable times rather than inventing continuity.
- Add an offline sample export for validation: node/link identity, epochs/radio run, probe/capture sequences, native ticks, callback time, host time, clock provenance and validity. Keep exports bounded or stream them.
- Compare radio versus callback interval differences. Do not call raw `node_time_us - radio_time_us` an absolute callback latency: clock origins differ. Exact latency belongs to synthetic truth; hardware estimates need an explicit offset model and uncertainty.
- Re-audit all 16 existing trials without changing their raw sessions or historical reports. Write a new versioned derived report.

**Tests and completion:** known wrap, reset, duplicate, delayed record and mixed-link fixtures produce the expected boundaries and intervals. No interval crosses an epoch/radio-run boundary. Record identities/counts remain unchanged. The report identifies which old callback gaps are also native receive gaps; no physical conclusion is made from callback timing alone.

## Phase 2 — Generate known CSI and preserve it through replay

**Files:** add an optional structured profile under `server/csi-host/src/synthetic/`, keeping `synthetic.rs` as the existing entry point; add integration tests such as `server/csi-host/tests/signal_replay.rs`. Place offline reference calculations in a new `sensing/validation/` directory, outside production DSP/ML APIs.

Tasks:

- Generate deterministic continuous complex channel values per link/subcarrier, with constant baselines, finite-rise pulses, amplitude-modulated oscillations and frequency sweeps. Use declared link gains/delays/correlation; these are mathematical fixtures, not a human-body propagation model.
- Store separate, versioned truth: continuous timestamps, exact event onset/duration, signal parameters, clock parameters and seed. Truth must never leak into the observed input used for reconstruction or clock fitting.
- Sample at ideal schedules or Phase 1 trace times. Trace gaps remain missing; do not count interpolated values as captured observations. State how many start phases fit inside each trace; do not silently loop/splice traces.
- Encode using the real protocol/frame/COBS implementation, record and replay through the existing session code. Model signed-byte quantization/clipping and invalid CSI bytes explicitly.
- Preserve the current default noise generator and its deterministic fixtures. Additive validation sidecars must not change the raw session contract or require the unfinished Arrow boundary.

**Tests and completion:** hand-computable signals have correct values, onset and duration; signed I/Q ordering and invalid-byte masks have explicit fixtures. Same seed/configuration reproduces output. Clean encode/record/replay preserves valid bytes exactly. Legacy synthetic tests remain unchanged. A flat/zero input cannot silently cause undefined normalization.

## Phase 3 — Compare short-change coverage and distortion

**Deliverable:** an offline runner and report generator in `sensing/validation/`, consuming structured truth plus sessions/trace-derived observations. Reference signal processing stays offline; production Rust remains acquisition-only.

Initial engineering sweep, not a claim about fall timescales:

| Dimension | Initial values |
| --- | --- |
| Requested per-link rate | 10, 25, 50, 100 Hz in simulation; existing 10/50 Hz traces separately |
| Pulse duration | 50, 100, 200, 500 ms |
| Event start | 100 evenly spaced phases across a schedule cycle; fixed seeded starts across the full measured trace |
| Required captured samples `m` | 1, 3, 5, 10; report infeasible combinations |
| Required fresh links `K` | 1, 2, 3 where the scenario supplies that many valid links |
| Loss | No loss; equal-total-loss independent versus burst patterns; shared outages across links |
| Added burst length | 20, 50, 100, 200 ms |

Run small deterministic fixtures in CI; use at least 20 fixed seeds for an initial offline simulated sweep and retain per-seed results. Synthetic seeds do not create independent human/room trials. Split waveform, noise and fault random streams so changing one parameter does not accidentally change all others. Avoid a full Cartesian product: begin with one-factor sweeps and selected interactions around failing cases.

Report:

- **Coverage:** fraction of injected events containing at least `m` valid captured samples on at least `K` eligible links; minimum/quantiles of samples per event and the longest native blind interval. Preserve the event denominator, including completely erased events.
- **Distortion:** peak and energy bias, onset and duration error, normalized waveform error with a declared denominator floor, and dominant temporal-frequency error within a declared test band.
- **Validity:** rejected/interpolated fraction and results both before and after any declared reconstruction. Missing data must not become zero-valued physical observations.

**Tests and completion:** ideal sampling recovers expected sample counts and analytic features within declared quantization/numerical tolerances. A pulse fully inside an injected outage is counted as missed, not removed from evaluation. Equal-total random and burst loss remain distinguishable. Report frequency sweeps across/above the ideal sampling limit without pretending aliased signals were recovered. Publish curves rather than selecting one arbitrary pulse duration as a fall requirement.

## Phase 4 — Enforce six-node wire and queue budgets

**Files:** add a host-only deterministic event simulator under `server/csi-host/src/synthetic/` or test support, with integration tests under `server/csi-host/tests/`. Reuse the real `csi-node` pair policy rather than a more favorable reimplementation.

Tasks:

- Start with 2, 3 and 6 nodes and 10/25/50/100 Hz target sweeps. Represent TX attempts, radio reception, callback availability, capture queue admission, encoding, serial service, host arrival and a mock downstream consumer as distinct events.
- Charge each real encoded frame's exact byte length at `10 / 921600` seconds per UART byte. Include Hello/Counters/echo traffic, the configured 32-slot capture queue and documented finite intermediate buffers. State which additional driver/FIFO capacities are known, measured or assumed.
- Use a virtual clock for deterministic capacity tests. Add a small paced PTY/live-hub integration test for path correctness; label physical UART behavior as needing hardware calibration.
- If the simulator needs leader scheduling decisions that currently live inside firmware, extract that decision logic into host-testable `csi-node` policy and check request/response/timeout/reboot traces against the firmware contract. Reusing only the response matcher does not make an idealized simulator equivalent to the complete firmware loop.
- Exercise equal-total independent/burst loss, half-duplex receive exclusions, one-way link loss, explicit correlated outages and driver completion delay. A configurable contention model is an assumption to sweep, not a prediction of ESP-NOW collision probability.
- Inject 50/100/200/500 ms node-export stalls and 100/500/1000 ms storage/consumer stalls, separately and repeatedly. Verify layer-specific loss and live-consumer isolation. Do not hide overload by allowing an unbounded queue or omitting a slow node.
- Include the current six-node estimate: 250 CSI records/s per receiver × approximately 339 framed bytes ≈ 84.8 kB/s versus 92.2 kB/s UART capacity, before control traffic. Compare this conservative estimate with exact encoded-byte results.

**Metrics:** actual offered/captured/exported rates, wire utilization, queue peaks, per-layer drops, per-link starvation, capture-to-consumer age p50/p95/p99/max, stale-window fraction and recovery after each stall. Declare a backlog-recovery criterion, such as returning queue occupancy and sample age to the pre-stall operating envelope; report the envelope and any failure to recover within the test horizon.

**Tests and completion:** byte conservation and service-rate bounds hold; modeled output cannot exceed wire capacity. A known overload produces counted loss or accumulating age rather than a fictitious clean pass. Slow consumers do not stall durable recording. Useful-event coverage from Phase 3 is recomputed under load so a large, loss-free but stale queue cannot appear optimal.

## Phase 5 — Stress alignment and artificial motion

Use known shared events/probes across synthetic receivers. Sweep offset/drift, delayed/outlier probes, missing shared links, reference handover, reboot and radio wrap/restart. Compare the actual clock fitter against hidden truth, and measure alignment/feature error and availability of fresh `K`-link windows. A small fit residual is not by itself ground-truth accuracy. The current two-board hardware pair cannot validate common-third-party-probe alignment; a third board is needed for that physical follow-through.

For a constant underlying channel, inject gain steps, phase offsets, clipping and invalid words at the modeled receiver stage. Measure spurious feature energy and false feature peaks before/after the declared preprocessing. Preserve unknown gain as unknown; synthetic compensation does not verify physical gain extraction. Use a genuine slow signal alongside gain artifacts to expose preprocessing that suppresses both nuisance and useful change.

**Completion:** exact control fixtures, bounded error where truth supports it, explicit invalid/disconnected intervals, no silent epoch stitching and a documented failure envelope. Do not introduce a universal alignment/gain threshold before relating it to feature distortion and downstream requirements.

## Reports and decision rule

Each run should save `config.json`, software/input hashes, seeds, versioned truth, observed sessions, clock/validity provenance, `metrics.json` and a readable summary with plots. Reference preprocessing and any interpolation must be versioned. Keep measured-trace, assumption-driven simulation and physical results in separate sections.

Do not collapse the result into a weighted score with invented weights. First reject configurations that violate invariants or are infeasible at their modeled load. Then compare coverage, distortion, fresh multi-link availability, tail age and resource cost under the same scenarios/budgets. A candidate is preferable for a stated use case if it improves the needed information without an unacceptable cost elsewhere; otherwise publish the tradeoff and leave the choice open. Sensitivity to visibility, collision and stall assumptions must appear in the conclusion.

Software invariants and numeric tolerances can be CI gates. Semantic thresholds (`m`, `K`, acceptable distortion/age and event duration) remain provisional until agreed with downstream feature/model needs. No result here should be labelled fall recall or fall-detection accuracy.

## First implementation slice

Implement **Phases 1–3 first**: native-time re-audit, constant/pulse/oscillation fixtures, loss-pattern replay, coverage and distortion reports for the three existing two-board modes. This produces the first defensible comparison using recorded acquisition patterns, without waiting for an N-node scheduler, an RF propagation model or a fall classifier. Follow with the six-node byte-limited test before increasing physical mesh load. Keep Phase 5 artifact/alignment experiments as a separate follow-up, then check the predicted failure cases on hardware.

This change documents the plan only. It does not implement tests, change firmware, run new hardware experiments or complete M5/M7.
