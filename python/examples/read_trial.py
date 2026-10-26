"""Usage: python examples/read_trial.py /path/to/export [--rx MAC] [--tx MAC]."""
import argparse
import numpy as np
from capstone_csi import Export

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("export")
parser.add_argument("--rx")
parser.add_argument("--tx")
args = parser.parse_args()
trial = Export(args.export)
print("session:", trial.manifest["session_id"])
print("geometry:", trial.geometry)
print("quality:", trial.metadata["summary"])
for batch in trial.batches(receiver=args.rx, transmitter=args.tx):
    print("link:", batch.key, "captures:", batch.table.num_rows)
    if set(batch.table.column("layout_status").to_pylist()) != {"decoded"}:
        print("raw only:", set(batch.table.column("layout_status").to_pylist()))
        continue
    ltf = "htltf" if "htltf" in batch.table.column("ltf")[0].as_py() else "lltf"
    samples = batch.complex(ltf)
    print(ltf, "shape:", samples.values.shape, "valid fraction:", samples.values.count() / samples.values.size,
          "mean unscaled magnitude:", float(np.ma.abs(samples.values).mean()))
    # Timing stays explicit. mapped_time_us is a relative receiver timeline,
    # may be null, and cannot be subtracted from host_mono_ns or wall time.
    print("mapped captures:", len(batch.table.column("mapped_time_us")) - batch.table.column("mapped_time_us").null_count)
