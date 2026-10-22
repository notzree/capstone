# Sensing host

Rust acquisition, raw session recording/replay, health accounting and clock alignment. CSI bytes remain unchanged; this workspace does no DSP or classification. M0–M3 are implemented and tested on the host. An initial original-ESP32 UART bench adapter is available in [firmware/](firmware/README.md); full M5 hardware acceptance and the Python Arrow boundary remain pending.

Proposed next validation: [signal preservation and acquisition metrics](SIGNAL_VALIDATION.md) and the [implementation plan](SIGNAL_VALIDATION_PLAN.md), including short-event coverage, known-signal distortion, timing, multi-link freshness and six-node UART capacity. These tests have not yet been implemented or passed.

## Run the live hub

From `sensing/`:

```sh
cargo run -p csi -- hub --config examples/hub.toml --out /tmp/csi-live --seconds 30
cargo run -p csi -- inspect /tmp/csi-live
cargo run -p csi -- replay /tmp/csi-live --speed max
```

Edit [examples/hub.toml](examples/hub.toml) to match your endpoints. Output must be a new directory. Omit `--seconds` to record until Ctrl-C or SIGTERM. Add `--geometry survey.json` to attach installation geometry. `RUST_LOG=debug` controls tracing to stderr.

Each `[[sources]]` has a unique `name` and either:

- `kind = "udp"`, `bind = "HOST:PORT"`, `peer = "NODE:PORT"`. Bind uses a numeric IP address. One frame+CRC per datagram. Each configured source has its own listening socket and fixed peer address; foreign peers are counted and ignored. `bind` port zero chooses a free port printed at startup. The peer's address is also where the hub sends time probes.
- `kind = "serial"`, `path = "/dev/..."`, `baud = 921600`. Serial uses 8N1, no flow control, COBS+zero-delimited frames. The hub retries missing/unplugged ports, discards partial framing state on reconnect and preserves DTR rather than explicitly toggling it. For Unix pseudo-terminal simulation, use `baud = 0` to skip hardware-specific baud/DTR ioctls; this is not a physical UART rate.

The host sends `TimeProbe` on the same connection, with host node id all-zero, epoch zero and a request sequence. A node replies with its own header and `TimeEcho` body, returning the exact `probe_id` and `host_time` and stamping `node_rx_us`/`node_tx_us`. Only outstanding requests on that source can contribute to host-time fitting. Replies without a matching request remain in the raw recording and increment `unmatched_echoes`.

The source's `active` state means valid frames are arriving. Each reported CSI link has its own `stale` flag based on CSI arrivals, so echoes/counters cannot hide a stopped sensing link. Cable connections, stale transitions and node epochs are separate: a reconnect does not invent a boot epoch. Firmware must change the epoch on reboot. UDP has no connection handshake; silence produces `stale`, and a resumed peer returns to `active`.

Link timing and gain order use the node's monotonic clock to distinguish fresh captures from late traffic across a radio-timer restart. Per-link `radio_restarts` counts observed origin changes; rates and gain-step frequency use accumulated capture spans within runs, and interval histograms never bridge a restart. Large sequence gaps immediately finalize the portion older than the 1,024-value reordering window. The boundary value remains eligible for a late arrival.

## Session output

The existing `manifest.json` and `segment-*.bin` remain the replay contract. Raw undecodable frames within the protocol size limit are kept; stream corruption and oversized inputs produce framing-error records. Oversized UDP payloads are rejected rather than truncated into potentially valid frames. Additional files are:

| File | Contents |
| --- | --- |
| `hub-config.toml` | Validated configuration used for this run |
| `hub-status.json` | Atomically replaced live status, cumulative source counters, bounded health view, current receiver/host mappings, queue peaks and capacity/drop counters |
| `hub-events.jsonl` | Source transitions, node epochs and health events |
| `clock-fit.jsonl` | Append-only receiver mappings, handovers and checkpoints; session-bound `csi-clock-fit/1` header |
| `host-clock.jsonl` | Versioned reference-timeline → host-monotonic fits, with contemporary wall anchors, adjusted RTT, residual and uncertainty |

`clock-fit.jsonl` checkpoints extend the original header/mapping format with current clock identities and cumulative unwrap/reset counts. The reader also accepts older files without checkpoints. Inspection recomputes its clock report from raw input with the requested/default parameters and separately reports stored-file status. The live current view is in `hub-status.json`. Live radio-run identities use non-repeating session generations after cache eviction; if continuity is lost and no mapped receiver survives, a new timeline is explicitly recorded. A returning receiver can still rejoin a surviving receiver's timeline through shared probes.

Host maps need a shared-probe fit and at least three correlated echoes. They use the lower-RTT half (at least three) of up to 32 recent echoes. A wall anchor records the wall clock at reception; it may jump independently of monotonic time. RTT/2 plus fit residual is an uncertainty estimate, not a guarantee against network asymmetry, radio clock error or callback latency. Check receiver fit residuals alongside it.

## Queue and memory behavior

One async task handles each source. A bounded queue feeds a dedicated recorder thread. The recorder writes a raw record before inspecting it or offering it to consumers. Live subscriber queues use `try_send`; each has its own drop counter. A slow subscriber never blocks recording. The `csi-host::live::subscription` API exposes these queues for the M4 publisher; Arrow/socket delivery itself is not implemented yet.

A full recorder queue backpressures source reads. There is no intentional recorder-queue drop. Physical UART buffers or the UDP kernel receive buffer may overflow if storage cannot keep up; frame/capture/probe sequence gaps expose resulting loss where sender counters allow it. Graceful shutdown stops reads, hands off already acquired records, drains the queue, finalizes pending frame/capture/probe gaps, flushes derived files and seals the session. Completed status and final loss events therefore include gaps that were still open in live snapshots. Recorder/source failures propagate and leave an incomplete manifest.

Defaults are 8,192 queued inputs and 300,000 clock samples. Clock history streams to disk; retained samples expire after the clock window plus up to a refit step, and admission over the cap increments `clock_capacity_rejects` while raw recording continues. Size this cap above `total CSI/s × (window + step)`; e.g. three receivers at 1,000/s require about 210,000 samples with 60 s/10 s defaults. The default cap is insufficient for a full 12 × 6 × 100/s mesh at that window; increase it explicitly.

Live health retains at most 256 nodes, two receiver epochs/node, 64 links/epoch, two transmitter epochs/link and 16 layouts/link. Evictions/admission rejects increment `health_history_evictions`; detailed historical accounting comes from offline inspection. Clock state admits at most 256 simultaneous receiver clocks. Timers are limited to 10 ms–1 h; source/subscriber counts are capped at 64 and queue capacities at 65,536. Source buffers, subscriber queues, echo history and clock/health caches are bounded independently of run length. The existing manifest still keeps one metadata entry per rotated segment, so total process metadata is not a strict constant across indefinitely many segment rotations; retention and unattended disk management remain M8.

## Checks

```sh
cargo build
cargo test
cargo check-nostd
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Live integration tests open loopback UDP sockets and Unix pseudo-terminals; run them with those local OS permissions available. Each throughput test sends three receivers × 1,000 CSI records/s for three seconds, plus echo traffic, with 384-byte CSI payloads. They assert exact CSI recording counts, segment integrity, recovered receiver drift/residuals, host timing and bounded counters. The UDP test stalls a consumer and asserts every downstream drop is counted. Other tests cover stale/resumed UDP peers, serial unplug/reconnect, split/corrupt framing, epoch changes, foreign/oversized UDP, unmatched echoes, recorder failures and clean shutdown. These establish host behavior; a pseudo-terminal does not validate USB bridges, physical baud throughput or RF sensing.
