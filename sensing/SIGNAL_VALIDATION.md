# Signal preservation and acquisition validation

Recorded: 2026-10-22. **Status: proposed tests; not implemented or passed.**

Implementation tasks, test matrices and completion criteria: [SIGNAL_VALIDATION_PLAN.md](SIGNAL_VALIDATION_PLAN.md).

## Purpose and evidence boundary

Establish whether the acquisition system preserves a brief, known change in CSI with usable timing, amplitude and multi-link context. Use these results to choose sampling rates, scheduling, clock tolerances, buffering and export capacity before evaluating a fall classifier.

The completed [mode comparison](bench/2026-10-21/mode-comparison/README.md) measured acquisition rate, sequence gaps and callback-time intervals. It did not establish fall-detection accuracy or prove that star or mesh sensing is superior. The existing [synthetic source](server/csi-host/src/synthetic.rs) produces noise and injects faults with ground truth; it validates transport, bookkeeping and clock behavior, not preservation of a structured sensing signal.

Synthetic signal tests can expose information lost or introduced by software and scheduling under stated assumptions. They cannot establish physical sensitivity to a person falling, spatial coverage in a real room, or generalization to elderly people. Those require subsequent controlled hardware and labelled activity experiments.

## Measurements to prioritize

| Measurement | Proposed synthetic test and output | Design decisions |
| --- | --- | --- |
| Coverage of short changes | Sweep a known pulse's duration and start phase through sampling schedules and loss traces. Report sample-count distribution, longest blind interval, and fraction of events with at least `m` valid samples on at least `K` links. Sweep `m` and `K`; do not assume a classifier requirement yet. | Per-link sampling rate, schedule fairness, burst-loss tolerance and redundancy. |
| Distortion of a known change | Generate amplitude pulses, oscillations and frequency sweeps. Compare recovered peak, energy, onset, duration and dominant temporal frequency against an ideal reference using the same declared preprocessing. | Sampling, resampling, filtering, timestamp precision and allowable jitter. |
| Artificial changes | Keep the underlying channel constant while injecting gain steps, phase offsets, quantization, clipping and invalid data. Measure spurious peaks and motion-feature energy; report whether invalid/unknown values remain explicitly marked. | Gain extraction, normalization, phase handling, clipping detection and validity masks. |
| Aligned, fresh measurements across links | Give synthetic links the same event and known timestamps, then inject offset, drift, timer wrap, restart, delay and correlated outages. Report alignment error against ground truth, inter-link sample skew, and fraction of fusion windows with `K` valid, fresh links. | Clock fitting, fusion windows, scheduling, epoch handling and degraded operation. |
| Freshness under realistic load | Emulate all node inputs with physical byte-rate limits and bounded queues. Inject export, disk and consumer stalls. Report capture-to-consumer age (p50/p95/p99/max), queue occupancy, drops, starvation and time to recover. | Transport capacity, batching, buffer sizing and backpressure. |

Normalized waveform error needs a documented reference and denominator floor; use absolute artifact energy for a constant or zero-valued reference. Do not optimize a synthetic waveform score by removing a genuine slow change through normalization. Any validity masking or interpolation must be reported, and interpolated points must not count as independent captured samples.

### Why average loss is insufficient

A synthetic 100 ms pulse contains about five samples under uniform 50 Hz sampling. A small number of scattered losses may preserve it, while a 100 ms acquisition outage can erase it entirely depending on onset timing. Compare event coverage and recovered-feature error at equal total loss, including independent, burst and correlated loss patterns.

The 100 ms example, and any proposed duration/frequency sweep, are engineering stress cases rather than validated fall timescales. Set final sampling and error budgets from the downstream features and labelled data; do not assume that 50 Hz is sufficient for fall detection.

## Timestamp requirements

- Use unwrapped native **radio receive timestamps** to assess physical sampling intervals. The existing comparison audit uses node timestamps taken in the CSI callback, so its interval statistics also include callback scheduling effects.
- Separately track radio capture, callback execution, UART export, host arrival and consumer delivery. Queue or callback delay must not be mistaken for a radio acquisition gap.
- Synthetic ground truth should make these times and clock origins explicit. Hardware delay estimates must account for different radio/node/host clock origins, mapping uncertainty, timer wraps and boot/radio-run identity.
- Reset or mark discontinuities; do not interpolate across unknown epochs or silently reuse stale clock mappings. Synchronizing transmission turns does not set node clocks equal, and timestamp alignment does not make separate radios phase coherent.
- Real shared-probe receiver alignment needs multiple receivers hearing the same third-party transmitter. The present two-board pair does not provide that physical validation.

## Concrete six-node capacity case

Assume six nodes each transmit 50 broadcast probes/s, and every other node receives every probe. Each node then exports:

```text
5 peers × 50 probes/s = 250 CSI records/s per receiver

Current 256-byte CSI record:
16-byte header + 60-byte CSI metadata + 256-byte CSI + 4-byte CRC = 336 bytes
With conservative COBS overhead and delimiter: at most about 339 bytes

250 × 339 = 84,750 bytes/s per receiver
921,600 baud / 10 bits per byte (8N1) = 92,160 bytes/s UART capacity
84,750 / 92,160 ≈ 92% utilization before health/timing messages
```

This is a capacity estimate, not a measured six-board result. Frame overhead varies with content. All-peer reception and the requested TX rate are assumptions to stress; report actual offered load too.

The synthetic exporter must enforce the UART byte budget, include control traffic and use the configured finite capture/export buffers. A pseudo-terminal that accepts bytes at host speed cannot validate this capacity. Inject repeated stalls and inspect both immediate loss and the time required to drain the backlog. A queue may avoid drops yet deliver data too late to be useful.

Use the resulting headroom/freshness curves to decide whether to batch, lower selected link rates, change scheduling or use another export transport. More directed links also do not automatically mean proportionally more independent sensing information.

## Proposed implementation order

1. **Timestamp audit:** recompute sampling intervals from stored radio ticks alongside callback intervals, preserving epochs and radio-run boundaries. Report the difference rather than relabelling prior callback statistics as RF timing.
2. **Known-signal replay:** add deterministic structured complex CSI with separate truth timestamps/events. Start with pulses and oscillations, then replay the same continuous signals through ideal sampling and measured acquisition schedules/loss patterns. Keep any reference feature extraction in offline validation; production Rust remains responsible for acquisition rather than DSP/classification.
3. **Coverage and distortion sweeps:** vary event duration, onset phase, sample rate, jitter, burst loss and correlated link outages. Report curves and failing seeds. Select provisional budgets explicitly; no pass threshold has yet been agreed.
4. **Six-node byte-limited load:** enforce per-node wire capacity and bounded buffers; add realistic stalls and compare data age, recovery and loss for candidate schedules.
5. **Artifact and alignment stress:** inject gain/phase changes, invalid bytes, drift, restarts and wraps; verify feature behavior and metadata/clock provenance before progressing to physical controlled-motion tests.

Archive generator parameters, seeds, input traces, firmware/configuration, reference preprocessing, masks, clock mappings and output metrics. Keep hypothetical inputs, replayed hardware traces, simulated results and physical measurements clearly labelled.

## Physical follow-through and sources

Changing ESP-NOW application payloads does not itself synthesize a fall-shaped propagation channel. Software tests need generated CSI values; physical signal-preservation tests need a controlled change in the channel or a suitable RF test setup. Later tests must establish repeatable motion response and spatial coverage, followed by labelled activity evaluation using fall recall, false alerts per room-day, detection delay and held-out people/rooms/sessions.

CSI gain and phase errors can hinder sensing; see [Ratnam et al., Optimal preprocessing of WiFi CSI for sensing applications](https://arxiv.org/abs/2307.12126). Hardware CSI format and invalid-first-word behavior are documented in [Espressif's CSI guide](https://docs.espressif.com/projects/esp-idf/en/latest/esp32/api-guides/wifi-driver/wifi-vendor-features.html). Our current receiver exports gain as unknown; synthetic correction success would not establish verified gain extraction on the boards.
