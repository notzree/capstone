"""CSI_STAR_TRIAL points at an archived physical star transport bundle."""
import os
from pathlib import Path
import unittest
from capstone_csi import Trial

@unittest.skipUnless(os.getenv('CSI_STAR_TRIAL'), 'set CSI_STAR_TRIAL for star bundle integration')
class StarTests(unittest.TestCase):
    def test_transport_trial_has_remote_inventory_without_invented_training_labels(self):
        trial = Trial(Path(os.environ['CSI_STAR_TRIAL']))
        self.assertIsNone(trial.plan)
        self.assertIsNone(trial.split)
        self.assertEqual(trial.intervals, [])
        self.assertEqual(len(trial.export.manifest['sources']), 1)
        self.assertGreater(len(trial.quality['star']['frozen_roster']), 0)
        batches = list(trial.batches())
        self.assertGreater(sum(len(b.table) for b in batches), 0)
        self.assertTrue(all(set(b.table.column('layout_status').to_pylist()) == {'decoded'} for b in batches))
        with self.assertRaisesRegex(ValueError, 'ground truth'):
            trial.activity_mask(batches[0], 'walking', time_basis='host_arrival_proxy')
