#!/usr/bin/env python3
"""Run the archived two-board mode/rate sweep, retaining sessions and flash logs."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import datetime
import json
from pathlib import Path
import subprocess
import sys
import time

p=argparse.ArgumentParser()
p.add_argument('--bench',type=Path,required=True)
p.add_argument('--seconds',type=float,default=45)
p.add_argument('--restore-only',action='store_true')
p.add_argument('--restore-condition',default='pair-10',help='profile to leave running after the sweep')
a=p.parse_args();root=Path(__file__).resolve().parents[3];bench=a.bench.resolve();profiles=json.loads((bench/'profiles.json').read_text());manifest=bench/'firmware/flash-manifest.json'
python=sys.executable;hub=root/'sensing/target/release/csi';audit=root/'sensing/target/debug/examples/audit_session';boards=root/'sensing/bench/2026-10-21/boards'
registry_path=bench/'registry.json';registry={'schema':'csi-mode-trials/1','placement':'user-reported ~70cm apart on desk, same plane','notes':['Acquisition pilot; two repetitions per condition; no activity labels inferred.','Forward/reverse profile order; target rates are per transmitter, achieved load reported separately.'],'trials':[]}
if registry_path.exists():
 if not a.restore_only:p.error('registry already exists; use a fresh bench directory')
 registry=json.loads(registry_path.read_text())

def save_registry():
 temp=registry_path.with_suffix('.json.tmp');temp.write_text(json.dumps(registry,indent=2)+'\n');temp.replace(registry_path)

def flash_node(profile,label,tag):
 node=profile['nodes'][label];cmd=[python,str(root/'sensing/firmware/tools/flash.py'),'--board',str(boards/node['mac'].replace(':','')),'--manifest',str(manifest),'--image-role',node['image_role']]
 for baud in [460800,115200]:
  with (bench/f'{tag}-flash-{label}-{baud}.log').open('w') as log:
   result=subprocess.run(cmd+['--baud',str(baud)],stdout=log,stderr=subprocess.STDOUT,cwd=root)
  if result.returncode==0:return
  print('FLASH RETRY',tag,label,baud,flush=True)
 raise RuntimeError(f'flash failed: {tag}/{label}')

def flash_pair(profile,tag):
 print('FLASH',tag,profile['condition'],flush=True)
 with ThreadPoolExecutor(max_workers=2) as pool:
  jobs=[pool.submit(flash_node,profile,label,tag) for label in ['a','b']]
  for job in jobs:job.result()
 time.sleep(5)

def record(profile,name,seconds,repeat=None):
 session=bench/name
 print('RECORD',name,seconds,'seconds',flush=True)
 with (bench/f'{name}-hub.log').open('w') as log:
  subprocess.run([str(hub),'hub','--config',str(bench/'hub.toml'),'--out',str(session),'--seconds',str(seconds)],stdout=log,stderr=subprocess.STDOUT,check=True,cwd=root)
 with (bench/f'{name}-audit.json').open('w') as output:
  subprocess.run([str(audit),str(session)],stdout=output,check=True,cwd=root)
 result=json.loads((bench/f'{name}-audit.json').read_text())
 if not json.loads((session/'manifest.json').read_text())['complete']:raise RuntimeError('incomplete session')
 if not all(c['integrity']['status']=='verified' for c in result['segment_checks']):raise RuntimeError('segment integrity failed')
 trial={'condition':profile['condition'],'mode':profile['mode'],'target_hz':profile['target_hz'],'repeat':repeat,'audit':f'{name}-audit.json','session':name,'seconds':seconds,'utc':datetime.datetime.now(datetime.timezone.utc).isoformat()}
 if repeat is not None:
  registry['trials'].append(trial);save_registry()
 else:
  if registry.get('restored_profile'):
   registry.setdefault('restoration_history',[]).append(registry['restored_profile'])
  registry['restored_profile']=trial;save_registry()
 print('RESULT',name,json.dumps([{'tx':x['tx'],'rx':x['rx'],'csi':x['records'],'hz':round(x['rate_hz'],3),'missing':x['probes_missing']} for x in result['links']]),'errors',result['stream_or_decode_errors'],flush=True)

restore=next((pr for pr in profiles if pr['condition']==a.restore_condition),None)
if restore is None:p.error('unknown restore condition')
try:
 if not a.restore_only:
  save_registry()
  index=0
  for repeat,sequence in [(1,profiles),(2,list(reversed(profiles)))]:
   for profile in sequence:
    index+=1;name=f'r{index:02d}-{profile["condition"]}'
    flash_pair(profile,name);record(profile,name,a.seconds,repeat)
finally:
 name='restored-'+restore['condition']
 if (bench/name).exists():name+='-'+datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
 flash_pair(restore,name.replace('restored-','restore-',1))
 record(restore,name,30)
 print('RESTORED',restore['condition'],'; ports released',flush=True)
