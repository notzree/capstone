import argparse
from collections import deque
import csv
import json
from pathlib import Path
import threading
import time
import numpy as np
from .signal import parse_line, amplitudes, unwrap_us, spectrum


def live_estimate(live, window_s):
    """Spectral estimate over the trailing window. Same heuristic as analyze; not a verified heart rate."""
    while len(live) > 1 and (live[-1][0] - live[0][0]) % 2**32 > window_s * 1e6:
        live.popleft()
    if len(live) < 2:
        return '  live: no frames yet'
    span = ((live[-1][0] - live[0][0]) % 2**32) / 1e6
    try:
        t = unwrap_us([d for d, _, _ in live])
        q, _, _ = spectrum(t, np.stack([a for _, _, a in live]))
    except ValueError as e:
        return f'  live [{span:.0f}s]: unavailable ({e})'
    flag = 'CANDIDATE' if q['candidate_bpm'] is not None else 'uncertain'
    return (f"  live [{span:.0f}s]: peak {q['spectral_peak_bpm']:.0f} BPM  ratio {q['peak_ratio']:.1f}  "
            f"agreement {q['subcarrier_agreement']:.2f}  {flag}")


def capture(args):
    import serial
    dest = Path(args.output)
    dest.mkdir(parents=True, exist_ok=False)
    (dest / 'session.json').write_text(json.dumps(dict(start_unix_s=time.time(), subject=args.subject,
        condition=args.condition, port=args.port, baud=args.baud, synthetic=False), indent=2))
    accepted = rejected = 0
    start = time.monotonic()
    # Rolling buffer for the live estimate: (device_us, rssi, amplitudes) over the last --live-window seconds.
    # The estimate runs in a worker thread so it never stalls serial reads.
    live = deque()
    next_live = start + args.live_every
    worker = None

    def run_live(snapshot):
        print(live_estimate(deque(snapshot), args.live_window), flush=True)
    try:
        with serial.Serial(args.port, args.baud, timeout=1) as ser, \
             (dest / 'raw_serial.log').open('w') as raw, (dest / 'csi.jsonl').open('w') as out:
            while time.monotonic() - start < args.seconds:
                line = ser.readline().decode('utf-8', errors='replace').strip()
                now = time.time()
                raw.write(f'{now:.6f}\t{line}\n')
                try:
                    item = parse_line(line)
                    if item is None:
                        continue
                    item['host_unix_s'] = now
                    out.write(json.dumps(item) + '\n')
                    accepted += 1
                    live.append((item['device_us'], int(item['rssi']), amplitudes(item)))
                    while len(live) > 1 and (live[-1][0] - live[0][0]) % 2**32 > args.live_window * 1e6:
                        live.popleft()
                    if accepted % 100 == 0:
                        out.flush()
                        rssi = np.mean([r for _, r, _ in live])
                        print(f'{accepted} frames, {rejected} rejected, RSSI {rssi:.0f} dBm', flush=True)
                except (ValueError, KeyError, TypeError, csv.Error):
                    rejected += 1
                if time.monotonic() >= next_live and (worker is None or not worker.is_alive()):
                    next_live += args.live_every
                    worker = threading.Thread(target=run_live, args=(list(live),), daemon=True)
                    worker.start()
    except KeyboardInterrupt:
        pass
    finally:
        (dest / 'capture_stats.json').write_text(json.dumps(dict(accepted=accepted, rejected=rejected)))
        print(f'Saved {accepted} frames to {dest}; rejected {rejected}')
    if accepted == 0:
        raise ValueError('No valid CSI frames. Check firmware, transmitter power and serial port.')


def reference(args):
    path = Path(args.output)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('x', newline='') as f:
        writer = csv.writer(f)
        writer.writerow(['unix_s', 'bpm', 'source'])
        print('Enter displayed BPM, then Return; q to finish. Time is when you press Return.')
        while True:
            value = input('BPM: ').strip()
            if value.lower() == 'q':
                return
            try:
                bpm = float(value)
                if not 20 <= bpm <= 250:
                    raise ValueError()
            except ValueError:
                print('Enter a number between 20 and 250, or q.')
                continue
            writer.writerow([time.time(), bpm, args.source])
            f.flush()


def analyze(args):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    path = Path(args.session)
    items = [json.loads(line) for line in (path / 'csi.jsonl').read_text().splitlines() if line]
    if not items:
        raise ValueError('Empty recording')
    # Do not silently combine transmitters, packet modes, or CSI layouts.
    layouts = {(x['mac'], len(x['iq']), x.get('channel'), x.get('sig_mode'), x.get('bandwidth'), x.get('stbc')) for x in items}
    if len(layouts) != 1:
        raise ValueError('Mixed CSI layouts/transmitters; record one fixed link and packet mode')
    t = unwrap_us([x['device_us'] for x in items])
    x = np.stack([amplitudes(item) for item in items])
    host = np.array([item['host_unix_s'] for item in items])
    refs = []
    if args.reference:
        with open(args.reference) as f:
            refs = [(float(r['unix_s']), float(r['bpm'])) for r in csv.DictReader(f)]
        if any(not np.isfinite(a) or not np.isfinite(b) or not 20 <= b <= 250 for a, b in refs):
            raise ValueError('Invalid reference timestamp/BPM')
    results, spectra = [], []
    for start in np.arange(0, t[-1] - args.window + 1e-6, args.window):
        mask = (t >= start) & (t < start + args.window)
        result = dict(start_s=float(start), end_s=float(start + args.window))
        try:
            quality, freq, power = spectrum(t[mask], x[mask])
            result.update(quality)
            spectra.append((freq * 60, power))
        except ValueError as e:
            result.update(status='rejected', reason=str(e), candidate_bpm=None)
        if mask.any():
            selected = [b for a, b in refs if host[mask][0] <= a <= host[mask][-1]]
            result['reference_count'] = len(selected)
            result['reference_mean_bpm'] = float(np.mean(selected)) if selected else None
            if selected and result['candidate_bpm'] is not None:
                result['absolute_error_bpm'] = abs(result['candidate_bpm'] - result['reference_mean_bpm'])
        results.append(result)
    if not results:
        raise ValueError(f'Need a complete {args.window:g}-second window')
    errors = [r['absolute_error_bpm'] for r in results if 'absolute_error_bpm' in r]
    metadata = json.loads((path / 'session.json').read_text())
    report = dict(synthetic=metadata.get('synthetic', False), frames=len(items), windows=results,
        candidate_window_fraction=sum(r['candidate_bpm'] is not None for r in results) / len(results),
        matched_windows=len(errors), mean_absolute_error_bpm=float(np.mean(errors)) if errors else None,
        warning='Spectral candidates only: breathing harmonics/motion may imitate heart rate. Manual references have display/entry lag.')
    (path / 'report.json').write_text(json.dumps(report, indent=2, allow_nan=False))
    fig, ax = plt.subplots(2, 1, figsize=(10, 7), constrained_layout=True)
    if spectra:
        for freq, power in spectra:
            ax[0].plot(freq, power, alpha=0.5)
    ax[0].set(xlabel='Frequency expressed as BPM', ylabel='Normalized spectral power', title='SYNTHETIC pipeline check' if report['synthetic'] else 'CSI spectral candidates — not verified heartbeats')
    for r in results:
        if r['candidate_bpm'] is not None:
            ax[1].scatter(r['start_s'] + args.window / 2, r['candidate_bpm'], c='tab:blue')
        if r.get('reference_mean_bpm') is not None:
            ax[1].scatter(r['start_s'] + args.window / 2, r['reference_mean_bpm'], marker='x', c='tab:orange')
    ax[1].set(xlabel='Seconds since first CSI frame', ylabel='BPM', title='Blue: candidate; orange: reference window mean')
    shown = [r[k] for r in results for k in ('candidate_bpm', 'reference_mean_bpm') if r.get(k) is not None]
    if shown:
        ax[1].set_ylim(min(shown) - 10, max(shown) + 10)
    fig.savefig(path / 'report.png', dpi=150)
    plt.close(fig)
    print(json.dumps(report, indent=2))


def demo(args):
    path = Path(args.output)
    path.mkdir(parents=True, exist_ok=False)
    rng = np.random.default_rng(42)
    start = 1700000000.
    (path / 'session.json').write_text(json.dumps(dict(synthetic=True, condition='synthetic_72_bpm')))
    with (path / 'csi.jsonl').open('w') as f:
        for t in np.arange(0, 65, .02):
            amp = np.rint(55 + 8 * np.sin(2*np.pi*1.2*t) + 12*np.sin(2*np.pi*.25*t) + rng.normal(0, 1.5, 32)).astype(int)
            iq = np.column_stack([np.zeros(32, dtype=int), amp]).ravel().tolist()
            f.write(json.dumps(dict(iq=iq, device_us=round(t*1e6), host_unix_s=start+t, mac='synthetic', first_word_invalid=False))+'\n')
    with (path / 'reference.csv').open('w') as f:
        f.write('unix_s,bpm,source\n')
        for t in range(0, 65, 5):
            f.write(f'{start+t},72,synthetic\n')
    print(f'Synthetic data only. Analyze with: csi-heartbeat analyze {path} --reference {path}/reference.csv')


def main():
    parser = argparse.ArgumentParser(description='Original ESP32 CSI heartbeat feasibility experiment')
    sub = parser.add_subparsers(dest='command', required=True)
    p = sub.add_parser('ports')
    p = sub.add_parser('capture')
    p.add_argument('--port', required=True)
    p.add_argument('--output', required=True)
    p.add_argument('--seconds', type=float, default=180)
    p.add_argument('--baud', type=int, default=921600)
    p.add_argument('--subject', required=True, help='Anonymous subject ID')
    p.add_argument('--condition', required=True, help='Free-form label, e.g. empty, seated, chest-taped')
    p.add_argument('--live-window', type=float, default=30, help='Seconds of trailing data for the live estimate')
    p.add_argument('--live-every', type=float, default=5, help='Seconds between live estimate lines')
    p = sub.add_parser('reference')
    p.add_argument('--output', required=True)
    p.add_argument('--source', required=True)
    p = sub.add_parser('analyze')
    p.add_argument('session')
    p.add_argument('--reference')
    p.add_argument('--window', type=float, default=30)
    p = sub.add_parser('demo')
    p.add_argument('--output', default='data/synthetic')
    args = parser.parse_args()
    try:
        if args.command == 'ports':
            from serial.tools.list_ports import comports
            for port in comports():
                print(f'{port.device}: {port.description}')
        else:
            if hasattr(args, 'window') and args.window < 20:
                raise ValueError('Window must be at least 20 seconds')
            if hasattr(args, 'live_window') and (args.live_window < 20 or args.live_every <= 0):
                raise ValueError('Live window must be at least 20 seconds and live interval positive')
            if hasattr(args, 'seconds') and args.seconds <= 0:
                raise ValueError('Duration must be positive')
            globals()[args.command](args)
    except (ValueError, OSError) as e:
        parser.exit(1, f'Error: {e}\n')
