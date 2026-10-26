import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import unittest

import numpy as np
import pyarrow as pa

from capstone_csi import CsiBatch, Trial, live_batches


@unittest.skipUnless(os.getenv("CSI_BIN") and os.getenv("CSI_TEST_SOCKETS"),
                     "set CSI_BIN and CSI_TEST_SOCKETS=1 for collection integration")
class TrialTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        root = Path(self.tmp.name)
        (root / "image.bin").write_bytes(b"test image, not physical firmware")
        (root / "build.json").write_text('{"fixture":true}')
        nodes = [{"id": f"02:00:00:00:00:0{i}", "source": name, "mode": "independent",
                  "requested_tx_hz": 50, "channel": 6, "image": "image.bin", "build_manifest": "build.json"}
                 for i, name in ((1, "a"), (2, "b"))]
        (root / "plan.json").write_text(json.dumps({"schema": "csi-trial-plan/1", "trial_id": "test-01",
            "day_id": "day-a", "participant_id": "p01", "split_group": "p01-day-a", "room": "test",
            "geometry_revision": "r1", "label_uncertainty_ms": 1, "activities": ["walking", "empty"],
            "positions": [{"id": "centre", "x": 1, "y": 1, "z": 0, "azimuth_deg": 90}], "nodes": nodes}))
        (root / "geometry.json").write_text(json.dumps({"schema": "csi-geometry/1", "room": "test", "revision": "r1",
            "origin": "test fixture", "units": "m", "nodes": [{"id": n["id"], "x": i, "y": 0, "z": 0,
                                                                      "tier": "floor"} for i, n in enumerate(nodes)]}))
        (root / "hub.toml").write_text('[[sources]]\nname="a"\nkind="udp"\nbind="127.0.0.1:0"\npeer="127.0.0.1:9"\n'
                                     '[[sources]]\nname="b"\nkind="udp"\nbind="127.0.0.1:0"\npeer="127.0.0.1:10"\n')
        self.out = root / "trial"
        socket = root / "c.sock"
        process = subprocess.Popen([os.environ["CSI_BIN"], "collect", "--plan", str(root / "plan.json"),
            "--config", str(root / "hub.toml"), "--geometry", str(root / "geometry.json"), "--out", str(self.out),
            "--seconds", "5", "--arrow-socket", str(socket), "--arrow-flush-ms", "20"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            while True:
                line = process.stdout.readline()
                if not line:
                    self.fail("collector failed before input prompt: " + process.stderr.read())
                if line.startswith("Commands:"):
                    break
            # Establish a real Python UDS reader; silence deliberately produces no CSI.
            import threading
            received, errors = [], []
            def consume():
                try:
                    received.extend(live_batches(socket))
                except Exception as error:
                    errors.append(error)
            consumer = threading.Thread(target=consume)
            consumer.start()
            process.stdin.write("begin walking centre\n")
            process.stdin.flush()
            time.sleep(0.05)
            process.stdin.write("end\nnote changed activity\nbegin empty\n")
            process.stdin.flush()
            time.sleep(0.02)
            process.stdin.write("stop\nbegin walking\n")  # queued commands after stop must be ignored
            process.stdin.flush()
            stdout, stderr = process.communicate(timeout=10)
            consumer.join(timeout=5)
            self.assertFalse(consumer.is_alive())
            self.assertFalse(errors)
            self.assertEqual(received, [])
            self.assertEqual(process.returncode, 0, stderr)
        finally:
            if process.poll() is None:
                process.kill()
                process.communicate(timeout=5)
        self.trial = Trial(self.out)

    def test_bundle_labels_missing_link_quality_and_live_config(self):
        trial = self.trial
        self.assertEqual(len(trial.intervals), 2)
        walk, empty = trial.intervals
        self.assertFalse(walk.truncated)
        self.assertTrue(empty.truncated)
        self.assertEqual(walk.position["azimuth_deg"], 90)
        self.assertEqual(trial.split["assignment"], "unassigned")
        self.assertEqual(len(trial.quality["missing_links"]), 2)
        self.assertTrue(all(not s["matches"] for s in trial.quality["source_identity_checks"]))
        self.assertEqual(list(trial.batches()), [])
        status = json.loads((self.out / "raw/arrow-status.json").read_text())
        self.assertEqual(status["flush_interval_ms"], 20)
        self.assertTrue(status["complete"])
        self.assertEqual(status["connections"], 1)

    def test_uncertainty_half_open_intervals_foreign_clocks_and_truncated_exclusion(self):
        trial = self.trial
        walk, empty = trial.intervals
        u = walk.uncertainty_ns
        self.assertGreater(walk.end_ns - walk.start_ns, 2 * u)
        times = [walk.start_ns, walk.start_ns + u - 1, walk.start_ns + u,
                 walk.end_ns - u - 1, walk.end_ns - u, walk.end_ns]
        batch = CsiBatch(pa.record_batch([pa.array(times, type=pa.uint64())], names=["host_mono_ns"])
                         .replace_schema_metadata({b"csi.session_id": trial.session_id.encode()}))
        np.testing.assert_array_equal(trial.activity_mask(batch, "walking", time_basis="host_arrival_proxy"),
                                      [False, False, True, True, False, False])
        np.testing.assert_array_equal(trial.activity_mask(batch, "walking", time_basis="host_arrival_proxy", trim_uncertainty=False),
                                      [True, True, True, True, True, False])
        with self.assertRaisesRegex(ValueError, "explicitly"):
            trial.activity_mask(batch, "walking", time_basis="mapped_time_us")
        foreign = CsiBatch(batch.table.replace_schema_metadata({b"csi.session_id": b"other"}))
        with self.assertRaisesRegex(ValueError, "another session"):
            trial.activity_mask(foreign, "walking", time_basis="host_arrival_proxy")
        end_batch = CsiBatch(pa.record_batch([pa.array([empty.start_ns], type=pa.uint64())], names=["host_mono_ns"])
                             .replace_schema_metadata(batch.table.schema.metadata))
        self.assertFalse(trial.activity_mask(end_batch, "empty", time_basis="host_arrival_proxy").any())
        self.assertTrue(trial.activity_mask(end_batch, "empty", time_basis="host_arrival_proxy",
                                           trim_uncertainty=False, include_truncated=True).any())

    def test_provenance_label_session_and_partial_trial_rejections(self):
        path = self.out / "labels.jsonl"
        old = path.read_bytes()
        path.write_bytes(old + b'{}\n')
        with self.assertRaisesRegex(ValueError, "Label hash"):
            Trial(self.out)
        path.write_bytes(old)
        artifact = self.out / "provenance/node-0-image.bin"
        old_artifact = artifact.read_bytes()
        artifact.write_bytes(b"changed")
        with self.assertRaisesRegex(ValueError, "Provenance hash"):
            Trial(self.out)
        artifact.write_bytes(old_artifact)
        metadata = self.trial.metadata.copy()
        metadata["session_id"] = "other"
        (self.out / "trial.json").write_text(json.dumps(metadata))
        with self.assertRaisesRegex(ValueError, "session/timebase"):
            Trial(self.out)
        metadata["complete"] = False
        (self.out / "trial.json").write_text(json.dumps(metadata))
        with self.assertRaisesRegex(ValueError, "Incomplete"):
            Trial(self.out)


if __name__ == "__main__":
    unittest.main()
