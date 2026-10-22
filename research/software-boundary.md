# Rust acquisition, Python science: proposed implementation boundary

2026-10-19. Design proposal only; the SDK examples below are not implemented. This complements the [paper-to-design mapping](evidence-to-design.md) and [final-system requirements](final-system.md).

Architecture update: [rust-workspace.md](rust-workspace.md) defines one Cargo workspace with shared `no_std` contracts/node logic, thin embedded TX/RX/TX+RX entrypoints, and `std` Linux RX/collector/SDK packages. TX is embedded, RX may be embedded or Linux, and all DSP/ML runs on Linux. The same contracts support the TX star and distributed TX+RX experiments.

Verification: [esp-csi-rs-verification.md](esp-csi-rs-verification.md) records successful host tests and firmware builds for a no_std Rust CSI library on original ESP32 and S3. It is a concrete alternative to the C/ESP-IDF firmware route below and already implements several proposed embedded data-contract pieces. Real capture/export verification is still pending; no firmware implementation choice has been finalized.

## Recommendation and team ownership

Build a hardware-independent Rust acquisition library with a recorder/replayer CLI and a thin Python SDK. Keep exploratory DSP, feature extraction, calibration and modeling in Python/NumPy/SciPy/PyTorch. Your Rust layer delivers correctly described, timestamped measurements and health evidence; teammates turn those into meaningful physical/activity features.

Use existing radio firmware/drivers/extractors. Rust owns embedded role scheduling, capture integration and export as well as host acquisition/transport; it does not replace vendor PHY implementations. Hardware-specific adapters isolate upstream dependencies from the common measurement and batch contracts.

| Layer | Language/toolchain | Owner boundary |
| --- | --- | --- |
| ESP32 capture/transmit firmware | Rust `no_std` backend using the pinned compile-verified `esp-csi-rs` candidate, conditional on real capture/combined-role verification. Official C/ESP-IDF reference firmware remains a fallback exporting the same contract. | You: packet scheduling, source sequences, capture callbacks, compact export, queue/drop counters and device health. |
| Intel/NXP/Linux radio extraction | Upstream firmware/driver and capture utility, with our Rust process adapter/parser. | You: provision/configure supported capture stack and consume its documented records. Driver rewrites are not a prerequisite. |
| Host acquisition/core | Rust | You: decoding, timestamps, bounded buffers, recording/replay, connection recovery, configuration, quality counters and Python boundary. |
| DSP and modeling | Python calling numerical libraries | Teammates: filtering, resampling policy, phase cleaning, PCA, FFT/STFT, autocorrelation, feature selection, calibration and learned models. |

Two Rust routes are distinct: the inspected CSI library uses `no_std` with `esp-hal`/`esp-radio`; the [ESP-IDF Rust target](https://doc.rust-lang.org/rustc/platform-support/esp-idf.html) uses a different SDK/runtime boundary. Do not assume wrapper support or wire compatibility transfers between them. Our proposed Rust firmware backend must reproduce native CSI fields, source attribution and sustained export on actual boards before adoption.

The [ESP-IDF CSI documentation](https://docs.espressif.com/projects/esp-idf/en/v5.5.1/esp32/api-guides/wifi.html#wi-fi-channel-state-information-configure) says callbacks execute in the Wi-Fi task and should queue work for another task. Copy necessary callback-owned bytes/metadata into bounded owned storage before returning; do not retain pointers into the driver's transient buffer. Avoid blocking serial writes, FFTs and Python involvement in this callback. Verify binding signatures and lifetime assumptions when crossing the C API.

## Buildable without new hardware, in order

1. **Measurement schema and parser fixtures.** Define source/link IDs, session epoch, clock domain/tick rate, sender sequence when exposed, host receive time, radio/LTF layout, chain map, subcarrier frequencies, units/scaling and validity. Preserve raw bytes. Implement the existing saved ESP32 text/JSONL adapter first; use captured upstream fixtures for other formats. Mark unverified fields unavailable.
2. **Deterministic recorder and replay source.** Real-time paced, accelerated and step replay through the same output API as live capture. Reuse the local empty/seated recordings. Use public fall data for model development if access succeeds, but do not pretend its event tensors are native packet captures with invented timestamps.
3. **Stream correctness and bounded buffers.** Frame partial input, cap lengths before allocating, reject incompatible layouts, detect clock wrap/restart and represent gaps. Track sender-sequence semantics explicitly; a callback counter cannot automatically prove RF packet loss. Assign a new session epoch after restart.
4. **Separate recording and live-consumer behavior.** Preserve capture independently of slow science code where resources permit. A live subscriber may discard stale batches using an explicit counted policy; archival overflow marks a recording incomplete. All queues/storage remain bounded. Disk full, bad framing and disconnects produce health errors, not fabricated no-motion data.
5. **Python batch API.** Package the same Rust core through PyO3/rust-numpy and maturin. Make replay work before adding vendor live adapters. Ship wheels for the team's actual OS/architecture/Python versions; the Mac can replay/analyze Linux recordings.
6. **Performance and fault harness.** Feed recorded packets at planned rates/link counts, inject gaps/duplicates/reordering/clock resets, stall the consumer and stop storage. Check bounded memory, reported loss, ordered timestamps within epochs and deterministic replay. Measure ingest → available-to-Python p95 latency separately from model/window latency.

Synthetic traces are suitable for transport and numeric correctness checks, not evidence that a fall model works. SDK shape/ownership tests can run now; radio compatibility, actual phase quality and RF coverage still require hardware.

## Proposed Python contract

Embedded export supports configurable accumulation up to an initial 500 ms, with earlier byte/sample-cap flushes; see [CSI export batching](rust-workspace.md#csi-export-batching-configurable-initially-500-ms). This is independent of Python's requested batch size/wait timeout: the SDK returns data when available and does not add a mandatory second batching delay. Preserve individual capture timestamps and native samples across both boundaries. Benchmark delivery overhead and actual gaps before claiming accuracy improvement.

Expose homogeneous per-link/per-layout batches. Do not force different radios/LTF layouts into identical semantics by reshaping them. A layout change ends the batch and emits a configuration event.

```python
# Proposed API; not currently implemented.
from capstone_csi import Replay

with Replay.open("demo/data/empty-01", pace="realtime") as source:
    while True:
        batch = source.next_batch(max_frames=256, timeout_ms=100)
        if batch is None:
            continue  # timeout; end-of-recording raises StopIteration

        iq = batch.iq                   # complex64 [T, RX, TX, K]
        timestamp_ns = batch.device_time_ns  # uint64 [T], session-relative
        valid = batch.valid             # bool [T, RX, TX, K]
        metadata = batch.metadata       # layout, scale, clock/session, source
        quality = batch.quality         # gaps, queue drops, parse errors, etc.

        # Teammates call their vectorized DSP pipeline with these arrays.
```

`RX/TX` here identify measured receive chains and transmitted spatial streams as defined by that adapter, not independent boxes; `K` indexes a documented subcarrier map. A singleton TX dimension is normal. ESP32's concatenated LTF buffers need explicit interpretation or a selected documented LTF; a byte count alone does not define K. A `complex64` view preserves a convenient numeric representation, not hardware calibration or equal physical units across radios. Keep raw native values/scale provenance available.

Each batch carries a link ID, session/clock epoch, first/last sender sequence if available, original ticks, host monotonic receive timestamps and optional wall-clock mapping. Converting clocks to nanoseconds does not imply nanosecond precision or synchronization. Keep independent receivers separate until alignment is measured. Preserve missing/invalid information; teammates choose resampling/interpolation and feature masks explicitly.

Do not bake the papers' five-second input window into the acquisition API. Deliver short batches with bounded waiting; Python constructs trailing windows for each method. Exposing both replay and live sources behind this contract lets everyone work before radio arrival.

## Binding, isolation and ownership

[PyO3](https://pyo3.rs/main/parallelism) provides Rust/Python integration, [rust-numpy](https://github.com/PyO3/rust-numpy) provides array conversion, and [maturin](https://www.maturin.rs/) builds Python packages/wheels. Python imports our Rust implementation; we do not need to embed a Python interpreter in a Rust application.

Batch across the boundary instead of invoking a Python callback for every packet. Rust I/O workers do not access Python objects. Release interpreter attachment while waiting for batches or doing independent Rust work, then construct/return Python arrays after reattaching. Avoid holding a core mutex while acquiring Python access.

Start with a clearly owned copy per batch. If later optimized to ownership transfer or zero-copy views, the Python object must keep the buffer alive and the producer must not reuse/mutate it while readers hold it. A read-only Python view alone does not stop a Rust producer from racing with it. Never return a borrowed view into a ring slot about to be overwritten.

The library and Python extension can initially run in the same process. That process's crash also ends its workers. For final unattended operation, run the recorder as a separate Rust service and consume a bounded batch stream over local IPC, with the Python SDK preserving the same array contract. This is process isolation, separate from FFI. Do not promise crash-independent recording merely because the I/O thread is written in Rust. Remote Linux capture also needs a process/network boundary; FFI cannot span machines. Keep bulk arrays off REST/JSON; reserve a small HTTP API for control/health if useful.

## Performance expectations to test

Do not infer real-time performance from language alone. Many [NumPy operations run in low-level code and release the GIL](https://numpy.org/doc/stable/reference/thread_safety.html); Python element-by-element loops, repeated allocations, serialization, overlap reprocessing and large models are different workloads.

Illustrative payload calculation, not a benchmark: 2 RX chains ×1 TX stream ×58 subcarriers ×8 bytes/complex64 ×1,000 packets/s is **0.928 MB/s per receiver**. Three such receivers are 2.784 MB/s; at 3,000 packets/s, 8.352 MB/s. Raw transport formats, metadata, copies, disk and model compute change the actual budget. A five-second window at 1,000 packets/s is 4.64 MB per receiver in this representation. Returning short batches avoids copying the entire overlapping window on every SDK call.

Measure parser/recording throughput, queue residence, array handoff cost, DSP duration and model inference separately. If the model advances every 0.5 s, processing must sustain that cadence with headroom; five seconds of context is a separate observation delay. Optimize proven hot spots in Rust after preserving a Python reference implementation and numerical tolerances.

The most useful first deliverable is **Rust replay/recording + a stable batched Python contract**, not a new Wi-Fi driver or a Rust port of every DSP operation.
