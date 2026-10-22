#!/usr/bin/env python3
"""Flash only a freshly identified board with a verified backup and hashed images."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

p = argparse.ArgumentParser()
p.add_argument('--board', type=Path, required=True, help='directory containing device.json and verified original-flash.bin')
p.add_argument('--manifest', type=Path, required=True)
p.add_argument('--baud', type=int, default=115200, choices=[115200, 460800], help='programming baud; verification still required')
p.add_argument('--image-role', help='manifest image role; defaults to original board role')
a = p.parse_args()
board = json.loads((a.board / 'device.json').read_text())
manifest = json.loads(a.manifest.read_text())
backup = a.board / 'original-flash.bin'
if board.get('backup_status') != 'verified' or backup.stat().st_size != board['flash_bytes']:
    p.error('a complete, verified backup is required')
if hashlib.sha256(backup.read_bytes()).hexdigest() != board['backup_sha256']:
    p.error('backup hash changed')
image_role = a.image_role or board['role']
role = manifest['roles'][image_role]
if role.get('node_mac', board['mac']).lower() != board['mac'].lower():
    p.error('firmware role targets a different board MAC')
files = role['files']
for entry in files:
    data = (a.manifest.parent / entry['file']).read_bytes()
    if len(data) != entry['bytes'] or hashlib.sha256(data).hexdigest() != entry['sha256']:
        p.error('firmware image hash/length mismatch: ' + entry['file'])
base = [sys.executable, '-m', 'esptool', '--chip', 'esp32', '--port', board['port_at_preparation'], '--baud', str(a.baud)]
identity = subprocess.check_output(base + ['read-mac'], text=True)
print(identity, flush=True)
found = re.search(r'MAC:\s*([0-9a-fA-F:]{17})', identity)
if not found or found.group(1).lower() != board['mac'].lower():
    p.error('connected board MAC differs from the backed-up board')
settings = manifest['settings']
options = ['--flash-mode', settings['flash_mode'], '--flash-freq', settings['flash_freq'], '--flash-size', settings['flash_size']]
pairs = [part for entry in files for part in (entry['offset'], str((a.manifest.parent / entry['file']).resolve()))]
subprocess.run(base + ['write-flash'] + options + pairs, check=True)
subprocess.run(base + ['verify-flash'] + options + pairs, check=True)
board['firmware_status'] = 'flashed_and_verified'
board['firmware_role'] = image_role
board['firmware_manifest_sha256'] = hashlib.sha256(a.manifest.read_bytes()).hexdigest()
board['flashed_at_utc'] = datetime.datetime.now(datetime.timezone.utc).isoformat()
(a.board / 'device.json').write_text(json.dumps(board, indent=2) + '\n')
