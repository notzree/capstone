"""Physical manual STOP, rediscovery, host-crash lease and stdin EOF checks."""
import json,queue,subprocess,threading,time
from pathlib import Path
import serial
base=Path(__file__).resolve().parent;root=base.parents[3];device='/dev/cu.usbserial-3'
base=base/'lifecycle-v2';base.mkdir()
import atexit
processes=[]
def cleanup():
 for process in processes:
  if process.poll() is None:
   process.terminate()
   try:process.wait(timeout=5)
   except subprocess.TimeoutExpired:process.kill();process.wait()
atexit.register(cleanup)
def launch(name):
 p=subprocess.Popen([str(root/'sensing/target/debug/csi'),'star','--device',device,'--json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
 processes.append(p)
 q=queue.Queue();log=(base/(name+'-control.jsonl')).open('x')
 def read():
  for line in p.stdout:
   log.write(line);log.flush()
   try:q.put(json.loads(line))
   except ValueError:pass
 t=threading.Thread(target=read,daemon=True);t.start()
 def send(c,**kw):p.stdin.write(json.dumps({'command':c,**kw})+'\n');p.stdin.flush()
 def wait(pred,seconds=20):
  deadline=time.monotonic()+seconds
  while time.monotonic()<deadline:
   try:v=q.get(timeout=.1)
   except queue.Empty:continue
   if pred(v):return v
  raise TimeoutError(name)
 def ready():
  for _ in range(45):
   send('status',id='status');v=wait(lambda v:v.get('id')=='status')['result']
   if v['state']=='Ready' and v['nodes'] and all(n['ready'] for n in v['nodes']):return v
   time.sleep(.3)
  raise RuntimeError('kit not ready')
 return p,t,log,send,wait,ready
p,t,log,send,wait,ready=launch('manual-and-crash');ready()
send('discover',id='discover');wait(lambda v:v.get('id')=='discover');ready()
send('start',id='start',out=str(base/'manual-stop'),duration=60);assert wait(lambda v:v.get('id')=='start')['ok']
for _ in range(5):wait(lambda v:v.get('event',{}).get('packet',{}).get('kind')=='GrantOutcome')
send('stop',id='manual-stop');result=wait(lambda v:v.get('id')=='manual-stop')['result'];assert result['stop_confirmed'],result
(base/'manual-stop-result.json').write_text(json.dumps(result,indent=2)+'\n');print('manual STOP confirmed',flush=True)
ready();send('start',id='crash-start',out=str(base/'crash-incomplete'),duration=60);assert wait(lambda v:v.get('id')=='crash-start')['ok']
for _ in range(3):wait(lambda v:v.get('event',{}).get('packet',{}).get('kind')=='GrantOutcome')
p.kill();p.wait();t.join();log.close();killed=time.monotonic_ns()
s=serial.Serial(port=None,baudrate=921600,timeout=.1);s.dtr=False;s.rts=False;s.port=device
with s,(base/'crash-passive-uart.jsonl').open('x') as f:
 deadline=time.monotonic()+4
 while time.monotonic()<deadline:
  b=s.read(4096)
  if b:f.write(json.dumps({'mono_ns':time.monotonic_ns()-killed,'bytes':list(b)})+'\n')
assert json.loads((base/'crash-incomplete/raw/manifest.json').read_text())['complete'] is False
p,t,log,send,wait,ready=launch('after-crash-and-eof');status=ready();assert status['run'] is None
send('start',id='eof-start',out=str(base/'eof-stop'),duration=60);assert wait(lambda v:v.get('id')=='eof-start')['ok']
wait(lambda v:v.get('event',{}).get('packet',{}).get('kind')=='GrantOutcome');p.stdin.close()
result=wait(lambda v:v.get('event',{}).get('event')=='run_finished')['event']['status'];assert result['stop_confirmed'];p.wait(timeout=8);t.join();log.close()
(base/'eof-stop-result.json').write_text(json.dumps(result,indent=2)+'\n');print('EOF STOP confirmed; crash archive stays incomplete; no automatic resume',flush=True)
