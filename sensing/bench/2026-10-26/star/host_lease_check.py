"""Suspend the host without closing/toggling the USB port; verify RF lease expiry."""
import json,os,queue,signal,subprocess,threading,time,atexit
from pathlib import Path
base=Path(__file__).resolve().parent;root=base.parents[3]
p=subprocess.Popen([str(root/'sensing/target/debug/csi'),'star','--device','/dev/cu.usbserial-3','--json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
def cleanup():
 if p.poll() is None:
  p.send_signal(signal.SIGCONT);p.terminate()
  try:p.wait(timeout=6)
  except subprocess.TimeoutExpired:p.kill();p.wait()
atexit.register(cleanup)
q=queue.Queue();log=(base/'host-lease-control.jsonl').open('x')
def read():
 for line in p.stdout:
  log.write(line);log.flush()
  try:q.put(json.loads(line))
  except ValueError:pass
thread=threading.Thread(target=read,daemon=True);thread.start()
def send(c,**kw):p.stdin.write(json.dumps({'command':c,**kw})+'\n');p.stdin.flush()
def wait(pred,seconds=20):
 end=time.monotonic()+seconds
 while time.monotonic()<end:
  try:v=q.get(timeout=.1)
  except queue.Empty:continue
  if pred(v):return v
 raise TimeoutError('lease test')
for _ in range(50):
 send('status',id='status');status=wait(lambda v:v.get('id')=='status')['result']
 if status['state']=='Ready' and status['nodes'] and all(n['ready'] for n in status['nodes']):break
 time.sleep(.3)
epoch=status['receiver']['epoch'];send('start',id='start',out=str(base/'host-lease'),duration=60);assert wait(lambda v:v.get('id')=='start')['ok']
for _ in range(5):wait(lambda v:v.get('event',{}).get('packet',{}).get('kind')=='GrantOutcome')
p.send_signal(signal.SIGSTOP);time.sleep(3.5);p.send_signal(signal.SIGCONT)
result=wait(lambda v:v.get('event',{}).get('event')=='run_finished')['event']['status']
assert result['radio_stop_reason']=='LeaseExpired',result
assert result['stop_confirmed'],result
send('status',id='after');status=wait(lambda v:v.get('id')=='after')['result'];assert status['receiver']['epoch']==epoch;assert status['run'] is None
send('shutdown',id='quit');p.wait(timeout=6);thread.join();log.close()
(base/'host-lease-result.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({'radio_stop_reason':result['radio_stop_reason'],'stop_confirmed':result['stop_confirmed'],'receiver_rebooted':False,'suspended_seconds':3.5}),flush=True)
