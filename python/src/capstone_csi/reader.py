from __future__ import annotations

from dataclasses import dataclass
import json
from pathlib import Path
import socket
from typing import BinaryIO, Iterator

import numpy as np
import pyarrow as pa
import pyarrow.parquet as pq

SCHEMA = b"csi-export/1"
GROUP = ("receiver", "receiver_epoch", "transmitter", "transmitter_epoch", "schedule", "layout_id")


@dataclass(frozen=True)
class CsiSamples:
    """Unscaled complex CSI, with null/hardware-invalid bins masked in place."""
    values: np.ma.MaskedArray  # [capture, subcarrier]; no temporal interpolation
    subcarrier: np.ndarray
    pilot: np.ndarray


@dataclass(frozen=True)
class CsiBatch:
    """One directed link, boot pair, schedule and layout. All columns stay accessible."""
    table: pa.RecordBatch

    @property
    def key(self) -> dict:
        return {name: self.table.column(name)[0].as_py() for name in GROUP}

    def complex(self, ltf: str = "htltf") -> CsiSamples:
        """Select one LTF without merging repeated frequency bins across LTFs.

        Raises on unsupported/unknown layouts. Raw CSI is still in native_csi.
        This performs no gain compensation, phase correction or normalization.
        """
        statuses = set(self.table.column("layout_status").to_pylist())
        if statuses != {"decoded"}:
            raise ValueError(f"Complex CSI unavailable: {sorted(statuses)}; inspect native_csi/hello_json")
        fields = {name: self.table.column(name).to_pylist()
                  for name in ("real", "imag", "valid", "ltf", "subcarrier", "pilot")}
        selection = np.asarray(fields["ltf"][0]) == ltf
        if not selection.any():
            raise ValueError(f"LTF {ltf!r} absent; available: {sorted(set(fields['ltf'][0]))}")
        if any(row != fields["ltf"][0] for row in fields["ltf"]):
            raise ValueError("Mixed LTF layouts in batch")
        if any(row != fields["subcarrier"][0] for row in fields["subcarrier"]):
            raise ValueError("Mixed subcarrier layouts in batch")
        real = np.asarray(fields["real"], dtype=np.float32)[:, selection]
        imag = np.asarray(fields["imag"], dtype=np.float32)[:, selection]
        valid = np.asarray(fields["valid"], dtype=bool)[:, selection]
        return CsiSamples(
            np.ma.array(real + np.complex64(1j) * imag, mask=~valid),
            np.asarray(fields["subcarrier"][0], dtype=np.int16)[selection],
            np.asarray(fields["pilot"][0], dtype=bool)[selection],
        )


def _check(schema: pa.Schema) -> None:
    if (schema.metadata or {}).get(b"csi.schema") != SCHEMA:
        raise ValueError("Unsupported or missing CSI export schema")


def _group(batch: pa.RecordBatch, receiver: str | None, transmitter: str | None) -> Iterator[CsiBatch]:
    columns = [batch.column(name).to_pylist() for name in GROUP]
    groups: dict[tuple, list[int]] = {}
    for i, key in enumerate(zip(*columns)):
        if receiver is not None and key[0] != receiver:
            continue
        if transmitter is not None and key[2] != transmitter:
            continue
        groups.setdefault(key, []).append(i)
    for indices in groups.values():
        selected = batch.take(pa.array(indices, type=pa.int32()))
        yield CsiBatch(selected.replace_schema_metadata(batch.schema.metadata))


def iter_batches(source: str | Path | BinaryIO, *, receiver: str | None = None,
                 transmitter: str | None = None) -> Iterator[CsiBatch]:
    """Read Arrow IPC streams or Parquet with one batch API.

    File-like sources are Arrow streams (also usable with socket.makefile('rb')).
    Does not own or close caller-supplied streams. Link filters use canonical MACs.
    record_index retains acquisition order; grouping can reorder adjacent links.
    """
    if isinstance(source, (str, Path)):
        path = Path(source)
        if path.is_dir():
            yield from Export(path).batches(receiver=receiver, transmitter=transmitter)
            return
        if path.suffix == ".parquet":
            reader = pq.ParquetFile(path)
            _check(reader.schema_arrow)
            for batch in reader.iter_batches(batch_size=1024):
                yield from _group(batch.replace_schema_metadata(reader.schema_arrow.metadata), receiver, transmitter)
            return
        with path.open("rb") as stream:
            yield from iter_batches(stream, receiver=receiver, transmitter=transmitter)
        return
    reader = pa.ipc.open_stream(source)
    _check(reader.schema)
    for batch in reader:
        yield from _group(batch, receiver, transmitter)


def live_batches(path: str | Path, *, receiver: str | None = None,
                 transmitter: str | None = None) -> Iterator[CsiBatch]:
    """Read csi-export/1 from a hub/collector/star UDS until explicit Arrow EOF.

    Star primary schema readiness is acknowledged before grants. Query the star
    controller's final status for radio STOP and loss; EOF alone is insufficient.
    File modes also retain status sidecars. A prolonged stall disconnects a reader.
    """
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
        connection.connect(str(path))
        with connection.makefile("rb") as stream:
            reader = pa.ipc.open_stream(stream)
            _check(reader.schema)
            if (reader.schema.metadata or {}).get(b"csi.primary_handshake") == b"CSI-READY/1":
                connection.sendall(b"CSI-READY/1\n")
            for batch in reader:
                yield from _group(batch, receiver, transmitter)


class Export:
    """Completed offline export, with original manifest, geometry and health report."""
    def __init__(self, path: str | Path):
        self.path = Path(path)
        self.metadata = json.loads((self.path / "export.json").read_text())
        if self.metadata.get("schema") != SCHEMA.decode() or self.metadata.get("complete") is not True:
            raise ValueError("Incomplete or unsupported CSI export")
        self.manifest = self.metadata["manifest"]
        self.geometry = self.metadata["geometry"]
        self.health = self.metadata["health"]
        name = self.metadata["data_file"]
        if name not in ("csi.arrow", "csi.parquet"):
            raise ValueError("Unexpected CSI data file")
        self.data = self.path / name

    def batches(self, *, receiver: str | None = None,
                transmitter: str | None = None) -> Iterator[CsiBatch]:
        total = 0
        for batch in iter_batches(self.data, receiver=receiver, transmitter=transmitter):
            session = batch.table.schema.metadata[b"csi.session_id"].decode()
            if session != self.manifest["session_id"]:
                raise ValueError("Data belongs to a different session")
            total += batch.table.num_rows
            yield batch
        if receiver is None and transmitter is None and total != self.metadata["summary"]["csi_rows"]:
            raise ValueError("CSI row count differs from export summary")
