import csv
import json
import numpy as np
from scipy import signal

FIELDS = 'type,id,mac,rssi,rate,sig_mode,mcs,bandwidth,smoothing,not_sounding,aggregation,stbc,fec_coding,sgi,noise_floor,ampdu_cnt,channel,secondary_channel,local_timestamp,ant,sig_len,rx_format,len,first_word,data'.split(',')


def parse_line(line):
    """Original ESP32 CSV schema from Espressif csi_recv; logs are ignored."""
    if not line.startswith('CSI_DATA,'):
        return None
    row = next(csv.reader([line]))
    if len(row) != len(FIELDS):
        raise ValueError(f'Expected original ESP32 25-column schema, got {len(row)}')
    item = dict(zip(FIELDS, row))
    iq = json.loads(item['data'])
    if len(iq) != int(item['len']) or len(iq) % 2 or len(iq) < 8:
        raise ValueError('Invalid CSI byte count')
    if any(not isinstance(x, int) or x < -128 or x > 127 for x in iq):
        raise ValueError('CSI values must be signed bytes')
    item['iq'] = iq
    item['device_us'] = int(item['local_timestamp'])
    item['first_word_invalid'] = bool(int(item['first_word']))
    return item


def amplitudes(item):
    iq = np.asarray(item['iq'], dtype=float).reshape(-1, 2)
    # Drop first four bytes consistently, including frames where they are valid.
    # This preserves column alignment when first_word_invalid changes.
    return np.hypot(iq[2:, 0], iq[2:, 1])


def unwrap_us(values):
    raw = np.asarray(values, dtype=np.int64)
    delta = np.diff(raw)
    delta = np.where(delta < -(2**31), delta + 2**32, delta)
    if np.any(delta <= 0):
        raise ValueError('Duplicate/out-of-order timestamps or board restart; split the recording')
    return np.r_[0., np.cumsum(delta) / 1e6]


def spectrum(times, values):
    """Filter along TIME (axis 0), then aggregate normalized subcarrier spectra."""
    times = np.asarray(times, dtype=float)
    values = np.asarray(values, dtype=float)
    if len(times) < 100 or times[-1] - times[0] < 20:
        raise ValueError('Need at least 20 seconds and 100 frames')
    dt = np.diff(times)
    if np.any(dt <= 0) or not np.isfinite(values).all():
        raise ValueError('Nonmonotonic time or nonfinite CSI')
    fs = 1 / np.median(dt)
    if fs < 10 or dt.max() > 0.25:
        raise ValueError('Insufficient sample rate (<10 Hz) or gap >250 ms')
    grid = np.arange(times[0], times[-1], 1 / fs)
    x = np.column_stack([np.interp(grid, times, col) for col in values.T])
    x = signal.detrend(x, axis=0)
    keep = np.std(x, axis=0) > 1e-6
    if not keep.any():
        raise ValueError('No varying subcarriers')
    x = x[:, keep]
    x /= np.std(x, axis=0)
    sos = signal.butter(4, [0.7, 3.0], btype='bandpass', fs=fs, output='sos')
    filtered = signal.sosfiltfilt(sos, x, axis=0)
    freq, power = signal.welch(filtered, fs=fs, nperseg=min(len(grid), int(fs * 20)), axis=0)
    band = (freq >= 0.8) & (freq <= 2.5)
    f, p = freq[band], power[band]
    p /= np.maximum(p.sum(axis=0), 1e-15)
    aggregate = np.median(p, axis=1)
    peak = int(np.argmax(aggregate))
    bpm = float(f[peak] * 60)
    ratio = float(aggregate[peak] / max(np.median(aggregate), 1e-15))
    agreement = float(np.mean(np.abs(f[np.argmax(p, axis=0)] * 60 - bpm) <= 6))
    # Heuristic spectral checks are NOT calibrated probability or motion rejection.
    qualified = ratio >= 5 and agreement >= 0.5
    return dict(candidate_bpm=bpm if qualified else None, spectral_peak_bpm=bpm,
                peak_ratio=ratio, subcarrier_agreement=agreement, sample_rate_hz=float(fs),
                max_gap_s=float(dt.max()), status='candidate_only' if qualified else 'uncertain'), f, aggregate
