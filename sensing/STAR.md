# USB ESP32 star kit

`csi star` owns one USB RX for its lifetime. Power the TX boards first or leave them powered; plug the RX into the Mac. Both reusable images use fixed-channel ESP-NOW, without a router, association or credentials. Star TXs listen while idle and emit one sensing probe only for a matching RX grant. Python owns signal processing.

From `sensing/`:

```sh
cargo build --release -p csi
./target/release/csi star --device auto
# If needed, choose the current RX device explicitly:
./target/release/csi star --device /dev/cu.usbserial-EXAMPLE
```

Auto selection passively checks USB serial candidates before sending any command, refuses ambiguity, and preserves DTR. Ports are opened exclusively. Close other serial monitors. A fresh attachment obtains a new token and the RX's cached discovery snapshot; it never resumes an earlier run. A port name is not a board identity.

The RX automatically discovers for ten seconds after boot. `nodes` distinguishes live/ready, stale, busy/incompatible and CSI-observed states. An attached client may initially see a cached entry marked stale until a fresh status response arrives. Idle status refreshes take approximately two seconds. Use `discover` after powering another TX; it runs a new ten-second window and is refused during measurement. Discovery does not determine positions or antenna orientation.

```text
nodes
status
discover --seconds 10
start --out ./trials/transport-01 --rate 10 --duration 60
stop
start --socket /tmp/csi-data.sock --rate 10 --duration 60
stop
start --out ./trials/transport-02 --socket /tmp/csi-data.sock --duration 60
stop
quit
```

`--rate` is requested probes/s per selected TX. Selection defaults to every compatible live idle TX; `--nodes aa:bb:cc:dd:ee:ff,11:22:33:44:55:66` selects a subset. Up to five TXs are admitted in v1 (the inventory holds eight). Admission uses a 20 ms grant budget and reserves 30% of UART capacity using the maximum framed acquisition size. Defaults are 10 Hz per TX and a one-hour duration cap. `--allow-long-run --duration 5400` explicitly permits a 90-minute test, with an absolute two-hour limit. Achieved rate and loss remain measurements, not promises.

START prepares all outputs, freezes TX identities/boot epochs and waits for every selected TX application ACK before issuing grants. One grant is outstanding at a time. STOP cancels grants, retries RF STOP up to three transmissions, and waits for an explicit USB export watermark. `stop_confirmed` and missing ACKs are separate from a complete sealed artifact. RX host-lease expiry is two seconds; TX run-lease expiry is at most three seconds. EOF, Ctrl-C, SIGTERM and quit stop/drain. Reboot/unplug terminates the attachment; reconnect and explicitly start a new run.

## Python streaming and output policy

```python
from capstone_csi import live_batches

for batch in live_batches('/tmp/csi-data.sock'):
    samples = batch.complex()
    # Existing native bytes, validity masks, LTFs and subcarrier indices are preserved.
```

Set `PYTHONPATH=/path/to/capstone/python/src` or install the local Python package. When a socket is requested, START waits up to ten seconds for a primary reader to read the Arrow schema and acknowledge `CSI-READY/1\n`. The updated `live_batches` does this automatically only when requested by schema metadata. Custom Arrow clients must implement that acknowledgement. No sensing grants precede it. Later readers get future batches only.

- **File:** canonical raw session in `raw/`, exact received CSIs controls (raw kind 4), ordered `star-events.jsonl`, `star-run.json`, `trial.json`, `quality.json` and a final `export/csi.arrow`. Overwrites are refused. Raw storage precedes Arrow delivery.
- **Stream:** no raw session, temporary recording directory, status sidecars or recovery spool. Session identity, surveyed geometry if supplied, RX and frozen TX metadata live in the schema; current/final health and RF run/schedule mapping remain queryable through `status`. Primary disconnect, timeout or bounded-queue loss stops the run. Memory labels are intentionally not offered as durable training labels.
- **Both:** the file bundle remains authoritative. Consumer drops/disconnections are counted and recording continues. Live and offline use the same native CSI decoder and Arrow encoder.

Client bounds are sixteen clients, eight batches/client, a 250 ms write timeout, a 256-record delivery input and a 20 ms flush. Queues never grow to hide stalled consumers. The serial/control loop is independent of disk/Arrow workers. A full raw-input queue or disk error initiates STOP and leaves counted loss/incomplete output. Final Arrow EOF does not establish radio stop or absence of loss: inspect the final control status (also stored in `star-run.json` for file modes).

Read a sealed transport trial with `Trial('trials/transport-01')` or `Export('trials/transport-01/export')`. A transport trial has no participant/activity/split ground truth. Existing `csi-trial/1` and acquisition v1/v2 recordings remain readable.

## Surveyed and labelled trials

Supply `start --out DIR --plan PLAN.json --geometry SURVEY.json ...` before `begin walking`, `end`, or `note "text"`. Labels reuse the existing activity/position validation and uncertainty handling. Undeclared activities are rejected. No example coordinates are inserted.

Use `csi-trial-plan/2` for star trials: all nodes reference the single physical source name `star-rx`; exactly one node is `mode: rx`, the selected remote nodes are `mode: tx`. Every canonical MAC must have a placement in the survey. Declared TX rates/channel must match START. Supply image/build artifact paths, day/split/participant metadata and declared activities as in [COLLECTION.md](COLLECTION.md). The collector snapshots artifacts but does not claim a flash readback of each declared TX image. Observed build IDs/boot epochs and ACKs remain separate evidence.

Labels use host monotonic arrival time (`host_arrival_proxy`), not calibrated capture time. One RX removes cross-receiver alignment but does not remove USB/operator delay. `TimeProbe`/`TimeEcho` exchanges remain in the raw stream for timing audits; star does not claim calibrated host/capture alignment. Unknown gain remains null and the hardware-invalid first word remains masked.

## Firmware preparation

Install the pinned ESP toolchain and `firmware/requirements-tools.txt`, source the toolchain environment, then:

```sh
python firmware/tools/prepare_star.py --out ./kit-images --kit-id 1 --channel 6
```

This creates `star-rx` and `star-tx` images, ELFs, build logs, source/compiler hashes and `flash-manifest.json`. Each image reports a nonzero source/settings build fingerprint; the manifest additionally hashes the exact prepared binaries. These are two reusable images with no compiled peer MACs. Distinct kits should use different nonzero kit IDs. A kit ID is not authentication. Changing channel requires preparing/reconfiguring the entire kit.

Flash only an identified original ESP32 with a verified full-flash backup using `firmware/tools/flash.py --board BOARD_DIR --manifest kit-images/flash-manifest.json --image-role star-rx` (or `star-tx`). The helper verifies MAC, backup and image hashes before writing, then verifies device bytes. Star roles reject combinations with each other, the opposite sensing capability, pair, gateway and wireless builds. Legacy roles remain available.

Wire details are pinned in `crates/csi-protocol/src/star.rs`: 36-byte header, exact typed body, CRC32, maximum 250 bytes; USB adds COBS/delimiter. Roster snapshots are paginated and acquisition kinds `0x81/0x82`, CSIt and CSIb remain unchanged. Application ACKs and serialized radio sends follow [Espressif's ESP-NOW guidance](https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/network/esp_now.html).

See [the physical bench](bench/2026-10-26/star/README.md) for measured results and pending acceptance gates. This kit does not implement DSP, classification or tomography.
