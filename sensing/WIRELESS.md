# Wireless CSI transport

This describes the preserved optional ESP32 gateway/mesh path. The current delivery priority is [star-first collection](PLAN.md#active-delivery-scope-usb-esp32-star-kit-2026-10-26); completing this path is not a prerequisite for that milestone.

Implemented 2026-10-24; updated 2026-10-26 so remote nodes use ESP-NOW without Wi-Fi association. Physical delivery through the Wi-Fi gateway still awaits an accessible test AP. The first two-board run returned no UDP records. The gateway's active scan saw 18 access points and zero exact matches for the configured SSID; association returned `NoAccessPointFound`. The computer was associated on channel 40 (5 GHz). This does not establish whether the router also offers a separate 2.4 GHz network.

## Data path

```text
ESP32 B -- direct ESP-NOW probe + original B readings --> ESP32 A gateway
ESP32 A -- direct ESP-NOW probe ----------------------> ESP32 B
ESP32 A -- Wi-Fi / UDP batches for A and B --> Rust hub -- UDS / Arrow --> Python
```

Only the gateway associates with the existing WPA2 network and follows its channel. Remote nodes start ESP-NOW on an explicit `CSI_RADIO_CHANNEL` without association, DHCP, a host IP, or network credentials. Remote builds never read `firmware/.wifi.env` and do not embed its SSID/password. Both nodes must use the same 2.4 GHz channel; the remote need only reach the gateway, not join the AP.

The initial remote configuration supports channels 1–11 and fails the build if its channel is missing or invalid. Choose the gateway AP's actual channel; channel 6 in the example below is only an example. Channel discovery, migration and recovery after an AP channel change are not implemented. Keep the AP channel fixed for now. A Linux hub with an integrated ESP-NOW adapter and a separate Ethernet/internet interface could avoid coupling the sensing channel to the AP; this implementation still uses the ESP32 Wi-Fi/UDP gateway. A gateway relay retains direct node-to-node sensing: traffic forwarded by an access point does not measure the original sender's direct link.

TX and CSI RX remain separately enabled capabilities. The tested build configurations are `pair,gateway` for A and `pair,wireless` for B. The lower-MAC pair leader requests turns; the responder attaches completed batches to responses. An export-only fallback can drain a ready batch when no request arrives. Its flag prevents the receiving callback from creating an extra sensing observation. Gateway A sends its own readings directly over UDP and relays B's batches unchanged. No UART CSI exporter or UART request task runs in these builds; USB remains usable for initial flashing, power and diagnostic logs.

Only direct vendor action frames with a matching physical source MAC become CSI observations. AP data frames, host requests and export-only carriers are filtered. This path still needs physical acceptance after association succeeds; build and loopback tests cannot establish RF compatibility or achieved rates.

## Batches, bounds and loss

`csi-wireless/1` wraps existing protocol-v2 binary frames. It preserves each original frame's CRC, node/epoch/sequence and native timestamps/CSI. The outer carrier adds the direct probe, sensing flag, turn tag, send timestamp, batch sequence, destination hub port and gateway relay-drop count, protected by a carrier CRC. This is a separate carrier version, not a change to the acquisition messages.

The carrier limit is 1,200 bytes or 16 complete records. The batch construction deadline is 50 ms from its first record by default; byte/count limits flush earlier. This is not a capture-to-Python latency bound: capture queues, ready queues, the next turn, network delivery and Arrow flushing add delay. ESP-NOW v2 is required for payloads larger than 250 bytes.

Queues are bounded: 32 captures, four completed peer batches, eight gateway uplink batches and four forwarded host requests. Overflow drops new work rather than blocking the capture callback. Node `export_drops` tracks its own rejected records; the gateway carrier's `relay_drops` tracks rejected remote records. Broadcast completion and UDP acceptance are not delivery acknowledgements. There is no retransmission or durable node spool; frame/capture/probe gaps and counters expose observable loss, and an unobserved trailing loss cannot be inferred from interior sequences alone.

## Configure and build

Put gateway network settings in ignored `firmware/.wifi.env`, then restrict its permissions. Only gateway builds read this file. Remote radio/batch/port settings are build environment variables; remotes do not use this file, even if it exists. Never commit the credentials file or gateway binaries containing credentials. Collection bundles snapshot firmware images, so keep bundles containing a gateway image in an ignored/private location too.

```dotenv
CSI_WIFI_SSID=YOUR_2_4_GHZ_NETWORK
CSI_WIFI_PASSWORD=YOUR_PASSWORD
CSI_HUB_ADDR=YOUR_COMPUTERS_IPV4
```

From `firmware/`, with the Espressif Rust toolchain environment loaded:

```sh
CSI_PEER_MAC=b0:b2:1c:0b:07:1c CSI_PROBE_INTERVAL_US=20000 CSI_HUB_PORT=9001 \
  cargo +esp build --locked --release --features pair,gateway

CSI_RADIO_CHANNEL=6 CSI_PEER_MAC=a0:b7:65:dd:e9:3c \
  CSI_PROBE_INTERVAL_US=20000 CSI_HUB_PORT=9002 CSI_BATCH_MS=50 \
  cargo +esp build --locked --release --features pair,wireless
```

Archive the first image privately before building the second. Wireless firmware reports version 0.3.1 for this role separation; previous 0.3.0 images still require association on both boards. Flash only the freshly MAC-identified board with a verified backup using `firmware/tools/flash.py`. This session used the separately installed `esp-wireless` toolchain alias; the alias itself is not a firmware requirement. The current two-board flash manifest is under `bench/2026-10-24/wireless/` and its credential-bearing images are under ignored `wireless-private/`.

Use one hub source per original receiver, even when they share one gateway endpoint:

```toml
[[sources]]
name = "wireless-a"
kind = "wireless"
bind = "0.0.0.0:9001"
node = "a0:b7:65:dd:e9:3c"

[[sources]]
name = "wireless-b"
kind = "wireless"
bind = "0.0.0.0:9002"
node = "b0:b2:1c:0b:07:1c"
```

The configured factory MAC and hub port select the source. Only a fully validated batch teaches the hub a reply endpoint, allowing the gateway's DHCP address to change. MAC/CRC checks identify and validate records; they are not cryptographic sender authentication. The gateway listens on UDP 9000. Host `TimeProbe` requests target the original receiver MAC; the gateway handles its own requests and forwards the peer's request through ESP-NOW. Echoes return in the normal batches.

From `sensing/`:

```sh
cargo run -p csi -- hub --config bench/2026-10-24/wireless/hub.toml \
  --out /tmp/wireless-session --seconds 30 \
  --arrow-socket /tmp/wireless-csi.sock --arrow-flush-ms 20
```

Python uses the existing `capstone_csi.live_batches()` API; no acquisition-wire parser is added to Python. See [EXPORT.md](EXPORT.md). `csi collect` also accepts wireless sources; supply the actual observed channel, new images/manifests and surveyed geometry as described in [COLLECTION.md](COLLECTION.md). Use private output directories when images embed network credentials.

## Persistence and validation

Raw record kind 3 stores the complete carrier datagram byte for byte. Older readers can skip this unknown kind; upgrade readers to inspect/export it. `Record::observations()` validates the entire carrier and yields original frames for health, clock fitting and export. Raw record counts count datagrams; decoded frame counts count their contents. All contents share the datagram's host arrival timestamps. Arrow `record_index` is the expanded observation ordinal. Source health exposes valid `wireless_batches`, `piggyback_batches`, `export_only_batches` and `last_relay_drops`; node epochs still carry their own sequences and counters.

Protocol tests cover truncation, mutation, bounds, inner CRC/identity and arbitrary input. Loopback integration tests cover raw preservation, finalized sequence gaps, expected-node routing, corrupt-batch endpoint rejection, single-use timing-echo correlation and identical live/offline Arrow native bytes, indices and host timestamps. Existing Rust and Python regression tests also run. Role-specific configuration checks can be run independently of the Xtensa target:

```sh
# From sensing/firmware/csi-esp/; uses the host compiler, no boards required.
rustc +stable --edition 2024 --test build.rs -o /tmp/csi-build-config-tests
/tmp/csi-build-config-tests
```

The 2026-10-26 checks cover remote credential/host-address exclusion, explicit/valid channels and gateway credential requirements. Remote, gateway and legacy pair release builds pass; the remote ELF was checked to exclude the actual local gateway SSID/password, and running the real build script beside a credential file confirmed it is ignored in remote mode. No serial boards were visible for flashing, so version 0.3.1 has not been physically tested. These tests validate software handling, not wireless sensing.

The next physical gate is: the gateway associates, while the remote starts without credentials or association on the same 2.4 GHz channel; both directed links arrive with no USB data reader; B's incoming source contains piggybacked records; gateway relay/export drops are visible; targeted echoes return for both identities; a live Python reader agrees with offline export. Then compare 20/50/100 ms node batching at a fixed Arrow flush setting, measuring achieved capture rate, interior gaps, batch fill/bytes and correctly scoped latency. A third board is required for the shared-probe receiver clock-fit gate.

General N-node enrollment/scheduling, dynamic rate control, OTA provisioning/update and capture-time label alignment remain follow-up work. No DSP or model runs on embedded nodes.
