#!/usr/bin/env python3
"""Build pinned upstream firmware; flash using generated offsets, never guessed ones."""
import argparse
import json
from pathlib import Path
import subprocess
import shutil
import sys

ROOT = Path(__file__).resolve().parents[1]
LOCK = json.loads((ROOT / 'firmware.lock.json').read_text())
REVISION = LOCK['revision']
p = argparse.ArgumentParser()
p.add_argument('action', choices=['build', 'flash'])
p.add_argument('role', choices=['send', 'recv'])
p.add_argument('--port')
p.add_argument('--baud', type=int, default=115200, help='Flashing speed; conservative default for USB hubs')
a = p.parse_args()
project = ROOT / 'vendor/esp-csi/examples/get-started' / ('csi_' + a.role)
actual = subprocess.check_output(['git', '-C', str(ROOT/'vendor/esp-csi'), 'rev-parse', 'HEAD'], text=True).strip()
if actual != REVISION:
    p.error('Upstream revision differs from firmware.lock.json')
if a.action == 'build':
    shutil.copyfile(ROOT / 'firmware' / f'{a.role}.dependencies.lock', project / 'dependencies.lock')
    subprocess.run(['docker', 'run', '--rm', '-v', f'{ROOT}/vendor/esp-csi:/esp-csi', '-w',
        f'/esp-csi/examples/get-started/csi_{a.role}', LOCK['idf_image_digest'],
        'bash', '-c', 'idf.py set-target esp32 && idf.py build'], check=True)
else:
    if not a.port:
        p.error('--port required for flash')
    build = project/'build'
    config = json.loads((build/'flasher_args.json').read_text())
    cmd = [sys.executable, '-m', 'esptool', '--chip', 'esp32', '--port', a.port,
           '--baud', str(a.baud), 'write_flash']
    cmd.extend(config['write_flash_args'])
    for offset, file in config['flash_files'].items():
        cmd.extend([offset, file])
    subprocess.run(cmd, cwd=build, check=True)
