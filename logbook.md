# Capstone Logbook

# 1: 2026-09-14 : First planning meeting
## content

Team met and chose a privacy-preserving fall-detection system using Wi-Fi CSI instead of cameras or wearables. Nodes around a room capture how a person changes RF paths; signal processing and ML distinguish walking / sitting / lying / falling, then the app alerts with room location. Stretch goal: post-fall respiration. Split work into RF hardware, app, ML, signal processing, and embedded/networking.

### Table 1: Last Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | First planning meeting: idea + workstream split | 2026-09-14 | 100% | Success | Locked in Wi-Fi CSI fall detection. Pipeline: TX → CSI RX nodes → processing → ML → temporal fall inference → app alert + location. Five workstreams assigned. |

# 2: 2026-09-17 : Supervisor secured, first ESP32 CSI hardware runs
## content

Secured our capstone supervisor. Emailed Prof. George Shaker (UWaterloo), who runs a closely related RF vital-sign / sensing project, to introduce the project and ask about overlap and advice.

Ran the first end-to-end hardware experiment for the stretch goal (post-fall vital signs) using two ESP32-WROOM-32 boards and the Espressif `esp-csi` examples (pinned rev `8633d67`, ESP-IDF 5.5.2, built in Docker). Backed up each board's original 4 MB flash, then flashed `csi_send` to TX (MAC `a0:b7:65:dc:01:68`) and `csi_recv` to RX (MAC `a0:b7:65:dd:e9:3c`). Link confirmed: ~100 pkt/s over ESP-NOW on channel 11 HT40, 190 subcarrier amplitudes per frame, RX streaming CSI at 921600 baud into the `csi-heartbeat` Python capture/analysis tool.

Recordings (180 s each unless noted, RX ~90 frames/s, ~23% over-the-air packet loss):

| Run | Condition | RSSI | Result |
| --- | --- | --- | --- |
| `empty-01` | Boards 1–2 m apart, nobody in room | −73 dBm | Heart-band spectrum is flat noise; heuristic gate still fired once (105 BPM) → false-positive floor. |
| `seated-01` | Subject seated between boards, Apple Watch ref ≈ 57–64 BPM | −70 dBm | Heart band indistinguishable from empty room. Breathing (~12/min) faintly visible (~1.7× empty-room power). Single gated candidate (117 BPM) was noise. |
| `chest-01` (62 s) | Both boards taped to chest, watch ≈ 80 BPM | −80 dBm | Whole-record peak 80 BPM (ratio 8), but runners-up at 53/49. Breathing 10/min dominant. |
| `chest-02` (115 s) | Same, boards moved late in run | −71 → −54 dBm | Breathing 13× heart-band power. Peaks pile at 48–57 BPM in every window regardless of breathing rate = filter leakage at the 48 BPM band edge, not heart rate. |

Findings:

- Hardware and capture pipeline work (no serial drops, timestamps clean). Chest motion / breathing is clearly detected once boards are near the body.
- Heart rate is not yet demonstrated. Two causes: (1) setup — every run had RSSI −70 to −80 dBm vs −35 dBm with boards side-by-side, so CSI amplitude is mostly receiver noise and packet loss comes in bursts that void 30 s analysis windows; taping the PCB antenna against skin detuned it further. (2) software — the 4th-order 0.7 Hz band-pass lets breathing energy (10–15× larger) leak into the bottom of the 48–150 BPM band, and breathing harmonics (10/min × 5, × 8…) land where heart rate lives, so the "~50 BPM" live readout was an artifact.
- Compared against IEEE 7980063 (PhaseBeat, Wang/Yang/Mao 2017): they used Intel 5300 NICs at 5 GHz, 400 pkt/s, phase *difference* between two RX antennas, and a directional TX antenna, plus Hampel detrending and a hard DWT band split at 0.625 Hz; they report 1.19 BPM median heart-rate error, degrading to 4.82 BPM with an omni antenna. Our single-antenna, amplitude-only, 90 Hz ESP32 setup is a weaker version of their amplitude *benchmark*, so breathing is realistic and heart rate is best-effort without hardware upgrades (directional antenna on a U.FL ESP32 variant, 200–400 pkt/s TX).

Tooling changes: `capture` now prints RSSI and a live trailing-window spectral estimate every 5 s (`--live-window`, `--live-every`), computed in a worker thread so serial reads never stall; `--condition` is free-form. Moved the repo out of iCloud Drive (`~/code/capstone/demo`) after iCloud repeatedly set the macOS hidden flag on `.venv`, which made Python skip the editable-install `.pth` file.

### Table 2: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Secure capstone supervisor | 2026-09-17 | 100% | Success | Supervisor confirmed. |
| All | Contact Prof. George Shaker re: related RF sensing project | 2026-09-17 | 50% | Success | Email sent; awaiting reply. |
| RZ | Flash esp-csi TX/RX firmware to two ESP32s and verify link | 2026-09-17 | 100% | Success | ~100 pkt/s, 190 subcarriers, original flash backed up per board. |
| RZ | First CSI recordings: empty room, seated, on-chest | 2026-09-17 | 100% | Roadblock | Breathing visible; heart rate not resolved. Root causes: RSSI −70 to −80 dBm, breathing leakage/harmonics in estimator. |
| RZ | Live heart-rate estimate + RSSI in capture tool | 2026-09-17 | 100% | Success | Threaded live estimate every 5 s. |
| RZ | Improve mounting to reach RSSI > −50 dBm; antennas clear of skin/metal | 2026-09-24 | 0% | Pending | Prerequisite for any heart-rate claim. |
| RZ | Estimator fixes: steeper high-pass / band-edge rejection, breathing-harmonic suppression, tolerate <1 s dropouts, 60 s live window | 2026-09-24 | 0% | Pending | Copy PhaseBeat pieces: hard 0.625 Hz split, Hampel detrend, best-subcarrier selection, 3-bin FFT interpolation. |
| RZ | Breath-hold validation run (90 s normal → 20–30 s hold → normal) with watch reference | 2026-09-24 | 0% | Pending | Decides whether the 80 BPM peak in `chest-01` is heartbeat or a breathing harmonic. |

# 3: 2026-09-21 : Meetings with Prof. George Shaker and advisor Ewen; elevator pitch
## content

Three meetings today: a guidance meeting with Prof. George Shaker, a check-in with our capstone advisor Ewen, and a team meeting to work out our elevator pitch.

Met with Prof. George Shaker for guidance. His group has worked on remote vital-sign tracking using radar, so his advice comes from a closely related sensing problem.

Architecture advice: look into open-source access points, and restructure the system around a single centralized receiver with a set of ESP32 transmitters spread around the room so that the collected CSI maps out the whole space, instead of the current one-TX/one-RX link. The receiver needs to be an open-source router because we must have access to the per-packet information to extract CSI; a stock router firmware will not expose it. He also suggested considering MIMO arrays / multi-antenna routers, which give multiple spatial channels per packet (and, as in PhaseBeat, make phase-difference methods possible).

Problem focus. He recommended concentrating on three problems, where a measurable improvement in any one would be a significant contribution:

1. Counting — distinguishing 1, 2, 3+ people in a room.
2. Localization — estimating where a person is in a room and tracking them across zones (e.g. living room → bed → washroom).
3. Positional / pose matching — classifying posture such as sitting down vs. standing up.

Decisions: we will attach a Raspberry Pi to an open-source router and use that as the centralized receiver, with the ESP32s acting as transmitters. Multi-antenna / MIMO-capable router hardware is preferred. This reframes the near-term work away from single-link heart-rate extraction (which the 2026-09-17 experiments and the PhaseBeat comparison showed is marginal on single-antenna ESP32 hardware) toward the counting / localization / pose problems that fit the fall-detection goal directly.

Also met with our advisor Ewen for a project check-in, then met as a team to discuss and draft our elevator pitch for the project.

### Table 3: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Guidance meeting with Prof. George Shaker | 2026-09-21 | 100% | Success | Recommended centralized RX + distributed ESP32 TX, open-source AP, MIMO; focus on counting, localization, pose. |
| All | Check-in with advisor Ewen | 2026-09-21 | 100% | Success | Project update. |
| All | Team meeting: elevator pitch | 2026-09-21 | 100% | Success | Discussed and drafted the elevator pitch. |
| All | Decide receiver architecture | 2026-09-21 | 100% | Success | Raspberry Pi + open-source router as centralized CSI receiver; ESP32s as transmitters. |
| All | Select open-source router / CSI-capable chipset with multi-antenna support | 2026-09-28 | 0% | Pending | Must expose per-packet CSI (e.g. Nexmon-class firmware or an OpenWrt-supported CSI tool). Confirm Raspberry Pi integration path. |
| All | Define evaluation for counting (1/2/3+), zone-level localization, and sit/stand pose | 2026-09-28 | 0% | Pending | Pick target metrics and data-collection protocol before buying hardware. |
| RZ | Re-scope ESP32 firmware to multi-TX role (unique IDs, staggered send schedule) | 2026-09-28 | 0% | Pending | Current `csi_send` example is single-TX; receiver must separate transmitters by MAC. |

# 4: 2026-09-24 : Capstone presentation
## content

Presented our capstone project: privacy-preserving Wi-Fi CSI fall detection, with the revised architecture (centralized open-source router + Raspberry Pi receiver, distributed ESP32 transmitters) and the focus problems from the Shaker meeting (counting, localization, pose).

### Table 4: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Capstone presentation | 2026-09-24 | 100% | Success | Presented project pitch and architecture. |

# 5: 2026-09-25 : Elevator pitch
## content

Presented our elevator pitch.

### Table 5: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Present elevator pitch | 2026-09-25 | 100% | Success | Elevator pitch presented. |

# 6: 2026-09-29 : Project summary and course deliverables
## content

Worked on the project summary, TCPS 2 certificate, individual contribution, and group work contract.

### Table 6: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Project summary | 2026-09-29 | — | In progress | |
| All | TCPS 2 certificate | 2026-09-29 | — | In progress | |
| All | Individual contribution | 2026-09-29 | — | In progress | |
| All | Group work contract | 2026-09-29 | — | In progress | |

# 7: 2026-10-01 : Meeting with Reem; rescope
## content

Met with Reem and rescoped the project. Next step is to contact caregivers to identify their pain points.

### Table 7: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Meeting with Reem | 2026-10-01 | 100% | Success | Project rescoped. |
| All | Contact caregivers to identify pain points | 2026-10-06 | 0% | Pending | |

# 8: 2026-10-06 : Nursing home visits
## content

Visited two nursing homes to talk with caregivers about their pain points.

### Table 8: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Visit nursing homes | 2026-10-06 | 100% | Success | Visited 2 nursing homes. |

# 9: 2026-10-07 : Follow-up with Prof. George Shaker
## content

Spoke with Prof. George Shaker again to get a contact at the Research Institute for Aging (RIA) so we can talk to more people.

### Table 9: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| All | Follow-up with Prof. George Shaker for an RIA contact | 2026-10-07 | 100% | Success | Asked for an introduction to the Research Institute for Aging. |
| All | Reach out to RIA contact for more interviews | 2026-10-14 | 0% | Pending | |

# 10: 2026-10-20 : Rust acquisition server, recording pipeline and live ingest
## content

Implemented the host acquisition foundation under `sensing/`, covering milestones M0–M3 in `sensing/PLAN.md`. The workspace separates the shared `no_std` protocol (`crates/csi-protocol`) from host acquisition (`server/csi-host`) and the `csi` command-line application (`server/csi`). Firmware will use a separate workspace. Rust handles acquisition, recording, timing and health; signal processing, tomography, ML and caregiver alerts are not implemented here.

Implemented functionality:

- **Wire protocol:** versioned binary messages for node identification and configuration reporting, CSI, cumulative node counters and host timing requests/replies. UDP carries frames with CRC protection; serial uses COBS framing with delimiter-based recovery after corruption. Protocol v2 adds optional per-record AGC (`u8`) and FFT (`i8`) gain. Existing v1 frames and M2 sessions still decode, inspect and replay, with gain absent. Golden-byte fixtures and property tests cover the formats and malformed input.
- **Recording and replay:** `csi record`, `csi replay` and `csi inspect`, with raw frame preservation, size/time-based segment rotation, SHA-256 integrity checks and a session manifest. Readers report incomplete recordings, missing/corrupt segments and torn tails. Re-recorded replay preserves raw segment bytes. A deterministic synthetic source injects loss, drops, corruption, duplicates, reordering, outages and reboots, with ground truth for checking health accounting.
- **Installation geometry:** versioned `geometry.json` records room/layout revision, node positions in metres, floor/upper tier and optional antenna orientation/notes. Recording validates and copies the survey; the manifest references its revision and hash. Inspection checks integrity and warns about missing node placements. Replay carries the geometry into derived sessions.
- **Receiver clock alignment:** shared broadcast probes pair receiver timestamps and fit relative offset/drift over a 60 s window, refitted every 10 s. Reference selection is deterministic, and receivers can connect through intermediate receivers when they do not share probes directly with the reference. Tests cover 32-bit timestamp wrap, reboot epochs and reference handover. Versioned mappings and residual percentiles are stored separately in `clock-fit.jsonl`; raw timestamps are never rewritten. `csi fit-clocks` also fits saved sessions.
- **Live acquisition:** `csi hub` reads configured serial/UART and UDP sources using TOML configuration. It records before forwarding to live consumers, uses bounded queues and counts subscriber drops without blocking recording. Serial inputs reconnect after unplugging; source and per-link stale states, boot epochs, framing/decode errors and loss/duplicate statistics are reported. Live clock samples and health history have explicit limits, while mapping history streams to disk. Ctrl-C, SIGTERM or a duration limit drains the queue and seals the session; recorder failures leave it incomplete.
- **Host timing and diagnostics:** correlated `TimeProbe`/`TimeEcho` exchanges map the receiver reference timeline to host monotonic time, with wall-clock anchors, RTT and fit uncertainty. Live status is written to `hub-status.json`, transitions to `hub-events.jsonl`, and host mappings to `host-clock.jsonl`. Usage and an example configuration are in `sensing/README.md` and `sensing/examples/hub.toml`.

Validation: all **97 workspace tests passed**, together with `cargo build`, `cargo check-nostd`, clippy with warnings denied and formatting checks. Separate loopback UDP and Unix pseudo-terminal tests each sent **3 receivers × 1,000 CSI records/s for 3 s**, with 384-byte CSI payloads and additional echo traffic. Each recorded all **9,000 CSI records** and verified segment integrity and receiver clock fits. The UDP test also stalled a live consumer and checked that its drops were counted while recording remained complete. Other tests cover serial unplug/reconnect, partial/corrupt framing, new boot epochs, stale links, foreign/oversized UDP inputs, unsolicited echoes, recorder failure and graceful shutdown.

Clock-window comparison on synthetic data (seed 21, 120 s, 3 receivers, 2 transmitters at 200 probes/s each, typical faults): the 60 s window recovered relative drift within **0.007824 ppm**, with worst parent-fit residual p99 **7.3705 µs**, worst pair alignment error **0.3620 µs**, and largest consecutive-fit jump **0.22538 µs**. Both 30 s and 60 s windows met the requested 1 ppm / 50 µs bars; 60 s provided more shared pairs and smaller mapping jumps in this test.

These are host software and synthetic-clock results, not a physical UART/RF throughput test or evidence of fall-detection accuracy. The current acquisition implementation has not yet been validated on boards. Gain extraction remains an integration gap: the pinned `esp-csi-rs` API does not expose the AGC/FFT fields for ESP32/S3, and compatibility with Espressif's extraction API still needs verification. Unknown gain is represented as absent. The next software milestone is M4: Arrow IPC/Parquet export and the Python reader. M5 requires firmware bring-up and a one-TX/two-RX bench run of at least one hour, including clock residual p99 below 1 ms. Adaptive schedule control remains design-only until M7.

### Table 10: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | M0–M1: Rust workspace and shared wire protocol | 2026-10-20 | 100% | Success | Shared `no_std` contract; CRC/COBS framing; v2 optional gain with v1 backward decoding. |
| RZ | M2: Raw sessions, deterministic replay and health inspection | 2026-10-20 | 100% | Success | Rotation/integrity checks, fault-injected synthetic source and byte-identical replay tests. |
| RZ | Versioned installation geometry | 2026-10-20 | 100% | Success | Survey copied into sessions, revision/hash checked, missing placements reported. |
| RZ | Shared-probe receiver clock fitting | 2026-10-20 | 100% | Success | 60 s window / 10 s refit; wrap, reboot, chaining and persistence tests pass. |
| RZ | M3: Serial/UDP live hub and host timing | 2026-10-20 | 100% | Success | 9,000/9,000 CSI records in each 3 s transport test; reconnect/stale states, bounded queues and consumer-drop accounting verified. Hardware validation remains separate. |
| RZ | Workspace verification | 2026-10-20 | 100% | Success | 97 tests pass; build, no_std, clippy and formatting checks pass. |
| RZ | M4: Arrow/Python handoff | — | 0% | Pending | Next software milestone; no Arrow publisher/export or Python reader implemented yet. |
| RZ | M5: Firmware, gain extraction and physical bench validation | — | 0% | Pending | Verify gain access and driver compatibility; disable Wi-Fi sleep, force HT MCS0 probes and complete the one-hour clock bench test. |

# 11: 2026-10-21 : First physical ESP32 TX/RX acquisition bring-up
## content

Identified two connected ESP-WROOM-32 boards as ESP32-D0WD-V3 revision 3.1 with 4 MB flash. Verified each board by MAC, saved complete original-flash backups and checked them against device digests before flashing. Added a separate Xtensa Rust firmware workspace using the shared protocol: fixed-channel ESP-NOW probes, native CSI over UART, bounded callback/export queues, Hello/Counters and TimeProbe/TimeEcho. The transmitter targets 100 Hz HT20/MCS0 long GI on channel 6; the receiver preserves native CSI and metadata with unknown gain absent. Both firmware roles build and pass clippy with warnings denied. Corrected duplicate broadcast-peer registration found during initial startup.

Flashed and verified both final images. A 60-second physical one-TX/one-RX recording produced **5,845 CSI captures at 97.65 Hz**. Every capture reported channel 6, HT20/MCS0 long GI and 256 native CSI bytes. Recorded **141 missing probe sequence numbers (approximately 2.36% between observed endpoints)**, with no capture/frame sequence gaps or pool/export drops. One initial partial-frame COBS error occurred at serial attachment; none occurred afterward. Each board answered 60 timing probes. The complete session's segment hash verified, and all 6,206 records replayed successfully.

Artifacts, board identities, verified backup hashes, flash provenance, raw session, inspection and repeatable recording commands are documented in `sensing/bench/2026-10-21/README.md`. This is an uncontrolled acquisition smoke test, not labelled motion/fall validation or completion of M5. Gain extraction, callback/RAM measurements, sustained wrap/recovery tests and two-receiver clock residuals remain pending. Next: fixed-placement empty/still/walking trials, then another RX for shared-probe alignment and a sustained run.

### Table 11: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Identify, back up and prepare two ESP32 boards | 2026-10-21 | 100% | Success | MAC-checked full-flash backups and verified TX/RX programming. |
| RZ | One-TX/one-RX physical acquisition smoke test | 2026-10-21 | 100% | Success | 5,845 CSI captures in 60 s; PHY/layout verified; raw recording and replay checked. |
| RZ | Complete M5 physical acceptance | — | — | Pending | Sustained two-RX clock test, wrap/recovery, gain extraction and callback/RAM measurements remain. |

# 12: 2026-10-21 : Dual-role ESP32s and packet-driven turn synchronization
## content

Extended the new Rust firmware so probe transmission and CSI reception are independent capabilities: TX-only, RX-only or both can use the same implementation. Both connected ESP-WROOM-32 boards now run TX+RX. The user reports approximately 70 cm separation on a desk, roughly in the same plane; placement and activity are not otherwise surveyed or labelled.

Implemented a known-pair request/response policy with **no fixed inter-node guard**. A versioned turn tag in the over-air probe padding correlates leader boot epoch and round; the responder sends once per fresh request, and the leader proceeds on an exact-round response or bounded recovery timeout. Timeout adapts to RTT and jitter, with loss backoff (30–500 ms, initial 150 ms). The 100 ms leader interval is a rate cap. Existing CSIp probes, UART protocol v2 and native CSI layout remain unchanged. Added the shared no_std `csi-node` policy and a read-only directed-session audit example. Subagent driver/code reviews identified the earlier pending-send cancellation issue and led to preserved send futures, a 2 s missing-completion reset, foreign-mailbox filtering and stale-response rejection.

Final 60-second baseline recorded **586 a0→b0 captures (9.79 Hz)** and **575 b0→a0 (9.61 Hz)**, with both TX/RX roles reported. Every capture used channel 6, HT20/MCS0 long GI and 256 native CSI bytes. Missing probe interiors were **12/598 (2.01%)** and **11/586 (1.88%)**, respectively. No framing/decode errors, self-captures, source/probe mismatches, capture/frame sequence gaps or pool/export drops were recorded.

In a 90-second recovery recording, reset the leader while continuously recording the follower. The follower remained in the same boot epoch and automatically resumed capture/replies when the leader appeared with a new epoch and sequence zero. The gap between the last pre-reset and first post-reset capture was **623.923 ms**. A final 30-second two-source recording confirmed both directions remained active. All sessions sealed with verified segment digests; baseline/recovery/post-recovery replay succeeded. All **117 workspace tests**, including **14 policy tests**, passed, together with no_std, strict Clippy/format checks and builds for TX-only, RX-only, combined periodic TX/RX and pair configurations.

Plan, image/source hashes, flashed-region verification logs, reset action timestamp, raw sessions and audits are in `sensing/bench/2026-10-21/dual-role/`. Original verified 4 MB board backups and fixed-role images remain preserved. These results establish two-node acquisition and recovery. Turn synchronization does not equal absolute clock synchronization; the over-air turn tag/RTT estimates are not retained in standard CSI records, so exact correlation/adaptation is policy-test/source evidence. The missing-completion watchdog was reviewed but not fault-injected. Arbitrary-node scheduling, shared-third-party-probe clock validation, gain extraction, callback/RAM measurements, sustained radio-wrap tests and labelled motion/fall evaluation remain pending.

### Table 12: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Independent TX/RX capabilities and pair turn policy | 2026-10-21 | 100% | Success | Both boards run TX+RX; packet-correlated turns and adaptive timeout without fixed guard. |
| RZ | Two-board baseline and leader-reboot recovery | 2026-10-21 | 100% | Success | Bidirectional CSI at ~10 Hz; automatic recovery with a 624 ms capture gap. |
| RZ | General mesh scheduling and complete M5/M7 gates | — | — | Pending | Pair pilot complete; larger participant schedules, common-probe clock tests and sustained/measurement gates remain. |

# 13: 2026-10-21 : Matched-rate ESP32 acquisition mode trials
## content

Ran 16 physical trials (45 s each, two repeats per condition) comparing fixed-role A→B, fixed-role B→A, independent dual TX/RX and packet-driven turns at 10 and 50 probes/s targets. The user reports approximately 70 cm desk separation, roughly in the same plane. The agent did not move the boards; room activity was not controlled or labelled. The first sweep ran forward and the second in reverse order, with verified firmware flashes/restarts and 5 s settling before each recording. Added a validated compile-time interval setting so all modes could use the same requested rate and source version (0.2.2).

Collected **31,482 CSI measurements over 12 minutes of trial recording**. At the 50 Hz target, two-run mean received rates were **47.96/48.49 CSI/s** for the two fixed-role directions, **48.97/49.22** for independent dual transmission and **48.95/48.60** for packet-driven turns (A→B/B→A). Interior sequence-loss fractions were **3.67%/2.60%**, **1.55%/1.03%** and **0.81%/0.71%**, respectively. Actual offered TX rates were reported separately: the pair offered slightly less traffic, and its follower only transmits after receiving leader requests. These loss values cannot by themselves establish better sensing coverage or collision avoidance.

Independent transmission produced slightly faster, more even sampling in this pilot. Worst observed capture gaps at 50 Hz were **81 ms** for independent dual and **114 ms** for pair turns. At 10 Hz, the two-way modes were similar (approximately 9.7–9.8 CSI/s per direction). RSSI changed substantially across the sweep, including 14–16 dB changes between fixed-role repeats; scheduling cannot be isolated as the cause of the observed loss ranking. Two desk repeats do not select the final star/mesh architecture or establish activity/fall-detection performance.

Every session sealed with verified segment hashes, expected source roles/directions, one epoch per board and consistent channel 6 / HT20 / MCS0 / long-GI / 256-byte CSI layout. There were **12 framing/decode errors within the first second of attachment and zero afterward**, with zero missing recorded frames/captures, queue/export drops, self-captures, source/probe mismatches or timestamp regressions. RX queue high water was 1/32. Full workspace tests, no_std, strict lint/format checks and the matched firmware builds passed.

Saved raw sessions, build/flash logs, registry, per-session audits, automated validation, detailed comparison tables and a standalone plot under `sensing/bench/2026-10-21/mode-comparison/`. Both boards were finally flashed/verified in **50 Hz packet-driven pair mode** to retain coordination while increasing sampling density. Its separate 30 s final check delivered **47.45/47.69 CSI/s** (B→A/A→B), with no errors after initial attachment or queue/export drops. Independent 50 Hz remains an available profile for trials prioritizing more even sampling. Empty/still/walking trials and larger-node clock/topology tests still need explicit labels and participation.

### Table 13: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Matched-rate acquisition mode comparison | 2026-10-21 | 100% | Success | 16 trials; raw sessions, rate/loss/gap/RSSI metrics and plots preserved. |
| RZ | Verify and leave coordinated 50 Hz pair ready | 2026-10-21 | 100% | Success | Both images MAC-checked, flashed and verified; final bidirectional recording passed. |
| RZ | Controlled activity and larger mesh validation | — | — | Pending | Requires labelled activities, more nodes and stable/repeated placements/RF conditions. |

# 14: 2026-10-22 : Lower-level validation metrics for a fall-sensing foundation
## content

Recorded a proposed validation plan connecting acquisition design to preservation of useful sensing information. The primary engineering question is whether the system preserves a brief, known CSI change with usable timing, magnitude and multi-link context, while avoiding artificial changes. Packet rate/loss and the completed two-board mode comparison alone cannot establish fall-detection accuracy or select the final star/mesh architecture.

Proposed measurements and experiments:

- **Coverage of short changes:** sweep pulse duration and onset phase through sampling schedules and random/burst/correlated loss. Report sample counts, longest blind interval and the fraction of events with enough valid samples on enough links. A 100 ms pulse at uniform 50 Hz has about five samples; a 100 ms acquisition outage can erase it. These durations are engineering stress cases, not validated fall timescales.
- **Distortion:** inject known amplitude changes, oscillations and frequency sweeps; measure recovered peak, energy, onset, duration and frequency error against an ideal reference. Use the curves to set sampling, resampling, filtering and jitter budgets.
- **Artificial changes:** hold the underlying channel constant while injecting gain/phase steps, quantization, clipping and invalid samples. Measure spurious peaks or motion-feature energy and verify explicit validity/unknown metadata. Gain remains unknown in the current receiver.
- **Aligned, fresh links:** inject clock offset/drift, wrap, restart, delay and correlated outages. Measure alignment error against ground truth, sample skew and the fraction of fusion windows containing enough fresh links. Timing alignment does not make radios phase coherent.
- **Freshness under load:** enforce real wire rates and bounded buffers, then inject export/disk/consumer stalls. Measure capture-to-consumer age, tail latency, queue occupancy, drops, starvation and recovery. Avoid treating loss-free but stale delivery as sufficient.

Confirmed the current six-node capacity estimate: if each node hears five peers transmitting 50 probes/s, it receives 250 CSI records/s. Current 256-byte CSI produces a 336-byte frame, or approximately 339 bytes with conservative COBS overhead/delimiter. This is 84,750 bytes/s per receiver against 92,160 bytes/s at 921600-baud 8N1, approximately **92% utilization before control traffic**. This is an analytical load case, not a measured six-node result. Synthetic tests must enforce the UART byte limit; host-speed pseudo-terminal throughput does not establish physical UART capacity.

Identified two implementation gaps: the current synthetic source uses noise rather than known sensing patterns, and existing comparison interval statistics use callback node timestamps rather than native radio receive ticks. Proposed order: audit native-radio versus callback timing, add deterministic structured CSI and known-signal replay, sweep coverage/distortion, then run byte-limited six-node load and artifact/alignment stress tests. Keep reference signal processing offline without changing production Rust's acquisition-only scope.

The detailed plan, metric definitions, capacity calculation, provenance requirements and evidence limits are in [sensing/SIGNAL_VALIDATION.md](sensing/SIGNAL_VALIDATION.md), linked from the sensing README and implementation plan. **This entry documents proposals only: no new synthetic experiments were implemented or passed.** Physical controlled-motion and labelled fall/non-fall evaluation remain separate requirements; changing ESP-NOW payloads does not itself synthesize a motion-induced propagation channel.

Implementation-planning follow-up: [sensing/SIGNAL_VALIDATION_PLAN.md](sensing/SIGNAL_VALIDATION_PLAN.md) now specifies five phases, concrete code locations, test matrices, output artifacts and completion criteria. The first implementation slice is native-radio timestamp auditing, structured known-signal replay and short-change coverage/distortion comparison using the existing measured traces. Follow with a byte-limited six-node simulator and artifact/alignment stress. Comparisons must report both equal per-transmitter rate and equal aggregate probe budgets, preserve conditional pair-response behavior, and separate measured replay from hypothetical N-node scheduling. Rank designs by the resulting coverage/distortion/freshness tradeoffs rather than an invented aggregate score. No tests were implemented as part of this planning follow-up.

### Table 14: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Document lower-level validation metrics and test plan | 2026-10-22 | 100% | Success | Metrics, timing correction, six-node UART estimate and proposed order recorded. |
| RZ | Implement native-timestamp audit and structured signal replay | — | 0% | Pending | Existing generator uses noise; known-signal preservation tests are not implemented. |
| RZ | Run byte-limited load, artifact and alignment sweeps | — | 0% | Pending | Proposed experiments; no results or acceptance thresholds claimed. |

# 15: 2026-10-23 : Initial Python CSI handoff
## content

Implemented the first data-handoff task from [sensing/PLAN.md](sensing/PLAN.md): `csi export` writes sealed sessions to Arrow IPC or Parquet, and the new `python/` package exposes the same batch API for those files and the hub's optional `--arrow-socket` stream. [sensing/EXPORT.md](sensing/EXPORT.md) documents installation, commands, schema, quality counters and limitations.

The versioned `csi-export/1` schema preserves native CSI, TX/RX/boot/schedule identities, sequences, radio/callback/arrival times, nullable relative clock mappings, radio configuration, nullable gain and geometry provenance. The initial original-ESP32 HT20 decoder exports signed real/imaginary values in separate indexed LTF blocks, retaining and masking null bins and the hardware-invalid first word. Unsupported layouts and captures before the first observed receiver configuration remain raw-only with an explicit reason. No gain compensation, normalization, phase correction, DSP or classification is applied.

Live delivery runs downstream of raw recording, with bounded input and per-client queues, whole-batch drop counters, stalled-client disconnects and `arrow-status.json`. Raw recording continues when consumers stall. Offline export validates original segment hashes/counts and geometry, refuses incomplete/damaged sessions and existing output directories, and produces a finalized health report. It recomputes causal receiver mappings without changing raw recordings or applying stored host-clock mappings.

Validation includes golden signed-I/Q/LTF/index/mask checks; damaged/incomplete export rejection; radio-reset and clock-capacity separation; both output formats; real-recording Python byte/mask/link checks; equality after re-recorded replay; local Arrow socket delivery agreement; a stalled reader while raw recording seals; and a Python-to-CLI socket startup/shutdown check. Workspace tests, `cargo check-nostd` and strict Clippy passed. Python integration checks used existing one-way and two-way desk recordings. Live transport tests used synthetic or silent loopback inputs; no new firmware flashing or RF trial was performed.

Next is the labelled collection workflow so teammates can attach trial/activity intervals and configuration provenance. Broader CSI layouts, applying host-time fits with uncertainty, gain extraction and physical one-TX/two-RX timing acceptance remain open. This handoff is not evidence of fall-detection or tomography accuracy.

### Table 15: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Initial Arrow/Parquet export, Python reader and live socket | 2026-10-23 | 100% | Success | Original ESP32 HT20 decoder; raw fallback, explicit validity/timing and stalled-reader isolation verified. |
| RZ | Reproducible labelled-trial workflow | — | 0% | Pending | Next task in the sensing roadmap. |
| RZ | Wider layouts, host capture-time mapping and gain extraction | — | 0% | Pending | Initial handoff retains unknowns rather than inventing calibrated values. |

# 16: 2026-10-23 : Labelled collection and configurable live Python delivery
## content

Implemented `csi collect` and the Python `Trial` reader to unblock ordinary-activity collection. A new bundle separates the raw session, snapshotted source config/geometry/application images/build manifests, session-bound labels, split assignment, Arrow export and quality report. The collector records declared mode/rate and observed firmware/configuration separately; it does not configure or flash nodes. [sensing/COLLECTION.md](sensing/COLLECTION.md) includes editable two-board templates and an empty/walking/standing/sitting walkthrough.

Operator `begin`, `end`, position and note commands are stamped on the hub's exact host monotonic source clock before entering the command queue. Activity switches share one boundary; EOF/stop/signals/deadline mark any still-open activity truncated. Python preserves half-open intervals, trims declared uncertainty, excludes truncated intervals by default, and requires explicit `host_arrival_proxy` matching. Capture-to-host alignment and physical operator/video timing remain unvalidated, so these annotations do not yet establish precise fall-event capture times.

Both `hub` and `collect` expose `--arrow-socket` with the existing `csi-export/1` Arrow IPC format; `--arrow-flush-ms` now accepts 5–1000 ms, default 100. A 20 ms setting supports smaller live batches at higher processing overhead; it is not an end-to-end latency guarantee. Python uses `live_batches()` during collection. Operator labels remain in the separate timestamped event log. Raw recording remains ahead of bounded lossy delivery.

Quality summaries now include per-link decoder/bin validity, mapped-clock and gain coverage, missing expected links, source identity and role/channel checks, plus first/last observed node counter windows. Deltas exclude pre-trial totals, ignore late snapshots and become unavailable after counter rollback; uncovered start/end intervals remain explicit. The health inspector's latest node counters are still cumulative since boot.

Validation covers source/survey/rate mismatches, non-overwrite behavior, interval switching and truncated closure, deadline shutdown while stdin stays open, labelled simulated binary CSI through UDP→record→export→quality, counter-window semantics, Python provenance/session rejection, uncertainty boundaries, commands after stop and collection with a 20 ms live UDS. No new ESP32 RF collection, flashing, timing calibration, classifier or tomography result was produced.

### Table 16: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Initial reproducible labelled-trial workflow | 2026-10-23 | 100% | Success | Collector, provenance, labels, quality, Python reader and ordinary-activity guide implemented. |
| RZ | Configurable live Arrow delivery during collection | 2026-10-23 | 100% | Success | Same UDS schema/API; 20 ms configuration tested locally; bounded queues remain. |
| RZ | Physical operator walkthrough and capture-to-host label alignment | — | 0% | Pending | Host-arrival proxy only; transport latency and operator timing still need validation. |

# 17: 2026-10-23 : Physical ESP32 live Python and collection trials
## content

Verified the two connected boards from fresh binary serial traffic: `a0:b7:65:dd:e9:3c` on `/dev/cu.usbserial-3` and `b0:b2:1c:0b:07:1c` on `/dev/cu.usbserial-0001`. Both reported original ESP32 firmware `0.2.2`, TX/RX enabled, channel 6 and 256-byte HT20 LLTF+HT-LTF CSI using protocol v2. The sandbox initially blocked serial access; the authorized serial run succeeded. No firmware flash or reset was needed.

Ran four 15-second normal acquisitions in 100/20/20/100 ms Arrow flush order, followed by a 15-second 20 ms run with a deliberately stalled second client. All **7,127 real CSI captures** reached the healthy Python reader and matched offline core identity/timestamp/native-byte hashes. Raw sessions and trial bundles sealed; the `Trial` reader reopened provenance, labels and quality and selected the software test interval. Capture rates in normal runs were 46.6–48.3 Hz per direction against the declared 50 Hz target.

After excluding each run's first two seconds, complete-frame hub-arrival-to-Python-yield delay was 58.3–58.7 ms median / 105.6–105.7 ms p95 with 100 ms flushing, versus 17.0 ms median / 29.3–29.4 ms p95 with 20 ms flushing. This measures the host pipeline before complex/model processing; it excludes radio capture/callback/UART delay before hub arrival. It does not calibrate physical event labels.

The stalled client's queue dropped 10 batches before disconnecting; the healthy reader received all 1,406 captures in that run and recording sealed. Recorder/publisher input drops, frame/capture sequence gaps and observed pool/export counter-drop deltas were zero. Five framing rejects occurred during initial mid-stream attachment at 129.8–132.8 ms, with no stream/decode errors after one second. The runs still contained 131 probe-sequence gaps before the delivery stage; acquisition rate and radio/callback loss remain separate from delivery integrity.

Evidence and reproducible harness: [sensing/bench/2026-10-23/live-python-051450/README.md](sensing/bench/2026-10-23/live-python-051450/README.md). Geometry is explicitly unsurveyed and `transport_test` labels are automated phase markers, not human activity ground truth; these recordings are excluded from training. Physical ordinary-activity labels, capture-to-host timing, native timestamp auditing and third-board shared-probe validation remain open.

### Table 17: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Verify real two-board CSI through collection/live Python/export | 2026-10-23 | 100% | Success | 7,127 captures matched; both directions active; raw/trial sealing and mask/provenance checks passed. |
| RZ | Compare live batch delay and stalled-reader isolation | 2026-10-23 | 100% | Success | 20 ms setting reduced observed host-pipeline median to 17 ms; stalled client did not lose healthy-reader captures. |
| RZ | Surveyed/person-labelled collection and capture-time calibration | — | 0% | Pending | Transport-only test; no physical activity or tomography claim. |


# 18: 2026-10-24 : Wireless batching and piggyback transport
## content

Implemented an initial two-node wireless export path. Direct ESP-NOW sensing packets can carry batches of the receiver's original binary protocol-v2 records; a configured gateway forwards peer batches and its own collected readings through Wi-Fi/UDP to the Linux/macOS Rust hub. Python consumes the existing UDS Arrow stream. Wireless builds do not run UART CSI export or UART requests. Direct action-frame/source-MAC and sensing-flag checks keep AP-forwarded traffic and export-only packets from becoming attributed sensing observations. DSP remains entirely on the host.

The separate `csi-wireless/1` carrier has a complete-batch CRC, original frame CRC/identity validation, probe/turn metadata, batch sequence, destination and relay-loss telemetry. Batches are bounded at 1,200 bytes/16 records, with a default 50 ms construction deadline and bounded capture/ready/relay queues. This deadline does not bound capture-to-Python latency. Local send/queue failures are counted; broadcast/UDP delivery is not acknowledged or retried. Raw record kind 3 preserves the entire datagram. Inspector, clock fitter, audit and live/offline export expand validated original observations, preserving native bytes and shared host arrival stamps. Health reports expose piggyback/export-only batches and the latest relay-drop count. Host timing requests target and route to the original receiver through the gateway.

Built gateway, peer and legacy firmware and flashed both freshly MAC-identified ESP32s with verified original backups and image digest checks. USB port names had swapped; the flash guard rejected the initial mismatches before writing. Test credentials and embedded-credential images are ignored/private, including any collection bundle that copies those images.

Software validation: 134 Rust workspace tests, no_std, clippy with warnings denied, formatting and six Python tests passed. Carrier tests exercise limits, truncation/mutation, inner CRC/identity and arbitrary input. Local UDP/UDS integration validates raw preservation, finalized gaps, node-targeted probes, corrupt endpoint rejection, single-use timing-echo correlation and native-byte/index/host-time equality between live Arrow and offline export. These checks validate software, not RF delivery.

Physical wireless acceptance is **pending**: the UDP-only 25-second session sealed with zero records. The gateway's active scan found 18 APs and zero exact configured-SSID matches; association returned `NoAccessPointFound`. The computer is connected on 5 GHz; the boards need an accessible 2.4 GHz SSID. No wireless rate, loss, freshness, fall or tomography result is claimed. Next: establish association, verify both directed links/piggyback forwarding and targeted echoes, compare live Python against offline recordings, then compare 20/50/100 ms node batching at fixed host flushing. General N-node scheduling, shared-probe timing with a third board, OTA/configuration management and capture-time label calibration remain open.

Implementation and setup: [sensing/WIRELESS.md](sensing/WIRELESS.md). Evidence and resume checklist: [wireless bench](sensing/bench/2026-10-24/wireless/README.md).

### Table 18: This Weeks Update

| Responsible Student Initial | Task Item | Expected Date | Current % | Status (Success or Roadblock) | Comment |
| --- | --- | --- | --- | --- | --- |
| RZ | Initial wireless carrier, batching, gateway and uniform Python handoff | 2026-10-24 | 100% | Success | Implemented and host-tested; both firmware variants built/flashed. |
| RZ | Physical wireless CSI acceptance | — | 0% | Roadblock | Configured SSID absent from ESP32 2.4 GHz scan; no UDP data received. |
| RZ | Wireless batch-age comparison and larger mesh validation | — | 0% | Pending | Requires working association, followed by measured RF/transport trials. |


# 19: 2026-10-26 : ESP-NOW remote nodes without Wi-Fi association
## content

Separated the wireless gateway and remote roles in firmware 0.3.1. Remote builds no longer read the ignored Wi-Fi credentials file, require a SSID/password/host IP, spawn the association task or wait for AP connectivity. They configure a required `CSI_RADIO_CHANNEL` (supported range 1–11), then start batching and direct ESP-NOW communication immediately. The radio controller is retained for the lifetime of the remote tasks. Only the gateway joins Wi-Fi, follows the AP channel, and forwards batches to Rust over UDP; the existing binary carrier and Python Arrow handoff remain unchanged.

The remote channel must match the gateway AP's actual channel. Automatic discovery/channel migration is not implemented; use a fixed AP channel for physical tests. This removes the remote credential/association dependency but does not solve the gateway's earlier missing-AP problem. A separate Linux-integrated ESP-NOW adapter with SPI/UART is an architectural option, not part of this change.

Validation: role-specific host tests cover no remote credential/host-address requests, explicit channel requirements, invalid channel rejection, and retained gateway network requirements. The actual build script was executed beside a credentials file and confirmed to ignore it in remote mode; a remote release ELF was checked to exclude the actual local gateway SSID/password. Remote, gateway and legacy pair release builds plus carrier/host integration checks pass. No ESP32 serial devices were visible, so no new flash or RF trial was performed. The earlier 0.3.0 on-device images remain the last verified flash state.

Build/configuration instructions and pending RF gates are updated in [sensing/WIRELESS.md](sensing/WIRELESS.md) and the firmware README.


# 20: 2026-10-26 : Narrow delivery scope to star-first collection
## content

Revised the active roadmap around multiple pre-flashed ESP32 transmitters and one central CSI receiver at the laptop/Linux hub. Retain the existing mesh/pair/piggyback and clock-fit code for later experiments. A single receiver removes the immediate distributed receiver-clock alignment and remote CSI forwarding work; transmitter scheduling, timestamp continuity, loss and label timing still need validation. Star is a delivery choice, not a claim of better fall or tomography accuracy.

The first gate is the receiver: a generic Wi-Fi interface or UDP socket does not provide CSI. This machine currently runs macOS with Wi-Fi identifier 0x14E4/0x4388; no native CSI backend for it is implemented/validated here. FeitCSI's primary documentation describes Linux AX200/AX210 capture with a customized driver; Nexmon CSI lists exact supported chip/firmware combinations. Neither establishes an already working ESP32-to-this-laptop link. A native receiver also needs a layout/precision-aware adapter rather than treating non-ESP32 samples as the existing original-ESP32 int8 layout.

Planned following gates: a bounded ten-second discovery roster, one reusable transmitter image without compiled peer pairs, hub-owned start/stop with command acknowledgements/retries/run identities and probe leases, integration with existing collection/provenance/labels/Arrow output, then a teammate kit walkthrough and multi-transmitter acceptance. Discovery is limited to reachable nodes on the configured channel/control network. Existing begin/end label commands do not control node transmission. Tomography and ML are Python-side modes over the same acquisition data, not separate embedded implementations.

Updated [sensing/PLAN.md](sensing/PLAN.md) with the active S0–S4 sequence and acceptance gates. No new discovery, node-control or native laptop-capture code is claimed; no driver installation, firmware flash or RF trial was performed for this decision. Receiver hardware/OS selection is pending. A common-endpoint star has fewer spatially diverse links than distributed receivers; tomography remains an experiment requiring surveyed geometry and measured coverage.


# 21: 2026-10-26 : Full implementation plan for the USB ESP32 star kit
## content

Selected option 2: one central ESP32 receiver attached by USB to the MacBook, with the remaining ESP32s powered as wireless transmitters. Rust owns acquisition/control/recording and Python owns processing. This removes native laptop/Pi CSI-driver selection and existing Wi-Fi-network provisioning from the first-release path. Existing mesh/pair/piggyback implementations are retained for later.

Expanded the existing [sensing/PLAN.md](sensing/PLAN.md) into a complete proposed boot-to-record design. RX boot starts a ten-second ESP-NOW broadcast discovery window; idle TXs ACK with randomized response slots, identities, boot epochs and capabilities. Repeated windows handle collisions and late power-on. The RX retains a bounded inventory/snapshot for a Mac client attaching after boot. START/STOP use application ACKs, retries and run ownership. A central one-outstanding-grant scheduler coordinates sensing without shared TX clocks; host/TX leases stop transmission after failures. Discovery/control traffic is excluded from CSI observations.

The plan includes file-only, genuine stream-only and file+UDS output, explicit Arrow framing/session identity, consumer readiness, bounded backpressure and stream-only stop-on-primary-failure. A file descriptor abstraction does not erase the difference between a raw archive and decoded Arrow, or between file durability and socket loss. Current host code always creates a recorder directory and writes publisher status there; stream-only needs a refactor. Current trial validation also assumes one physical input source per node; the kit instead needs one RX input source and separate remote TX participants.

Specified proposed CLI/REPL/JSON control, raw star-control journaling, firmware feature boundaries, conservative rates and serial admission, physical placement/label limitations, staged P0–P6 implementation and deterministic/integration/physical acceptance tests. Defaults are five TXs at 10 requested probes/s each, pending RF validation; five × 50 Hz would approach the current 921600-baud serial limit. No new firmware/control/output feature is claimed as implemented by this planning task. Updated roadmap links and collection guidance; retained older evidence and mesh acceptance backlog.
