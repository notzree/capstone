"""Usage: python examples/read_collection.py /path/to/trial-bundle."""
import argparse
from capstone_csi import Trial

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("trial")
parser.add_argument("--activity")
parser.add_argument("--arrival-proxy", action="store_true", help="Explicitly match labels to host arrivals, not capture times")
args = parser.parse_args()
trial = Trial(args.trial)
print("trial:", trial.plan["trial_id"], "participant:", trial.plan["participant_id"], "split:", trial.split)
print("intervals:", *trial.intervals, sep="\n")
print("missing links:", trial.quality["missing_links"])
print("source identity:", trial.quality["source_identity_checks"])
for q in trial.link_quality():
    print("link quality:", q)
if args.activity:
    if not args.arrival_proxy:
        parser.error("Activity matching requires --arrival-proxy; capture-to-host alignment is not implemented")
    for batch in trial.activity_batches(args.activity, time_basis="host_arrival_proxy"):
        print("selected:", batch.key, batch.table.num_rows)
