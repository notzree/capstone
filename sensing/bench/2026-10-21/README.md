# Two-board ESP-WROOM-32 bring-up — 2026-10-21

This records the initial fixed-role baseline. Both boards were subsequently updated to dual TX/RX with packet-driven turns; see [current configuration and comparison](mode-comparison/README.md).

Both connected boards were identified, fully backed up, flashed and verified. The final one-TX/one-RX recording confirms real CSI through the Rust live hub. No AP or Raspberry Pi was used. Boards continue their roles whenever powered; the recorder has stopped cleanly and released both ports.

| Initial role | Verified MAC | Serial port at preparation | USB location |
| --- | --- | --- | --- |
| TX | `b0:b2:1c:0b:07:1c` | `/dev/cu.usbserial-0001` | `2-1.3` |
| RX | `a0:b7:65:dd:e9:3c` | `/dev/cu.usbserial-3` | `2-1.4` |

Both are ESP32-D0WD-V3 revision 3.1, 40 MHz crystal, 4 MB flash, using CP2102 bridges. The bridges share the serial string `0001`; match MACs rather than relying on bridge serial numbers. Paths can change after reconnecting. Update `hub.toml` if needed.

## Preparation and recovery

Each MAC-named `boards/` directory contains `device.json`, identification/backup logs and a complete 4 MB `original-flash.bin`. Full-flash device digest verification passed for both backups. Backup hashes are in `device.json`; backups are Git-ignored and must be retained separately when copying the project. No eFuses were changed.

Final TX/RX ELFs and app images, bootloader, partition table, image hashes, offsets and build source hash are in `firmware/flash-manifest.json`. Both boards passed post-write verification of all three flash regions. `flash.log` contains the final successful flash; `flash-01.log` and `flash-manifest-01.json` describe the earlier attempt. The earlier app images were replaced by the corrected images. Build and flash instructions/provenance are in [the firmware README](../../firmware/README.md).

The first smoke test (`smoke-01`) exposed duplicate broadcast-peer registration: the radio driver had already registered that peer. The TX startup guard was corrected before the final build/flash. Both firmware roles build and pass clippy with warnings denied.

## Final 60-second smoke test

`smoke-02/` is a sealed raw session (6,206 records), with verified segment SHA-256 and successful unpaced replay of all records. `smoke-02-inspection.json`, `smoke-02-raw-audit.json` and `summary.json` preserve the results. `raw-audit.rs` is the small read-only audit used with the existing host/session and shared protocol decoders.

- **5,845 CSI captures**, **97.65 captures/s**; target TX rate 100 Hz.
- All captures: correct TX identity, channel **6**, HT (`sig_mode=1`), **MCS0**, **20 MHz**, long GI, no STBC; **256 native CSI bytes**. All 5,845 raw CSI payloads were distinct. `first_word_invalid=true` is retained: downstream processing must exclude the first four CSI bytes.
- **141 missing probe sequence numbers (~2.36% between observed endpoints)**. No duplicate/late probes, capture-sequence gaps, frame-sequence gaps, pool drops or export drops. This identifies missing probes before recorded captures; it does not isolate RF loss from TX scheduling/driver behavior. Driver acceptance alone does not establish over-air delivery.
- RX queue high water: **2 / 32**. No CSI gaps above 100 ms; largest observed capture interval **39.52 ms**.
- Both boards produced **60 TimeEcho replies**. Receiver-to-host timing mapping was established. One receiver supplies no evidence of cross-receiver clock alignment; its reference residual is absent, not a measured zero.
- One `bad_cobs` record occurred at **0.132 s**, immediately on attaching to an already-streaming RX; none occurred later. No frame decode errors or USB disconnects. Initial partial-frame resynchronization is expected when attaching mid-stream.
- Gain remains **absent**, explicitly, on every capture. No gain extraction or compensation claim.

Placement and room activity were not controlled or labelled. This is acquisition bring-up, not motion/fall detection or the full M5 gate. The default offline inspector also reports TX node silences above its 100 ms threshold because TX sends health messages at 1 Hz; these are separate from CSI-link gaps.

## Start the next trial

From `sensing/`, with both boards connected and no other serial monitor open:

```sh
cargo run --release -p csi -- hub --config bench/2026-10-21/hub.toml --out bench/2026-10-21/trial-01 --seconds 60
cargo run --release -p csi -- inspect bench/2026-10-21/trial-01
```

Use a fresh output directory for each trial. Choose and record fixed board positions/orientation, then collect labelled empty-room, still-person and walking trials without moving the boards. After that, test reboot/unplug recovery and a sustained 90-minute run. Add a third board as a second RX for the shared-probe clock test; the full M5 gate still requires sustained two-receiver timing and gain/callback/RAM verification.
