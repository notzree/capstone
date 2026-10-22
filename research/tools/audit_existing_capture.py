"""Read-only audit of existing CSI JSONL captures; no signal/model claims.

Run from repository root: python3 research/tools/audit_existing_capture.py
"""
import json
from pathlib import Path
import statistics


def percentile(values, q):
    if not values:
        return None
    ordered = sorted(values)
    pos = (len(ordered) - 1) * q
    lo = int(pos)
    hi = min(lo + 1, len(ordered) - 1)
    return ordered[lo] + (ordered[hi] - ordered[lo]) * (pos - lo)


def audit(folder):
    metadata = json.loads((folder / 'session.json').read_text())
    timestamps, layouts = [], set()
    invalid_first_word = 0
    with (folder / 'csi.jsonl').open() as stream:
        for line in stream:
            if not line.strip():
                continue
            item = json.loads(line)
            timestamps.append(int(item['device_us']) & 0xffffffff)
            layouts.add((len(item['iq']), item.get('channel'), item.get('sig_mode'),
                         item.get('bandwidth'), item.get('rx_format')))
            invalid_first_word += bool(item.get('first_word_invalid'))
    gaps = [((b - a) & 0xffffffff) / 1000 for a, b in zip(timestamps, timestamps[1:])]
    # A backwards clock jump can wrap into a huge apparent gap. Report it;
    # never silently infer a sampling rate across a possible board restart.
    suspect_clock = any(gap > 0x7fffffff / 1000 for gap in gaps)
    span = sum(gaps) / 1000
    serial_sizes = []
    raw_log = folder / 'raw_serial.log'
    if raw_log.exists():
        with raw_log.open() as stream:
            for line in stream:
                if '\tCSI_DATA,' in line:
                    serial_sizes.append(len(line.split('\t', 1)[1].encode()))
    mean_serial_bytes = statistics.mean(serial_sizes) if serial_sizes else None
    return {
        'session': folder.name, 'synthetic': metadata.get('synthetic'),
        'condition': metadata.get('condition'), 'frames': len(timestamps),
        'device_span_s': span, 'suspect_clock_restart': suspect_clock,
        'accepted_frames_per_s': ((len(timestamps) - 1) / span
                                  if span > 0 and not suspect_clock else None),
        'duplicate_timestamps': sum(gap == 0 for gap in gaps),
        'gap_ms': {'median': statistics.median(gaps) if gaps else None,
                   'p95': percentile(gaps, .95), 'p99': percentile(gaps, .99),
                   'max': max(gaps) if gaps else None},
        'gaps_over_100ms': sum(gap > 100 for gap in gaps),
        'first_word_invalid_frames': invalid_first_word,
        'layouts_iqbytes_channel_sigmode_bandwidth_rxformat': sorted(layouts, key=str),
        'serial': {'raw_csi_lines': len(serial_sizes),
                   'mean_record_bytes': mean_serial_bytes,
                   'ideal_records_per_s_at_default_921600_8N1':
                       92160 / mean_serial_bytes if mean_serial_bytes else None,
                   'assumption': 'Default baud only; actual session baud is unrecorded.'},
        'note': 'Accepted log records, not transmitted-packet loss or fall accuracy.'
    }


if __name__ == '__main__':
    root = Path(__file__).resolve().parents[2] / 'demo' / 'data'
    print(json.dumps([audit(path.parent) for path in sorted(root.glob('*/csi.jsonl'))], indent=2))
