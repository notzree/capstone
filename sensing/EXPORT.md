# CSI handoff to Python

USB star discovery, radio start/stop and file/stream/both output: [star kit guide](STAR.md).

Implemented 2026-10-23: offline Arrow IPC/Parquet export, a thin Python reader, and optional live Arrow IPC delivery over a Unix socket. Rust owns the wire and native-layout decoders. Python receives indexed complex samples and metadata; it does not parse the acquisition protocol.

For recording activities and setup provenance, use the [labelled collection workflow](COLLECTION.md): `csi collect` creates a complete trial bundle and supports the same live socket.

## Export an existing trial

From `sensing/`, choose a sealed raw session and a **new output directory outside it**:

```sh
cargo run -p csi -- export bench/2026-10-21/mode-comparison/r08-pair-50 --out /tmp/pair-arrow
cargo run -p csi -- export bench/2026-10-21/mode-comparison/r08-pair-50 --out /tmp/pair-parquet --format parquet
```

The output contains `csi.arrow` (an Arrow IPC stream) or `csi.parquet`, `export.json`, and the exporter's recomputed `clock-fit.jsonl`. `export.json` includes the original session manifest, validated geometry when present, segment checks, row/layout counts and a completed health report with pending sequence gaps finalized. Undecodable frames and framing failures remain in the raw session and are counted in the report; only valid CSI messages become table rows. A complete export can therefore contain reception loss or invalid wire frames; complete means export finished, not that every sample was usable.

Sealed segments must pass their original hash/count checks. Incomplete or damaged sessions are rejected, and failed new exports are removed. Existing output directories are refused. Raw recordings and their clock sidecars are unchanged.

## Load complex CSI

From the repository root:

```sh
python3 -m venv python/.venv
python/.venv/bin/pip install -e ./python
python/.venv/bin/python python/examples/read_trial.py /tmp/pair-arrow
```

```python
from capstone_csi import Export

trial = Export("/tmp/pair-arrow")  # also accepts a Parquet export directory
print(trial.geometry, trial.metadata["summary"])
for batch in trial.batches(receiver="a0:b7:65:dd:e9:3c"):
    if set(batch.table.column("layout_status").to_pylist()) != {"decoded"}:
        continue  # raw bytes and the reason are still available
    csi = batch.complex("htltf")
    # csi.values: complex64 masked array [capture, 64 bins], in native bin order
    # csi.subcarrier: 0..31, -32..-1; csi.pilot identifies pilot bins
    print(batch.key, csi.values.shape)
    print(batch.table.column("node_time_us"))
```

Each batch contains one directed link, receiver/transmitter boot pair, schedule and layout. Grouping may reorder neighbouring links; `record_index` retains input order. For wireless carriers it is the ordinal of the expanded original observation, rather than a raw-datagram index; all observations in a carrier share the original host arrival timestamps. `trial.batches()` streams the files rather than loading the whole recording. A consumer collecting all returned batches into a list takes responsibility for that memory. All Arrow columns remain accessible through `batch.table`.

## Live delivery

On macOS/Linux, add a new socket path to the existing hub command:

```sh
# From sensing/; configure real source paths in examples/hub.toml first.
cargo run -p csi -- hub --config examples/hub.toml --out /tmp/live-trial --arrow-socket /tmp/csi.arrow.sock --arrow-flush-ms 20
```

In Python, using the same `CsiBatch` API:

```python
from capstone_csi import live_batches

for batch in live_batches("/tmp/csi.arrow.sock"):
    print(batch.key, batch.table.num_rows)
    # batch.complex("htltf") works once layout_status is decoded.
```

The socket is local, mode `0600`, and must not already exist. A client receives future batches and a schema containing the session manifest snapshot and geometry. It does not receive earlier data. The publisher keeps its own configuration cache even when no clients are connected; captures preceding the first observed `Hello` remain raw-only. Normal hub shutdown drains publisher inputs and finishes connected streams, then removes its socket. An unclean process exit can leave a socket path; inspect it before removing/reusing it.

Recording happens before delivery. The publisher has an 8,192-record input subscription, batches at most 1,024 CSI rows, and flushes pending rows every 100 ms by default under normal processing load. `--arrow-flush-ms` accepts 5–1000 ms; the example selects 20 ms for smaller batches at higher IPC overhead. This is not an end-to-end latency guarantee. Up to 16 clients each have an eight-batch queue. Full client queues drop whole batches; a socket write stalled for 250 ms disconnects that client, and its stream may end with a truncation exception. Reconnect to receive future batches. Neither a stalled reader nor publisher failure blocks raw recording.

`arrow-status.json` reports publisher input-record drops, client-batch drops, write failures, connection counts, cache evictions and clock-capacity rejects. `hub-status.json` separately reports acquisition health and raw-subscriber drops. Delivery loss counters are cumulative publisher totals, not per-client attribution. Live `record_index` counts records reaching the publisher; if its input subscription drops records, it is not the original raw-session ordinal. Use sequences and status counters to account for loss. Live status covers delivery; the raw manifest's `complete` field remains the authority for session completion.

The live encoder caps receiver-epoch annotation caches at 256, per-link probe trackers at 4,096, and receiver-clock samples at 300,000. Annotation-cache eviction starts new local sequence observations and can temporarily make layouts unknown until the next `Hello`; raw data remains available. Live clock admission limits/identity eviction can make mappings unavailable or introduce a new timeline. Offline export has full session history and can recover rows lost only in delivery. Live and offline use the same schema, layout decoder and annotation code. Their clock versions/run identities follow the respective live/offline clock policies and are not interchangeable; delivery loss or cache eviction can also change fit availability.

## Schema and interpretation

`csi-export/1` is stored in Arrow schema metadata for both formats. The original session manifest, session id, decoder version, geometry and clock provenance are included there; the export directory also carries the completed health summary. Non-null real/imaginary lists are native signed integer values, with parallel `ltf`, `subcarrier`, `valid` and `pilot` lists. No normalization, gain compensation, phase correction, temporal interpolation or classifier runs during export.

| Column group | Meaning |
| --- | --- |
| `record_index`, `source_index`, `protocol_version` | Expanded observation position, original source index and per-frame wire version; source details are in the manifest. |
| `receiver`, `receiver_epoch`, `transmitter`, `transmitter_epoch`, `schedule`, `probe_seq`, `source_mac` | Directed link and boot/schedule identity. Probe-derived fields are null for unattributed CSI; the radio source MAC is preserved separately. |
| `frame_seq`, `capture_seq`, `frame_arrival`, `capture_arrival`, `probe_arrival` | Sequences and first/in-order/gap/late/duplicate/stale arrival classification. Duplicates remain visible. A gap annotation is provisional; completed missing counts are in offline health. |
| `node_time_us`, `rx_timestamp_us`, `radio_time_us`, `radio_run` | Raw callback time, raw wrapping radio ticks, inferred unwrapped radio time and admitted radio-run identity. Unwrapping uses the existing host clock model and is nullable. Late/duplicate captures are retained but do not mutate that model or get a guessed run. |
| `host_mono_ns`, `host_wall_ns` | Original host arrival time since session start, and Unix wall arrival time. These are not capture-time estimates. |
| `timeline`, `mapping_version`, `mapped_time_us`, `fit_p99_us`, `mapping_hops` | Causally recomputed default receiver-clock fit. Only compare mapped times within the same session and timeline. Missing fits stay null, including the initial fit warmup. Parent residual p99 is not an end-to-end uncertainty bound; it is null for the reference, not zero. |
| `rssi_dbm`, `noise_floor_dbm`, `sig_mode`, `mcs`, `cwb`, `stbc`, `channel`, `secondary_channel`, `rx_state` | Per-capture radio metadata. Remaining RX flags/rate/antenna fields are in `rx_metadata_json`. |
| `gain_agc`, `gain_fft`, `hello_json` | Nullable native gain; the configuration/firmware last observed for this receiver boot. Unknown gain is not a zero-gain measurement. |
| `layout_id`, `layout_status`, `first_word_invalid`, `native_csi` | Layout/configuration identity, decoder outcome, hardware flag and exact native bytes. |
| `real`, `imag`, `ltf`, `subcarrier`, `valid`, `pilot` | Parallel lists, null for unknown/unsupported layouts. Invalid bins remain in position and retain their raw decoded values; `complex()` masks them. |

Clock mappings are recomputed using default fit options, from fresh captures seen so far, without backfilling earlier rows. They do not apply a session's custom stored fit or its stored host-clock mappings. `mapped_time_us` is a relative receiver timeline, **not host monotonic or wall time**. Native and arrival timestamps remain available for downstream alignment/label work. The health inspector's separate clock summary can differ from per-row causal availability.

Decoder `esp32-ht20/1` supports original ESP32, no secondary channel, no LTF merge, non-HT/HT20 and enabled LLTF/HT-LTF/STBC-HT-LTF blocks whose expected length matches exactly. Other chip/bandwidth/configuration combinations remain raw-only with a reason. This is intentionally narrower than the wire protocol. Synthetic noise sessions may use an unsupported configuration and are not automatically complex-CSI sensing fixtures.

The byte ordering, LTF order, index table and invalid-word rule follow [Espressif's original-ESP32 CSI documentation](https://docs.espressif.com/projects/esp-idf/en/v5.3.1/esp32/api-guides/wifi.html#wi-fi-channel-state-information). For HT20, each LTF has 64 stored bins; LLTF keeps nonzero indices −26…26, HT-LTF keeps nonzero indices −28…28. Pilot bins are marked separately and retained as valid observations. `first_word_invalid` masks the first two complex entries of the entire native buffer. An unsuccessful RX state masks all entries. Thus a 256-byte LLTF+HT-LTF capture contains two indexed blocks, not 128 independent usable subcarriers.

Golden signed-I/Q/index/mask checks and real-recording byte checks validate the decoder implementation against this documented convention. They do not calibrate phase, amplitude, receiver gain or physical tomography accuracy. Wider layout coverage and mapped host capture time with uncertainty remain follow-up work. [Labelled collection](COLLECTION.md) now records separate host-time annotations with explicit proxy matching in Python.

## Verification

From `sensing/`, run `cargo test`, `cargo check-nostd`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`. Tests include damaged/incomplete session rejection, radio-reset/capacity separation, raw preservation, both formats, live/offline delivery agreement and a stalled client while raw recording seals. Socket tests require local socket permissions.

From the repository root, after building `csi`:

```sh
PYTHONPATH=python/src \
CSI_BIN="$PWD/sensing/target/debug/csi" \
CSI_TEST_SESSION="$PWD/sensing/bench/2026-10-21/mode-comparison/r08-pair-50" \
CSI_TEST_SOCKETS=1 \
python/.venv/bin/python -m unittest discover -s python/tests -v
```

The real-session integration checks both formats, signed byte decoding, masks, link selection and re-recorded replay equality. The CLI/Python socket check uses a silent loopback source, with no physical hardware. Live delivery tests use synthetic records; no new RF collection or fall/tomography acceptance is implied.
