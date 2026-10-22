# `sensing/` implementation plan

Status 2026-10-21: M0–M3 are implemented. Initial original-ESP32 physical TX/RX bring-up and a packet-driven dual-role pair are implemented; see `firmware/README.md` and `bench/2026-10-21/dual-role/`. This does not complete M5 hardware acceptance or M7's arbitrary-node adaptive scheduling/control path. M4 remains pending. The acquisition path preserves native CSI; this workspace does no DSP or classification.

## Scope

`sensing/` holds all Rust in the monorepo: the shared `no_std` wire contract, the host server that ingests CSI from sensing nodes and local receivers, records and replays sessions, tracks per-link health and hands batches to the Python DSP/ML pipeline, and (from M5) the sensing-node firmware. None of it does signal processing. Python is a sibling of `sensing/` (proposed `capstone/python/`). The fall-event/alert lifecycle and caregiver-facing API are out of scope here; they need an owner elsewhere.

## Layout

```text
sensing/
├── Cargo.toml            host workspace: crates/* + server/*; excludes firmware/
├── rust-toolchain.toml   pinned 1.97.1 + riscv32imac-unknown-none-elf for the no_std check
├── .cargo/config.toml    `cargo check-nostd`
├── crates/               shared by firmware and server; no_std
│   └── csi-protocol/     wire contract
├── server/               host-only (std)
│   ├── csi-host/         ingest, sessions, replay, health, delivery
│   └── csi/              one binary: `csi record | replay | inspect | fit-clocks | hub`
└── firmware/             (M5) separate workspace: own Cargo.lock, rust-toolchain.toml (`esp`)
                          and .cargo/config.toml (Xtensa target, build-std, flash runner)
```

Placement rule: a package used by both sides goes in `crates/` and must stay `no_std`; host-only code goes in `server/`; embedded-only code goes in `firmware/`. Firmware depends on shared crates by path (`../crates/csi-protocol`). A crate belongs to only one workspace, so shared crates are members of the host workspace, which is where their tests, clippy and `check-nostd` run. `csi-protocol` has no dependencies, so the two lockfiles cannot disagree about it.

Added when their milestone starts, not before:

| Package | Location | Milestone | Notes |
| --- | --- | --- | --- |
| `csi-node` | `crates/` | M5, M7 | Implemented initial no_std pair request/response policy, turn tags and adaptive timeouts; general scheduling remains pending. |
| `csi-esp`, `tx-node`, `rx-node`, `txrx-node` | `firmware/` | M5, M7 | `esp` toolchain + Xtensa target for original ESP32. If the bench comparison selects ESP32-C5 nodes, they build for a RISC-V target instead; keep `csi-esp` chip-generic until then. |
| `csi-linux` | `server/` | M6, optional | Adapter for a rich-receiver reference (FeitCSI/NXP), only if one is selected; Linux-only behind `cfg` so macOS development builds still work. |
| Python reader | `capstone/python/` | M4 | Pure-Python (`pyarrow`) reader of the Arrow stream and files; no PyO3. |

The research's separate `csi-hub` and `csi-cli` are one `csi` binary with subcommands until a deployment reason to split appears; they share every dependency.

From `sensing/`, check everything with `cargo build && cargo test && cargo check-nostd && cargo clippy --all-targets -- -D warnings`.

## Design

The wire format (M1, now v2), session format (M2), geometry and shared-probe clock fit below are implemented; changing an existing layout means bumping its version. Live ingest is implemented in `csi-host::live`. An initial original-ESP32 TX/RX UART bench adapter is in `firmware/`; Python delivery and full M5 hardware acceptance remain pending.

**Wire format.** `csi-protocol` defines its own format; esp-csi-rs's `wire::RxMeta` served as the field checklist. A 16-byte little-endian header (version, kind, node, epoch, seq) is followed by a fixed-layout body; `Csi` ends with length-prefixed native CSI bytes, unchanged. Layouts are documented on each type and pinned by fixtures in `crates/csi-protocol/tests/golden/`. Decoding never allocates or panics. New kinds can be added without a version bump; hosts are updated before nodes.

**Framing.** A frame is a message plus CRC-32/ISO-HDLC (the same as Python's `zlib.crc32`). UDP carries one frame per datagram, below the path MTU. Byte streams (UART, TCP) carry COBS-encoded frames separated by zero bytes; receivers resync at the next delimiter, so boot garbage or a mid-frame start costs one rejected frame. A v2 HT40 record (384 CSI bytes) is a 464-byte frame, up to 467 bytes on a stream (COBS worst case plus delimiter). v1 was a 462-byte frame, up to 465 stream bytes.

**Messages.**

- Probe (over-air, in the ESP-NOW frame body): magic, version, transmitter node, transmitter epoch, schedule id, sequence; 23 bytes plus optional padding. `Probe::find` locates it anywhere in a received payload.
- Node → host: `Hello` (chip, roles, firmware version and commit, configured channel, ESP-IDF CSI config, pool capacity; repeated periodically); `Csi` (capture sequence, node monotonic µs, radio timestamp, 802.11 source MAC, attached probe or none, `first_word_invalid`, the ESP32 `wifi_pkt_rx_ctrl_t` fields, native bytes); `Counters` (cumulative per epoch: TX requested/accepted, callbacks, filtered, captured, pool drops, export drops, export bytes, pool high water); `TimeEcho`.
- Host → node: `TimeProbe`. Kinds `0x81` (schedule) and `0x82` (config) are reserved and get defined with M7.
- Loss is attributable per layer: gaps in the header `seq` are transport loss once counted export drops are subtracted, gaps in `capture_seq` are capture-pool drops, and gaps in the probe sequence are over-air or callback loss.
- There is no `Layout` message: the CSI byte layout follows from `Hello` (chip, CSI config) plus each record's RX metadata and length, so no record depends on a separately sent descriptor. The table mapping these to subcarrier indices lands with M4.
- **Receiver gain (protocol v2).** `RxMeta` now carries the receiver's AGC gain (`u8`) and FFT gain (`i8`) as optional `RxGain`, with RX flags bit 5 indicating validity. Without them a gain step cannot be told apart from a person's attenuation, and the analysis that detects someone lying still after a fall needs exactly that. Extraction from `rx_ctrl` is chip-specific (Espressif's `esp_csi_gain_ctrl_get_rx_gain`) and unverified on original ESP32, so the field is optional per record. Encoders emit `VERSION = 2`; decoders accept v1 and v2, with gain absent for v1. Golden v1 fixtures and a frozen M2 session prove backward decoding and byte-identical replay. The unchanged over-air probe retains its independent `PROBE_VERSION = 1`. Synthetic truth tracks injected and observable gain steps; inspection reports per-link valid fraction and steps/minute.
- Batching (M5+): a new kind with one header per batch and per-record time deltas; a record is never truncated. Default flush interval ≤100 ms once adaptive scheduling runs, so schedule changes follow a tracked person promptly.

**Clocks.** Each `Csi` record carries the radio's native 32-bit receive timestamp (`rx_timestamp_us`) and the node's 64-bit monotonic time at callback (`node_time_us`); the header carries the boot epoch. The host adds monotonic and wall arrival times. Per-source clock mappings are separate versioned records in the session, stored with their fit residuals, and raw values are never rewritten. Two mechanisms produce them:

- **Cross-node alignment from shared probes.** Every receiver that hears one broadcast probe (`tx`, `epoch`, `seq`) received it at the same instant, to within propagation and detection jitter. The hub pairs `rx_timestamp_us` across receivers for the same probe and fits each receiver's offset and drift against one reference timeline (reference-broadcast synchronization). Every node also receives, so every node gets a mapping, and a Linux receiver hearing the same probes joins the same fit. The fit runs continuously over a sliding window because crystals drift apart by tens of milliseconds per hour. Fall features need a few milliseconds of alignment; one sample at 200 Hz is 5 ms.
- **Host time from request/echo** over the export link itself (`TimeProbe`/`TimeEcho`), which works identically over UART, USB and UDP. It maps the reference timeline to host monotonic and wall time for labels and alerts, where tens of milliseconds is enough, and measures export latency. NTP is not planned.

ESP-IDF documents `rx_timestamp_us` as precise only with modem sleep and light sleep off, so firmware calls `esp_wifi_set_ps(WIFI_PS_NONE)`. Its timer starts at `esp_wifi_start()`, so its origin differs from `node_time_us`, and it wraps every ~71.6 minutes, so the host unwraps it against `node_time_us`.

**Python boundary.** The hub publishes homogeneous per-link/per-layout batches as an Arrow IPC stream over a Unix domain socket. `csi export` writes the same schema to Arrow/Parquet files for offline work. Replay goes through the same publisher, so live and replayed data are indistinguishable to Python. Columns carry per-record identity, capture ticks, host arrival time, mapped time (nullable), sender sequence (nullable), RX metadata including gain (nullable), validity and native CSI bytes; layout, clock provenance and the installation geometry revision go in batch/schema metadata. Bulk data never goes through JSON or HTTP.

**Control path (M7).** The Python processor runs the spatial stage (tomography and tracking) and asks for more probes from the transmitters whose links cross the tracked person. It sends schedule requests (per-transmitter rate) to the hub over a control socket. The hub owns the airtime budget: it validates each request, versions it, records it in the session and distributes it as a `Schedule` (`0x81`) message that takes effect at a stated cycle number rather than a clock time. Python never talks to nodes directly, and a missing or stale request falls back to the uniform background schedule.

**Sessions on disk (M2).** One directory per session (`csi-host::session`):

- `manifest.json` (`csi-session/1`): session id (UUIDv7), recorder and protocol versions, rotation limits, sources (name, kind, free-form details), sealed segments with record count, size, SHA-256 and time span, `derived_from` for replays, and a `complete` flag. It is rewritten atomically (temp file, fsync, rename) at creation, at every seal and at close.
- `segment-NNNNNN.bin`: an 8-byte header, then records of a 24-byte header (payload length, host monotonic ns since session start, host wall ns, source index, kind) and the payload. Frames are stored exactly as received, after COBS removal and before any decoding, so a frame that fails its CRC is still kept. Byte-stream framing failures are records too, so replay reproduces every health count. Segments rotate on size or on recorded time (not wall time), so a replay re-recorded with the same limits is byte-identical.
- Labels, events, split assignments and `synthetic-truth.json` live in separate files beside these.
- Installation geometry (`geometry.json`, `csi-geometry/1`) is a separate versioned file beside these: node id → position (x, y, z in metres), tier (floor or upper), antenna orientation, room and layout revision. The tomography stage needs it, and a re-survey after moved nodes produces a new revision rather than an edit. `record --geometry FILE` validates and copies the original bytes; the manifest stores room, revision and SHA-256. Inspection checks the hash and reports missing receiver/transmitter placements. Re-recording inherits the geometry unless explicitly overridden. A moved node requires a new survey and session.
- `clock-fit.jsonl` (`csi-clock-fit/1`): session-bound header, timeline handovers and versioned per-receiver affine mappings with fit windows, parent/hop provenance, drift, offset, inlier/outlier counts and residual p50/p95/p99/max. `record` and re-recorded `replay` derive it; `fit-clocks DIR` creates one for older sessions (`--force` explicitly replaces one). Raw segments remain unchanged.

The reader verifies each sealed segment's hash and count. It also reads segment files a crash left unsealed, stops at a torn or corrupt record and reports it, and reports missing segments. An unfinished session reads as incomplete, never as quiet.

**Health accounting (M2).** `Inspector` consumes records one at a time, so the M3 hub can run it live. It tracks frame, capture and per-link probe sequences per node epoch, with a 1,024-value window in which late arrivals still fill gaps. It also keeps a per-link node-time interval histogram (exact below 1.024 ms, within 1/128 above), layouts, node counters, time-echo RTT, silences and decode/stream errors per source. Events carry the time the problem was first seen.

**Host runtime.** M2 is synchronous: recording, replay and inspection are file I/O. M3 uses tokio for live sources: one task per source (serial, UDP, later an extractor subprocess) with bounded channels throughout. The recorder sits ahead of any live consumer, and live subscribers get lossy per-subscriber queues with drop counters. `thiserror` in libraries, `anyhow` in the binary; `tracing` and validated TOML configuration are included with `csi hub`. A dedicated recorder thread isolates blocking disk I/O and clock/health computation from source tasks. Live clock mappings are appended incrementally, and health/clock caches have explicit caps; see `README.md` for limits and overload behavior.

**Tests.** Golden-byte fixtures checked in. Property tests round-trip random messages through every layer and feed arbitrary and mutated bytes to every decoder; coverage-guided fuzzing (cargo-fuzz) needs nightly and is not set up. The synthetic source (`csi record … synthetic`) is deterministic per seed. It injects air loss, pool and export drops, corruption, truncation, duplicates, reordering, stream errors, outages and reboots, and keeps ground truth of what an observer can detect. Tests assert the inspector's counts equal that truth exactly across 40+ seeds. Synthetic data only proves transport and bookkeeping, never sensing.

**Proposed signal-preservation validation (2026-10-22).** [SIGNAL_VALIDATION.md](SIGNAL_VALIDATION.md) records lower-level metrics; [SIGNAL_VALIDATION_PLAN.md](SIGNAL_VALIDATION_PLAN.md) specifies implementation phases, comparison budgets, test matrices, completion criteria and reporting. The existing synthetic CSI is noise; structured signal tests remain unimplemented. Begin with native-radio timestamp auditing and known-signal replay/coverage, then the six-node byte-limited load test. These proposals do not complete any hardware or fall-detection acceptance gate.

## Milestones

| | Deliverable | Done when |
| --- | --- | --- |
| M0 | Workspace layout | Build, test, clippy and `check-nostd` pass. **Done.** |
| M1 | `csi-protocol`: messages, probe, COBS + CRC framing, version handling | Round-trips; rejects truncated, oversized, unknown-version and bad-CRC frames without allocating; golden fixtures pinned; arbitrary-input property tests pass. **Done.** |
| M2 | Session recorder/reader, synthetic source, `csi record`/`replay`/`inspect` | Replay is deterministic (paced, accelerated, step); re-recording a replay is byte-identical; `inspect` reports per-link rates, gap p50/p95/p99, sender-sequence gaps, drops and layouts; injected faults surface as health events. **Done:** counts match injected ground truth exactly; release build records 3 × 1,000 records/s about 110× faster than real time. |
| M3 | Live ingest: serial and UDP sources, per-link health, time echo, shared-probe clock fit | Sustains 3 links × 1,000 records/s from a synthetic sender over a pty and loopback UDP with bounded memory; unplug/restart yields stale/reconnect state and a new epoch rather than silently bridged gaps; the clock fit recovers injected per-receiver offset, drift, 32-bit wrap and reboot from a synthetic multi-receiver source. **Done:** `csi hub` plus UDP/pty throughput, echo/host-time fitting, stale/reconnect/epoch, bounded clock and subscriber-drop tests. Physical serial throughput and RF behavior remain hardware gates. |
| M4 | Python handoff: Arrow IPC publisher on a Unix socket, `csi export` to Arrow/Parquet, thin `pyarrow` reader, ESP32 layout table (subcarrier map) | One Python API for replay and live; a stalled consumer drops counted batches while recording continues unaffected. |
| — | **Hardware gate** | Everything below needs real boards. |
| M5 | `csi-node`, `csi-esp`, `tx-node`, `rx-node` speaking M1 over UART | Passes the gate in `research/esp-csi-rs-verification.md`; sender-sequence loss, pool drops, export drops and host rejects reported separately; RAM and callback time measured. Firmware runs with `WIFI_PS_NONE`; a one-TX, two-RX bench run of at least 1 h reports shared-probe fit residuals (p50/p95/p99) and drift per receiver, with p99 under 1 ms. |
| M6 | Several TX nodes; optional `csi-linux` adapter if a rich-RX reference is selected | Real ESP32 probes attributed per source; if built, rich-RX records map into the same `Csi` message with their own layout. |
| M7 | `txrx-node`, `Schedule`/`Config` messages, control socket and distribution | Each node emits observable probes and exports peer CSI; no self- or export-frame contamination; a schedule request from Python changes per-transmitter rates at the stated cycle on every node, within the hub's airtime budget, and is recorded in the session. M7 is on the critical path: the mesh is the primary architecture. |
| M8 | Unattended operation | Service supervision, disk full, rotation/retention, restart recovery and node loss produce explicit health state. |

M1–M4 need no hardware. Until M4 lands, Python teammates can generate synthetic sessions with `csi record --out DIR synthetic …`; the session format is documented in `csi-host::segment` and `csi-host::manifest`.

## Corrections and gaps found in the research

1. **Baud was recorded.** Every real `demo/data/*/session.json` contains `"baud": 921600`, though the research repeatedly says it is unrecorded. The ~71–74 lines/s text-serial ceiling therefore did apply to those captures.
2. **Sender sequence is available on ESP32.** The old receiver's `id` column is the sender's counter read from the ESP-NOW payload inside the CSI callback (`demo/vendor/esp-csi/examples/get-started/csi_recv/main/app_main.c:170`); esp-csi-rs reads its control-frame sequence from the payload the same way (`protocol.rs::control_sequence_in`). In `empty-01` the sender emitted ~90.9/s (not the requested 100) and 23.8% of its sequence numbers never reached the log. That is RF loss and/or the blocking `ets_printf` in the callback stalling the Wi-Fi task; separate counters will tell which. The research treats sender/CSI association as probably unavailable; it can be a first-class field.
3. **NTP conflicts with UART export.** The research makes LAN NTP the timing baseline for both topologies and also recommends starting with wired UART/USB export, but a UART-only node has no network for NTP. Decided: echo over the export link for host time; cross-node alignment uses shared probes (see Clocks). esp-hal firmware also lacks ESP-IDF's SNTP, as the research notes.
4. **UART is byte-limited; batching doesn't fix it.** A v2 HT40 record takes up to 467 bytes on the stream, so at 8N1 it fits ~197/s at 921,600 baud, ~428/s at 2 Mbaud and ~642/s at 3 Mbaud; 1,000/s needs ~4.67 Mbaud, which depends on the boards' USB-UART bridge. The 500 ms batching mainly helps UDP. Other levers: higher baud, choosing which LTFs to capture as an explicit capture configuration (HT20 LLTF + HT-LTF is 256 B), or network export.
5. **One workspace vs Xtensa.** Original ESP32 firmware needs the `esp` toolchain, `build-std` and an Xtensa target. Inside a stable host workspace this breaks `cargo test --workspace` and rust-analyzer's default checks unless every command excludes the firmware packages. Decided: `sensing/firmware/` is a separate workspace.
6. **PyO3 plus IPC is two boundaries.** The research proposes an in-process PyO3 SDK first and a separate recorder with IPC for unattended use. Decided: Arrow IPC only, which avoids per-OS/per-Python maturin wheels and the GIL/buffer-ownership rules.
7. **The event/alert lifecycle has no owner.** `build-plan.md` lists `events` and `app` modules and `final-system.md` requires durable, restart-safe events with delivery and acknowledgement, but no document assigns them to Rust or Python. Decided: not `sensing/`; an owner is still needed.
8. **Scope drift from the logbook.** On 2026-09-21 the team chose a Raspberry Pi + open-source router receiver, and the supervisor suggested focusing on counting, localization and pose. The research makes fall detection primary, counting a stretch goal, and deprioritizes routers. The server is topology-agnostic (a router CSI exporter is one more source adapter), but the priority should be confirmed with the supervisor. Since 2026-10-20 the mesh's spatial stage also yields zone-level localization as a by-product.
9. **Receiver gain is not recorded.** The research (`research/limits/`) shows AGC steps are about the same size as a person's effect on amplitude, and stationary detection needs gain-compensated attenuation. The original M1 `RxMeta` had no gain fields. Implemented: protocol v2 adds them (see Messages), while v1 remains decodable.

## Decisions

Decided 2026-10-19:

- All Rust lives under `sensing/`: shared `no_std` crates in `crates/`, host packages in `server/`, firmware in a separate `firmware/` workspace.
- Python boundary is Arrow IPC + Arrow/Parquet export; no PyO3.
- `sensing/` covers acquisition, recording and health only; the event/alert service is elsewhere.
- Host-time sync is request/echo over the export link.

Decided 2026-10-20:

- Cross-node clock alignment is a hub-side fit of each receiver's offset and drift from shared broadcast probes.
- Firmware disables Wi-Fi modem sleep (`WIFI_PS_NONE`) so receive timestamps stay precise.
- Protocol v2 adds per-record AGC and FFT gain with a validity flag; v1 frames remain decodable.
- Installation geometry is a versioned session file.
- Adaptive scheduling: Python requests, the hub enforces the airtime budget and distributes `Schedule`; nodes never take requests from Python directly.
- The two-tier ESP32 mesh is the primary architecture; the rich-RX adapter is optional.


## Shared-probe fit implementation and validation (2026-10-20)

`csi-host::clock::ClockFitter` uses a **60 s sliding window, refitted every 10 s**, with at least 20 shared probes per edge. Iterative least squares rejects residuals beyond five robust standard deviations (MAD, with a 1 µs floor). Reference selection is deterministic: most eligible neighbours, then most shared probes, then lowest `(node, epoch, radio run)` key. A maximum spanning tree by shared-probe count connects indirect receivers; equal edges prefer fewer hops and then lower keys. The reference remains until its samples age out. An already mapped receiver carries the timeline across handover; if none remains, a new timeline id explicitly marks the discontinuity. Epochs and detected radio-timer resets get distinct clock keys.

Measured with `cargo test -p csi-host --test clock candidate_windows_meet_the_bars -- --nocapture`: seed 21, 120 s, 3 receivers, 2 transmitters at 200 probes/s each, typical faults and reception jitter. These are software tests, not hardware results.

| Window | Worst relative drift error | Worst parent-fit residual p99 | Worst pair alignment error | Largest consecutive-fit jump | Median inlier pairs |
| --- | --- | --- | --- | --- | --- |
| 30 s | 0.007824 ppm | 7.3705 µs | 0.5945 µs | 6.10291 µs | 7,231 |
| 60 s | 0.007824 ppm | 7.3705 µs | 0.3620 µs | 0.22538 µs | 10,777 |

Both pass the requested 1 ppm / 50 µs bars. The 60 s default supplies more overlapping pairs and smaller mapping jumps in this test; real crystal temperature drift may justify a shorter window after the M5 bench run. Additional tests cover 32-bit wrap, reboot/epoch separation, timeline handover, receivers connected only through intermediates, deterministic persistence and replay. Residuals describe each edge against its parent, not an end-to-end error bound for a long chain.

Current boundaries: shared-probe mappings align receiver clocks to a relative reference. The live hub now issues and correlates request/echo exchanges and persists a separate reference-to-host map in `host-clock.jsonl`, with RTT/residual uncertainty and wall anchors. Live fitting streams mapping history to disk and bounds samples and identity caches; see `README.md` for capacities, rejected-fit counts and the manifest's per-segment metadata growth. `inspect` recomputes its displayed fit from raw records with defaults and separately reports stored-file status; it does not display a custom stored fit as its computed report. Live health and current mappings appear in `hub-status.json`. The synthetic source models distinct receiver/transmitter identities and configurable hearing topology; a combined TX+RX node and explicit self-probe exclusion still need M7 validation.


## Firmware and control follow-through

The initial firmware adapter now implements independently selectable fixed-channel ESP-NOW TX/RX (including both together), native CSI over UART, Hello/Counters and TimeProbe/TimeEcho. A known-pair request/response mode uses exact-round correlation and adaptive RTT timeouts without a fixed inter-node guard. See [firmware/README.md](firmware/README.md) and the [bench record](bench/2026-10-21/README.md). This does not complete M5's sustained, two-receiver, timing or gain gates. No rich-receiver adapter or schedule wire body is implemented. Keep kinds `0x81` and `0x82` reserved until M7.

- **M5 timing and PHY:** disable modem sleep with `esp_wifi_set_ps(WIFI_PS_NONE)` (or the verified equivalent in the selected Rust driver), disable light sleep, and reapply after Wi-Fi restart. Force ESP-NOW broadcast probes to HT MCS0, checking the driver return and received PHY metadata. The pinned upstream emitter already has a per-peer `force_peer_ht` helper in [`emitter/espnow.rs`](https://github.com/csi-rs/esp-csi-rs/blob/6cfbd6b73472c73696683b288b833c1910097061/src/lib/emitter/espnow.rs); reuse requires checking startup/restart ordering and reception, not merely successful sends.
- **Gain extraction audit:** inspected the existing local checkout at upstream commit `6cfbd6b73472c73696683b288b833c1910097061` (0.13.0). Its [`wire::RxMeta` and `VendorRx`](https://github.com/csi-rs/esp-csi-rs/blob/6cfbd6b73472c73696683b288b833c1910097061/src/lib/wire/mod.rs) have no AGC/FFT gain fields. The classic [`csi/esp.rs`](https://github.com/csi-rs/esp-csi-rs/blob/6cfbd6b73472c73696683b288b833c1910097061/src/lib/csi/esp.rs) conversion used by ESP32/S3 does not extract them. Thus the pinned public packet API cannot supply our gain fields on either chip as it stands.
- **Espressif comparison:** [`esp_csi_gain_ctrl` 0.1.5](https://components.espressif.com/components/espressif/esp_csi_gain_ctrl/versions/0.1.5/readme) documents ESP32 and ESP32-S3 support and `esp_csi_gain_ctrl_get_rx_gain(const void *rx_ctrl, uint8_t *agc_gain, int8_t *fft_gain)`, matching our field types. This is an ESP-IDF component, not evidence of compatibility with the pinned `esp-hal`/`esp-radio` stack. At M5, verify raw RX-control access and layout for the exact chip, radio library and SDK revision; decide whether to extend the upstream callback adapter or use an IDF bridge. Confirm gain values belong to the captured frame, and compare against Espressif output on both chips before marking validity. Until verified, export `gain = None`; never substitute RSSI or zero for an unknown gain. Raw CSI and gain compensation remain separate; Rust does no DSP.
- **Chip boundary:** keep capture, gain extraction, power-save and PHY setup behind `csi-esp` adapters. Keep role/schedule policy host-testable in `csi-node`. Original ESP32/S3 Xtensa details must not enter shared protocol or policy, allowing an ESP32-C5 RISC-V bench candidate.
- **M5 acceptance:** one transmitter and two receivers for at least one hour, sleep disabled and received HT/MCS verified; report p50/p95/p99 residuals, drift, wraps, resets, loss/drop counters and gain validity per receiver. Require residual p99 below 1 ms and preserve the session/configuration. Synthetic microsecond results do not satisfy this gate.
- **M5+ batching:** flush within 100 ms by default during adaptive scheduling, including partial batches; retain bounded queues, explicit overflow counters and complete records.
- **M7 control:** Python submits per-transmitter rates to the hub's control socket. The hub validates transmitter identities, finite rates, freshness and total estimated airtime, assigns a monotonic schedule version, records accepted requests/decisions, and distributes a schedule effective at a future cycle. Nodes report the active version/cycle; missing, stale or invalid requests fall back to the uniform background schedule with recorded reason. Retry/acknowledgement, partial rollout, cycle origin/wrap and reboot recovery must be specified and tested at M7 before claiming simultaneous mesh application. Keep the wire body deferred.
- **M6:** `csi-linux` remains optional and unstarted.
