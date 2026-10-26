# Physical ESP32 → Rust → live Python check

Date: 2026-10-23. **Passed on the two connected original ESP32 boards.** No flashing or reset was needed. This validates acquisition/recording/delivery and the trial reader, not physical activity recognition or capture-to-host calibration.

## Hardware and method

- `/dev/cu.usbserial-3`: `a0:b7:65:dd:e9:3c`; `/dev/cu.usbserial-0001`: `b0:b2:1c:0b:07:1c`. Fresh binary `Hello` reports identified `Esp32`, firmware `0.2.2`, both TX/RX roles, channel 6, HT20 LLTF+HT-LTF, pool capacity 32. Observed frames used protocol v2 and 256 native CSI bytes with `first_word_invalid=true`.
- The first sandboxed attempt could not open the serial ports and received no data: [reachability/](reachability/). With serial permission, the eight-second [reachable/](reachable/) check succeeded. All physical trials below used serial/local-socket permission.
- The current firmware was retained. Preserved expected pair-50 image/build artifacts were copied into trial provenance; there was no new flash readback, so the running image hash is not independently verified by this test. Boot epochs remained unchanged throughout the trials.
- Four normal 15-second acquisitions used 100/20/20/100 ms flush order. A fifth 15-second acquisition used 20 ms flushing with one healthy Python consumer and a second client that read the Arrow schema, then stopped reading. Collection starts/stops and export add a little wall time beyond the requested windows.
- [run_trials.py](run_trials.py) invokes `csi collect`, consumes `live_batches()`, decodes HT-LTF into masked complex64 arrays, and compares each live capture's core identity/time/native-byte hash with the offline export. [summarize.py](summarize.py) performs read-only postflight checks, including trial loading and activity-proxy selection.
- Geometry is explicitly unsurveyed placeholder data. The `transport_test` interval marks the automated test window, not a person's activity. Participant is null and the split group is `hardware-pipeline-test-exclude-training`. These recordings are not ordinary-activity or tomography training data.

## Delivery timing

Delay is **the same-host wall-clock difference from complete-frame arrival at the Rust hub to `CsiBatch` yield in Python**, before `complex()` or model processing. It includes recorder/health/publisher/IPC/Python batch overhead. It excludes RF reception, callback delay and UART time before the complete frame reaches the hub. It is not radio-capture-to-Python latency.

The first two seconds of each recording are excluded from the table's delay statistics. Quantiles use nearest rank across individual captures. Ranges are the two separate normal runs, not pooled quantiles or confidence intervals. The observed wall-minus-monotonic range was at most 94.6 µs; this is a clock-stability sanity check, not a capture-time calibration.

| Arrow flush setting | Median delay | p95 delay | p99 delay | Largest delay |
| --- | --- | --- | --- | --- |
| 100 ms, two runs | 58.3–58.7 ms | 105.6–105.7 ms | 111.1 ms | 119.6–123.8 ms |
| 20 ms, two runs | 17.0 ms | 29.3–29.4 ms | 32.1 ms | 42.3–44.0 ms |

Capture rates in the four normal runs were **46.6–48.3 captures/s per direction**, against the declared 50 Hz target. These rates use the inspector's existing callback-time statistics; a native-radio sampling-time audit remains separate work. No scheduling mode or firmware setting was changed for the comparison.

## Integrity, labels and stalled-reader result

- All **7,127 CSI captures** from the five acquisitions reached the healthy Python consumer. Core identity, original timestamps and native bytes matched the offline export for every live capture. No missing live captures were found, including the post-warmup completeness check.
- All raw sessions and trial bundles sealed. `Trial()` reopened them, verified provenance/label hashes and session bindings, and selected the software test interval using explicit `host_arrival_proxy` matching. All intervals were explicitly ended, with no fabricated physical activity labels.
- Supported captures decoded into `[capture, 64]` HT-LTF complex64 arrays. Offline checks retained all 128 LLTF+HT-LTF entries, masked the invalid first two entries, and found 107 valid bins per decoded capture. Captures preceding the first observed `Hello` remained raw-only.
- Recorder subscriber/input drops, frame-sequence gaps and capture-sequence gaps were zero. Both nodes' observed counter windows had zero pool/export-drop deltas. This excludes unobserved pre-trial periods and does not mean every transmitted radio probe was captured.
- Across all five acquisitions there were **131 probe-sequence gaps**, attributable to air/callback stages rather than the Python-delivery path. The packet-driven follower's conditional opportunities complicate a symmetric interpretation of directional probe loss, so no universal packet-loss or sensing-quality score is inferred.
- Five COBS framing rejects occurred while attaching to already streaming serial ports at 129.8–132.8 ms. The per-trial [audit files](r01-flush-100-audit.json) show no stream/decode errors after the first second. There were no source reconnects, reboot/epoch changes or observed radio restarts during the trials.
- In the stalled-client run, that client's queue dropped **10 batches** before its writer timed out/disconnected. The healthy reader still received all **1,406 captures**; raw recording sealed, publisher input drops were zero, and normal client delay remained 17.0 ms median / 28.9 ms p95. Its one write failure is the expected stalled-reader disconnect.

## Evidence

Machine-readable aggregate: [summary.json](summary.json). Full per-trial timing/health results: [results.json](results.json). Each `r*-result.json` contains its metrics; each `r*/` contains raw recordings, labels/provenance, export and quality. Commands, stdout/stderr and per-trial audits are preserved alongside them.

| Trial | Setting | CSI live = recorded |
| --- | --- | --- |
| [r01](r01-flush-100/) | 100 ms | 1,425 |
| [r02](r02-flush-20/) | 20 ms | 1,424 |
| [r03](r03-flush-20/) | 20 ms | 1,439 |
| [r04](r04-flush-100/) | 100 ms | 1,433 |
| [r05](r05-flush-20-stalled-reader/) | 20 ms + stalled secondary reader | 1,406 |

Remaining gates: capture-to-host alignment/label uncertainty, an actual person-labelled walkthrough, surveyed geometry, native-radio time auditing, one-TX/two-RX common-probe synchronization and sustained M5 acceptance. Two nodes alone cannot validate shared-probe alignment between two receivers of a third transmitter.
