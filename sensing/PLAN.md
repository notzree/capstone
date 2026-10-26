# `sensing/` implementation plan

Status 2026-10-26: M0–M3, the initial M4 handoff and the initial labelled-trial workflow are implemented. [EXPORT.md](EXPORT.md) covers Arrow/Parquet and Python; [COLLECTION.md](COLLECTION.md) covers recording, annotations, quality and live UDS with configurable flushing. The decoder covers original ESP32 HT20; wider layouts remain raw-only. Original-ESP32 fixed-role, independent TX/RX and packet-driven pair modes have two-board bench evidence; see [firmware/README.md](firmware/README.md) and [mode-comparison results](bench/2026-10-21/mode-comparison/). This does not complete M5 hardware acceptance or M7's arbitrary-node adaptive scheduling/control path. The acquisition path preserves native CSI; this workspace does no DSP or classification.

## Active delivery scope: USB ESP32 star kit (2026-10-26)

**Implementation status 2026-10-26:** option 2 is implemented through P1–P5, with two-board physical discovery/start/stop and file/stream/both evidence. See [STAR.md](STAR.md) and the [star bench](bench/2026-10-26/star/README.md). P6 remains a hardware acceptance gate: five-TX scaling, the full failure matrix and a 90-minute wrap run are not established by the one-TX bench. Native laptop/Pi CSI extraction is removed from the first-release critical path. Preserve existing pair/mesh/wireless-piggyback modes and recordings as optional legacy paths. Implement and test this plan in order; do not expand it into arbitrary network/topology support.

### 1. User outcome and fixed v1 scope

A teammate receives one pre-flashed RX ESP-WROOM-32 and up to five pre-flashed TX boards. They power the TXs, plug the RX into their MacBook, launch one Rust client, see the discovered TX roster, and start/stop acquisition. The MacBook performs all DSP/ML/tomography in Python. No router, internet connection, Wi-Fi association or credentials are required for the kit.

```text
Power-only TX boards ← ESP-NOW control broadcasts ← RX ESP32 ← USB commands ← Rust client
Power-only TX boards → direct HT20 sensing probes → RX ESP32 → USB CSI      → Rust client
                                                                           ├─ raw trial files
                                                                           └─ Arrow/UDS → Python
```

The RX is the sole CSI receiver. Its transmissions are control messages, not extra sensing links. A TX must receive commands, even though its sensing role is TX-only; it does not capture/forward CSI. With the six owned boards, the initial kit is one RX and five TXs. Firmware table capacity is eight TXs, but supporting eight at useful rates is not a v1 acceptance claim.

Use two reusable images, `star-rx` and `star-tx`, for the original ESP32 hardware. Emit a real build identifier derived during image preparation rather than presenting the current zero commit placeholder as verified provenance. Make the two star roles mutually exclusive and reject combinations with `pair`, `gateway` or legacy wireless modes at build time. A star RX still initializes an ESP-NOW sender for control even though its sensing role is RX-only; a star TX initializes control reception without enabling CSI capture or a UART CSI exporter. Identity comes from factory MAC plus a fresh nonzero boot epoch. No per-peer MAC is compiled into these images. Provision a shared non-secret `kit_id` and fixed channel once when preparing a kit; the lab default channel is 6. Channel changes require explicit reconfiguration of the whole kit, not automatic hopping in v1. Separate kits should have separate IDs. A kit ID is isolation metadata, not sender authentication.

Success means the complete physical workflow works from cold boot and the team can inspect data quality; it does not establish fall-detection accuracy or a useful tomography reconstruction. Node positions/antenna orientations remain a survey task, not something discovery determines.

### 2. Existing pieces to reuse, and gaps found in the code

| Area | Reuse | Required change |
| --- | --- | --- |
| Radio | Original-ESP32 CSI callback, HT20/MCS0 setup, source/probe checks, bounded capture queue | Star-specific filtering, control reception and one central scheduler; no legacy pair policy in the star build. |
| Acquisition wire | Protocol-v2 `Csi`, `Hello`, `Counters`, `TimeProbe`/`TimeEcho`, COBS + CRC framing | Separate versioned star control/status envelope; retain frozen v1/v2 acquisition decoding. |
| Shared policy | `crates/csi-node` no_std, host-testable policies | RX discovery/run/roster and TX command/lease policies; no chip-specific calls in the policies. |
| Host | Serial adapter, raw sessions, health, replay, exporter, live Arrow publisher | One serial owner and command router; file/stream/both run modes; inventory and control events. |
| Python | `capstone_csi.Export`, `Trial`, `live_batches`, masks/layout decoder | Examples and stream completion/health access; no new radio parser or embedded DSP. |
| Collector | Labels, geometry, provenance, quality, split grouping | Support many remote TX inventory entries behind one physical RX source. |

Pre-implementation baseline: `live::start` requires an output directory and always creates a `SessionWriter`; `delivery::Publisher` writes status sidecars into that directory. Genuine stream-only output needs a deliberate refactor, not a throwaway hidden recording directory. `trial.rs` currently requires a unique configured source for every node; that cannot represent five radio TXs behind one USB RX. `collect begin/end` label activities and do not yet control radio transmission. These capabilities now live in `star::StarController` and `csi star`. Legacy `live::start`/`collect` retain their recording-oriented APIs; the star service uses optional raw storage and memory health rather than a hidden session directory.

### 3. Protocol boundaries and identities

Keep three boundaries distinct:

1. **Air control:** short `CSIs/1` messages over ESP-NOW, all no larger than 250 bytes, so the new control path does not depend on ESP-NOW v2 fragmentation. Types: `DISCOVER`, `DISCOVER_ACK`, `START`, `COMMAND_ACK`, `GRANT`, `RUN_KEEPALIVE`, `STOP`, `STATUS_REQUEST`, `STATUS_REPLY`.
2. **Air sensing:** existing `CSIp` identity plus a small versioned star probe tag containing the run token and grant token, padded as needed for the validated HT20 frame format. Only these explicitly tagged, selected-source probes produce star CSI observations. Discovery, ACKs, status and grants do not.
3. **USB:** existing COBS-delimited acquisition frames plus the same bounded star control envelope for host commands, device events and roster snapshots. The host demultiplexes control before the CSI parser. Allocate a new raw `RecordKind::StarControl` (code 4; 1–3 already used) for exact received control bodies, preserving serial order and host arrival times when recording. Old readers skip the unknown record kind rather than mis-decoding it as CSI. Define no_std codecs and golden bytes before wiring radio code.

USB-specific request types are `ATTACH`, `DISCOVER`, `GET_ROSTER`, `GET_STATUS`, `START`, `STOP` and `HOST_HEARTBEAT`. Replies/events include `RX_READY`, `NODE_SEEN`, `ROSTER_SNAPSHOT`, `DISCOVERY_COMPLETE`, `COMMAND_RESULT`, `RUN_STATE` and `DRAIN_COMPLETE`. The latter reports the final capture/export sequence boundaries and export/drop totals for the stopped run, and is serialized only after all retained records preceding that boundary have been sent. RF command ACK and USB export drain completion are different events. Enumerate all request/result codes and reject unsupported versions explicitly in P1.

`CSIs/1` has a fixed bounded header: magic/version, kind, body length, kit ID, sender MAC/boot epoch, intended receiver/owner identity, correlation ID, then a kind-specific body and CRC32. Receiver address may mean broadcast; do not mistake a wildcard destination for permission to change an existing owner's run. Pin exact offsets, maximum sizes and unknown-kind rules in the codec milestone. Do not reuse reserved acquisition kind IDs `0x81/0x82` or change existing `CSIt`/`CSIb` layouts. Check header identities against the physical ESP-NOW source MAC before accepting an air control message.

Identifiers serve different purposes:

| Identity | Scope / purpose |
| --- | --- |
| Factory MAC + boot epoch | Distinguishes a device from a later reboot, independently of serial port names. |
| Discovery nonce + round | Correlates replies to one live discovery window and permits fresh response backoff each round. |
| Host attachment token | Fences commands from an old Mac process/connection. A new attachment never resumes a previous measurement automatically. |
| RX MAC/epoch + command ID | Correlates application ACKs and bounds duplicate-command handling within the owning RX boot. |
| Host run UUID + RF run token | Binds output, frozen roster and commands; record their mapping in run metadata/events. |
| Probe schedule generation | Nonrepeating u32 within an RX boot, carried in existing `Probe.schedule` to separate runs in raw CSI. Refuse exhaustion instead of reusing a generation. |
| Grant token | Identifies one permitted transmission for one TX/epoch in one run. |

Preserve the TX's own monotonic probe sequence; a single global grant sequence must not be used as every TX's `Probe.seq`, which would falsely look like per-link packet loss. Grant-token correlation is separate. The RX records grant outcomes with the corresponding RX capture/probe sequence so expected opportunities and actual transmitted/captured probes are not conflated.

Broadcast commands and replies carry logical destinations in their bodies; only the addressed RX/TX applies them. This keeps the initial peer setup to the broadcast peer, without compiling MAC pairs. A radio send-completion callback is not an application acknowledgement. Espressif explicitly recommends application ACKs/retries where delivery matters. [ESP-NOW reference](https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/network/esp_now.html). The prototype's broadcast controls are not cryptographically authenticated; deployment hardening remains separate.

### 4. Discovery: boot behavior and the ten-second window

**TX power-on:** configure kit/channel, initialize ESP-NOW control reception, enter `IDLE`. Do not emit sensing probes. Listen indefinitely, so a TX powered well before the RX is still discoverable. No Wi-Fi association, compiled RX address or synchronized clocks are needed.

**RX power-on:** initialize radio, UART and bounded roster, announce receiver capabilities, and begin a ten-second discovery window automatically. This runs even if the Mac client has not opened the serial port yet. Cache the latest roster and discovery result in bounded RAM; send a full snapshot on each host attachment, not just events the client may have missed.

Discovery sequence:

1. RX chooses a fresh discovery nonce and broadcasts `DISCOVER(kit_id, rx_identity, nonce, round, channel)` every 500 ms for ten seconds: 20 rounds, no sensing traffic.
2. A compatible TX receiving a matching request schedules one `DISCOVER_ACK` in one of 16 response slots, 20 ms apart, plus 0–5 ms jitter. Derive slot selection from TX identity, nonce and round, with fresh randomness. New rounds change collisions; do not have all nodes ACK immediately.
3. The reply echoes discovery identity and reports TX identity/boot, firmware/build identity, protocol/capabilities, sensing role, channel, current state and any current run owner. The RX validates CRC, source identity, kit/channel and active nonce, then upserts by factory MAC. A changed boot epoch replaces the previous boot observation with an explicit reboot event.
4. Repeated replies refresh the same entry, never create duplicate nodes. Late replies after the window and replies for an old nonce are ignored/counted. Version mismatches and a full eight-node table are reported, not silently admitted.
5. RX emits ordered node updates, `DISCOVERY_COMPLETE` and a full roster snapshot. Zero nodes is a normal explicit result that prevents START, not a permanently spinning discovery task.

An entry records first/last seen, capabilities, state, discovered boot, protocol compatibility and ownership. Use control status requests roughly every two seconds while idle to refresh known nodes with staggered replies; mark an entry stale after six seconds without any valid response. While measuring, valid probes/status refresh liveness; status requests must fit around measurement/control work and must not delay STOP or leases. Discovery proves reachability, not a usable CSI link. Show separate `discovered`, `ready`, `CSI observed`, `stale`, `busy` and `incompatible` information.

Explicit `discover` reruns the same ten-second protocol. Refuse it during a run; the operator stops first. A TX powered after the boot window is found on the next discovery. Never add it to an active run or silently change other TX rates. If the host attaches while discovery is active, report progress and wait for completion before START. If it attaches later, replay the cached snapshot and current liveness.

### 5. RX/TX state machines and ownership

| RX state | Allowed behavior |
| --- | --- |
| `DISCOVERING` | Control-only discovery/status; no grants or CSI run. |
| `READY` | Roster/status; prepare a new output/run, rediscover, or stop idempotently. |
| `STARTING` | Validate/freeze selected roster, arm capture, broadcast START and collect application ACKs. |
| `MEASURING` | Issue grants, capture/export matching CSI, refresh leases/status and monitor health. |
| `STOPPING` | Cancel future grants, broadcast STOP, collect ACKs, drain already acquired data. |
| `FAULT` | No grants; expose fault/status and require explicit recovery/new discovery as appropriate. |

TX states are `IDLE`, `ARMED` and `MEASURING` (plus transient fault). Discovery ACK does not start measurement. A valid START binds the selected TX boot to one RX identity, run token and bounded lease; it replies with `ARMED`. Only a valid GRANT in that armed run permits a sensing probe. STOP clears the active grant/run and replies with `IDLE`.

A TX already owned by a different live RX/run replies `BUSY`; another kit or receiver must not steal it through a broadcast. Releasing an owner occurs on matching STOP or lease expiry. Deduplicate commands using RX identity, run token and command ID; replay the cached result without repeating a side effect. Conflicting retries with changed bodies are rejected. Unknown/old runs and grants for the wrong TX boot are rejected. Bound cache/table memory and test command/token wrap behavior; reboot creates a fresh identity and defaults to IDLE.

### 6. START, grants, STOP and failure handling

**START transaction:** the host first validates sink paths/consumers and creates a fresh run UUID. Reject an empty/stale/incompatible/busy selection, conflicting output paths, an existing run, or a rate above the admitted budget. Freeze the roster with each TX boot, declared firmware/channel and requested rates. Arm receiver capture/output before RF can start. RX allocates an RF token and schedule generation, then broadcasts START containing the selected roster/epochs, rate policy, duration and lease. Only selected TXs ACK/arm.

Use a first-release command ACK timeout of 500 ms with three total START/STOP transmissions and jittered/staggered ACK slots; retry the same command ID. START succeeds only when every selected TX application-ACKs the applied settings. On failure, send STOP to the attempted set, send no grants, and report the exact missing/rejected nodes. A partially armed fleet is not a successful start. The ARMED state itself produces no sensing traffic.

**One hub, one outstanding grant:** use RX-managed round-robin grants initially, not autonomous periodic TX timers or distributed TDMA. A GRANT is broadcast but names one selected TX boot/run/token and permits exactly one probe. The TX responds promptly using local elapsed time, rejects a request delayed more than 10 ms after its radio receive callback, and never sends a second probe for a duplicate grant. RX matches the probe tag to the outstanding grant and source. Keep bounded recent-grant history to classify a known late/duplicate probe separately from an unissued probe; reject unissued or wrong-run observations and never silently treat them as current samples. Advance on the matching observation or a 20 ms timeout. Timeout creates a missing-opportunity event; late/unsolicited probes cannot satisfy a new grant. After timeout, leave a 10 ms recovery quiet interval before granting another node, to reduce overlap with an already delayed response. This is a measured software scheduling policy, not a hard real-time collision-free guarantee.

The default starts conservatively at 10 requested probes/s per TX, for at most five TXs (50/s aggregate). Pacing caps the round rate; successful low-latency responses do not trigger an unlimited burst. A missing node must not stall the fleet indefinitely or silently increase other nodes' rates. Keep the active roster/nominal slots fixed; mark degraded links and record timeouts. A rebooted/late TX cannot rejoin that run: stop/discover/start again. Achieved rate, gaps and response-time tails remain reported separately from requested rate.

**Leases:** host heartbeat to RX every 500 ms, expiring after 2 s; RX renews the selected TX run lease every 500 ms, with a 3 s maximum TX lease. Use local monotonic clocks only. Host heartbeat expiry, USB disconnect, fatal host sink error, shutdown or maximum run duration cancels grants and initiates STOP. TX lease expiry clears its run even if STOP was lost. The RX must stop renewing a run whose host lease has expired. Discovery may run without a host; measurement may not. No automatic resume after a host/RX/TX reboot.

**STOP transaction:** cancel pending/future grants before sending STOP; request/ACK all selected TXs, retaining missing-ACK details through retries. Drain already acquired RX/serial records with a bounded deadline before closing sinks; preserve late/control/error records and flag their relationship to the run. A STOP ACK reports the last probe attempt and counters. Never claim every TX confirmed idle when an ACK was missing; record `stop_confirmed`, lease-expiry fallback and any gap in telemetry separately. Artifact sealing can succeed while radio/coverage quality is degraded; do not overload the existing `complete` field to mean zero loss or all radio ACKs received. A final watermark/ACK must establish the RX export drain boundary; a quiet serial interval alone is not proof that every queued record was exported.

The three-second TX lease and drain deadline are safety/recovery bounds, not capture-to-Python latency claims. Cap runs at one hour by default/configured duration; a sustained test must still separately cross the roughly 71.6-minute radio-timestamp wrap.

### 7. Output modes: one acquisition path, explicit sink policy

A regular file and UDS stream are both usable through file descriptors, and Arrow can write through a common `Write`-like interface. That does not make them identical products: a session archive has multiple files and durable provenance; a socket has connection boundaries, no seek and consumer backpressure. Separate transport from encoding. Keep CSI wire parsing in Rust and reuse the existing Arrow/Python schema.

| Mode | Operator choice | Persistence / failure policy |
| --- | --- | --- |
| File only | `start --out DIR` | New raw session + control journal, labels/provenance/quality and final Arrow export. Refuse overwrite. Raw data is written before downstream interpretation. Disk failure initiates STOP and leaves visibly incomplete output. |
| Stream only | `start --socket PATH` | Arrow IPC over UDS, with no hidden data files, spool or mandatory recording directory. Session/run metadata and current/final health live in bounded memory and are exposed through control/status. Require a primary consumer handshake before grants. If it disconnects, times out or overflows its bounded queue, report counted loss and stop the run. The data cannot be recovered from disk by Rust. |
| File + stream | Both options | Persist raw data first and stream decoded Arrow concurrently. Slow/disconnected consumers get counted delivery losses/disconnection; recording continues. This is the recommended training/development mode. |

Reject START with neither sink. Bind/create/check all requested outputs before arming TXs. When a socket is requested, wait for the primary client to read the Arrow schema before enabling probes, with a ten-second startup timeout; otherwise fail startup without RF acquisition. Later additional clients receive future data only, with partial-subscription semantics. Preserve the existing maximum of 16 clients, eight queued Arrow batches/client, roughly 250 ms write timeout and 20 ms configured flush as initial defaults, subject to the focused tests below. Stream-only primary failure is fatal to that run; file+stream consumer failure is not. Never buffer unbounded CSI to conceal a slow Python process.

Refactor `live`/delivery into a serial/control acquisition service plus optional raw storage and Arrow delivery workers. Build session/run identity and metadata in memory, then persist it only for file modes. Remove required status-directory/`SessionWriter` dependencies from stream-only execution; replace them with a health/report interface that can have a file sink or memory-only implementation. No fake always-successful disk writer. Keep serial reading and command/heartbeat work independent of blocking disk/UDS writes. Fatal sink signals reach the controller; bounded queues and CSI/export/drop counters expose any records lost before shutdown.

Continue the existing acquisition `Csi` bytes and `csi-export/1` columns where their meanings remain valid. Add new schema/metadata versions if run/grant or source semantics require it, with old fixtures retained. Stream mode provides a fresh session UUID, RX identity, frozen TX roster, firmware/config references and geometry when supplied in the schema metadata. Final status is separately queryable; Arrow EOF alone cannot say that RF stop was acknowledged or no data was lost. Stream-only labels/status have no automatic durability; use file/both for reproducible training, or explicitly persist them in the consumer. `Trial` remains a file-bundle reader, while `live_batches` works without a trial directory.

File mode stores canonical raw records; the final Arrow file and UDS use the same decoder/encoder for matching CSI values. Do not imply that a raw segment and a decoded Arrow stream are byte-identical formats merely because both use FDs. Implement byte writers via `std::io::Write` / an internal `ByteSink`, while retaining separate `RawSessionSink` and `ArrowSink` format responsibilities. Literal arbitrary inherited-FD output and shell stdout binary piping are deferred; the first release supports named output directories and named UDS sockets with correct lifecycle semantics.

### 8. MacBook command workflow

These interfaces are available; see [STAR.md](STAR.md). One `csi star` process owns the RX serial port for its lifetime:

```sh
csi star --device auto
# Or explicitly: csi star --device /dev/cu.usbserial-EXAMPLE
```

On attachment, verify a `star-rx` capability/version handshake, obtain its identity and replay its discovery snapshot. Show discovery progress or its result. Then provide a small REPL:

```text
nodes
status
# Repeat discovery after plugging in another TX:
discover --seconds 10

# Durable trial, default 10 requested probes/s per selected TX:
start --out ./trials/walking-01 --rate 10 --duration 60
begin empty
end
begin walking
end
stop

# Stream without recording; connect Python while START waits for its schema handshake:
start --socket /tmp/csi-data.sock --rate 10 --duration 60
stop

# Record while processing live:
start --out ./trials/walking-02 --socket /tmp/csi-data.sock --rate 10 --duration 60
stop
quit
```

`--rate` always means requested probes/s **per TX**, not aggregate, and status prints both. Selection defaults to all compatible, live TXs in the completed roster; allow explicit stable node IDs to select a subset. File-run defaults may create a minimal transport trial with no invented room/participant/activity ground truth. A labelled training run must supply/confirm surveyed geometry, plan/activities and participant/day/split metadata before `begin`; do not silently use example coordinates. Reuse the existing plan/geometry loaders and annotation commands, including rejection of undeclared activities.

Use an internal typed `StarController` for `discover`, `nodes`, `status`, `start`, `stop` and labels, shared by the REPL and tests. An optional newline-JSON stdin/stdout mode provides command IDs, replies and ordered events for automation; logs/prompts go to stderr. The CSI UDS is data-only and never mixes console text/control JSON into Arrow. A second local control socket/GUI is not needed for the first release. stdin EOF, Ctrl-C, termination and `quit` initiate STOP and bounded drain; an unclean crash is handled by radio/host leases.

Auto device selection must enumerate appropriate USB-serial candidates, preserve DTR on open and verify the actual RX handshake before sending a command. Do not transmit commands to arbitrary serial ports or use a stale saved port name. If multiple compatible RXs are connected, require explicit selection. Hold exclusive ownership so a serial monitor or second Rust client cannot consume half the protocol. Boot text and attachment mid-frame resynchronize through the existing deframer. Reconnect requires a new handshake/snapshot and an explicit new run; no invisible continuation across RX boots.

### 9. Star provenance, labels and quality

Represent **physical ingest sources** separately from **radio participants**. There is one USB source tied to the RX. Remote TX inventory includes identity/boot, capabilities, firmware/build IDs, observed settings and link checks; it is not five extra serial sources. Change collector validation and Python provenance loading accordingly, with a new trial schema version if necessary and readers that still accept existing `csi-trial/1` bundles. Keep artifact-completion and measurement-quality meanings separate.

Persist the frozen roster and applied settings/ACKs, discovery/run/command mappings, start/stop/lease outcomes, grant opportunities/timeouts, physical link identity, RX capture/export counters, TX counter snapshots and sink/client loss counters. Receiver-generated inventory/status must not masquerade as TX-originated acquisition frames or make `SourceStatus.node` alternate between RX and TX. TX image hashes declared in the kit manifest are expected artifacts; observed firmware/build identity is separate, and neither is a new on-device flash readback.

Capture timestamps are the single RX radio clock plus callback time; host arrival and operator-label times remain separate. Reuse TimeProbe/TimeEcho to characterize host mapping where valid. One receiver removes cross-receiver alignment, not USB delay or operator/video alignment. Record run boundaries, clip stale runs/late grants explicitly and preserve native data. Continue masking the hardware-invalid first word, preserving subcarrier indices and keeping unknown gain null. Discovery cannot infer surveyed positions, antenna orientation or physical activity labels.

### 10. Rate, memory and radio budget

The current receiver UART is 921600 baud, 8N1: at most 92,160 serial bytes/s before application overhead. A typical 256-byte native CSI record is 336 bytes before COBS/delimiter (16-byte header + 60-byte CSI metadata + 256 native + 4 CRC), approximately 339 bytes framed. Five TXs at 50 probes/s need about 84,750 bytes/s for CSI alone, leaving little margin for status/control/host stalls. Do not advertise five links at 50 Hz as already supported.

Start the bring-up profile at five × 10 Hz, approximately 17 kB/s CSI. This is a transport-validation default, not an endorsed sampling rate for fall-model training. Before collecting a training dataset, select and publish a measured profile appropriate to the intended signal/model; do not silently lower the sample rate just to make serial admission pass. Fewer TXs can leave more rate budget per link. Admission should reserve at least 30% of the serial budget for framing/control/queue variation, calculate demand from the negotiated layout/max record size, and reject excessive requested rates with an explanation. Also check the scheduler's grant/timeout budget. Rate admission is an engineering bound, not an achieved-rate promise. Measure before raising baud/rates or optimizing packet size.

Initial bounds: eight TX roster entries, existing 32 captured-CSI slots, explicit bounded control/RX-event queues and dedup caches, one outstanding grant, bounded raw/publisher input queues and the client bounds above. Give STOP/lease/control handling priority without starving CSI export. Serialize radio sends and retain each driver's pending send future to completion/watchdog reset, as the pinned driver requires. Never perform UART writes, disk I/O, CRC-heavy record encoding or long policy work inside the CSI callback. Report all queue peaks/drop categories separately.

### 11. Implementation sequence and acceptance gates

Each stage lands with focused tests and preserves previous recording fixtures. These are dependencies, not parallel hardware promises.

| Stage | Concrete work / files | Done when |
| --- | --- | --- |
| P0: baseline | Reuse `firmware/csi-esp`, serial hub, current decoder and bench harness. Back up/identify any additional boards before flashing. | One TX → one USB RX → Mac Rust → Python works from the actual flashed images; native CSI layout/channel/rate/gaps are recorded. No AP required. |
| P1: wire + policies | Add `crates/csi-protocol/src/star.rs`, its golden/property tests and `crates/csi-node/src/star.rs`; extend raw record handling for star controls. Pin the envelope, command/result enums, bounds and token rules. | Deterministic host tests cover discovery/backoff, duplicates, incompatible/full roster, owners/boots, START rollback, grant matching, STOP and lease expiry. Existing v1/v2/mesh golden tests remain unchanged. |
| P2: discovery firmware | Add isolated `star-rx` / `star-tx` features and `firmware/csi-esp/src/star.rs`; separate control RX from sensing RX. Implement boot discovery, reply scheduling and cached roster/snapshot. | Power TX first or RX first; a complete ten-second window discovers the compatible powered set. Repeat discovery finds a late TX without reflashing or duplicate entries. Host attaching after the window still sees the roster. |
| P3: run firmware | Add broadcast START/ACK/GRANT/KEEPALIVE/STOP handling, star CSI filtering, priorities, watchdogs and final export watermark. | One selected TX emits only on matching grants; duplicates/old commands cannot emit extra probes. RX/host failure stops grants/probes within declared leases. Start/stop works repeatedly before scaling. |
| P4: Mac controller | Add `server/csi/src/star.rs`, a host controller/serial command router under `csi-host`, capability handshake, auto device selection, REPL/JSON control and run lifecycle. | One process owns serial, shows discovery/ACK/state progress and handles unplug/reboot/signals/EOF without silently resuming. Sink failure can stop RF without blocking the serial/control loop. |
| P5: sinks + provenance | Refactor `live/mod.rs`, `live/source.rs`, `delivery.rs`, `trial.rs` and exporter metadata; add star raw-control inspection/replay and corresponding Python readers/tests. Implement file first, then both, then genuine stream-only. | File trials seal/reopen; live/offline CSI agree; stream-only creates no hidden artifacts and does not require `SessionWriter` or a directory; slow-consumer policies match section 7. One RX source plus many TX participants passes provenance/quality checks. |
| P6: full kit | Run the matrix below, archive supported images/hashes and test configuration, update `firmware/README.md`, `COLLECTION.md`, `EXPORT.md`, Python examples and a short teammate walkthrough. | A teammate can perform the cold-boot discovery/start/label/stop/read workflow on their own MacBook without modifying code or per-node MAC configuration. |

Build/check the host workspace with `cargo build`, `cargo test`, `cargo check-nostd`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` from `sensing/`. Build firmware star TX/RX and legacy TX/RX/pair/wireless variants separately with the pinned Xtensa toolchain and role inputs. Run Python integration tests against both preserved old sessions and new star fixtures. Socket/serial hardware tests may need sandbox permissions; denied hardware access is not a passing RF test.

### 12. Test matrix and release criteria

**Deterministic software tests:** fake-clock discovery rounds, randomized/backoff collisions with repeatable seeds, zero/duplicate/late nodes, capacity/version errors, RX/TX reboot, stale nonce/owner/run rejection, duplicate START/STOP/GRANT, partial START ACK failure, missing STOP ACK, late probe, lease expiry, maximum duration, token exhaustion and input corruption/truncation. Test that discovery/control frames cannot produce CSI rows and that grant gaps do not become false per-TX probe losses. Test bounded memory/control priority and counters under a stalled capture exporter.

**Host/sink tests:** serial/PTY boot noise and partial frames, multiple-device ambiguity, exclusive ownership, RX disconnect/new epoch, start with no sink, overwrite refusal, absent/slow/disconnected UDS clients, schema readiness before the first grant, disk-full/write failure, bounded queues, signal/EOF stop and final watermark/drain. In stream-only tests, use an unwritable/no output directory and assert no recording/status files appear. Verify control-event order, raw replay and live/offline native-byte/timestamp/index equality for file+stream. Old trial schemas/fixtures still load; new provenance has one RX source and all selected TXs, with no fabricated source or missing geometry hidden.

**Physical progression:** one TX, two TXs, then all five. Run file, stream and both modes. Repeat cold-boot discovery with different power/host-attach orders for at least ten cycles; explicitly test late power-on and rediscovery. At the default channel/rate, require the exact expected roster in each completed window, correct per-link MAC/epoch/run attribution and no CSI while stopped beyond the declared in-flight drain. Missing nodes are a failed discovery acceptance run, not proof of absence.

For the initial clean bench, target at least 90% of requested per-link rate with no host/serial capture-export drops, no decode failures after startup resynchronization and zero live/offline native-data mismatches for a healthy consumer. Report RF/grant loss separately; do not redefine the requested rate or hide failing links to pass. Record interval p50/p95/p99/max, largest gaps, queue peaks and explicit sink losses. These are initial engineering thresholds, not fall-detection requirements; tune only with recorded evidence and explanations.

Exercise unplug/crash of the Mac, loss of one TX, RX reboot, wrong kit, second RX contention, deliberate stalled Python and repeated start/stop. Verify lease bounds physically and that stale-run packets never become new-run data. Complete a 90-minute sustained run across radio-timestamp wrap with an explicit duration-limit override, and report RAM/queue/counter health. Measurement labels and surveyed geometry are a separate ordinary-activity walkthrough; do not stage or claim falls as part of a transport test.

### 13. Deferred work and missing assumptions

- Auto channel discovery/hopping, remote OTA/reprovisioning, arbitrary platforms and native Mac/Linux CSI drivers are deferred. A pre-flashed kit assumes matching channel/kit settings and the correct TX/RX image.
- The central RX is physically at the USB cable/board position, not necessarily the laptop's centre. Record its placement/orientation; USB reach limits placement unless the host moves.
- One RX still shares airtime among probes and control and cannot receive while sending its own control. Packet scheduling and achieved-rate tests are necessary even without distributed clock sync.
- Raw/data filenames, Arrow schema, session/run identities, EOF/error semantics and label delivery are part of the interface; an FD alone does not specify them. Training needs durable labels/provenance; stream-only intentionally supplies no hidden recovery archive.
- USB bridge driver/access, a working data cable, power stability, serial-monitor conflicts and board-role identification are part of the teammate setup. Preserve DTR to avoid unintended resets and observe any reset as a new boot.
- A star offers N direct links sharing one receiver, not a mesh's crossing geometry. ML/tomography are Python consumers of the same data; neither is implemented by a firmware mode switch or guaranteed by discovery.
- Existing mesh/piggyback and its independent hardware-acceptance backlog stay available, but do not gate this kit. The supported first-release path has one RX and at most five TXs.

**Next acceptance work:** expand the archived one-TX star bench to two and five TXs, physical contention/lease/unplug/stall cases and the 90-minute timestamp-wrap run. The bounded envelope, policies, firmware, Mac controller and three output modes are implemented. No native laptop NIC work or Wi-Fi-network provisioning is required for option 2.


## Next steps: unblock data collection and tomography

Earlier handoff work and evidence (2026-10-22 onward; the star-first scope above now determines implementation order): give teammates a dependable way to collect labelled CSI and use it in Python for model training and radio tomography. Further mode comparisons support this work; selecting a universal winning radio mode is not a prerequisite for the first data handoff.

**First usable milestone:** a teammate can record a labelled trial and load its complex CSI, timing, validity and geometry in Python without learning the binary protocol.

### Starting point

- Firmware sends probes and captures peer CSI as separate capabilities. Fixed-role mode sends one way; independent TX/RX sends both ways periodically; the packet-driven pair uses requests and responses. Two ESP-WROOM-32 boards have been tested at a target of 50 measurements/s **per direction**, with achieved rates reported per trial. This is about 100 measurements/s combined, not 25 per direction. The current USB bench needs neither an access point nor a Raspberry Pi.
- Protocol v2, raw recording/replay, serial/UDP ingest, sequence and drop accounting, clock fitting and versioned installation geometry exist. Sessions retain native CSI and provenance for later reprocessing.
- Pair turns coordinate transmission opportunities; they do not synchronize node clocks. Shared-probe receiver-clock fitting and host request/echo mapping exist, but physical alignment between receivers still needs a third board hearing the same probes.
- The Python/export interface, original-ESP32 HT20 complex-layout decoder and initial collection/label workflow are available. Remaining pieces include validated capture-to-host label alignment, wider layout coverage, receiver-gain extraction and general N-node scheduling. There is no classifier or tomography solver in this workspace.

### 1. Deliver the offline Python handoff, then live delivery (M4)

**Initial delivery implemented 2026-10-23:** `csi export`, the thin reader under `python/`, and `hub --arrow-socket` share `csi-export/1`. See [EXPORT.md](EXPORT.md) for usage, validation and limits. Existing real sessions exercise both file formats and re-recorded replay; local-socket tests exercise delivery and stalled-client isolation. No new firmware/RF trial was needed. Step 2's initial workflow is also available below.

Keep acquisition and protocol decoding in Rust; Python owns DSP, training and tomography. Broader layouts and applying host-time mappings remain follow-ups to this initial handoff.

The handoff must include:

- Verified complex CSI with explicit LTF/subcarrier layout and a validity mask. Verify ESP32 signed imaginary/real ordering and concatenated LTF blocks against golden fixtures. Preserve indices when masking null/invalid samples, including `first_word_invalid`; 256 native bytes must not be presented as 128 independent usable subcarriers.
- TX/RX and link identities; native radio ticks, callback time, host arrival and nullable mapped time; clock timeline, fit provenance and uncertainty. Never silently bridge epochs, radio resets or unmapped periods.
- Sequence/gap information, RX and capture configuration, nullable gain with validity, and session/geometry references. Retain access to native bytes for audit and future decoder corrections.

**Done when:** a Python example loads an existing real session, selects a directed link and returns indexed complex samples with time and validity. Golden fixtures establish decoding, raw replay produces the same exported values, and live delivery uses the same schema. A stalled live reader drops counted batches while recording continues unaffected.

### 2. Make labelled collection reproducible

**Initial workflow implemented 2026-10-23:** [COLLECTION.md](COLLECTION.md) documents `csi collect`, editable templates and Python `Trial`. The collector snapshots/hashes declared images/build manifests, source configuration and geometry; binds activity/position intervals and uncertainty to the hub's source-arrival clock; keeps labels/splits outside raw recording; and seals an Arrow export and quality report. Firmware provenance is operator-declared, not a new on-device verification. The collector does not reconfigure nodes.

**Physical pipeline check, 2026-10-23:** [five two-board trials](bench/2026-10-23/live-python-051450/README.md) delivered all 7,127 recorded CSI captures to the healthy Python reader, verified live/offline identity/time/native-byte equality, and sealed despite a stalled secondary reader. Normal 20 ms flushing measured about 17 ms median / 29 ms p95 from complete-frame hub arrival to Python yield; 100 ms flushing measured about 58–59 ms / 106 ms. This is transport validation with software phase labels and unsurveyed geometry, not physical activity/tomography ground truth or capture-to-host alignment.

Quality includes link/decoder validity and clock/gain coverage, finalized gaps, observed counter-window deltas, missing links and source/role/channel checks. Ordinary-activity walkthroughs cover empty, walking, standing and sitting. `collect --arrow-socket PATH --arrow-flush-ms 20` feeds Python live; bounded delivery still protects raw recording. Labels remain a separate event log. Any staged-fall collection needs its own agreed protocol.

**Remaining timing gate:** validate capture-to-host alignment and operator/video boundary timing before treating labels as precise event ground truth. Python currently requires explicit `host_arrival_proxy` matching, trims declared boundary uncertainty, and excludes truncated intervals by default. That uncertainty does not bound transport delay. No new physical labelled trial was performed to validate the operator walkthrough.

**Done when:** another teammate can follow the walkthrough, collect and label a trial, reopen it in Python and identify missing or poor-quality links without reverse-engineering the recorder.

### 3. Validate signal preservation alongside the handoff

Use [SIGNAL_VALIDATION.md](SIGNAL_VALIDATION.md) for metrics and [SIGNAL_VALIDATION_PLAN.md](SIGNAL_VALIDATION_PLAN.md) for implementation details. Start with native-radio timestamp auditing, decoder fixtures and known-signal replay; existing callback-interval measurements alone do not establish acquisition timing. Then test brief-change coverage, distortion, artifacts and queue/load behavior with explicit fault assumptions.

**Done when:** each reported result identifies measured versus simulated inputs, timing provenance, achieved rate, loss and validity. Synthetic preservation tests inform acquisition design; they do not establish fall accuracy. Do not hold all teammate access to recordings until the entire comparison suite is complete.

### 4. Deferred mesh transport acceptance and shared-receiver timing

**Wireless implementation, 2026-10-24:** [WIRELESS.md](WIRELESS.md) describes bounded ESP-NOW piggyback batches and a Wi-Fi/UDP gateway that feeds the same Rust recorder and live Python Arrow API without UART CSI. Raw carriers retain complete original protocol-v2 records. Host corruption/routing/live-offline tests and both firmware builds pass; both boards were flashed and verified. This is an initial configured two-node relay, not general mesh scheduling or OTA management.

**Role separation, 2026-10-26:** wireless firmware 0.3.1 removes AP association, credentials-file reads and association waits from remote nodes. They require an explicit channel (1–11) matching the gateway AP. Only the gateway uses network credentials/UDP. Remote/gateway/legacy builds and role-configuration tests pass; the remote ELF excludes local network credentials. These new images have not been flashed: no serial boards were visible. Channel discovery/migration and a Linux-integrated SPI/UART radio adapter remain unimplemented.

**Immediate blocker:** the gateway scan found 18 APs but zero configured-SSID matches and association returned `NoAccessPointFound`. The UDP-only physical session sealed with zero records. The computer is on 5 GHz; confirm an accessible 2.4 GHz SSID before running the two-direction piggyback/relay/Python acceptance and 20/50/100 ms node batching comparison. No wireless throughput/latency result exists yet. Evidence: [wireless bench](bench/2026-10-24/wireless/README.md).

After that acceptance:

Identify and back up each additional board before flashing. First run one TX and two RX so both receivers hear common probes; measure clock-fit coverage, residuals, drift and recovery. Complete the sustained M5 gates below, including the one-hour run, wrap/restart handling, gain availability, callback timing and RAM measurements.

Next establish a low-rate three-node independent TX/RX baseline. The firmware has the two capabilities needed for this experiment, but more-than-two-node behavior is not physically validated yet. Check per-node encoded UART demand, queue peaks and drops before increasing rates or connecting all six boards. A receiver hearing five transmitters exports five links; its byte budget grows accordingly.

**Done when:** simultaneous directed links are correctly attributed and loss is accounted for under a declared rate/configuration. Keep a general coordinated N-node scheduler as separate M7 work: the current configured-peer, lower-MAC-leader pair cannot simply be expanded to six nodes.

### 5. Provide a controlled tomography dataset

Survey fixed node positions and antenna orientations, capture an empty-room baseline, then record a person at known positions with repeated sessions. Supply aligned link observations, validity and geometry so Python teammates can validate calibration and a forward model before evaluating a coarse 2D change/occupancy map.

**Done when:** a teammate can compare baseline and occupied trials by link and known position, with gaps and timing uncertainty visible. Unknown receiver gain means raw amplitude is not calibrated physical attenuation. Additional directed links do not automatically provide independent spatial information; height/posture claims require suitable placement and further validation. This milestone does not establish a dense 3D point cloud or fall-detection accuracy.

The numbered items in this older handoff section preserve implementation history and follow-up work. The active USB-star P0–P6 plan above determines current delivery order; the milestone table below remains the longer-term acceptance reference.

## Scope

`sensing/` holds all Rust in the monorepo: the shared `no_std` wire contract, the host server that ingests CSI from sensing nodes and local receivers, records and replays sessions, tracks per-link health and hands batches to the Python DSP/ML pipeline, and (from M5) the sensing-node firmware. None of it does signal processing. The Python reader lives in the sibling [python/](../python/README.md). The fall-event/alert lifecycle and caregiver-facing API are out of scope here; they need an owner elsewhere.

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
│   └── csi/              one binary: `csi record | replay | inspect | fit-clocks | export | hub | collect`
└── firmware/             (M5) separate workspace: own Cargo.lock, rust-toolchain.toml (`esp`)
                          and .cargo/config.toml (Xtensa target, build-std, flash runner)
```

Placement rule: a package used by both sides goes in `crates/` and must stay `no_std`; host-only code goes in `server/`; embedded-only code goes in `firmware/`. Firmware depends on shared crates by path (`../crates/csi-protocol`). A crate belongs to only one workspace, so shared crates are members of the host workspace, which is where their tests, clippy and `check-nostd` run. `csi-protocol` has no dependencies, so the two lockfiles cannot disagree about it.

Added when their milestone starts, not before:

| Package | Location | Milestone | Notes |
| --- | --- | --- | --- |
| `csi-node` | `crates/` | M5, M7 | Implemented initial no_std pair request/response policy, turn tags and adaptive timeouts; general scheduling remains pending. |
| `csi-esp`, `tx-node`, `rx-node`, `txrx-node` | `firmware/` | M5, M7 | `esp` toolchain + Xtensa target for original ESP32. If the bench comparison selects ESP32-C5 nodes, they build for a RISC-V target instead; keep `csi-esp` chip-generic until then. |
| `csi-linux` | `server/` | Deferred | Native receiver adapter only if a later hardware comparison selects it; not needed for the USB ESP32 star kit. |
| Python reader | `capstone/python/` | M4 | Implemented `capstone_csi`: `pyarrow`/NumPy reader of Arrow streams and Parquet; no PyO3. |

The research's separate `csi-hub` and `csi-cli` are one `csi` binary with subcommands until a deployment reason to split appears; they share every dependency.

From `sensing/`, check everything with `cargo build && cargo test && cargo check-nostd && cargo clippy --all-targets -- -D warnings`.

## Design

The wire format (M1, now v2), session format (M2), geometry and shared-probe clock fit below are implemented; changing an existing layout means bumping its version. Live ingest is implemented in `csi-host::live`, with optional Python delivery in `csi-host::delivery`. An initial original-ESP32 TX/RX UART bench adapter is in `firmware/`; full M5 hardware acceptance remains pending.

**Wire format.** `csi-protocol` defines its own format; esp-csi-rs's `wire::RxMeta` served as the field checklist. A 16-byte little-endian header (version, kind, node, epoch, seq) is followed by a fixed-layout body; `Csi` ends with length-prefixed native CSI bytes, unchanged. Layouts are documented on each type and pinned by fixtures in `crates/csi-protocol/tests/golden/`. Decoding never allocates or panics. New kinds can be added without a version bump; hosts are updated before nodes.

**Framing.** A frame is a message plus CRC-32/ISO-HDLC (the same as Python's `zlib.crc32`). UDP carries one frame per datagram, below the path MTU. Byte streams (UART, TCP) carry COBS-encoded frames separated by zero bytes; receivers resync at the next delimiter, so boot garbage or a mid-frame start costs one rejected frame. A v2 HT40 record (384 CSI bytes) is a 464-byte frame, up to 467 bytes on a stream (COBS worst case plus delimiter). v1 was a 462-byte frame, up to 465 stream bytes.

**Messages.**

- Probe (over-air, in the ESP-NOW frame body): magic, version, transmitter node, transmitter epoch, schedule id, sequence; 23 bytes plus optional padding. `Probe::find` locates it anywhere in a received payload.
- Node → host: `Hello` (chip, roles, firmware version and commit, configured channel, ESP-IDF CSI config, pool capacity; repeated periodically); `Csi` (capture sequence, node monotonic µs, radio timestamp, 802.11 source MAC, attached probe or none, `first_word_invalid`, the ESP32 `wifi_pkt_rx_ctrl_t` fields, native bytes); `Counters` (cumulative per epoch: TX requested/accepted, callbacks, filtered, captured, pool drops, export drops, export bytes, pool high water); `TimeEcho`.
- Host → node: `TimeProbe`. Kinds `0x81` (schedule) and `0x82` (config) are reserved and get defined with M7.
- Loss is attributable per layer: gaps in the header `seq` are transport loss once counted export drops are subtracted, gaps in `capture_seq` are capture-pool drops, and gaps in the probe sequence are over-air or callback loss.
- There is no `Layout` message: the CSI byte layout follows from `Hello` (chip, CSI config) plus each record's RX metadata and length, so no record depends on a separately sent descriptor. M4's initial decoder maps original ESP32 HT20 to indexed LTF blocks; unsupported combinations retain raw bytes.
- **Receiver gain (protocol v2).** `RxMeta` now carries the receiver's AGC gain (`u8`) and FFT gain (`i8`) as optional `RxGain`, with RX flags bit 5 indicating validity. Without them a gain step cannot be told apart from a person's attenuation, and the analysis that detects someone lying still after a fall needs exactly that. Extraction from `rx_ctrl` is chip-specific (Espressif's `esp_csi_gain_ctrl_get_rx_gain`) and unverified on original ESP32, so the field is optional per record. Encoders emit `VERSION = 2`; decoders accept v1 and v2, with gain absent for v1. Golden v1 fixtures and a frozen M2 session prove backward decoding and byte-identical replay. The unchanged over-air probe retains its independent `PROBE_VERSION = 1`. Synthetic truth tracks injected and observable gain steps; inspection reports per-link valid fraction and steps/minute.
- Batching (M5+): a new kind with one header per batch and per-record time deltas; a record is never truncated. Default flush interval ≤100 ms once adaptive scheduling runs, so schedule changes follow a tracked person promptly.

**Clocks.** Each `Csi` record carries the radio's native 32-bit receive timestamp (`rx_timestamp_us`) and the node's 64-bit monotonic time at callback (`node_time_us`); the header carries the boot epoch. The host adds monotonic and wall arrival times. Per-source clock mappings are separate versioned records in the session, stored with their fit residuals, and raw values are never rewritten. Two mechanisms produce them:

- **Cross-node alignment from shared probes.** Every receiver that hears one broadcast probe (`tx`, `epoch`, `seq`) received it at the same instant, to within propagation and detection jitter. The hub pairs `rx_timestamp_us` across receivers for the same probe and fits each receiver's offset and drift against one reference timeline (reference-broadcast synchronization). Every node also receives, so every node gets a mapping, and a Linux receiver hearing the same probes joins the same fit. The fit runs continuously over a sliding window because crystals drift apart by tens of milliseconds per hour. Fall features need a few milliseconds of alignment; one sample at 200 Hz is 5 ms.
- **Host time from request/echo** over the export link itself (`TimeProbe`/`TimeEcho`), which works identically over UART, USB and UDP. It maps the reference timeline to host monotonic and wall time for labels and alerts, where tens of milliseconds is enough, and measures export latency. NTP is not planned.

ESP-IDF documents `rx_timestamp_us` as precise only with modem sleep and light sleep off, so firmware calls `esp_wifi_set_ps(WIFI_PS_NONE)`. Its timer starts at `esp_wifi_start()`, so its origin differs from `node_time_us`, and it wraps every ~71.6 minutes, so the host unwraps it against `node_time_us`.

**Python boundary (initial M4 implemented).** `hub --arrow-socket` publishes homogeneous per-link/per-layout Arrow IPC batches over a Unix socket. `csi export` writes the same `csi-export/1` schema to Arrow/Parquet; a re-recorded replay exports identically at the data-column level. The Python reader exposes the same batch API for files and live streams. Columns carry record identity, raw capture/arrival times, nullable relative mapped time and fit provenance, sender sequences, RX metadata, nullable gain, validity and native bytes. Schema metadata carries layout/configuration, session and geometry provenance. Bulk data uses Arrow, while small quality/provenance sidecars use JSON. See [EXPORT.md](EXPORT.md) for the limited decoder support, causal fit semantics and bounded lossy live delivery; applying stored host-time fits is still pending.

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
| M4 | Python handoff: Arrow IPC publisher on a Unix socket, `csi export` to Arrow/Parquet, thin `pyarrow` reader, ESP32 layout table (subcarrier map) | **Initial delivery implemented:** one batch API for live and files/re-recorded replay; counted stalled-client drops while raw recording seals; golden signed-I/Q, LTF/index/mask fixtures and real-session checks. Decoder currently supports original ESP32 HT20; broader layouts and mapped host capture time remain follow-ups. See [EXPORT.md](EXPORT.md). |
| — | **Hardware gate** | Everything below needs real boards. |
| M5 | `csi-node`, `csi-esp`, `tx-node`, `rx-node` speaking M1 over UART | Passes the gate in `research/esp-csi-rs-verification.md`; sender-sequence loss, pool drops, export drops and host rejects reported separately; RAM and callback time measured. Firmware runs with `WIFI_PS_NONE`; a one-TX, two-RX bench run of at least 1 h reports shared-probe fit residuals (p50/p95/p99) and drift per receiver, with p99 under 1 ms. |
| M6 | Several TX nodes into one USB ESP32 receiver at the MacBook | Follow the active P0–P6 star-kit plan: discovery, run control, direct link identity and file/stream/both delivery. Native laptop NIC capture is deferred. |
| M7 | `txrx-node`, `Schedule`/`Config` messages, control socket and distribution | Each node emits observable probes and exports peer CSI; no self- or export-frame contamination; a schedule request from Python changes per-transmitter rates at the stated cycle on every node, within the hub's airtime budget, and is recorded in the session. Deferred for the first star release; P1–P5 above define its smaller discovery/run-control scope. Preserve current mesh code for a later comparison. |
| M8 | Unattended operation | Service supervision, disk full, rotation/retention, restart recovery and node loss produce explicit health state. |

M1–M4 need no new hardware. Python teammates can export existing real recordings using [EXPORT.md](EXPORT.md). Synthetic sessions remain useful for transport/health tests; the current noise generator can use layouts outside the initial complex decoder's support.

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
- The USB ESP32 star kit is the current delivery priority. Preserve the two-tier ESP32 mesh and native laptop receiver adapters as later options.


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
- **Star kit / M6:** use the existing ESP32 USB capture adapter at the MacBook. `csi-linux` remains unstarted and deferred; the active P0–P6 plan does not depend on native NIC CSI extraction.
