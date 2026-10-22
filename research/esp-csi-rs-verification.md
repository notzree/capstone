# esp-csi-rs verification

2026-10-19. **Software verification passed for original ESP32 and ESP32-S3; real hardware capture is still unverified.** No new product implementation, firmware flash or driver installation was performed.

## Exact source and observed results

Inspected and built [csi-rs/esp-csi-rs](https://github.com/csi-rs/esp-csi-rs/tree/6cfbd6b73472c73696683b288b833c1910097061), commit `6cfbd6b73472c73696683b288b833c1910097061`, package `0.13.0`. Source was cloned into `/private/tmp/esp-csi-rs-verification-20261019`.

| Check | Observed outcome | What it establishes |
| --- | --- | --- |
| Host wire/policy tests | **24 passed, 0 failed** on native Apple Silicon | Serialization, layout tables, malformed-input handling, synthetic source and reporting-policy tests work on the host. |
| Original ESP32 release builds | **5 examples linked successfully:** `sniffer`, `emitter`, `esp_now_simplex`, `hw_check`, `csi_callback` | The pinned source and locked dependencies compile/link for `xtensa-esp32-none-elf`, including the queued-callback example. |
| ESP32-S3 release builds | **The same 5 examples linked successfully** | Compile/link compatibility for `xtensa-esp32s3-none-elf`. Each produced a linker warning about a LOAD segment with RWX permissions; this did not fail linking. Runtime/memory behavior is untested. |
| ESP32-S2 | **No feature/alias in this library** | Generic Rust/ESP-IDF support for S2 does not establish support in this particular crate. Do not select S2 on that assumption. |
| Live original ESP32 capture | **Not run** | Only Bluetooth/debug-console serial devices were visible. No real CSI rate, stability, motion response or cross-board interoperability was established. |
| Live S3 capture | **Not run** | No S3 board was inspected or connected. |

The upstream [compile run](https://github.com/csi-rs/esp-csi-rs/actions/runs/38183278906) also passed its original ESP32/S3 compile and defmt jobs. It ran at `72cb565…`; the difference to our checked HEAD is README only. Those CI results supplement, rather than replace, the actual local builds above.

Machine-readable artifact hashes/paths are in [results.json](verification/esp-csi-rs/results.json). Build logs and the host-test dependency lock are saved alongside it. The upstream checkout remained unchanged in tracked files.

## Toolchain and repeatable commands

The crate uses **no_std + esp-hal + esp-radio + Embassy**, not an ESP-IDF application through `esp-idf-sys`. A dependency named `esp-bootloader-esp-idf` does not make it an ESP-IDF runtime. Selected dependencies resolved from its lock include `esp-hal 1.2.2`, `esp-radio 1.0.0-beta.1` and `esp-rtos 0.4.0`.

The existing host compiler was Rust `1.97.1`. An isolated ESP toolchain was installed with `espup 0.18.0` under `/private/tmp/capstone-rust-verification`; embedded rustc is `1.97.0-nightly (8ea53bcd7 2026-07-08) (1.97.0.0)`. Default user toolchains and shell startup files were unchanged. Docker was unavailable, so builds ran natively.

Host tests, using a temporary dependency cache and the existing stable compiler:

```sh
CARGO_HOME=/private/tmp/capstone-rust-verification/cargo \
  cargo +stable test --manifest-path wire-tests/Cargo.toml \
  --target aarch64-apple-darwin
```

For firmware, from the temporary upstream checkout after sourcing its isolated compiler environment:

```sh
. /private/tmp/capstone-rust-verification/export-esp.sh
export CARGO_HOME=/private/tmp/capstone-rust-verification/cargo
export RUSTUP_HOME=/private/tmp/capstone-rust-verification/rustup

MONITOR_BAUD=921600 cargo +esp build --locked --release \
  --target xtensa-esp32-none-elf --features esp32,statistics \
  --example sniffer --example emitter --example esp_now_simplex \
  --example hw_check --example csi_callback
```

Replace `esp32` with `esp32s3` in both target and chip feature for S3. These are **build-only** commands. The repository's short `cargo esp32` / `cargo esp32s3` aliases use `cargo run` and invoke the flash runner; do not confuse them with build-only aliases. Temporary paths are session artifacts, not durable toolchain installations.

The first offline firmware attempt lacked a compiler sysroot dependency (`rustc-literal-escaper`); fetching compiler build-std dependencies resolved it without patching the crate. Subsequent firmware/callback builds worked offline. This was an environment preparation issue, not a library compile failure.

## What we can reuse and must configure deliberately

Source inspection confirms an owned raw-CSI buffer copy, chip/layout tags, invalid-first-word metadata, session/sequence/timestamp fields, a bounded delivery queue, binary postcard/COBS reporting and an upstream decoder. This overlaps substantially with the embedded contract in [software-boundary.md](software-boundary.md). Prefer adapting it over rebuilding the same firmware abstractions immediately.

Important implementation decisions:

- **Queued capture:** original ESP32 has no USB-Serial-JTAG path. Default `auto` logging can take the synchronous UART path and format/write inside the Wi-Fi callback. Use the explicit async `CsiNodeClient` delivery path with one drainer; measure export/backpressure. Turning on a feature named `async-print` is not by itself proof that a particular auto-selected UART path is asynchronous.
- **Mode selection:** explicitly select `CsiDeliveryMode::Async` if a callback was previously installed. At this revision `next_csi_packet` only lazily switches when the current delivery mode is Off; a preceding callback can otherwise leave the consumer parked. Some comments describe broader replacement behavior than the code implements.
- **Raw scientific data:** use full reporting rather than threshold/decimation policies for the first validation recordings. Distinguish capture, publication, queue, logging and host-decode rates. An emitted stream sequence is not a sender transmission counter or a direct measure of RF loss.
- **Host integration:** `wire-tests` compiles the embedded project's wire source by path. The root crate still has embedded dependencies; do not assume importing the whole crate into a native Python/Rust host package will work. Reuse its decoder, or extract/vendor the pinned wire module with its license and version compatibility, after the hardware check.
- **Chip-specific TX:** [upstream emitter documentation](https://github.com/csi-rs/esp-csi-rs/blob/6cfbd6b73472c73696683b288b833c1910097061/docs/emitter-support.md) reports that its S3 raw-injection path did not radiate despite success returns, and uses ESP-NOW broadcast for classic ESP32/S3. This is an upstream hardware observation, not reproduced here; our test must check reception, not only successful send calls.
- **No performance transfer:** compilation and synthetic tests do not establish usable unique CSI/s, signal cleanliness, fall sensitivity or generalization. Keep the paper-derived hardware/representation requirements in force.

## Hardware gate before adopting it

1. Connect and identify the actual original ESP32 board(s), USB bridge and flash size. Preserve existing firmware with a verified backup before replacing it. Old port names and binary provenance must not be assumed current.
2. Use a matched transmitter/receiver on one fixed channel. Two boards provide the most controlled test. With one board, an AP/known traffic source can establish basic reception, but is a different test.
3. Use `hw_check` with matched `HW_CH`, appropriate `HW_MODE`, `HW_PHY=ht20`, `HW_REPORT=report` and `HW_LOG=serialized`; record source/configuration and actual serial baud. Compile transmitter and receiver settings separately. The already-built stock `hw_check` defaults to sniffer/channel 6; it is not a preconfigured transmitter.
4. Decode a short raw capture using upstream `wire-decode`. Verify correct source attribution, actual CSI payload/layout and invalid-prefix handling; calculate accepted-frame rates/gap percentiles and compare device/host drop counters. Ambient sniffer traffic must not be mistaken for the controlled transmitter.
5. Record empty/stationary/walking conditions, repeat after restart, then run a sustained recording with bounded queues. Extend beyond the 32-bit clock wrap (~71.6 minutes) to verify the timestamp-widening behavior separately. A transport pass is not fall-model evidence.

**Decision:** a credible, locally compile-verified candidate for an all-Rust ESP32/S3 capture implementation. Hold the actual firmware adoption decision until real CSI capture/export passes. Broader project implementation has not started as part of this verification.
