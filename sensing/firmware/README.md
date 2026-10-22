# ESP-WROOM-32 bench firmware

Initial original-ESP32 adapter for the first one-TX/one-RX bench. This is the start of M5, not completion of its sustained-run, callback-timing or gain-extraction gates. It uses the same `csi-protocol` crate as the host. No external access point is required.

Select capabilities: `--features tx`, `--features rx`, or `--features tx,rx`. `--features pair` enables both with the packet-driven pair policy described below. Both use channel 6, 921600-baud UART0 (GPIO1 TX/GPIO3 RX), modem sleep disabled, and a random boot epoch initialized after Wi-Fi starts. No automatic light-sleep policy is enabled.

- TX broadcasts a padded `CSIp` probe with a target interval of 10 ms in periodic mode, forcing HT20/MCS0 long GI. Each probe carries the transmitter MAC, boot epoch, sequence and unscheduled schedule id zero. Sends are awaited to completion; actual rate may be lower due to driver latency. `tx_accepted` counts pending/ready-success submissions and is a lower bound if a very fast failure completes during the first poll; verify peer reception separately. A 2 s missing-completion watchdog resets the node while retaining the pending future until reset.
- RX enables CSI and filters for a valid probe whose transmitter matches the received source MAC. Native CSI bytes and RX metadata are copied in the callback into a bounded 32-slot queue. UART framing/writes happen in an async exporter, outside the callback. LLTF/HT-LTF are enabled; STBC second LTF, merging and channel smoothing are disabled. Gain is absent until extraction is verified.
- Both export periodic `Hello`/`Counters` and answer `TimeProbe` with `TimeEcho`. Bootloader console output may precede binary data; the initial delimiter and host deframer permit resynchronization. UART writes and incoming host commands have bounded buffers. This adapter has no UDP export, geometry, host schedule updates, DSP or ML.

## Build

The firmware is a separate workspace using the Xtensa `esp` Rust toolchain. From `sensing/firmware/` with `espup`'s compiler environment sourced:

```sh
cargo +esp build --locked --release --features tx
cargo +esp build --locked --release --features rx
```

Each role replaces `target/xtensa-esp32-none-elf/release/csi-esp`; preserve the first ELF before building the other role. The prepared images and role-specific ELFs are archived under `../bench/2026-10-21/firmware/` with hashes and a flash manifest.

For this preparation run, the already-installed isolated compiler environment was used:

```sh
. /private/tmp/capstone-rust-verification/export-esp.sh
export CARGO_HOME=/private/tmp/capstone-rust-verification/cargo
export RUSTUP_HOME=/private/tmp/capstone-rust-verification/rustup
```

These temporary toolchain paths may need recreating later. The firmware source, lockfile, prepared images and device backups are preserved in the project.

## Prepared boards and recording

Consult `../bench/2026-10-21/mode-comparison/README.md` for the current 50 Hz pair configuration, MAC/port mapping and measured mode comparisons; the parent bench README preserves the initial fixed-role baseline. From `sensing/`:

```sh
cargo run --release -p csi -- hub --config bench/2026-10-21/dual-role/hub.toml --out bench/2026-10-21/dual-role/trial-01 --seconds 60
cargo run --release -p csi -- inspect bench/2026-10-21/dual-role/trial-01
```

Choose a fresh output directory for each trial. Both boards are recorded so transmitter requested/accepted counts can be compared with receiver captures. Any board with RX enabled produces CSI. Rediscover and match MACs if USB device paths change.

## Flash provenance and backups

The prepared app images are generated from each role's ELF by esptool. The bootloader and partition table were reused from the pinned ESP-IDF 5.5.2 build already in `demo/`, after checking their hashes against `demo/firmware.lock.json`. Flash offsets come from that build's generated `flasher_args.json`: bootloader `0x1000`, partition table `0xa000`, factory application `0x20000` with a 1 MB application partition. The generated app images fit that partition. See `flash-manifest.json` for input hashes and settings.

Each board has a complete 4 MB original flash backup, SHA-256, and a device-digest verification log in its MAC-named `../bench/2026-10-21/boards/` directory. Raw flash backups are excluded from Git. `tools/flash.py` requires a verified backup, checks backup/image hashes and the currently connected board MAC, uses manifest offsets, then verifies flashed bytes. Its Python dependencies are pinned in `requirements-tools.txt`.

## Combined TX/RX and packet-driven pair mode

The two capabilities remain enabled together; Wi-Fi is not reinitialized between turns. RX copies native CSI while TX runs as a separate async task. The independent ESP-NOW receive queue is drained. Self probes are rejected before capture/export; only the configured peer and schedule can signal pair control.

`pair` requires `CSI_PEER_MAC` at build time. The lexicographically lower node MAC is leader. For these two boards:

```sh
CSI_PEER_MAC=b0:b2:1c:0b:07:1c cargo +esp build --locked --release --features pair  # a0 board
CSI_PEER_MAC=a0:b7:65:dd:e9:3c cargo +esp build --locked --release --features pair  # b0 board
```

Archive each image before building the next. The flasher supports `--image-role` and verifies the manifest's target MAC as well as the connected MAC and full-flash backup.

A request/response tag (`CSIt` v1, 14 bytes) occupies the existing over-air probe's padding. It carries kind, leader boot epoch and round. Existing `CSIp` probes and UART protocol v2/CSI layout are unchanged. Each request grants one immediate response; there is **no fixed guard delay**. Only the exact-round response releases the leader's next turn. Its timeout adapts from local round-trip samples (EWMA RTT plus four deviations, 30–500 ms; initial 150 ms), doubles on timeout, and recovers on successful rounds. A 100 ms leader interval caps the nominal rate at 10 rounds/s. Timeouts advance to a fresh round, not an ambiguous retransmission of an old round. Responder input older than 30 ms is discarded; duplicate/backward sequences and four retired leader epochs are rejected.

This synchronizes transmission turns rather than setting both clocks equal. Queue/airtime delays still apply; it is not guaranteed TDMA or the arbitrary-N adaptive scheduler. Two participants do not supply shared third-party probes for cross-receiver clock residuals. The over-air turn tag is not included in UART CSI records, so recorded sessions validate link identity/rate/loss; exact correlation and timeout adaptation are supported by policy tests and source review, without a claim of recorded per-round timing telemetry.

See the [dual-role bench plan and results](../bench/2026-10-21/dual-role/README.md). The previous one-TX/one-RX images and sessions remain archived. Both connected boards now run pair firmware.

## Matched-rate comparison builds

`CSI_PROBE_INTERVAL_US` optionally sets the compile-time probe interval (1000–1000000 us). Without it, periodic TX targets 100 Hz and pair leader targets 10 Hz. Pair responders remain request-driven. `CSI_PROBE_INTERVAL_US=100000` compares 10 Hz targets across modes; `20000` sets a 50 Hz target. Actual enqueue/completion/received rates can be lower. Peer and interval build inputs and per-image hashes are archived by the [mode-comparison plan](../bench/2026-10-21/mode-comparison/PLAN.md).

`tools/run_comparison.py` runs the matched profiles with MAC checks, flashed-byte verification, warmup, raw recording and offline audits, then restores the 10 Hz bidirectional pair baseline by default; use `--restore-condition pair-50` to select the current 50 Hz configuration. `tools/compare_trials.py` produces per-trial/aggregate JSON, Markdown and SVG. Programming baud can be selected with `tools/flash.py --baud 460800`; the runner falls back to115200 on flash failure, with both attempts logged. UART acquisition baud stays921600 regardless of programming baud.
