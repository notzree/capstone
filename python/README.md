# Python CSI reader

USB star discovery, radio start/stop and file/stream/both output: [star kit guide](../sensing/STAR.md).

Install with `python -m pip install -e ./python` from the repository root, preferably inside a virtual environment. See [the handoff guide](../sensing/EXPORT.md) for recording/export commands, live delivery, schema, timing and validity rules, and tests.

```python
from capstone_csi import Export, live_batches

trial = Export("/tmp/pair-arrow")
for batch in trial.batches(receiver="a0:b7:65:dd:e9:3c"):
    if set(batch.table.column("layout_status").to_pylist()) == {"decoded"}:
        samples = batch.complex("htltf")
        print(samples.values.shape, samples.subcarrier)
```

`samples.values` is a masked complex array. Invalid bins retain their indices. Values are unscaled; unknown gain and missing mapped time stay explicit. Training, DSP and tomography remain separate workstreams.

For activity-labelled recordings, see [the collection guide](../sensing/COLLECTION.md). `Trial("/path/to/bundle")` wraps the export with checked provenance, intervals, split grouping and quality. Activity selection requires explicit `time_basis="host_arrival_proxy"`; no capture-time alignment is silently assumed. `live_batches("/tmp/csi.sock")` reads the same Arrow batches during `csi collect` or `csi hub`; `--arrow-flush-ms 20` selects a smaller flush interval.
