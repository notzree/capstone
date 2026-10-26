# Two-board wireless transport preparation, 2026-10-24

Result: implementation, host tests and firmware builds pass; **physical wireless delivery is not yet verified**. Both identified boards were flashed and image digests verified. Original full-flash backups remain under `../../2026-10-21/boards/`.

| Board | Current USB port | Build | Destination |
| --- | --- | --- | --- |
| `a0:b7:65:dd:e9:3c` | `/dev/cu.usbserial-0001` | `pair,gateway`, firmware 0.3.0 | computer UDP 9001 for its own records; relay peer to 9002 |
| `b0:b2:1c:0b:07:1c` | `/dev/cu.usbserial-3` | `pair,wireless`, firmware 0.3.0 | ESP-NOW piggyback to gateway, original destination 9002 |

Port names had swapped since the saved setup. The flash tool rejected both initial MAC mismatches before writing. The saved port assignments were corrected using freshly observed MACs. A later diagnostic reader overlapped a verification attempt; that attempt failed and was rerun without a concurrent reader. Final flash logs contain successful independent digest verification. USB ports are development identifiers and must be rechecked next time.

`flash-manifest.json` records image hashes and non-secret settings: 20,000 µs leader probe interval, 50 ms batch construction age, hub IPv4 `10.120.1.57`. The peer is a responder; this interval is a target, not an achieved capture rate. Images contain local credentials and are ignored under `wireless-private/`. Do not move them, or collection bundles that snapshot them, into public artifacts. Network settings are in ignored `firmware/.wifi.env`.

## Physical evidence

- `smoke-01` ran before either initial flash could proceed; zero records is not a wireless firmware trial.
- `smoke-02.stdout.log`: UDP-only hub listening on 9001/9002; 25-second session sealed with **zero records**. Configuration has no serial source. Diagnostic consoles were subsequently used to diagnose startup, not to ingest CSI.
- `a-association.log`: repeated `NoAccessPointFound`, RSSI -128.
- `a-scan.log`: gateway startup scan found 18 visible APs and zero exact target-SSID matches, followed by `NoAccessPointFound`.
- Computer network inspection reported channel 40, 5 GHz, WPA2 Personal. That does not prove the absence of a router's separate 2.4 GHz band. ESP-WROOM-32 needs the target SSID on accessible 2.4 GHz.
- No wireless CSI rate, piggyback yield, radio loss or capture-to-Python latency was measured. Geometry is not surveyed; there are no human-activity labels or fall/tomography results.

## Software validation

134 Rust tests passed across the workspace, including new carrier property/corruption checks and local UDP/UDS tests; `cargo check-nostd`, clippy with warnings denied and host formatting passed. Six Python tests passed using the preserved real pair session and local socket integration. Gateway, peer and legacy pair release firmware builds pass. Local carrier tests prove byte-preserving raw persistence, whole-batch validation, finalized gaps, expected-node probe routing, rejection of corrupt endpoint learning and live/offline Arrow equality of native bytes, indices and host timestamps. They do not prove RF operation.

## Resume

1. Confirm the exact 2.4 GHz SSID is visible; update ignored credentials if it differs. Rebuild/reflash if configuration changes. Both nodes need the same RF channel. Verify the computer's hub address remains correct.
2. Start `csi hub` using `hub.toml`, a new output directory and a live Arrow socket. Keep serial readers closed for the data-path proof.
3. Require both MAC identities and both directed CSI links, nonzero peer `piggyback_batches`, no invalid framing/layouts, and meaningful export/relay loss counters. Check targeted echoes for both origins; a two-node pair alone does not establish shared-probe receiver alignment.
4. Feed `capstone_csi.live_batches()` and compare recorded native bytes/identity/timestamps against offline export. Check finalized sequence loss and decoded-bin validity, not just packet count.
5. Compare node batch construction ages 20/50/100 ms while fixing host Arrow flushing at 20 ms. Report achieved rate, interior gaps, batch fill and bytes, queue/drop counters and host-arrival-to-Python delay. Node buffering precedes hub arrival and is not included in that delay. Preserve private firmware/collection provenance per setting.

See [WIRELESS.md](../../../WIRELESS.md) for architecture, build instructions, limits and the remaining acceptance gates.
