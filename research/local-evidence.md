# What already works locally

Inspected 2026-10-19. This is a filesystem audit of saved artifacts, not a new hardware experiment. The existing `demo/README.md` and `firmware.lock.json` say no hardware was captured; current saved recordings contradict that status. Preserve the old demo and correct its status when doing implementation work.

## Measured saved captures

| Session | Saved non-synthetic frames | Device time span | Accepted frames/s | 99th percentile gap | Largest gap |
| --- | ---: | ---: | ---: | ---: | ---: |
| empty-01 | 12,470 | 179.93 s | 69.30 | 44.01 ms | 121.63 ms |
| seated-01 | 12,779 | 179.90 s | 71.03 | 43.97 ms | 104.78 ms |
| chest-01 | 4,157 | 62.24 s | 66.77 | 63.04 ms | 330.57 ms |
| chest-02 | 7,895 | 114.94 s | 68.68 | 51.90 ms | 913.79 ms |
| chest-03 | 12,793 | 179.89 s | 71.11 | 44.01 ms | 374.06 ms |

All five have a consistent saved tuple of 384 IQ bytes, channel 11, sig_mode 1, bandwidth 1, rx_format 1. They all mark `first_word_invalid`; interpretation must follow the pinned ESP-IDF buffer layout. There are no duplicate device timestamps in these recordings. Negative signed device timestamps appear in chest sessions; interpreting the timestamp modulo 2^32 produces forward progress. Audit results are in [local-capture-audit.json](local-capture-audit.json).

These are accepted log records, not a measurement of transmitted-packet loss. The firmware requests 100 transmissions/s, but send-call overhead, callback work, transport, and RF loss can all lower the saved rate. A receiver callback counter is not automatically the sender's sequence number.

## A likely transport bottleneck worth fixing first

The capture CLI defaults to 921,600 baud. Assuming that baud was used with UART 8N1, the maximum payload rate is 92,160 bytes/s. Saved raw CSI lines average about 1,248 bytes (empty) and 1,296 bytes (seated), excluding host-added timestamps. That imposes an ideal ceiling of **73.9 and 71.1 lines/s**, respectively, before other logging or stalls. The observed rates are 69.3 and 71.0/s.

This is strong evidence that verbose serial export may be limiting capture. It is not proof that RF reception itself tops out at 70/s. The session manifest does not record baud, so verify the actual serial configuration. At 1,296 bytes/record, exporting 100/s would require roughly 1.30 Mbaud with 8N1 framing; exporting 500/s would require roughly 6.48 Mbaud. A binary record around 450 bytes would require roughly 2.25 Mbaud at 500/s. These are transport arithmetic, not promised working rates.

First implementation experiment: count sender attempts/successes, CSI callbacks, queue drops, and exported records independently; move printing out of the Wi-Fi callback; benchmark compact binary export and realistic serial or network transport. Do not simply raise a constant to 1,000 and call that 1 kHz sensing. If wireless export is used, measure the extra airtime and gaps it creates.

## Reusable code and missing capability

- `demo/src/csi_heartbeat/cli.py`: serial capture, raw log retention, parsed JSONL, host/device timestamps, session metadata, accepted/rejected counters. Reuse these concepts and the parser; add a versioned fall-sensing schema and labels.
- `demo/src/csi_heartbeat/signal.py`: layout checks, invalid-prefix handling, and timestamp unwrapping are useful starting points. Its heart-rate band, 30-second windows, spectral heuristic, and forward/backward filtering are unsuitable as the fall detector.
- `demo/firmware.lock.json`: ESP32 target, pinned ESP-CSI source revision, image digest, and firmware artifact hashes provide useful reproducibility scaffolding. Check which binary actually produced each recording.
- No labeled fall dataset, event classifier, calibrated occupancy model, continuous fall alert policy, or cross-room test evidence was found in this demo.

The old heartbeat reports label candidates in 20% of windows for both the empty and seated examples. The seated report has only one matched reference window, with approximately 55.4 BPM absolute error. These do not establish heartbeat or stationary occupancy capability. Chest-taped sessions are also a different geometry from a fixed, contactless room installation.

## Repeat the audit

From the repository root:

```sh
python3 research/tools/audit_existing_capture.py
```

The script reads existing recordings and prints aggregate metadata. It does not connect to devices, alter captures, or expose participant identifiers or radio MAC addresses. The existing raw recordings remain local under `demo/data/`.
