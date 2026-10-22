# ESP32 heartbeat feasibility experiment

Two original **ESP32-WROOM-32** boards capture a single Wi-Fi link. Your computer records CSI and estimates spectral candidates for average heart rate. This is a feasibility experiment, not established heartbeat tracking. No RuView or trained neural network is required.

## Bootstrap status

Python tools are installed, and both transmitter and receiver firmware compiled successfully using OrbStack. The binaries are in each upstream example’s `build/` directory. No ESP32 was connected, so flashing and real CSI capture remain untested. `firmware.lock.json` records the exact upstream revision and intended ESP-IDF image. Python dependencies are locked in `uv.lock`.

## Run the software now

```sh
uv sync --locked
uv run csi-heartbeat ports
uv run csi-heartbeat demo
uv run csi-heartbeat analyze data/synthetic --reference data/synthetic/reference.csv
uv run python -m unittest discover -s tests -v
```

Open `data/synthetic/report.png`. This is a **synthetic 72 BPM pipeline check**, not hardware evidence. Output directories must be new to prevent overwriting recordings.

## Firmware: two boards first

The checked-out source is in `vendor/esp-csi` (ignored by git). On another checkout, restore it with:

```sh
git clone https://github.com/espressif/esp-csi.git vendor/esp-csi
git -C vendor/esp-csi checkout 8633d67152db2808f141cc1595970aa9cf406045
```

Start Docker Desktop or OrbStack, then:

```sh
uv run python scripts/firmware.py build send
uv run python scripts/firmware.py build recv
```

Both examples use channel 11, HT40 and the same transmitter MAC; sender requests 100 packets/second. Receiver serial output is 921600 baud. Actual arrival timing is measured; no fixed sampling frequency is assumed. We retain upstream source and notices. Both builds passed with ESP-IDF 5.5.2. The image digest and binary hashes are recorded in `firmware.lock.json`; component dependency locks are in `firmware/`.

Connect only the intended board and identify its port:

```sh
uv run csi-heartbeat ports
uv run python -m esptool --port /dev/cu.YOUR_PORT chip_id
uv run python scripts/firmware.py flash send --port /dev/cu.YOUR_PORT
```

Label it TX. Connect the second board and repeat with `flash recv`; label it RX. Flashing replaces existing firmware. The helper uses build-generated offsets and targets `esp32`, never S3/C6. On macOS, Docker builds firmware and native Python flashes USB; Docker USB passthrough is not required. If bootloader connection fails, hold BOOT while connecting/resetting, then release after connection.

Power TX by USB. Connect RX to the computer with a data cable. This ESP-NOW setup does not need router credentials. Initially use just one transmitter and one receiver; these upstream examples are not configured for a six-node mesh.

## First recording

Mount boards about 1–2 metres apart near chest height, with the seated subject between them. Keep position, orientation and furniture fixed. Record one person at a time. Record placement and watch model in a separate session note.

```sh
uv run csi-heartbeat capture --port /dev/cu.YOUR_RX_PORT \
  --output data/empty-01 --seconds 180 --subject none --condition empty

uv run csi-heartbeat capture --port /dev/cu.YOUR_RX_PORT \
  --output data/seated-01 --seconds 180 --subject p01 --condition seated
```

While the seated capture runs, a friend can log the subject's watch BPM from a second terminal every 5–10 seconds:

```sh
uv run csi-heartbeat reference --output data/reference-p01.csv --source apple-watch-manual
```

Enter each displayed BPM and press Return. Enter `q` to finish. The subject should stay still; have the observer read the display without moving the subject's wrist repeatedly. Use the watch worn by the person being measured, not the observer's watch. Repeat for the Fitbit wearer in a separate recording.

```sh
uv run csi-heartbeat analyze data/empty-01
uv run csi-heartbeat analyze data/seated-01 --reference data/reference-p01.csv
```

Each capture saves raw serial logs, parsed complex CSI bytes, device microsecond timestamps, host UTC Unix timestamps, session metadata and accepted/rejected counts. Each analysis saves `report.json` and `report.png`. Recordings and wearable data stay local under ignored `data/`.

## Watches as references

Apple Watch and a heart-rate-capable Fitbit are useful for initial **window-average BPM comparisons**. They do not establish individual heartbeat timing, ECG accuracy or HRV performance of the CSI system. Apple Watch measures continuously during a workout; reported BPM still has averaging, missing samples and display lag. Use a suitable workout mode while seated for this test and note it in session notes. Fitbit Air is screenless: read its BPM in the Google Health app. It also supports heart-rate sharing with compatible apps/equipment; direct Bluetooth logging is a possible next step and is not implemented here. Verify the actual recording/export cadence instead of assuming a particular rate.

Manual entries are timestamped when Return is pressed, not when the watch measured them. For better comparison use timestamped exports converted to this CSV format (automatic Apple/Fitbit export import is not implemented):

```csv
unix_s,bpm,source
1700000000,72,apple-watch
1700000005,73,apple-watch
```

Timestamps must be UTC Unix seconds corresponding to the capture, BPM finite and 20–250. Filter exports to the correct person/device first. Analyzer averages only samples inside each window; it does not fill missing references or search for a time shift that improves agreement. Sparse samples and manual entry lag limit the error metric.

## What analysis means

CSI IQ bytes become amplitudes, arranged as **time × subcarrier**. We discard the first four bytes consistently, reject mixed packet layouts/transmitters, unwrap the 32-bit device clock, and resample using measured timestamps. We detrend and filter along time, then aggregate normalized subcarrier spectra in the 48–150 BPM band over 30-second windows.

Windows with long gaps, too few samples or no variation are rejected. A candidate requires heuristic spectral prominence and agreement across subcarriers. These checks are not calibrated confidence, presence detection or motion rejection. Breathing harmonics, gain changes and periodic movement can pass them. An empty room can also produce candidates; quantify this instead of treating the gate as proof of a heartbeat. The analysis is offline and uses noncausal filters.

Compare empty-room candidates, seated candidate coverage, reference sample counts and matched-window BPM error. Repeat on a later day and the other subject. Inspect signal quality before tuning thresholds; keep later recordings held out. A synthetic pass verifies software mechanics only. No heartbeat detection accuracy is claimed.

## Sources

- Firmware: https://github.com/espressif/esp-csi
- Apple Watch measurements: https://support.apple.com/en-au/120277
- Google/Fitbit heart-rate tracking: https://support.google.com/googlehealth/answer/14237938
- Fitbit Air live sharing: https://support.google.com/googlehealth/answer/14236705
- Google/Fitbit export: https://support.google.com/googlehealth/answer/14236615

The host analysis is written for this project. It does not copy the Pulse-Fi-inspired prototype's subcarrier-axis filtering or depend on its unavailable trained model. Identification/code search for IEEE document 7980063 remains pending its title.
