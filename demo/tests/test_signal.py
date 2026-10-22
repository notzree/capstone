import csv
import io
import json
import unittest
import numpy as np
from csi_heartbeat.signal import parse_line, amplitudes, unwrap_us, spectrum

class SignalTests(unittest.TestCase):
    def test_original_esp32_ht_frame(self):
        # HT rx_format=1 is NOT an error code. Test the pinned firmware's schema.
        row = ['CSI_DATA','1','aa:bb:cc:dd:ee:ff'] + ['0']*22
        row[18], row[21], row[22], row[23], row[24] = '100000','1','8','1',json.dumps([1,2,3,4,3,4,5,12])
        out = io.StringIO()
        csv.writer(out).writerow(row)
        parsed = parse_line(out.getvalue())
        np.testing.assert_allclose(amplitudes(parsed), [5,13])
        row[22] = '10'
        out = io.StringIO()
        csv.writer(out).writerow(row)
        with self.assertRaises(ValueError):
            parse_line(out.getvalue())

    def test_logs_ignored(self):
        self.assertIsNone(parse_line('I (123) wifi: connected'))

    def test_clock_wrap_and_reset(self):
        np.testing.assert_allclose(unwrap_us([2**32-10000, 0, 10000]), [0,.01,.02])
        with self.assertRaises(ValueError):
            unwrap_us([10000, 100])

    def test_time_axis_heart_peak_with_breathing(self):
        t = np.arange(0, 30, .02)
        rng = np.random.default_rng(1)
        x = (10*np.sin(2*np.pi*.25*t) + 3*np.sin(2*np.pi*1.2*t))[:,None]
        x = x + rng.normal(0,.2,(len(t),16))
        result, _, _ = spectrum(t, x)
        self.assertAlmostEqual(result['candidate_bpm'],72,delta=3)

    def test_gap_rejected(self):
        t = np.r_[np.arange(0,15,.02), np.arange(16,31,.02)]
        with self.assertRaises(ValueError):
            spectrum(t, np.ones((len(t),4)))

    def test_flat_rejected(self):
        t = np.arange(0,30,.02)
        with self.assertRaises(ValueError):
            spectrum(t, np.ones((len(t),4)))

    def test_noise_is_uncertain(self):
        t = np.arange(0,30,.02)
        x = np.random.default_rng(45).normal(size=(len(t),32))
        result, _, _ = spectrum(t, x)
        self.assertIsNone(result['candidate_bpm'])

if __name__ == '__main__':
    unittest.main()
