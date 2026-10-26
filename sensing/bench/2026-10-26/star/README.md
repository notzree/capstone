# USB star implementation and two-board bench, 2026-10-26

The star kit workflow is implemented and exercised on one TX and one USB RX. This bench establishes transport/control behavior. Five-TX physical scaling, true power-cycle acceptance and the 90-minute wrap run remain pending.

## Current prepared kit

| Role | Factory MAC | Current USB port | Image |
| --- | --- | --- | --- |
| RX | `a0:b7:65:dd:e9:3c` | `/dev/cu.usbserial-3` | `firmware-release-v2/star-rx.bin` |
| TX | `b0:b2:1c:0b:07:1c` | `/dev/cu.usbserial-0001` | `firmware-release-v2/star-tx.bin` |

Both are original ESP32-D0WD-V3 / ESP-WROOM-32 with CP2102 bridges. Live MAC identification found the port names had swapped from the previous bench. The existing verified full-flash backups were hash-checked before every flash. `v2-flash-star-rx.log`, `v2-flash-star-tx.log` and `firmware-release-v2/flash-manifest.json` preserve image/source/compiler hashes and successful device verification. The two star images have no compiled peer MAC or network credentials. Kit 1, fixed channel 6, HT20/MCS0, 921600-baud UART.

Use [the teammate guide](../../../STAR.md) for operation and firmware preparation. `firmware-release-v2/` is the final supported image pair. Earlier `firmware*` directories and failed/preliminary trials are retained as development evidence.

## Final-image physical results

Requested rate is **10 probes/s per TX**, one TX here. `summary.json` records grant opportunities/timeouts and capture interval tails.

| Trial | Mode / trigger | Native captures | Live rows | Offline rows | Confirmed TX STOP |
| --- | --- | ---: | ---: | ---: | --- |
| `v2-both` | file + stream, 10 s | 97 / 100 opportunities | 97 | 97 | yes |
| `v2-stream` | genuine stream-only, 8 s | 78 / 80 opportunities | 78 | — | yes |
| `v2-equality` | file + stream, 5 s | 46 / 50 opportunities | 46 | 46 | yes |
| `v2-disconnect` | primary closes after first batch | 1 | 1 | — | yes, run stops early |
| `v2-file-primary-loss` | file + stream; primary closes after first batch | 46 | 1 | 46 | yes; recording continues to duration |
| `lifecycle-v2/manual-stop` | explicit STOP before 60 s limit | see bundle | — | retained | yes |
| `lifecycle-v2/eof-stop` | stdin closes while measuring | see bundle | — | retained | yes |
| `host-lease` | host process suspended 3.5 s, USB held open | see bundle | — | retained | yes; `LeaseExpired` |

All final mode trials reported zero capture/export drops and zero host sink-input drops. All CSI layouts decoded; received PHY tuples were `(sig_mode=1, mcs=0, cwb=0, channel=6, native_len=256)`. Unknown AGC/FFT gain remains null. Missed RF/grant opportunities are reported separately, not hidden as successful samples. The 10-second both run had three missing opportunities; its capture interval p50/p95/p99/max was 100.146/107.726/202.969/205.887 ms. Requested 10 Hz is a transport profile, not a model-training recommendation.

`check.py` drives the real CLI and the Python `live_batches` schema acknowledgement. The tightened `v2-equality` comparison requires exact equality of native bytes, RX/host timestamps, sequence/index fields, complex real/imaginary values, LTF labels, subcarrier indices, pilot flags and validity masks. It indexes required columns directly; missing comparison columns fail. Live/offline mismatches: zero. The stream-only runs have no session directory or Rust status files: their `*-result.json` / control logs are explicit bench captures of the ephemeral final status, written by this test harness.

## Discovery, restart and failure evidence

`discovery-results.json` and `discovery-00..09.jsonl` archive ten EN-reset/discovery cycles on `firmware-release/`, alternating TX-first/RX-first ordering. All ten produced the exact single-TX roster, new boot epochs and no measurement start. Cycle 2 attaches the host after the ten-second window and receives the cached roster. These are EN-reset tests, not physical unplug/power-cycle tests; the later v2 change corrects run/STOP roster state, not discovery scheduling.

`lifecycle-v2/manual-and-crash-control.jsonl` exercises explicit rediscovery, START, manual STOP, then another START in the **same serial attachment**. `after-crash-and-eof-control.jsonl` exercises reconnect without automatic resume and EOF-triggered STOP. The deliberately killed recorder leaves `crash-incomplete/raw/manifest.json` incomplete. Opening the separate Python serial monitor after that kill reset the CP2102-connected RX; that particular passive capture cannot establish a no-reset host-lease bound.

`host_lease_check.py` avoids that confound by using SIGSTOP/SIGCONT on the client while keeping its existing USB FD open. The final status reports `radio_stop_reason=LeaseExpired`, a confirmed TX STOP, the same RX boot epoch and no automatic resume. Unit tests separately assert the local two-second RX host lease and bounded three-second TX lease; this bench does not claim calibrated capture-to-host timing.

Preliminary `first-file`, `paced-file`, `final-*`, `release-*` and the first failed lifecycle attempt preserve bugs found and corrected: a >128-byte UART START overflow, duration-boundary STOP acknowledgement, missing initial Hello, role-identity fingerprint ambiguity, and a confirmed-stopped TX incorrectly left busy in the RX roster. Final-image rows above are the acceptance evidence for the corrected run paths.

## Software validation

- Host workspace build/test, no_std shared-crate check, clippy with warnings denied, formatting and diff whitespace checks.
- Star envelope golden layouts, strict malformed/version/kind/CRC handling, bounded packets and property tests; existing acquisition v1/v2 and wireless fixtures remain supported.
- Policy tests cover ownership, reboot/identity fencing, discovery capacity/backoff, duplicate/conflicting commands, stale grants, rollback, timeout quiet intervals, leases and token exhaustion. A five-TX fake-clock test produces ten sequential probes per TX in one second with fixed round-robin pacing; this is a software test, not five-node RF evidence.
- Controller tests cover no-sink/overwrite rejection before RF, primary schema readiness, stream-only filesystem absence, primary disconnect, file/live completion, labelled `csi-trial/2` provenance, and three repeated START/STOP cycles without rediscovery.
- Seven Python integration tests passed with both the preserved `dual-role/baseline-02` recording and `v2-both`; old trial/UDS workflows, Arrow/Parquet, masks, provenance and explicit arrival-label semantics remain covered.
- `build-matrix.json` records successful legacy TX/RX/pair/wireless/gateway builds and rejection of conflicting star features. Final star images were built separately with the pinned Xtensa compiler; see their logs and manifest.

## Remaining acceptance

Use two/five actual TX boards, true power-order/unplug trials, late TX power-on and full-kit rediscovery, wrong kit/second-RX contention, TX loss/reboot, deliberately stalled consumers, injected disk-full errors, and a 90-minute timestamp-wrap run. Surveyed geometry/activity collection still needs a real ordinary-activity walkthrough. The implementation supplies no classifier or tomography solver, and these transport results establish neither.
