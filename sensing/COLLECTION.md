# Collect labelled CSI trials

USB star discovery, radio start/stop and file/stream/both output: [star kit guide](STAR.md).

The selected next kit is [USB ESP32 star collection](PLAN.md#active-delivery-scope-usb-esp32-star-kit-2026-10-26), including discovery and node start/stop. That plan is not yet implemented; the instructions below describe the existing configured-source collector and its activity labels.

Implemented 2026-10-23: `csi collect` records a raw session, operator activity intervals, immutable setup/artifact snapshots, an Arrow export and a quality report. Optional live Arrow IPC feeds Python during collection. Firmware is unchanged; the collector records declared settings and observed metadata rather than configuring or flashing nodes.

## Prepare the setup once

Use [examples/collection-plan.json](examples/collection-plan.json), [examples/collection-hub.toml](examples/collection-hub.toml) and [examples/collection-geometry.json](examples/collection-geometry.json) as templates. Edit copies for the actual installation:

- Survey node coordinates in metres, antenna directions and known participant positions. Choose a room/revision and use the same values in the plan and geometry. Example coordinates are illustrative, not a measured survey. Moving nodes requires a new geometry revision/session.
- Confirm the serial paths, stable node MACs and source names. The plan declares exactly one node for every source; every node needs a geometry placement. Include transmitting nodes so their firmware and counters also have provenance.
- Identify the application image and build/flash manifest used on each board. Paths resolve relative to the plan file. Both files are copied and SHA-256 hashed. The template references preserved pair-50 bench artifacts; those references do not verify what is currently flashed.
- Declare `mode` (`tx`, `rx`, `independent` or `pair`), channel and requested per-transmitter rate. RX-only nodes use zero TX rate; pair plans require exactly two pair nodes. These are declarations, not commands to the radio. Observed roles/channel and achieved link rates are reported separately. A dual-role pair at 50 Hz targets 50 captures/s per direction.
- Set a unique human trial id, day id, pseudonymous participant id (or null for empty-only collection), and `split_group`. Use the same group for recordings that must stay together when making train/validation/test splits. Declare activity names and an estimated keyboard/operator boundary uncertainty in milliseconds. The example's 500 ms is an operator estimate, not a measured timing guarantee.

The raw recorder also observes periodic `Hello` messages with firmware version/commit and actual CSI configuration. Unknown gain remains null. Artifact hashes establish the files included with the trial; they are not an on-device flash readback or proof that the declared mode/rate was applied.

## Record and annotate

From `sensing/`, with the edited files:

```sh
cargo run -p csi -- collect \
  --config examples/collection-hub.toml \
  --plan examples/collection-plan.json \
  --geometry examples/collection-geometry.json \
  --out /tmp/ordinary-trial-01 \
  --arrow-socket /tmp/csi.sock --arrow-flush-ms 20
```

Wait for the command prompt. In the collector terminal, type commands as activities actually begin/end:

```text
begin empty
end
begin walking
note walking across the surveyed area
end
begin standing example-centre
end
begin sitting
end
stop
```

Leave a useful interval between `begin` and `end`; this block is a command reference, not a script to paste all at once. Use your surveyed position id in place of `example-centre`. `begin` switches from an open activity at the same boundary. `end` explicitly closes it. Unknown activities/positions and malformed commands are rejected visibly. A note does not label the surrounding data. Unlabelled periods remain unlabelled.

`stop`, stdin EOF, Ctrl-C, SIGTERM and an optional `--seconds N` upper bound all stop collection and drain/seal the recorder. An open activity is closed as **truncated**, because its physical end was not explicitly marked. Python excludes truncated intervals from activity selection by default. Commands queued after `stop` are ignored. An incomplete bundle remains marked incomplete and is refused by the training reader; its raw/annotation files are retained for inspection rather than replaced or assigned guessed labels.

For the first teammate walkthrough, record an empty-room baseline followed by walking, standing and sitting, with explicit ends. Repeat with the same geometry and a new trial id, then reopen both trials and inspect quality. This is ordinary-activity collection; staged falls require a separately agreed protocol.

## Process live in Python

Install the reader as described in [EXPORT.md](EXPORT.md), then run a separate Python program:

```python
from capstone_csi import live_batches

for batch in live_batches("/tmp/csi.sock"):
    if set(batch.table.column("layout_status").to_pylist()) == {"decoded"}:
        samples = batch.complex("htltf")
        # samples.values: masked complex CSI [captures, subcarriers]
        # Feed your processing/model here.
```

The agreed format is the versioned `csi-export/1` Arrow IPC stream over UDS. The same API works with `csi hub --arrow-socket PATH --arrow-flush-ms 20`. The default flush interval is 100 ms; 5–1000 ms is configurable. Smaller intervals trade smaller batches for more IPC/processing overhead. Flushing is a target under processing load, not an end-to-end latency guarantee: acquisition, UART, scheduling and Python processing add latency.

Raw recording precedes delivery; bounded queues isolate stalled consumers. See `raw/arrow-status.json` for input/batch drops, write failures and the selected interval, and `raw/hub-status.json` for acquisition health. Connect before the activities you want to process live; clients receive future data only. The socket carries CSI batches, not the operator-label event log. Labels are currently persisted separately in `labels.jsonl`. No new live label/control protocol is defined here.

## Bundle layout and provenance

```text
ordinary-trial-01/
├── trial.json             csi-trial/1; plan, bound session, hashes, stop reason, complete
├── provenance/            original + normalized plan, source config, geometry,
│                          node-N-image.bin and node-N-build.json snapshots
├── labels.jsonl           csi-labels/1; session-bound header, begin/end/note/stop events
├── split.json             csi-split/1; session, split group, unassigned by default
├── quality.json           csi-trial-quality/1; metrics, missing links, identity/config checks
├── raw/                   normal immutable session, copied geometry/config, health/clocks
└── export/                csi-export/1 Arrow stream, export.json and recomputed clock fit
```

Raw data, labels, split assignments and derived outputs remain separate. Image/build numbering corresponds to the zero-based order of nodes in the snapshotted plan. Host collection uses the snapshotted source config and geometry. `trial.json` becomes complete only after labels close, raw recording seals, export succeeds and quality is written. The Python reader checks artifact/label hashes, session/schema/timebase bindings and geometry/plan consistency. Correcting labels should create a versioned derived annotation set; do not edit originals in place or assign activities retroactively to old unlabelled desk recordings.

`split.json` deliberately starts `unassigned`. After choosing a dataset split, set `assignment` to `train`, `validation` or `test` while preserving session/group fields. Keep a whole split group together; this field alone does not enforce dataset-wide separation across participants, days or rooms. Preserve the assignment used for each training run.

## Read labels and quality

```python
from capstone_csi import Trial

trial = Trial("/tmp/ordinary-trial-01")
print(trial.intervals, trial.split, trial.geometry)
print(trial.quality["missing_links"])
print(trial.quality["source_identity_checks"])
for quality in trial.link_quality():
    print(quality)
for batch in trial.batches():
    pass  # same batches as Export, with no automatic label-to-clock join
```

Labels use the **exact host monotonic source-arrival clock** shared by the hub and collector. The input thread stamps a command after reading its completed line, before queueing it. This is an operator annotation timestamp, not a measurement of the physical action's onset. Wall timestamps are retained but are not used to order intervals. Intervals are half-open `[begin, end)`.

Capture-to-host time alignment is still pending. To match labels against host packet arrivals as an explicitly approximate baseline:

```python
for batch in trial.activity_batches("walking", time_basis="host_arrival_proxy"):
    pass
```

Selection trims the declared uncertainty from both ends and excludes truncated intervals by default. Intervals shorter than twice the uncertainty select no captures. This does **not** compensate or bound UART/export latency. Never compare host label nanoseconds directly to node microseconds or a relative receiver timeline. For precise event training, validate capture-to-host alignment and operator/video timing before treating these selections as ground truth. `activity_mask` offers the same explicit matching for an existing batch; another session's data is rejected.

Quality reports include:

- Per directed link/boot/schedule: records, decoder coverage, valid/stored bin counts, gain availability and mapped-clock coverage. `Trial.link_quality()` adds fractions and an explicitly named host-arrival rate. The inspector's health report separately contains capture-time rates, gap distributions, finalized sequence losses and radio resets.
- Expected same-channel directed links implied by declared roles, missing links, expected versus observed source MACs, and observed role/channel checks. Matching roles cannot distinguish independent from packet-driven operation; use image/build provenance for that declaration. No observed `Hello` means configuration is unknown.
- Counter windows: first/last observed snapshots, their times and deltas for TX, callbacks, captures, pool/export drops and bytes. These deltas exclude pre-trial totals but cover only the observed counter interval, not the uncovered beginning/end of the trial. A single snapshot has no delta; counter rollback invalidates it. Health's latest node counters remain cumulative since boot.

There is no universal automatic pass/fail quality score. Inspect missing/misidentified links, sequence loss, validity, gain and timing coverage before selecting a trial for a task. Unknown gain and the original ESP32's uncalibrated amplitude/phase remain limitations; recorded positions do not establish tomography accuracy.

## Verification

Rust tests exercise interval transitions/rejections, truncated shutdown, deadline shutdown with stdin held open, setup validation, non-overwrite behavior, and simulated binary CSI over loopback UDP through recording/export/quality. Counter tests cover pre-trial totals, late snapshots and rollback. Python tests exercise collection with 20 ms live UDS, session/provenance rejection, half-open uncertainty trimming, truncated exclusions and commands after stop.

Run the workspace checks from `sensing/`; local socket tests require socket permissions. Run the Python integration command in [EXPORT.md](EXPORT.md), which also discovers the trial tests.

**Physical follow-up, 2026-10-23:** [five two-ESP32 trials](bench/2026-10-23/live-python-051450/README.md) passed with 7,127 real CSI captures delivered to Python and matched against offline exports. With 20 ms flushing, post-warmup hub-frame-arrival-to-Python delay was about 17 ms median / 29 ms p95; 100 ms flushing gave about 58–59 ms / 106 ms. A stalled secondary reader disconnected without losing captures for the healthy reader or recorder. These were transport tests with unsurveyed geometry/software phase markers, not physical activity labels or capture-to-host calibration. No flashing was needed.
