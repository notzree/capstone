# Dual TX/RX and packet-driven turn synchronization

Historical firmware 0.2.1 bring-up. The boards now run the tested 50 Hz pair configuration from the [mode comparison](../mode-comparison/README.md).

Implementation and hardware iteration follow [PLAN.md](PLAN.md). User-reported placement: two original ESP-WROOM-32 boards about **70 cm apart**, on a desk, roughly in the same plane. Positions/orientations and room activity are not surveyed or labelled.

Both boards now keep **TX and RX enabled**. A request packet grants the peer its response turn; an exact-round response lets the leader continue. There is **no fixed guard delay**. Leader recovery timeout adapts from round-trip observations (initial 150 ms, bounded 30–500 ms with loss backoff). A 100 ms leader interval caps the target at 10 rounds/s. This synchronizes turns, not absolute node clocks, and is not a guaranteed radio slot reservation.

| Node | Port | Pair function | CSI capability |
| --- | --- | --- | --- |
| `a0:b7:65:dd:e9:3c` | `/dev/cu.usbserial-3` | Leader | TX + RX |
| `b0:b2:1c:0b:07:1c` | `/dev/cu.usbserial-0001` | Responder | TX + RX |

## Implementation

- `tx` and `rx` features are independent; `tx,rx` enables both in periodic mode; `pair` enables both with peer-correlated turns. Thus fixed-role star experiments and the bidirectional pair use the same capture/export/send code.
- Pair build configuration names the peer MAC; the lower MAC leads. Wrong peer, schedule, malformed tag, duplicate/backwards sequence, stale request and four retired leader epochs cannot grant a response. Old responses cannot acknowledge a different leader boot/round. No role/channel restart is needed between turns.
- A 14-byte `CSIt` v1 extension occupies over-air probe padding. Existing `CSIp` layout and UART **protocol v2** are unchanged. No DSP or gain compensation is performed.
- Driver sends remain alive through completion, replacing the old poll-once/drop behavior. A missing send-completion callback triggers whole-node reset after 2 s. This watchdog path was source-reviewed, not deliberately fault-injected on hardware. `tx_accepted` is a lower bound when an enqueue/very-fast-completion failure is ambiguous in this driver's API; it is never peer-delivery evidence.
- CSI callback/export queues remain bounded. The separate ESP-NOW application-body queue is drained, and self probes are rejected before export. Peer/schedule filtering occurs before the turn mailbox, so foreign probes cannot overwrite a valid peer trigger.
- `csi-node` is shared `no_std` policy with 14 tests covering wire compatibility, exact correlation, local-clock timing, zero-guard flow, duplicates/loss, wrap, retired epochs, stale input and adaptive timeout/backoff. Stable host and Xtensa firmware use separate workspaces.

Firmware source/build details are in [the firmware README](../../../firmware/README.md). `firmware/flash-manifest.json` archives configuration, source/input hashes, images/ELFs, generated offsets and target/peer MACs for final firmware **0.2.1**. Both boards passed flashed-region digest verification; see `flash-a0.log`/`flash-b0.log`. Original 4 MB backups remain in `../boards/`, with verified backup hashes and unchanged raw files. `firmware-attempt-01/` and corresponding flash logs retain the first dual-role images (0.2.0), before stale-input expiry and missing-completion recovery were tightened.

## Recorded results

- `smoke-01/`: first packet-driven hardware attempt, 30 s. Confirmed both directed links and both TX/RX role flags. Preserved with its audit and initial image manifest.
- `baseline-02/`: final firmware 0.2.1, clean 60 s with both UART sources. **586 a0→b0 CSI records at 9.79 Hz**, **575 b0→a0 at 9.61 Hz**. Missing probe interiors: **12 / 598 (2.01%)** and **11 / 586 (1.88%)**, respectively. No framing/decode errors, self-captures, source/probe mismatches, duplicates, capture/frame sequence gaps, pool drops or export drops.
- All baseline captures: correct peer, schedule 1, channel 6, HT20, MCS0, long GI, no STBC, 256 native CSI bytes. `first_word_invalid=true` is preserved; processing must exclude the first four CSI bytes. Gain is absent. Every captured CSI payload in each directed link is distinct by SHA-256, including the invalid first word; this is not a signal-quality or activity claim.
- `recovery-03/`: 90 s continuous follower recording with a controlled EN reset of the leader (action timestamp in `reset-actions.jsonl`). The follower stayed in epoch **2463175849**; leader probes changed from epoch **876912938** to **2475717287**, starting at sequence **0**. Gap between the last pre-reset and first post-reset capture: **623.923 ms**. Recorded 275 pre-reset and 610 post-reset captures; post-reset rate **9.86 Hz**. No stream/decode errors, capture/frame gaps, queue/export drops or self-captures. The leader UART was left free for reset control, so its own boot Hello was verified in the following recording.
- `post-recovery-04/`: final 30 s with both UART sources, **293 a0→b0** and **285 b0→a0** captures. Both report firmware 0.2.1 and TX+RX roles; no framing/decode errors, capture/frame gaps or queue/export drops. Peer misses remain separately counted (6 and 7, respectively).
- All four sessions sealed successfully with segment digests verified. Unpaced replay completed for the final baseline (1,521 records), recovery (1,155 records) and post-recovery run (758 records). All **117 workspace tests**, including **14 policy tests**, passed with local socket permissions; no_std, strict Clippy and formatting passed. TX-only, RX-only, combined periodic TX/RX and pair feature configurations all passed firmware build/Clippy checks.

Audit JSON files retain per-epoch roles/counter deltas, directed-link identity/sequence/layout, native-byte uniqueness, timestamp samples, finalized Inspector health and segment integrity. CSI records retain the standard Probe, **not** the turn extension: exact round/timeout behavior is unit-test/source evidence; these recordings establish bidirectional CSI and recovery, not per-round timing telemetry or collision-free airtime. Two nodes cannot provide common third-party probes for shared-probe receiver alignment; there is no measured cross-receiver residual here.

## Repeat a trial

From `sensing/`:

```sh
cargo run --release -p csi -- hub --config bench/2026-10-21/dual-role/hub.toml --out bench/2026-10-21/dual-role/trial-01 --seconds 60
cargo run --offline -p csi-host --example audit_session -- bench/2026-10-21/dual-role/trial-01 > bench/2026-10-21/dual-role/trial-01-audit.json
cargo run --release -p csi -- inspect bench/2026-10-21/dual-role/trial-01
```

Use fresh output paths. Ports can change after reconnecting: identify MACs before editing configuration or flashing. Both boards continue their pair roles while powered, even without a recorder. No AP or Pi is required.

Next gates: labelled empty/still/walking trials at fixed positions; add a third participant for common-probe clock tests and develop a participant schedule for arbitrary N; sustained 90-minute wrap/recovery test; gain extraction and callback/RAM measurements. Pair synchronization is a tested starting point for mesh sensing, not completion of arbitrary-node scheduling, M5 or M7.
