"""CSI_BIN=/.../csi CSI_TEST_SESSION=/.../raw python -m unittest discover -s tests -v"""
import io
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import numpy as np
import pyarrow as pa

from capstone_csi import Export, iter_batches


class ReaderTests(unittest.TestCase):
    def test_refuses_unversioned_stream(self):
        stream = io.BytesIO()
        with pa.ipc.new_stream(stream, pa.schema([("x", pa.int8())])):
            pass
        stream.seek(0)
        with self.assertRaisesRegex(ValueError, "schema"):
            list(iter_batches(stream))

    @unittest.skipUnless(os.getenv("CSI_TEST_SESSION") and os.getenv("CSI_BIN"),
                         "set CSI_TEST_SESSION and CSI_BIN for Rust/Python integration")
    def test_real_session_arrow_parquet_complex_and_metadata_agree(self):
        with tempfile.TemporaryDirectory() as tmp:
            rows = []
            for fmt in ("arrow", "parquet"):
                out = Path(tmp) / fmt
                subprocess.run([os.environ["CSI_BIN"], "export", os.environ["CSI_TEST_SESSION"],
                                "--out", str(out), "--format", fmt], check=True, capture_output=True)
                export = Export(out)
                collected = []
                decoded = 0
                for batch in export.batches():
                    collected.extend(batch.table.to_pylist())
                    if set(batch.table.column("layout_status").to_pylist()) != {"decoded"}:
                        with self.assertRaisesRegex(ValueError, "unavailable"):
                            batch.complex()
                        continue
                    decoded += batch.table.num_rows
                    s = batch.complex("htltf")
                    self.assertEqual(s.values.shape, (batch.table.num_rows, 64))
                    np.testing.assert_array_equal(s.subcarrier, np.r_[0:32, -32:0])
                    self.assertTrue(np.all(s.values.mask[:, 0]))
                    for row in batch.table.to_pylist():
                        # Independent byte check catches swapped I/Q and unsigned conversion.
                        native = np.frombuffer(row["native_csi"], dtype=np.int8)
                        np.testing.assert_array_equal(row["imag"], native[0::2])
                        np.testing.assert_array_equal(row["real"], native[1::2])
                        self.assertFalse(row["valid"][0])
                        if row["first_word_invalid"]:
                            self.assertFalse(row["valid"][1])
                        self.assertIsNone(row["gain_agc"])
                self.assertGreater(decoded, 0)
                self.assertEqual(len(collected), export.metadata["summary"]["csi_rows"])
                self.assertEqual(decoded, export.metadata["summary"]["decoded_rows"])
                self.assertTrue(any(r["mapped_time_us"] is not None for r in collected))
                selected = list(export.batches(receiver=collected[0]["receiver"], transmitter=collected[0]["transmitter"]))
                self.assertTrue(selected)
                self.assertFalse(list(export.batches(receiver="00:00:00:00:00:00")))
                rows.append(sorted(collected, key=lambda row: row["record_index"]))
            self.assertEqual(rows[0], rows[1])
            replay = Path(tmp) / "replayed"
            subprocess.run([os.environ["CSI_BIN"], "replay", os.environ["CSI_TEST_SESSION"],
                            "--record", str(replay), "--speed", "max"], check=True, capture_output=True)
            out = Path(tmp) / "replay-export"
            subprocess.run([os.environ["CSI_BIN"], "export", str(replay), "--out", str(out)], check=True, capture_output=True)
            rerecorded = [r for batch in Export(out).batches() for r in batch.table.to_pylist()]
            self.assertEqual(rows[0], sorted(rerecorded, key=lambda r: r["record_index"]))

    @unittest.skipUnless(os.getenv("CSI_BIN") and os.getenv("CSI_TEST_SOCKETS"),
                         "set CSI_BIN and CSI_TEST_SOCKETS=1 for local-socket CLI integration")
    def test_hub_socket_python_schema_shutdown_and_cleanup(self):
        import time
        from capstone_csi import live_batches
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            config = root / "hub.toml"
            # No physical source: a loopback-only peer with no sender is intentional.
            config.write_text('[[sources]]\nname="test"\nkind="udp"\nbind="127.0.0.1:0"\npeer="127.0.0.1:9"\n')
            session = root / "raw"
            path = root / "csi.sock"
            # macOS limits the full Unix socket path length.
            self.assertLess(len(str(path).encode()), 104)
            process = subprocess.Popen([os.environ["CSI_BIN"], "hub", "--config", str(config),
                                        "--out", str(session), "--seconds", "1", "--arrow-socket", str(path)],
                                       stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 5
                while not path.exists() and time.monotonic() < deadline and process.poll() is None:
                    time.sleep(0.01)
                self.assertEqual(list(live_batches(path)), [])
                stdout, stderr = process.communicate(timeout=5)
                self.assertEqual(process.returncode, 0, stderr.decode())
                import json
                status = json.loads((session / "arrow-status.json").read_text())
                self.assertTrue(status["complete"])
                self.assertEqual(status["connections"], 1)
                self.assertFalse(path.exists())
            finally:
                if process.poll() is None:
                    process.terminate()
                    process.communicate(timeout=5)


if __name__ == "__main__":
    unittest.main()
