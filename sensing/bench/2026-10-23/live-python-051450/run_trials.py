"""Real-board Arrow delivery trials. Does not flash/reset boards or infer human activities."""
from __future__ import annotations

from collections import Counter, defaultdict
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time

import numpy as np
import pyarrow as pa

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[3]
sys.path.insert(0, str(REPO / 'python/src'))
from capstone_csi import Trial, live_batches

CSI = REPO / 'sensing/target/debug/csi'
CORE = ('record_index','source_index','receiver','receiver_epoch','frame_seq','capture_seq',
        'transmitter','transmitter_epoch','probe_seq','node_time_us','rx_timestamp_us','host_mono_ns','host_wall_ns','native_csi')


def write(path, value):
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')


def distribution(values):
    if not values:
        return None
    a = np.asarray(values, dtype=np.float64)
    a.sort()
    q = lambda p: float(a[max(0, int(np.ceil(p * len(a))) - 1)])
    return dict(count=len(a), min=float(a[0]), p50=q(.5), p95=q(.95), p99=q(.99), max=float(a[-1]), mean=float(a.mean()))


def digest(row):
    values = [row[k].hex() if isinstance(row[k], bytes) else row[k] for k in CORE]
    return hashlib.sha256(json.dumps(values, separators=(',', ':')).encode()).hexdigest()


def inputs():
    plan_path = REPO / 'sensing/examples/collection-plan.json'
    plan = json.loads(plan_path.read_text())
    for n in plan['nodes']:
        for k in ('image','build_manifest'):
            n[k] = str((plan_path.parent / n[k]).resolve())
    plan.update(day_id='2026-10-23', participant_id=None, split_group='hardware-pipeline-test-exclude-training',
                room='unsurveyed-transport-bench', geometry_revision='unsurveyed-transport-20261023',
                activities=['transport_test'], positions=[],
                notes='Automated transport test markers, not physical activity labels. Geometry is unsurveyed placeholder data; do not use for tomography or model training. Firmware image files are preserved expected pair-50 artifacts; running Hello identities/roles/config are checked, not a flash readback.')
    geometry = json.loads((REPO / 'sensing/examples/collection-geometry.json').read_text())
    geometry.update(room=plan['room'], revision=plan['geometry_revision'],
                    origin='UNSURVEYED placeholder coordinates; acquisition/transport test only.',
                    notes='Current physical placement has not been surveyed. All coordinates/orientations are placeholders, not observations.')
    for n in geometry['nodes']:
        n.update(x=0.0, y=0.0, z=0.0)
        n.pop('antenna', None)
    write(ROOT / 'geometry.json', geometry)
    return plan


def trial(number, flush_ms, duration_s, stall=False):
    name = f'r{number:02d}-flush-{flush_ms}' + ('-stalled-reader' if stall else '')
    out = ROOT / name
    plan = inputs()
    plan['trial_id'] = name
    plan_path = ROOT / f'{name}-plan.json'
    write(plan_path, plan)
    sock = Path(f'/private/tmp/csi-hw-{ROOT.name[-6:]}-{number}.sock')
    command = [str(CSI),'collect','--config',str(ROOT/'hub.toml'),'--plan',str(plan_path),
               '--geometry',str(ROOT/'geometry.json'),'--out',str(out),'--seconds',str(duration_s+15),
               '--arrow-socket',str(sock),'--arrow-flush-ms',str(flush_ms)]
    write(ROOT / f'{name}-command.json', command)
    stderr = (ROOT / f'{name}.stderr.log').open('w')
    stdout = (ROOT / f'{name}.stdout.log').open('w')
    process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=stdout, stderr=stderr, text=True)
    stats = dict(rows=0, batches=0, decoded_rows=0, decode_errors=[], consumer_error=None,
                 latencies_ms=[], batch_rows=[], callback_intervals_us=defaultdict(list), live_hashes={},
                 warmup_rows=0, warmup_latencies_ms=[], wall_minus_mono_ns=[])
    ready = threading.Event()
    stall_connection = stall_stream = stall_reader = None
    consumer = None
    try:
        deadline = time.monotonic()+10
        while not sock.exists():
            if process.poll() is not None or time.monotonic()>deadline:
                raise RuntimeError('collector did not expose socket; inspect logs')
            time.sleep(.01)

        def consume():
            try:
                for batch in live_batches(sock):
                    received_wall_ns = time.time_ns()
                    mono_ns = time.monotonic_ns()
                    ready.set()
                    rows = batch.table.to_pylist()
                    stats['batches'] += 1
                    stats['batch_rows'].append(len(rows))
                    stats['rows'] += len(rows)
                    stats['wall_minus_mono_ns'].append(received_wall_ns-mono_ns)
                    for row in rows:
                        stats['live_hashes'][row['record_index']] = digest(row)
                        age_ms = (received_wall_ns-row['host_wall_ns'])/1e6
                        stats['latencies_ms'].append(age_ms)
                        if row['host_mono_ns']>=2_000_000_000:
                            stats['warmup_rows'] += 1
                            stats['warmup_latencies_ms'].append(age_ms)
                    if set(batch.table.column('layout_status').to_pylist()) == {'decoded'}:
                        try:
                            block = batch.complex('htltf')
                            assert block.values.shape == (len(rows),64)
                            assert block.values.dtype == np.complex64
                            assert np.all(block.values.mask[:,0])
                            stats['decoded_rows'] += len(rows)
                        except Exception as e:
                            stats['decode_errors'].append(repr(e))
            except Exception as e:
                stats['consumer_error'] = repr(e)
                ready.set()

        consumer = threading.Thread(target=consume, daemon=True)
        consumer.start()
        if not ready.wait(8) or stats['consumer_error']:
            raise RuntimeError(f'Python received no real CSI: {stats["consumer_error"]}')
        process.stdin.write('begin transport_test\n')
        process.stdin.flush()
        if stall:
            stall_connection = socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
            stall_connection.settimeout(5)
            stall_connection.connect(str(sock))
            stall_stream = stall_connection.makefile('rb')
            stall_reader = pa.ipc.open_stream(stall_stream)  # Read schema, then deliberately never read CSI.
        started = time.monotonic()
        while time.monotonic()-started<duration_s:
            if process.poll() is not None:
                raise RuntimeError('collector exited during trial')
            time.sleep(.05)
        process.stdin.write('end\nstop\n')
        process.stdin.flush()
        process.stdin.close()
        process.wait(timeout=15)
        consumer.join(timeout=5)
        if process.returncode or consumer.is_alive():
            raise RuntimeError(f'collector exit={process.returncode}, consumer alive={consumer.is_alive()}')
        recorded = Trial(out)
        quality = recorded.quality
        arrow_status = json.loads((out/'raw/arrow-status.json').read_text())
        hub_status = json.loads((out/'raw/hub-status.json').read_text())
        offline_hashes = {}
        missing_after_warmup = []
        offline_rows = 0
        for batch in recorded.batches():
            for row in batch.table.to_pylist():
                offline_rows += 1
                offline_hashes[row['record_index']] = digest(row)
                if row['host_mono_ns']>=2_000_000_000 and row['record_index'] not in stats['live_hashes']:
                    missing_after_warmup.append(row['record_index'])
        mismatches = [index for index, value in stats['live_hashes'].items() if offline_hashes.get(index)!=value]
        health = []
        for node in quality['health']['nodes']:
            for epoch in node['epochs']:
                health.append(dict(node=node['node'],epoch=epoch['epoch'],hello=epoch['hello'],
                                   frames_missing=epoch['frames_missing'],captures_missing=epoch['captures_missing'],
                                   links=epoch['links']))
        clock_offsets = stats['wall_minus_mono_ns']
        result = dict(name=name, started_utc=datetime.now(timezone.utc).isoformat(), flush_ms=flush_ms,
                      requested_duration_s=duration_s, stalled_secondary_client=stall,
                      raw_complete=recorded.export.manifest['complete'],trial_complete=recorded.metadata['complete'],
                      sources=hub_status['sources'], subscriber_drops=hub_status['subscriber_drops'],
                      live_rows=stats['rows'],offline_rows=offline_rows,live_batches=stats['batches'],
                      live_decoded_rows=stats['decoded_rows'],consumer_error=stats['consumer_error'],decode_errors=stats['decode_errors'],
                      host_arrival_to_python_yield_ms=distribution(stats['latencies_ms']),
                      after_2s_host_arrival_to_python_yield_ms=distribution(stats['warmup_latencies_ms']),
                      batch_rows=distribution(stats['batch_rows']),
                      wall_minus_monotonic_range_us=(max(clock_offsets)-min(clock_offsets))/1000 if clock_offsets else None,
                      live_offline_core_mismatch_count=len(mismatches),
                      missing_live_core_after_2s_count=len(missing_after_warmup),
                      quality_links=list(recorded.link_quality()),missing_links=quality['missing_links'],
                      source_identity_checks=quality['source_identity_checks'],health=health,
                      counter_windows=quality['summary']['counter_windows'],arrow_status=arrow_status,
                      intervals=[dict(activity=i.activity,start_ns=i.start_ns,end_ns=i.end_ns,truncated=i.truncated) for i in recorded.intervals],
                      latency_scope='Same-host wall-clock delta: complete raw frame arrival at hub to CsiBatch yielded in Python, before complex()/model. Excludes radio capture-to-host/UART latency before frame arrival. Clock offset range monitored; no capture-to-host calibration implied.')
        result['checks_passed'] = all([result['raw_complete'],result['trial_complete'],not result['consumer_error'],
            not result['decode_errors'],not result['missing_links'],not mismatches,not missing_after_warmup,
            all(s['matches'] for s in result['source_identity_checks']),not any(result['subscriber_drops']),
            arrow_status['input_record_drops']==0,stats['decoded_rows']>0])
        write(ROOT/f'{name}-result.json',result)
        print(json.dumps({k:result[k] for k in ['name','checks_passed','live_rows','offline_rows','after_2s_host_arrival_to_python_yield_ms','missing_live_core_after_2s_count']},allow_nan=False),flush=True)
        return result
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=15)
        if stall_stream is not None: stall_stream.close()
        if stall_connection is not None: stall_connection.close()
        if consumer is not None: consumer.join(timeout=5)
        stdout.close();stderr.close()


if __name__ == '__main__':
    results=[]
    for number, flush in enumerate((100,20,20,100),1):
        results.append(trial(number,flush,15))
    results.append(trial(5,20,15,stall=True))
    write(ROOT/'results.json',results)
    assert all(r['checks_passed'] for r in results), 'At least one physical pipeline check failed; inspect result files.'
    assert results[-1]['arrow_status']['client_write_failures']>=1, 'Stalled-client disconnect was not exercised.'
    print('ALL_PHYSICAL_PIPELINE_CHECKS_PASSED',flush=True)
