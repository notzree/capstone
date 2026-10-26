"""Read-only postflight checks and summary of the saved hardware trials."""
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parents[3]
sys.path.insert(0, str(REPO/'python/src'))
from capstone_csi import Trial

results = json.loads((ROOT/'results.json').read_text())
rows = 0
postflight = []
for result in results:
    trial = Trial(ROOT/result['name'])
    assert trial.split['assignment']=='unassigned'
    assert trial.split['group']=='hardware-pipeline-test-exclude-training'
    assert all(not i.truncated for i in trial.intervals)
    selected_rows = sum(b.table.num_rows for b in trial.activity_batches('transport_test',time_basis='host_arrival_proxy'))
    assert selected_rows>500
    versions = set()
    lengths = set()
    beyond_u32 = 0
    checked = 0
    for batch in trial.batches():
        versions.update(batch.table.column('protocol_version').to_pylist())
        for row in batch.table.to_pylist():
            lengths.add(len(row['native_csi']))
            beyond_u32 += row['radio_time_us'] is not None and row['radio_time_us'] > 2**32
            if row['layout_status']=='decoded':
                assert row['first_word_invalid']
                assert len(row['real'])==128 and len(row['imag'])==128
                assert row['valid'][:2]==[False,False]
                assert sum(row['valid'])==107
                checked += 1
    assert versions=={2}
    assert lengths=={256}
    audit = json.loads((ROOT/(result['name']+'-audit.json')).read_text())
    assert audit['stream_or_decode_errors_after_first_second']==0
    assert all(s['decode_errors']==0 and s['epoch_changes']==0 for s in result['sources'])
    assert all(node['frames_missing']==0 and node['captures_missing']==0 for node in result['health'])
    assert all(w['delta'] and w['delta']['pool_drops']==0 and w['delta']['export_drops']==0 for w in result['counter_windows'].values())
    assert all(c['roles_and_channel_match'] for c in trial.quality['node_configuration_checks'])
    assert result['live_rows']==result['offline_rows']
    rows += result['offline_rows']
    postflight.append(dict(name=result['name'],activity_proxy_selected_rows=selected_rows,decoded_bin_masks_checked=checked,
        protocol_versions=sorted(versions),native_lengths=sorted(lengths),unwrapped_ticks_beyond_u32=beyond_u32,
        startup_rejects=audit['stream_or_decode_error_samples_first_32'],rejects_after_first_second=audit['stream_or_decode_errors_after_first_second'],
        scope='Software transport_test phase marker only; no physical activity ground truth inferred.'))
normal = [r for r in results if not r['stalled_secondary_client']]
settings = {}
for flush in (100,20):
    group=[r for r in normal if r['flush_ms']==flush]
    settings[str(flush)]={f'{key}_range_ms':[min(r['after_2s_host_arrival_to_python_yield_ms'][key] for r in group),
                                            max(r['after_2s_host_arrival_to_python_yield_ms'][key] for r in group)]
                          for key in ('p50','p95','p99','max')}
    settings[str(flush)]['capture_rate_hz_range']=[min(l['rate_hz'] for r in group for n in r['health'] for l in n['links']),
                                                max(l['rate_hz'] for r in group for n in r['health'] for l in n['links'])]
summary=dict(schema='csi-live-hardware-summary/1',physical_trial_count=len(results),requested_acquisition_s=75,
    raw_and_healthy_live_csi_rows=rows,normal_settings=settings,
    stalled_secondary_client=results[-1]['arrow_status'],postflight=postflight,
    total_probe_sequence_gaps=sum(l['probes_missing'] for r in results for n in r['health'] for l in n['links']),
    maximum_wall_minus_monotonic_range_us=max(r['wall_minus_monotonic_range_us'] for r in results),
    caveats=['Latency is hub complete-frame arrival to Python batch yield, not radio-capture to Python.',
             'Two repetitions at each normal setting in 100/20/20/100 order; a separate 20 ms stalled-reader run.',
             'Per-frame temporal statistics use the existing callback-time inspector; native timestamp auditing remains planned.',
             'Probe sequence gaps remain possible over air/callback before capture. No frame/capture sequence or observed pool/export counter drops occurred in these runs.',
             'Five COBS framing rejects occurred while attaching at 129.8-132.8 ms; none after the first second. Initial captures before Hello stayed raw-only.',
             'No firmware flashing, board resets, surveyed geometry or physical activity labels.',
             'Shared-probe multi-receiver synchronization still needs a third board.'])
(ROOT/'summary.json').write_text(json.dumps(summary,indent=2,allow_nan=False)+'\n')
print(json.dumps({k:summary[k] for k in ('physical_trial_count','raw_and_healthy_live_csi_rows','normal_settings','total_probe_sequence_gaps','maximum_wall_minus_monotonic_range_us')},indent=2))
print('stall client_batch_drops',results[-1]['arrow_status']['client_batch_drops'],'write_failures',results[-1]['arrow_status']['client_write_failures'])
