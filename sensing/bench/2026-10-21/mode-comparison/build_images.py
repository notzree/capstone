"""Build matched capability/rate images; invoked with the isolated esp environment sourced."""
import hashlib,json,os,shutil,subprocess
from pathlib import Path
root=Path.cwd();out=root/'sensing/bench/2026-10-21/mode-comparison';images=out/'firmware';images.mkdir(exist_ok=True)
old=root/'sensing/bench/2026-10-21/dual-role/firmware'
m=json.loads((old/'flash-manifest.json').read_text())
for f in ['bootloader.bin','partition-table.bin']:shutil.copy2(old/f,images/f)
configs=[('rx','rx',100000,None)]
for hz in [10,50]:
 period=1000000//hz
 configs.extend([(f'tx-{hz}','tx',period,None),(f'independent-{hz}','tx,rx',period,None),(f'pair-a-{hz}','pair',period,'b0:b2:1c:0b:07:1c'),(f'pair-b-{hz}','pair',period,'a0:b7:65:dd:e9:3c')])
for name,features,interval,peer in configs:
 env=os.environ.copy();env['CSI_PROBE_INTERVAL_US']=str(interval)
 if peer:env['CSI_PEER_MAC']=peer
 else:env.pop('CSI_PEER_MAC',None)
 base=['cargo','+esp','build','--offline','--release','--features',features,'--manifest-path','sensing/firmware/Cargo.toml','--target','xtensa-esp32-none-elf','--config','sensing/firmware/.cargo/config.toml']
 print('BUILD',name,flush=True)
 subprocess.run(base,env=env,check=True)
 shutil.copy2(root/'sensing/firmware/target/xtensa-esp32-none-elf/release/csi-esp',images/f'{name}.elf')
 subprocess.run(['/private/tmp/capstone-bench-tools/bin/python','-m','esptool','--chip','esp32','elf2image','--flash-mode','dio','--flash-freq','40m','--flash-size','4MB','-o',str(images/f'{name}.bin'),str(images/f'{name}.elf')],check=True)
 base[2]='clippy';subprocess.run(base+['--','-D','warnings'],env=env,check=True)
node={'a':'a0:b7:65:dd:e9:3c','b':'b0:b2:1c:0b:07:1c'}
profiles=[];roles={}
for hz in [10,50]:
 for mode in ['star-a','star-b','independent','pair']:
  condition=f'{mode}-{hz}';pr=dict(condition=condition,mode=mode,target_hz=hz,nodes={})
  for label,mac in node.items():
   name=(f'tx-{hz}' if mode==f'star-{label}' else 'rx') if mode.startswith('star') else f'{mode}-{label}-{hz}' if mode=='pair' else f'independent-{hz}'
   key=f'{condition}-{label}';files=[]
   for offset,f in [('0x1000','bootloader.bin'),('0x20000',name+'.bin'),('0xa000','partition-table.bin')]:
    data=(images/f).read_bytes();files.append(dict(offset=offset,file=f,bytes=len(data),sha256=hashlib.sha256(data).hexdigest()))
   roles[key]=dict(node_mac=mac,features=next(c[1] for c in configs if c[0]==name),probe_interval_us=next(c[2] for c in configs if c[0]==name),peer_mac=node['b' if label=='a' else 'a'] if mode=='pair' else None,files=files)
   pr['nodes'][label]=dict(mac=mac,image_role=key)
  profiles.append(pr)
m['roles']=roles;m['firmware_version']='0.2.2';m.pop('schedule',None);m.pop('tx_probe_rate_hz',None)
paths=['sensing/firmware/csi-esp/src/main.rs','sensing/firmware/csi-esp/build.rs','sensing/firmware/csi-esp/Cargo.toml','sensing/crates/csi-node/src/lib.rs','sensing/firmware/Cargo.lock','sensing/firmware/.cargo/config.toml','sensing/firmware/rust-toolchain.toml']
m['source_files_sha256']={f:hashlib.sha256((root/f).read_bytes()).hexdigest() for f in paths};m['source_sha256']=m['source_files_sha256'][paths[0]]
m['build_command']='CSI_PROBE_INTERVAL_US=<interval> CSI_PEER_MAC=<peer for pair> cargo +esp build --offline --release --features <features>'
(images/'flash-manifest.json').write_text(json.dumps(m,indent=2)+'\n')
(out/'profiles.json').write_text(json.dumps(profiles,indent=2)+'\n')
print('BUILT',len(configs),'images and',len(profiles),'profiles',flush=True)
