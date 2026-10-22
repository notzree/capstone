#!/usr/bin/env python3
"""Pulse a previously identified ESP32's EN line without reading its UART stream."""
import argparse
import datetime
import json
from pathlib import Path
import time
import serial

p = argparse.ArgumentParser()
p.add_argument('--board', type=Path, required=True)
p.add_argument('--after-seconds', type=float, default=0)
p.add_argument('--log', type=Path, required=True)
a = p.parse_args()
if not 0 <= a.after_seconds <= 60:
    p.error('delay must be between 0 and 60 seconds')
board = json.loads((a.board / 'device.json').read_text())
if board.get('firmware_status') != 'flashed_and_verified':
    p.error('reset requires identified and verified firmware metadata')
time.sleep(a.after_seconds)
# Never read/write UART bytes: the live hub retains the recording stream.
port = serial.Serial(port=None, baudrate=921600)
port.dtr = False
port.rts = False
port.port = board['port_at_preparation']
with port:
    event = {'action': 'EN reset', 'mac': board['mac'], 'port': port.port,
             'wall_ns': time.time_ns(),
             'utc': datetime.datetime.now(datetime.timezone.utc).isoformat()}
    port.rts = True
    time.sleep(0.15)
    port.rts = False
with a.log.open('a') as log:
    log.write(json.dumps(event) + '\n')
print(json.dumps(event), flush=True)
