"""Physical star smoke/mode checks. Run with PYTHONPATH=python/src and pyarrow installed."""
import argparse,json,queue,subprocess,threading,time
from pathlib import Path
from capstone_csi import live_batches,Trial
p=argparse.ArgumentParser();p.add_argument('--device',default='/dev/cu.usbserial-3');p.add_argument('--mode',choices=['file','both','stream','disconnect','both-disconnect'],default='file');p.add_argument('--name',required=True);p.add_argument('--seconds',type=int,default=5);a=p.parse_args()
root=Path(__file__).resolve().parents[4];base=Path(__file__).resolve().parent
log=(base/(a.name+'-control.jsonl')).open('x');q=queue.Queue()
process=subprocess.Popen([str(root/'sensing/target/debug/csi'),'star','--device',a.device,'--json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
def read():
 for line in process.stdout:
  log.write(line);log.flush()
  try:q.put(json.loads(line))
  except ValueError:pass
 q.put({'eof':True})
threading.Thread(target=read,daemon=True).start()
def send(command,**kw):process.stdin.write(json.dumps({'command':command,**kw})+'\n');process.stdin.flush()
def wait(predicate,seconds=20):
 deadline=time.monotonic()+seconds
 while time.monotonic()<deadline:
  try:v=q.get(timeout=.25)
  except queue.Empty:continue
  if v.get('eof'):raise RuntimeError(process.stderr.read())
  if predicate(v):return v
 raise TimeoutError('control response')
try:
 for i in range(40):
  send('status',id='status');s=wait(lambda v:v.get('id')=='status')['result']
  if s['state']=='Ready' and s['attached'] and s['nodes'] and all(n['ready'] for n in s['nodes']):break
  time.sleep(.5)
 else:raise RuntimeError('kit not ready')
 options={'id':'start','duration':a.seconds}
 if a.mode in ('file','both','both-disconnect'):options['out']=str(base/a.name)
 sock=Path('/tmp')/('csi-star-'+a.name+'.sock')
 if a.mode!='file':options['socket']=str(sock)
 send('start',**options)
 rows=[];errors=[]
 def consume():
  try:
   for batch in live_batches(sock):
    rows.extend(batch.table.to_pylist())
    if a.mode.endswith('disconnect'):break
  except Exception as e:errors.append(str(e))
 if a.mode!='file':
  wait(lambda v:v.get('event',{}).get('event')=='outputs_ready')
  worker=threading.Thread(target=consume);worker.start()
 start=wait(lambda v:v.get('id')=='start');assert start.get('ok'),start
 result=wait(lambda v:v.get('event',{}).get('event')=='run_finished',a.seconds+15)['event']['status']
 if a.mode!='file':worker.join(5);assert not worker.is_alive();assert not errors,errors
 if a.mode in ('file','both','both-disconnect'):
  trial=Trial(base/a.name);offline=[r for b in trial.export.batches() for r in b.table.to_pylist()]
  result['offline_rows']=len(offline)
  if a.mode=='both':
   keys=['receiver','receiver_epoch','transmitter','transmitter_epoch','schedule','probe_seq','capture_seq','node_time_us','rx_timestamp_us','native_csi','ltf','subcarrier','valid','real','imag','pilot','host_mono_ns','host_wall_ns','record_index']
   # Same raw bytes/times/indices/masks through the shared Rust encoder.
   assert [{k:r[k] for k in keys} for r in offline]==[{k:r[k] for k in keys} for r in rows]
 result['live_rows']=len(rows);result['consumer_errors']=errors
 (base/(a.name+'-result.json')).write_text(json.dumps(result,indent=2)+'\n')
 print(json.dumps({k:result.get(k) for k in ['stop_confirmed','captured','exported','device_drops','sink_input_drops','offline_rows','live_rows','stop_reason']}),flush=True)
finally:
 if process.poll() is None:
  try:send('shutdown',id='quit');process.wait(timeout=8)
  except Exception:process.kill();process.wait()
 log.close()
