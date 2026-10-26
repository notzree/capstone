"""Ten EN-reboot/discovery cycles. These are not physical power-cycle tests."""
import json,queue,subprocess,threading,time
from pathlib import Path
import serial
base=Path(__file__).resolve().parent;root=base.parents[3];rx='/dev/cu.usbserial-3';tx='/dev/cu.usbserial-0001'
def reset(path):
 s=serial.Serial(port=None,baudrate=921600);s.dtr=False;s.rts=False;s.port=path
 with s:s.rts=True;time.sleep(.15);s.rts=False
results=[]
for cycle in range(10):
 # Alternate TX-first/RX-first; one late attachment verifies cached inventory.
 if cycle%2==0:reset(tx);time.sleep(.2);reset(rx)
 else:reset(rx);time.sleep(.2);reset(tx)
 if cycle==2:time.sleep(11)
 p=subprocess.Popen([str(root/'sensing/target/debug/csi'),'star','--device',rx,'--json'],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True,bufsize=1)
 q=queue.Queue();log=(base/f'discovery-{cycle:02}.jsonl').open('x')
 def reader():
  for line in p.stdout:
   log.write(line);log.flush()
   try:q.put(json.loads(line))
   except ValueError:pass
 threading.Thread(target=reader,daemon=True).start()
 def send(c,**kw):p.stdin.write(json.dumps({'command':c,**kw})+'\n');p.stdin.flush()
 deadline=time.monotonic()+18;status=None
 while time.monotonic()<deadline:
  send('status',id='s');until=time.monotonic()+.8
  while time.monotonic()<until:
   try:v=q.get(timeout=.1)
   except queue.Empty:continue
   if v.get('id')=='s':
    status=v['result'];break
  if status and status['state']=='Ready' and status['attached'] and len(status['nodes'])==1 and status['nodes'][0]['id']=='b0:b2:1c:0b:07:1c' and status['nodes'][0]['ready']:break
  time.sleep(.2)
 else:raise RuntimeError(f'discovery failed {cycle}: {status}')
 result={'cycle':cycle,'order':'TX first' if cycle%2==0 else 'RX first','late_host_attach':cycle==2,'receiver_epoch':status['receiver']['epoch'],'nodes':status['nodes'],'measurement_started':status['run'] is not None}
 results.append(result);print(json.dumps(result),flush=True)
 send('shutdown',id='q');p.wait(timeout=6);log.close()
(base/'discovery-results.json').write_text(json.dumps(results,indent=2)+'\n')
