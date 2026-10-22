# Matched-rate mode comparison

Setup: user-reported ~70cm desk separation, same plane. No board movement requested. Room activity is not controlled unless the user supplies a label; no empty/still/walking assumptions.

Compare four configurations at requested 10 and 50 probes/s per transmitting node:

1. Star A→B: a0 TX-only, b0 RX-only.
2. Star B→A: b0 TX-only, a0 RX-only.
3. Independent dual: both TX+RX, independent periodic probe tasks.
4. Packet-driven pair: both TX+RX, exact-round turn correlation and adaptive timeout.

Two 45-second recordings per configuration/rate, with five seconds warm-up following flash. Run one sweep forward and the second in reverse order to reduce order bias. Eight conditions, sixteen recordings. Pilot only: two repetitions do not establish statistical superiority or room generalization.

All images use the same firmware source/version0.2.2, wire protocolv2, native CSI config, channel6, HT20/MCS0 longGI, 64-byte overair bodies, UART921600 and driver-completion handling. Only capabilities, pair policy and nominal interval differ. Build configuration is archived. Actual requested/accepted/captured rates must be reported, since awaited driver completion and response/loss recovery can limit achieved rates.

Rank acquisition modes by per-direction received rate, missing probe interiors, capture interval p50/p95/p99/max, native CSI layout and queue/export/host health. Report transmission-request deltas and actual offered rate to avoid treating a lower-send-rate mode as equivalent load. Star has only one directed link: its aggregate count cannot be compared as the same coverage as two-way modes. Packet loss cannot isolate RF collisions from driver/extractor behavior. This is not a fall/motion accuracy test. Unknown gain prohibits claiming one mode has a more stable physical channel just from amplitude variation.

Original backups and previous good pair images are retained. Each flash checks connected MAC against image+backup metadata and verifies written bytes. Restore the selected bidirectional baseline, verify both roles and seal the final check recording. If modes trade throughput for reliability, report that tradeoff explicitly instead of inventing a universal winner.

## Completion

All eight conditions completed twice in forward/reverse order: 16 ×45s, 31,482 CSI measurements, correct roles/directions/PHY and verified session digests. There were no steady-state framing/decode errors, timestamp regressions or queue/export drops;12errors occurred within the first second of attachment. Mean received rate at50Hz was48.97–49.22/s for independent dual and48.60–48.95/s for pair turns. Pair turns showed lower interior sequence-loss fractions but slightly lower offered rate and longer maximum gaps; RF levels varied materially, so no universal/causal mode winner is asserted. A final30s verified pair50recording is registered separately. Both boards remain in this coordinated50Hz mode. See README.md, validation.json and results/.
