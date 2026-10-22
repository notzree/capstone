# Rust workspace and node roles

2026-10-19. Agreed implementation direction; proposed structure, not implemented crates. This refines [build-plan.md](build-plan.md) and [software-boundary.md](software-boundary.md). Hardware and topology remain experimental; this structure supports either outcome.

## Deployment contract

One repository and **one Cargo workspace with multiple packages**. Share measurement contracts and role logic; build separate firmware and host executables for their actual targets. There is no requirement that a sensing receiver has an OS.

| Responsibility | Environment | Work |
| --- | --- | --- |
| Sensing TX | Embedded; original ESP32/S3 candidates | Emit identifiable probes on a configured schedule. |
| Sensing RX | Embedded `no_std`, or Linux `std` with a supported CSI extractor | Capture CSI from configured peer transmitters, timestamp, preserve native values/metadata, buffer and export. |
| TX+RX | Embedded combination of the above | Share one radio owner; transmit during scheduled opportunities and listen otherwise. |
| Collector/processor | Linux with an OS | Ingest local/remote CSI, record/replay, track quality; Python handles all scientific signal processing, fusion, calibration and ML. |

**TX/RX describe sensing roles.** An RX-only node can still transmit export/management traffic. Sending CSI to Linux does not turn that receiver into a sensing TX. `no_std` does not imply there is no executor or RTOS; it means the shared/embedded code cannot depend on Rust's standard library.

The Linux machine can host a CSI receiver itself, consume measurements from a CSI-capable router, or only collect from embedded receivers. A powerful CPU alone does not make its Wi-Fi interface CSI-capable. The user's option 2 also allows a Linux TX/RX hub; because the current baseline keeps sensing TX embedded, treat Linux probe transmission as an optional hardware-dependent experiment, not a requirement. Ordinary host/export transmissions are separate.

### Option 1: TX star

```text
embedded TX A ── probes ─┐
embedded TX B ── probes ─┼──> supported Linux CSI RX
embedded TX C ── probes ─┘          │
                               Rust hub
                                   │ batches / local IPC
                              Python DSP + ML
```

If a router extracts CSI, it is the RF receiver and exports to the Linux hub. An ordinary router forwarding application packets does not preserve original-hop CSI. Original ESP32 probes constrain a shared sensing link to compatible 2.4 GHz modes; verify the Linux extractor accepts their actual frame/PHY format.

### Option 2: distributed TX+RX, central processing

```text
embedded A TX+RX <── scheduled probes ──> embedded B TX+RX
       ↕                                     ↕
       └──────────> embedded C TX+RX <────────┘
          each exports its own captured CSI
                           │
                   Linux Rust collector <── optional local CSI RX
                           │
                      Python DSP + ML
```

The sensing graph can be a full mesh or a selected subset. Export can use direct connections/a star; multi-hop routing is not needed. A Linux hub cannot reconstruct A→B CSI by receiving A's or B's normal payload: B must export B's measurement. Include any compatible central receiver streams as separate links, rather than replacing them implicitly.

## Proposed directory and package structure

```text
capstone/
├── Cargo.toml                 # virtual workspace; explicit resolver/default-members
├── Cargo.lock                 # shared, committed at implementation time
├── crates/
│   ├── csi-protocol/          # no_std: versioned messages, IDs, bounded codec
│   ├── csi-node/              # no_std: tx/rx role logic, schedules, counters
│   ├── csi-esp/               # no_std: ESP radio/runtime/clock/export integration
│   ├── csi-linux/             # std: supported Linux CSI extractor adapters
│   ├── csi-host/              # std: ingest, sessions, replay, batches, health
│   └── csi-python/            # std: thin PyO3/rust-numpy bindings, added later
├── firmware/
│   ├── tx-node/               # entrypoint crate: sensing TX
│   ├── rx-node/               # entrypoint crate: sensing RX + export
│   └── txrx-node/           # entrypoint crate: sensing TX+RX + export
├── apps/
│   ├── csi-hub/               # std entrypoint: Linux local RX and/or remote ingest
│   └── csi-cli/               # std entrypoint: record/replay/inspect/health
├── python/
│   └── capstone_processing/   # DSP, features, models, fusion, evaluation
├── configs/                   # node/room/link schedules and host configuration
└── research/                  # evidence and implementation plans
```

These are boundaries to build incrementally, not a requirement to create empty packages immediately. Use modules inside each package for framing, counters and transport; split further only when a separate dependency/target boundary warrants it. Existing `demo/` remains reference code/data.

### Package responsibilities and dependency direction

| Package | Owns | Does not depend on |
| --- | --- | --- |
| `csi-protocol` | Probe/export/control schemas, layout/clock IDs, lengths, version negotiation, framing primitives and golden fixtures | HAL, Tokio, filesystem, Python or a radio vendor runtime |
| `csi-node` | `TxConfig`, `RxConfig`, `NodeMode::{Tx, Rx, TxRx}`, schedule policy, source allowlist, capture/export accounting; small clock/radio/sink interfaces as needed | Linux/Python; a mandatory executor or heap |
| `csi-esp` | One radio owner, pinned upstream CSI adapter, owned callback copies, bounded queues, ESP executor/task wiring, UART/network export and board-specific configuration | `csi-host`, NumPy or models |
| `csi-linux` | Actual extractor record parsing/configuration; local capture as a source of native observations | ESP HAL; a presumed portable low-level CSI API across NICs |
| `csi-host` | Remote ingest and legacy-file adapters, source/session registry, source-time handling, recording/replay, host batching, quality reporting and IPC | ESP runtime or Python interpreter |
| `csi-python` | Convert owned host batches to arrays; expose replay/live subscription through the same SDK | Firmware or scientific algorithms |

Firmware entrypoints depend on `csi-esp` and shared node/protocol code. Host apps depend on `csi-host`, with the local `csi-linux` adapter enabled only when needed. The Python extension depends on host/protocol libraries. Both firmware and host can consume `csi-protocol`; the host must not import `csi-esp` merely to decode its wire format.

`csi-node` shares hardware-independent role policy, not every platform's I/O loop. A Linux driver may own timing/queues internally; adapt its actual capabilities rather than forcing it into an ESP callback model. `csi-host` is the common measurement destination for both `std` and `no_std` receivers.

## Role composition and radio ownership

Keep TX and RX logic in shared modules. The three firmware entrypoints should be thin wrappers selecting defaults/capabilities and calling the same runtime. `txrx-node` composes both roles in one image; it does not launch the `tx-node` and `rx-node` executables on a microcontroller. A supported image can select `NodeMode` at boot; switching live roles comes later and starts a new configuration epoch.

One component owns the radio configuration/channel and arbitrates probe TX, receive capture and network export. Do not create two driver instances for the same peripheral. An ordinary ESP32 radio is half-duplex: TX+RX means time sharing, not simultaneous reception of its own probe. Listen to other configured sources during their slots. Actual simultaneous CSI capture, ESP-NOW/associated traffic and export support remains a hardware verification gate.

The Linux hub distributes a versioned schedule (active node IDs, source filters, fixed channel/PHY, probe rates/slots and export budget). Embedded nodes execute that schedule with local monotonic clocks. A first lab version may use fixed configured offsets; measure drift/collisions before claiming coordinated slots. Reconnects must not create a catch-up transmission burst. Count requested sends, driver-accepted sends and confirmed observations separately; a successful send API is not proof that another node received a probe.

## Lightweight embedded RX pipeline

```text
driver CSI callback
  → copy native bytes + available source/radio/time metadata into bounded storage
  → return immediately
drainer task
  → identify/filter permitted source, validate lengths, encode export record
  → bounded UART/network transport
Linux ingest
  → validate/decode → record native observations → publish batches
Python
  → resample/filter/features → link fusion → model → event policy
```

Do only capture-related bookkeeping at the edge: timestamps, sequence tracking, known-invalid prefix flags, integrity checks, framing and counters. Preserve native CSI; defer filtering, phase cleaning, FFT/STFT, PCA, feature extraction, normalization, learned compression and inference to Linux. Float conversion can also wait for the host. Do not threshold away quiet measurements or decimate silently to reduce export traffic.

Copy callback-owned bytes before returning. Never block the radio callback on UART, network, storage or locks held by an exporter. Size frame pools from the backend's maximum CSI payload and available RAM; do not assume all chips have the original ESP32 layout. Upstream's existing queue/codec can be reused through an adapter after the [live validation gate](esp-csi-rs-verification.md#hardware-gate-before-adopting-it).

Start with wired/USB/UART export where convenient to isolate sensing. Wireless export is a separate deployment test: its airtime, buffering and radio activity can reduce usable sensing time. Mark export/management frames so they are not accidentally treated as controlled sensing probes. A separate channel requires a separate radio or measured channel switching; one radio cannot continuously listen on two channels.

Queue policy must be explicit: when full, count and report discarded records with their sequence scope; never block indefinitely. The Linux archival stream marks gaps/incompleteness. Live consumers may discard stale batches with counters. Embedded RAM is not an unlimited offline recording buffer; define the maximum tolerated outage from measured bytes/s and buffer capacity.

### CSI export batching: configurable, initially 500 ms

The team accepts approximately half a second of transport buffering if it improves measurement quality. Support batched CSI export in embedded RX and TX+RX nodes, with an initial **500 ms maximum accumulation interval** and an independent byte/sample cap. Flush when the oldest pending observation reaches that age or the cap is reached, whichever occurs first. This is a starting configuration to measure, not a guarantee of delivery or total alert latency. Linux receiver adapters can also batch downstream delivery without delaying local recording unnecessarily.

Keep sampling and delivery rates independent: capturing 1,000 observations/s and flushing twice/s still supplies up to 1,000 timestamped observations/s. Timestamp every observation at capture, preserve native IQ, source identity, capture epoch, sequence semantics and validity, and carry the relevant clock-mapping reference. Encode common metadata once per batch and per-sample timestamp deltas where lossless and unambiguous; batching is not permission to average, decimate or overwrite measurements.

Use bounded acquisition storage and a separate nonblocking exporter. Drain/seal batches without stopping the capture callback. Radio reception still pauses during same-radio transmissions; coordinate and pace export opportunities with probe scheduling. Stagger node flush phases rather than making every NTP-synchronized node send a burst on the same half-second boundary. Do not reserve half the experiment for collection and half for sending by default.

For UDP, a logical batch may span many independently decodable datagrams. Cap datagrams against the verified path MTU; do not send a half-second recording as one oversized datagram or silently truncate a native observation. Include node/epoch, batch/part sequence, source/layout and time references needed to identify gaps/reordering. If an individual record cannot fit, define bounded explicit application-level fragmentation or select a suitable framed stream transport. [RFC 8085, Section 3.2](https://www.rfc-editor.org/rfc/rfc8085.html#section-3.2) explains why IP fragmentation should be avoided. Transport choice remains an implementation decision to benchmark.

Budget memory from **aggregate incoming observations across all links**: approximately `bytes_per_observation × observations_per_second × 0.5 s`, plus metadata, in-flight buffers and headroom. Illustrative only: 384 CSI bytes × 1,000 observations/s × 0.5 s is 192,000 bytes before overhead. Do not assume the original ESP32 has that much free memory after the Wi-Fi runtime; flush smaller batches earlier as needed. The exporter must still sustain the average byte rate.

Batching can reduce repeated headers, send calls and packet overhead; it does not eliminate the native CSI payload volume. Better fall accuracy is a hypothesis if reduced export overhead preserves more usable captures. Large bursts may instead increase contention or create sensing gaps, so compare configurable intervals such as 20/100/250/500 ms at fixed probe schedule and sample representation.

Measure accepted CSI/s, gap distribution, buffer high-water marks/drops, bytes and airtime per useful observation, and capture-to-host latency. Account for batching, paced drain, network transfer, central reordering, model context and computation in event-to-alert latency. The host buffer uses mapped capture time and a declared lateness policy; it must not blindly add another full 500 ms after every batch arrives.

## Two distinct message contracts

1. **Over-air sensing probe:** protocol/version, transmitter ID, boot/session scope, sequence and schedule/configuration ID, if the selected frame format permits it. Keep probes compact and PHY/size controlled.
2. **CSI export record:** measurement receiver ID and capture epoch, observed RF source identity, source sequence **only if demonstrably associated**, receiver capture counter/time, layout/PHY/channel/chain metadata, native CSI bytes, invalid flags and export sequence. Send stable layout descriptions once with an ID and repeat them on reconnect; never make a capture undecodable because its descriptor was lost.

The CSI callback may not expose the probe payload. Do not match a sender sequence to CSI by assuming callback order; use an actually available frame identifier/correlation mechanism or mark it unavailable. Export transport sender and measured over-air transmitter are different fields. Sender-sequence gaps, missing callbacks, capture-pool drops, export losses and host rejects require distinct counters; not every gap can be attributed to RF loss.

Every receiver has its own clock. Preserve native ticks, units, timestamp origin, wrap/reset information and a boot/capture epoch. The hub adds arrival timestamps and estimates inter-clock mapping with uncertainty. Scheduled probe slots alone do not synchronize receiver clocks, and timestamps converted to nanoseconds do not become phase coherent. Begin with per-link windows and feature/score fusion; raw complex cross-node fusion requires separate justification.

### Timing baseline: local NTP server

Following the team's preference, use a LAN NTP server at the Linux hub as the first timing approach for both topology options. Each sensing node uses an NTP-compatible client and records synchronization health. NTP does not require `std`; client integration must match the selected embedded UDP stack. ESP-IDF's lwIP SNTP facilities are a separate backend option, not APIs automatically available in the proposed `esp-hal` firmware.

The [NTP FAQ](https://www.ntp.org/ntpfaq/NTP-s-algo/#5131-how-accurate-will-my-clock-be) describes sub-millisecond synchronization on favorable quiet local networks. This is a feasibility target, not a measured bound for ESP32 Wi-Fi under sensing/export load. Measure relative receiver capture-time error, not only each client's reported offset to the server.

- Preserve capture-time monotonic ticks and boot/session identity. Record a versioned mapping from that actual capture clock to the NTP-referenced timeline; synchronizing a separate wall clock alone is insufficient.
- Keep raw timestamps immutable when an offset estimate changes. Record corrections and their effective ranges so replay can reproduce alignment without artificial jumps in sample intervals.
- Track last successful exchange, round-trip delay, offset/drift estimates when available, and stale/unsynchronized state. A client returning “synchronized” is not proof of an error bound.
- Configure refresh based on measured drift and timing needs. [ESP-IDF SNTP](https://docs.espressif.com/projects/esp-idf/en/v5.5.1/esp32/api-reference/system/system_time.html#sntp-time-synchronization) defaults to hourly refresh and immediate wall-clock updates; those defaults are not our acquisition policy.
- Validate with repeated common identifiable probes, checking callback/timestamp jitter, reboot recovery, warm-up, long-run drift and network load. Record median/p95/p99 alignment residuals, then test the downstream model against injected timing shifts.

The central reorder buffer remains useful after this clock mapping. It handles transport delay, while NTP-based mapping addresses clock differences. Transmission-slot coordination is a separate mechanism with its own guard times and measured jitter. Local NTP is the baseline, not a guarantee of tight radio scheduling or cross-node RF phase coherence.

Use one source-native representation on the wire and homogeneous per-link/layout batches on Linux. The Python `[T, RX, TX, K]` dimensions describe antenna chains/spatial streams/subcarriers within a measured link, not boxes in the room. A layout/configuration change emits an event and starts a new batch.

## Workspace/toolchain rules

Use a virtual root workspace with explicit members, `resolver = "3"` if compatible with the selected toolchains, and host-only `default-members` such as `csi-host`/`csi-cli`. One workspace does not imply one executable, one target or one compiler invocation. See the [Cargo workspace reference](https://doc.rust-lang.org/cargo/reference/workspaces.html).

Keep shared crates `#![no_std]` with default features disabled on dependencies; prefer bounded/caller-owned storage without a required allocator. Add `std` functionality in host crates or through additive optional features. TX and RX are composable capabilities, not mutually exclusive features. Select exactly one chip per firmware build and reject incompatible chip features. Cargo unifies enabled dependency features, so an indiscriminate `--workspace --all-features` build is unsuitable for mixed chip/host targets. See the [Cargo feature reference](https://doc.rust-lang.org/cargo/reference/features.html).

Do not set a workspace-wide embedded default target or flash runner. Use explicit per-package target/toolchain commands and build-only aliases; flashing is a separate command. Host CI uses stable Rust; ESP builds use the pinned verified ESP compiler and chip-specific target. A root lockfile records dependencies; firmware/host build jobs still run separately. Record upstream revisions and licenses when adapting wire code.

`esp-csi-rs` is a candidate backend, not the shared architecture. The inspected root crate has embedded dependencies, so its wire decoder must be reused/extracted deliberately rather than importing its entire firmware library on Linux. Current evidence is 24 host tests and successful ESP32/S3 builds; real TX/RX capture and combined-role operation remain unverified. C/ESP-IDF reference firmware can export the same contract if the Rust backend fails its capture gate.

## Implementation order and acceptance evidence

| Step | Deliverable | Evidence before proceeding |
| --- | --- | --- |
| 1. Contract + replay | `csi-protocol`, initial `csi-host`/`csi-cli`, saved ESP32 adapters and session format | Golden bytes; reject truncated/oversized/unknown records; preserve native layout/clock provenance; reproduce saved recording counts and timing. |
| 2. Host subscription | Short owned batches and thin Python SDK; separate recorder process/IPC for unattended use | Replay/live-shaped inputs share API; stalled Python does not silently corrupt recording; bounded queues and honest incomplete-session reporting. |
| 3. Embedded roles | `csi-node`, `csi-esp`, thin TX/RX firmware entrypoints | No-std target builds; independent role policy checks; real two-board capture/export gate; measured RAM, callback cost, rates/gaps and restart behavior. |
| 4. Star system | Several embedded TX + candidate Linux RX/collector adapter | Extractor accepts actual probes; stable source attribution; per-link usable rates and continuous recording with all sources. |
| 5. Combined firmware | `txrx-node` using the same radio owner and bounded exporter | Each board both emits observable probes and exports peer CSI; no self/export-frame contamination; real schedule/export costs measured. |
| 6. Topology comparison | Fixed-rate subsets plus actual star/selected/full schedules | Same-hardware topology control, separate rich-RX system comparison, fixed held-out splits, event recall at equal false-alert budget. |
| 7. Sustained deployment | Configured Linux recorder, Python pipeline and service supervision | Node/network failures, clock drift, disk full, process restart, missing links and prolonged runs produce explicit health state. |

Steps 1–2 and role-policy simulation can start without hardware. Steps 3–5 require real compatible radios. Python science work proceeds on recorded/public data while acquisition develops. The physical experiment and selection rule are in [experiment-sensing-mesh.md](experiment-sensing-mesh.md); neither topology is selected by the crate layout.
