#!/usr/bin/env python3
"""Prepare two reusable, non-peer-specific star images with hashed source/build provenance."""
import argparse, hashlib, importlib.util, json, os, shutil, subprocess, sys
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--out',type=Path,required=True);p.add_argument('--kit-id',type=int,default=1);p.add_argument('--channel',type=int,default=6);p.add_argument('--toolchain',default='esp');a=p.parse_args()
if not 1<=a.kit_id<=0xffffffff or not 1<=a.channel<=11:p.error('kit-id must be nonzero u32 and channel 1..11')
if a.out.exists():p.error('output already exists')
if importlib.util.find_spec('esptool') is None:p.error('install firmware/requirements-tools.txt in this Python environment')
firmware=Path(__file__).resolve().parents[1];sensing=firmware.parent;out=a.out.resolve();out.mkdir(parents=True)
env=dict(os.environ,CSI_KIT_ID=str(a.kit_id),CSI_STAR_CHANNEL=str(a.channel))
manifest={'schema':'csi-flash/1','settings':{'flash_mode':'dio','flash_freq':'40m','flash_size':'4MB'},'kit_id':a.kit_id,'channel':a.channel,'roles':{},'provenance':'source/build fingerprint and image SHA256; factory MAC/boot observed separately'}
for name in ['bootloader.bin','partition-table.bin']:shutil.copy2(sensing/'bench/2026-10-24/wireless'/name,out/name)
for role in ['star-tx','star-rx']:
 with (out/(role+'-build.log')).open('w') as log:subprocess.run(['cargo','+'+a.toolchain,'build','--locked','--offline','--release','--features',role],cwd=firmware,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
 shutil.copy2(firmware/'target/xtensa-esp32-none-elf/release/csi-esp',out/(role+'.elf'))
 subprocess.run([sys.executable,'-m','esptool','--chip','esp32','elf2image','--flash-mode','dio','--flash-freq','40m','--flash-size','4MB','-o',str(out/(role+'.bin')),str(out/(role+'.elf'))],check=True)
 files=[]
 for name,offset in [('bootloader.bin','0x1000'),('partition-table.bin','0xa000'),(role+'.bin','0x20000')]:
  data=(out/name).read_bytes();files.append({'file':name,'offset':offset,'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest()})
 assert files[-1]['bytes']<=1048576,'image exceeds factory app partition'
 manifest['roles'][role]={'features':role,'kit_id':a.kit_id,'channel':a.channel,'files':files}
sources={}
for directory in [firmware/'csi-esp/src',sensing/'crates/csi-node/src',sensing/'crates/csi-protocol/src']:
 for f in sorted(directory.rglob('*.rs')):sources[str(f.relative_to(sensing))]=hashlib.sha256(f.read_bytes()).hexdigest()
for f in [firmware/'Cargo.lock',firmware/'csi-esp/Cargo.toml',firmware/'csi-esp/build.rs']:sources[str(f.relative_to(sensing))]=hashlib.sha256(f.read_bytes()).hexdigest()
manifest['sources_sha256']=sources
manifest['compiler']=subprocess.check_output(['rustc','+'+a.toolchain,'--version','--verbose'],env=env,text=True)
(out/'flash-manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
print(out/'flash-manifest.json')
