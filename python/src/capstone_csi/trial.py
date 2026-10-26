"""Session-bound operator labels. Arrival-time matching is an explicit proxy."""
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path

import numpy as np
import pyarrow as pa

from .reader import CsiBatch, Export


@dataclass(frozen=True)
class Interval:
    activity: str
    start_ns: int
    end_ns: int
    uncertainty_ns: int
    position: dict | None
    truncated: bool
    end_reason: str


def _hash(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


class Trial:
    """A completed collect bundle with provenance, labels, split group and quality."""
    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.metadata = json.loads((self.path / "trial.json").read_text())
        m = self.metadata
        if m.get("schema") not in ("csi-trial/1", "csi-trial/2") or m.get("complete") is not True:
            raise ValueError("Incomplete or unsupported trial; retain it for inspection, not automatic training")
        if (m.get("raw_session"), m.get("export_dir"), m.get("labels_file")) != ("raw", "export", "labels.jsonl"):
            raise ValueError("Unexpected trial bundle layout")
        self.export = Export(self.path / "export")
        self.session_id = self.export.manifest["session_id"]
        if m["session_id"] != self.session_id or m.get("label_timebase") != "host_mono_ns":
            raise ValueError("Trial session/timebase does not match export")
        for artifact in m["artifacts"]:
            file = (self.path / artifact["file"]).resolve()
            if not file.is_relative_to(self.path.resolve()) or _hash(file) != artifact["sha256"]:
                raise ValueError(f"Provenance hash/path mismatch: {artifact['file']}")
        if _hash(self.path / "labels.jsonl") != m["labels_sha256"]:
            raise ValueError("Label hash mismatch; preserve originals and version corrections separately")
        self.plan = m["plan"]
        if m.get("schema") == "csi-trial/2" and self.plan is None:
            self.geometry = self.export.geometry
            self.quality = json.loads((self.path / "quality.json").read_text())
            if self.quality.get("session_id") != self.session_id or not self.quality.get("complete"):
                raise ValueError("Quality report belongs to another session or is incomplete")
            self.split = None
            self.intervals, self.notes = [], []
            return
        if json.loads((self.path / "provenance/plan-normalized.json").read_text()) != self.plan:
            raise ValueError("Trial plan differs from its provenance snapshot")
        self.geometry = self.export.geometry
        if (not self.geometry or self.geometry["room"] != self.plan["room"]
                or self.geometry["revision"] != self.plan["geometry_revision"]
                or m.get("geometry") != self.export.manifest.get("geometry")):
            raise ValueError("Trial geometry does not match plan/export")
        self.quality = json.loads((self.path / "quality.json").read_text())
        if (self.quality.get("schema") != "csi-trial-quality/1" or self.quality.get("session_id") != self.session_id
                or self.quality.get("complete") is not True):
            raise ValueError("Quality report belongs to another session/schema")
        self.split = json.loads((self.path / "split.json").read_text())
        if (self.split.get("schema") != "csi-split/1" or self.split.get("session_id") != self.session_id
                or self.split.get("group") != self.plan["split_group"]
                or self.split.get("assignment") not in ("unassigned", "train", "validation", "test")):
            raise ValueError("Invalid split assignment/session/group")
        self.intervals, self.notes = self._labels()

    def _labels(self):
        events = [json.loads(line) for line in (self.path / "labels.jsonl").read_text().splitlines()]
        if not events:
            raise ValueError("Missing label header")
        header = events[0]
        if (header.get("schema"), header.get("type"), header.get("session_id"), header.get("timebase")) != (
                "csi-labels/1", "header", self.session_id, "host_mono_ns"):
            raise ValueError("Labels belong to a different session/timebase/schema")
        uncertainty = header.get("uncertainty_ns")
        if type(uncertainty) is not int or uncertainty != self.plan["label_uncertainty_ms"] * 1_000_000:
            raise ValueError("Invalid label uncertainty")
        intervals, notes = [], []
        active = None
        last = 0
        stopped = False
        for event in events[1:]:
            t = event.get("mono_ns")
            if stopped or type(t) is not int or t < last:
                raise ValueError("Labels are unordered or continue after stop")
            last = t
            kind = event.get("type")
            if kind == "begin":
                if active is not None or event.get("activity") not in self.plan["activities"]:
                    raise ValueError("Overlapping or undeclared activity")
                position = event.get("position")
                if position is not None and position not in self.plan["positions"]:
                    raise ValueError("Undeclared position")
                active = event
            elif kind == "end":
                if active is None or event.get("id") != active.get("id") or type(event.get("truncated")) is not bool:
                    raise ValueError("End has no matching activity/closure status")
                intervals.append(Interval(active["activity"], active["mono_ns"], t, uncertainty,
                                          active.get("position"), event["truncated"], event["reason"]))
                active = None
            elif kind == "note":
                notes.append(event)
            elif kind == "stop":
                if active is not None or t != self.metadata["stopped_mono_ns"]:
                    raise ValueError("Unclosed activity or inconsistent trial stop")
                stopped = True
            else:
                raise ValueError("Unknown label event")
        if not stopped or active is not None:
            raise ValueError("Labels have no completed stop")
        return intervals, notes

    def batches(self, **filters):
        """Read all CSI; no labels are silently attached to capture clocks."""
        yield from self.export.batches(**filters)

    def activity_mask(self, batch: CsiBatch, activity: str, *, time_basis: str,
                      trim_uncertainty: bool = True, include_truncated: bool = False):
        """Match [start,end) using host arrivals only with explicit proxy opt-in.

        Operator uncertainty is trimmed from both boundaries by default. This
        does not bound UART/export delay or turn arrival time into capture time.
        Automatically closed/truncated intervals are excluded by default.
        """
        if time_basis != "host_arrival_proxy":
            raise ValueError("Use time_basis='host_arrival_proxy' explicitly; host capture-time alignment is not implemented")
        if self.plan is None:
            raise ValueError("Transport trial has no activity ground truth; record a surveyed labelled trial")
        if activity not in self.plan["activities"]:
            raise ValueError("Undeclared activity")
        if (batch.table.schema.metadata or {}).get(b"csi.session_id", b"").decode() != self.session_id:
            raise ValueError("Cannot label CSI from another session")
        times = batch.table.column("host_mono_ns").to_numpy(zero_copy_only=False)
        mask = np.zeros(len(times), dtype=bool)
        for interval in self.intervals:
            if interval.activity != activity or (interval.truncated and not include_truncated):
                continue
            trim = interval.uncertainty_ns if trim_uncertainty else 0
            start, end = interval.start_ns + trim, interval.end_ns - trim
            if end > start:
                mask |= (times >= start) & (times < end)
        return mask

    def activity_batches(self, activity: str, *, time_basis: str,
                         trim_uncertainty: bool = True, include_truncated: bool = False, **filters):
        for batch in self.batches(**filters):
            mask = self.activity_mask(batch, activity, time_basis=time_basis,
                                      trim_uncertainty=trim_uncertainty, include_truncated=include_truncated)
            if mask.any():
                selected = batch.table.filter(pa.array(mask)).replace_schema_metadata(batch.table.schema.metadata)
                yield CsiBatch(selected)

    def link_quality(self):
        """Coverage fractions plus explicit host-arrival rate; inspect health for capture gaps/rates."""
        for row in self.quality["summary"]["links"].values():
            q = dict(row)
            n = q["records"]
            for name, numerator in (("decoded_fraction", q["decoded_records"]),
                                    ("gain_fraction", q["gain_records"]),
                                    ("mapped_fraction", q["mapped_records"])):
                q[name] = numerator / n if n else None
            q["valid_fraction_of_decoded_bins"] = q["valid_samples"] / q["stored_samples"] if q["stored_samples"] else None
            span = q["last_host_mono_ns"] - q["first_host_mono_ns"]
            q["host_arrival_rate_hz"] = (n - 1) * 1e9 / span if n > 1 and span > 0 else None
            yield q
