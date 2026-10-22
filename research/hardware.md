# Hardware and acquisition decision

Research date: 2026-10-19. Recommendation and engineering thresholds below are proposed decisions, not measured fall-detection performance. No hardware was bought, flashed, or connected during this research.

## Decision

**Latest architecture constraint:** sensing TX is embedded; sensing RX may be embedded or Linux, with all scientific processing on Linux. The matched Linux TX/RX pair discussed below is a controlled capture reference, not the current deployment requirement. Assess richer RX candidates with actual compatible embedded probes, and compare them against embedded TX+RX nodes. The [workspace plan](rust-workspace.md) and [topology experiment](experiment-sensing-mesh.md) govern implementation; no final receiver or topology has been selected.


New hardware is explicitly in scope. Evaluate **NXP 88W8997 with its exact supported host/BSP/capture utility** for its connection to FallAware/CSI-Bench, alongside **one matched AX210/FeitCSI transmitter–receiver link on native Linux** as the practical default when NXP bundle access is unresolved. The original ESP32 is an optional low-cost comparison; it need not fail before richer acquisition begins. Borrow suitable Linux computers where convenient; purchasing appropriate hosts is also in scope. Start with one TX spatial stream and two RX chains; only add a second receiver after one link works reliably. A learned model cannot recover a fall that never measurably perturbs the sensing links. Final deployment hardware remains a measured selection; see [final-system.md](final-system.md).

This is a practical engineering ranking, not a claim that AX210 already achieves a published fall accuracy. Most strong published fall evidence in the supplied proposal uses Intel 5300 or NXP 88W8997. A model trained on those platforms is not directly interchangeable with AX210 or ESP32 CSI.

## What we actually have

- `demo/README.md` and `demo/firmware.lock.json` describe original ESP32-WROOM-32, ESP-IDF 5.5.2, upstream ESP-CSI revision `8633d67152db2808f141cc1595970aa9cf406045`, one TX/RX, requested 100 packets/s, channel 11 and HT40. The local vendored sender source sets `CONFIG_SEND_FREQUENCY=100`; the receiver prints textual CSI directly from its Wi-Fi callback. [H1]
- The README/lock claim that hardware capture is untested is **stale**: local nonsynthetic `empty-01`, `seated-01`, and chest captures exist. Root's full inspection is in [local-evidence.md](local-evidence.md). Empty/seated accepted rates are approximately 69.3/71.0 frames/s. These are past logged observations, not a live board inventory or fall dataset.
- The inspected empty/seated packets have 384 CSI bytes. CSI serial lines average about 1,247/1,295 bytes before line endings. **If** capture used the documented/default 921,600 baud and 8N1, the ideal serial payload ceiling is 92,160 bytes/s, or approximately 73.8/71.1 such lines/s. This is strong evidence for a possible transport bottleneck; session metadata does not prove the actual baud. Confirm baud and instrument enqueue/drop/write counters before blaming the RF link or increasing the requested rate. A 100 Hz setting is not 100 usable unique observations/s.
- User clarification: the boards are original ESP32, and “MacBook antennas” means the MacBook's built-in Wi-Fi. The inspected [Nexmon CSI support table](https://github.com/seemoo-lab/nexmon_csi) lists particular phone/Pi/router chip-firmware pairs, not a supported MacBook/macOS capture path. [FeitCSI](https://feitcsi.kuskosoft.com/getting_started/) requires its supported radio/driver and PCI access. Do not make the MacBook's built-in radio a sensing dependency; use it for serial capture, development, analysis, training and UI. The exact MacBook model is not needed for this design decision, and no blanket claim about all possible research patches is intended.

## Ranked platform comparison

| Priority | Concrete platform | What it adds | Limitation / decision gate |
|---|---|---|---|
| Comparison: already available family | Original ESP32 boards + pinned ESP-CSI; prior recordings used a two-board chain, current board count to confirm | Low-cost packet source; local data and host tooling already exist | Single RX chain, 2.4 GHz; likely textual-output ceiling. Resolve signal quality and rate before collecting many labels. |
| Preferred Wi-Fi development link | **AX210NGW M.2 2230** in each of two native Linux hosts, correct PCIe adapters and two antennas/card; FeitCSI plus its patched iwlwifi | Two RX chains; controlled injection; selectable HT/VHT/HE and wider bandwidth; useful within-receiver ratio/phase-difference experiments | Host/driver integration work; phase calibration required; no verified project fall performance. Prefer a matched TX to eliminate an interoperability unknown. [H3–H6] |
| 3: low-cost newer MCU trial | Two **ESP32-C5** devkits with official ESP-CSI; exact firmware/board revision recorded | 2.4/5 GHz options; official vendor ranks its CSI above classic/S3 | Still **1T1R**, including when an antenna-diversity switch is present. Faster CPU or 5 GHz alone does not create simultaneous dual-RX CSI. Validate rate, gain artifacts and exact 5 GHz firmware first. [H7–H8] |
| 4: router form factor | Exact **ASUS RT-AC86U / BCM4366c0 / 10_10_122_20**, Nexmon CSI; separate verified OFDM TX | Multiple cores/streams on supported radio, up to 80 MHz; Ethernet backhaul | Exact chip+firmware requirement, firmware patch/build effort, proprietary platform. Generic “ASUS”, Wi-Fi 6, or OpenWrt support is insufficient. [H9] |
| 5: reuse if already available | Exact **Raspberry Pi 3B+/4B/5 BCM43455c0** and supported Nexmon CSI firmware | Small Linux receiver with an established extraction path | Does not inherently deliver two simultaneous receive antennas; kernel/firmware pairing and monitored-band packet source still need validation. Current upstream now documents recent-kernel/Pi 5 path; old blanket “Pi 5 unsupported” advice is stale. [H9] |
| 6: legacy alternative | Matched **TP-Link TL-WDR4300 hardware revision with supported AR9344**, or AR9580/AR9590 boards, patched Atheros CSI OpenWrt | Multi-chain 802.11n; embedded router form factor | Used hardware and old build stack; TX must generate suitable HT sounding packets. Stock OpenWrt and arbitrary ESP32 packets do not satisfy this automatically. [H10–H12] |
| Experimental | **ESP-CRAB**, a specialized common-crystal PCB with two C5 chips + external C5 TX | Purpose-built paired receiver phase measurements | Not two ordinary C5 devkits. Obtain exact PCB, full hardware information, firmware and working capture evidence before scheduling around it. Availability/cost not verified. [H13] |

**ESP32-S3 is not the default upgrade purchase:** its datasheet specifies 2.4 GHz HT20/40 and 1T1R. It may improve implementation practicality, but it does not supply the two synchronous RX chains that phase-ratio methods often assume. Espressif's qualitative ranking C5 > C6 > C3≈S3 > classic is not a fall-classification benchmark. [H7, H14]

**NXP 88W8997 remains an evidence-led candidate:** FallAware/CSI-Bench make it relevant, but their headline results do not establish that a readily purchased NXP module plus public firmware will reproduce their capture stack. See the repository/dataset audit for availability. Do not order an arbitrary 88W8997 module until host-interface, capture software, firmware, antenna dimensions and license are resolved.

## Proposed bill of materials

All amounts below are **planning allowances in CAD, not checked vendor quotes**; exclude tax/shipping. They are not purchase instructions. Borrow/reuse hosts where possible. Exact Canadian stock and total prices remain procurement work.

| Package | Parts / quantity | Planning allowance |
|---|---|---:|
| ESP32 pilot | Existing 2 boards; 2 reliable USB data/power cables; rigid mounts; tape measure; optional USB powered hub | C$30–100 incremental |
| AX210 research pair, reused hosts | 2 × AX210NGW; 2 × compatible PCIe adapters if hosts need them; 4 × band-matched antennas and correctly fitting pigtails; rigid mounts; Ethernet | C$180–350 incremental |
| AX210 pair needing hosts | Above plus 2 used Linux computers with confirmed compatible slots and usable Ethernet | C$450–950 total |
| C5 comparison pair | 2 × ESP32-C5 devkits, cables and mounts | C$60–150 total |
| Used Atheros router pair | 2 exact-revision compatible routers, power supplies, Ethernet and mounts | C$100–250 total; sourcing uncertain |
| Nexmon router option | Exact RT-AC86U plus compatible packet-source hardware, Ethernet and mounts | C$150–350 incremental with a borrowed TX; source-dependent |
| Pi option | Supported Pi, PSU, storage and verified packet source | C$100–220 for receiver kit, source extra |

For AX210, check **M.2 2230 PCIe Wi-Fi**, not just slot shape. Intel documents Wi-Fi over PCIe and Bluetooth over USB; a USB adapter exposing Bluetooth does not carry Wi-Fi CSI. AX211/CNVio-family parts are not drop-in equivalents to the chosen AX210NGW plan. Avoid designing around a Mac USB dongle or Apple-Silicon VM PCI passthrough. Native Linux on a borrowed x86 system is the lower-uncertainty first host. [H3, H4, H6]

Use the antennas supplied/matched to the radio initially. Do not casually splice salvaged MacBook antennas or mismatch tiny snap connectors. More/larger antennas do not fix oscillator errors or create independent synchronized receivers.

## Acquisition architecture

```text
fixed packet TX ── Wi-Fi channel perturbed by person ── RX with logged CSI
                                                        │
                                             serial/PCIe capture
                                                        │
                                            local Linux/desktop logger
                                                        │
                                      raw recording + quality metrics
                                                        │
                                       offline baseline → live inference
```

Use a dedicated sensing channel and fixed mounts. For AX210 start with a permitted 5 GHz non-DFS channel, HT20 where supported by the selected mode, one TX stream, fixed MCS, both receive chains, and requested 500 packets/s; then compare HT40 and try 1,000 after stable logging. These are test settings to validate, not promised achievable rates or optimal settings. Compare 20/40 MHz before chasing 80/160 MHz; the latter adds output and parser complexity. Existing ESP32 experiments remain on their separately documented 2.4 GHz configuration.

FeitCSI exposes independent RX/TX counts, subcarrier count, MAC, host timestamp, FTM clock, RSSI, and rate flags in its header. Keep these with raw complex data. The official format warns about poor pilot-bin values; its pinned `Csi.cpp::fixCsiBug()` has special handling for 160 MHz. Preserve raw records and independently test parser dimensions; do not assume the GUI's processed plot is a lossless raw dataset. [H5–H6]

The FeitCSI tool depends on a separate patched driver. Pin **both** application and driver plus Linux kernel, firmware file/hash, radio configuration, parser revision and packet source. Its installation guide explains kernel-specific driver builds; “any Linux” marketing is not proof that the selected current kernel compiles and runs. Establish a reproducible known-good host image before OS updates. [H4]

### Multi-link synchronization

One TX → two spatially separated RX nodes is the first expansion: it gives complementary coverage while sharing a packet source. Add a source sequence counter and match observations to transmitted packets where the capture stack exposes enough metadata; log loss independently for each receiver. Host clock synchronization helps event/video alignment but **does not** phase-lock radios.

Keep three cases separate:

1. **Two RX chains inside one NIC:** measured on the same received packet; ratio/conjugate products can cancel some shared packet phase errors. Residual chain offsets, switches, AGC and phase jumps require empirical characterization. This is the first phase-feature experiment, not a calibrated angle-of-arrival guarantee.
2. **Two independent ESP32s/NICs:** independent oscillators and packet-detection offsets. Align time for amplitude/event fusion; do not subtract absolute CSI phase across boxes as though they form an antenna array.
3. **Common-clock/custom hardware:** ESP-CRAB targets shared-crystal reception. It reduces a hardware obstacle but does not establish project-level fall accuracy, distributed receiver synchronization or arbitrary array scalability. [H13]

Fuse per-link features or decisions with an explicit missing-link mask initially. Do not require carrier-phase coherence for this first multi-link fall detector. Keep Ethernet/USB logging separate from sensing traffic when possible. A mesh transports data; it is not evidence of better sensing coverage.

## First experiment and stop/go gates

**Day 1–2: prove one exact link.** Inventory board/chipset and cables; recover exact baud/radio settings from runtime; inspect empty, stationary-person, walking, fast sit/lie, door/fan and adjacent-room motion. No induced unsafe falls are required for this gate. Record 10–15 minutes/condition, three repeats, then at least two hours continuous ordinary activity and an unattended empty-room run. Record mounts/height/orientation/distance, source MAC, firmware/kernel, packet layout and power resets.

**Engineering acceptance criteria to pre-register** (chosen targets, not published universal thresholds):

- Distinguish unique received packets from duplicate callbacks and output rows; disclose source sequence support and count missing/corrupt samples.
- Fixed tensor dimensions, chain identity and frame type for the chosen mode; no silent mixed bandwidth or source contamination. Parse errors below 0.1% of produced records.
- For a selected requested rate, at least 90% useful unique samples with no unexplained gaps above 100 ms in clean one-link tests; report median/p95/p99 gaps instead of hiding them through interpolation. If the source's own transmitted rate is lower than configured, correct that first.
- Motion changes the signal reproducibly relative to empty-room variability at the intended room positions; the same event survives restart and a later session. At this stage report distributions and coverage failures, not an invented “motion SNR” cutoff.
- Two-hour capture completes without reset/disconnect; sustained output and processing queues stay bounded. Repeat after firmware/rate changes.

**ESP32-specific first fix to evaluate:** queue compact records outside the Wi-Fi callback, add transmitted/received/enqueued/dropped/written counters, verify actual baud, then try binary batching or a less verbose format. At 384 raw bytes × 1,000/s, CSI bytes alone would require 384 kB/s before metadata, exceeding the default serial line's ideal 92 kB/s ceiling. Raising packet rate alone therefore cannot produce a lossless 1 kHz version of this exact current output path. [H1–H2; local log calculation]

**Stop or change hardware:** if an exact RX cannot produce stable CSI in two focused integration days, test a borrowed alternative rather than losing weeks to driver work. If transport passes but small/fall-like ADLs are dominated by gain/noise artifacts, compare the richer receiver before large data collection. If placement has blind spots, change link geometry or add a receiver; fine-tuning cannot substitute for absent signal. If a richer receiver still fails held-out-room/continuous-ADL gates, narrow the project claim to a calibrated research prototype and consider an explicitly evaluated radar/fusion alternative with the team.

## Sources and reproducibility status

Primary sources were inspected on 2026-10-19. Commit hashes were read from GitHub's public commit API; published documentation and source were inspected, but no remote driver/model was installed or executed.

- **[H1]** Espressif ESP-CSI, pinned/current observed HEAD `8633d67152db2808f141cc1595970aa9cf406045` (2026-04-22): [sender](https://github.com/espressif/esp-csi/blob/8633d67152db2808f141cc1595970aa9cf406045/examples/get-started/csi_send/main/app_main.c), [receiver](https://github.com/espressif/esp-csi/blob/8633d67152db2808f141cc1595970aa9cf406045/examples/get-started/csi_recv/main/app_main.c). Local `demo/firmware.lock.json` matches this revision; build/log provenance is discussed separately in local evidence.
- **[H2]** Espressif, [ESP-IDF 5.5.1 classic ESP32 CSI packet layout, callback and HT20/40 documentation](https://docs.espressif.com/projects/esp-idf/en/v5.5.1/esp32/api-guides/wifi.html#wi-fi-channel-state-information). CSI byte order is imaginary then real; LTF layout depends on packet metadata; invalid leading bytes are flagged. Do not treat every array entry as an independent usable subcarrier.
- **[H3]** [FeitCSI repository](https://github.com/KuskoSoft/FeitCSI/tree/a86a7d3b02b20aeb42aa11d4b96e6d5493a24763), observed HEAD `a86a7d3b02b20aeb42aa11d4b96e6d5493a24763` (2025-05-17). AX200/AX210 support is explicit; speculative newer-card support is not used in our recommendation.
- **[H4]** [FeitCSI installation](https://feitcsi.kuskosoft.com/installation_and_upgrade/), [PCI-passthrough requirement](https://feitcsi.kuskosoft.com/getting_started/); [driver repository](https://github.com/KuskoSoft/FeitCSI-iwlwifi/tree/729678ca0bbcd0234a68d8e3859dfa6ac98e1a38), observed HEAD `729678ca0bbcd0234a68d8e3859dfa6ac98e1a38` (2025-05-17).
- **[H5]** [FeitCSI CLI](https://feitcsi.kuskosoft.com/command_line_interface/), [raw format](https://feitcsi.kuskosoft.com/csi_format/); [pinned parser implementation](https://github.com/KuskoSoft/FeitCSI/blob/a86a7d3b02b20aeb42aa11d4b96e6d5493a24763/src/Csi.cpp), [injection implementation](https://github.com/KuskoSoft/FeitCSI/blob/a86a7d3b02b20aeb42aa11d4b96e6d5493a24763/src/PacketInjector.cpp). Source inspection verifies dimensions/configuration and special processing paths, not measured throughput or calibrated phase.
- **[H6]** Intel, [AX210 specifications](https://www.intel.com/content/www/us/en/products/sku/204836/intel-wifi-6e-ax210-gig/specifications.html): 2×2, M.2 form factors, Wi-Fi PCIe / Bluetooth USB, 2.4/5/6 GHz.
- **[H7]** Espressif, [ESP-CSI solution and chip ranking](https://docs.espressif.com/projects/esp-techpedia/en/latest/esp-friends/solution-introduction/esp-csi/esp-csi-solution.html). Vendor qualitative claims are not independent fall evaluation.
- **[H8]** Espressif, [ESP32-C5 datasheet](https://documentation.espressif.com/esp32-c5_datasheet_en.pdf): 1T1R with 2.4/5 GHz and antenna diversity.
- **[H9]** [Nexmon CSI](https://github.com/seemoo-lab/nexmon_csi/tree/1232edf2cf1e7ae0561676c49e4dd4eb22c8a27e), observed HEAD `1232edf2cf1e7ae0561676c49e4dd4eb22c8a27e` (2026-09-30). Exact supported chip/firmware table, UDP core/stream format, and recent-kernel/Pi5 note inspected.
- **[H10]** [Atheros CSI Linux source](https://github.com/xieyaxiongfly/Atheros-CSI-Tool/tree/f9bcbd1a17a5c2b6bb75953822e54ff9f0f8b7e3), observed HEAD `f9bcbd1a17a5c2b6bb75953822e54ff9f0f8b7e3` (2026-01-15); [official compatible hardware](https://wands.hk/external/wifi/AtherosCSI/Hardware.html).
- **[H11]** [Atheros CSI OpenWrt source](https://github.com/xieyaxiongfly/Atheros_CSI_tool_OpenWRT_src/tree/e3495cc55de9c08ce078840e7813c7c8001c9d42), observed HEAD `e3495cc55de9c08ce078840e7813c7c8001c9d42` (2026-01-15), [userspace tools](https://github.com/xieyaxiongfly/Atheros_CSI_tool_OpenWRT_UserSpaceApp_src).
- **[H12]** Authors' [Collect CSI wiki](https://github.com/xieyaxiongfly/Atheros_CSI_tool_OpenWRT_src/wiki/Collect-CSI), primary [raw wiki text](https://raw.githubusercontent.com/wiki/xieyaxiongfly/Atheros_CSI_tool_OpenWRT_src/Collect-CSI.md) read directly: HT sounding requirement and paired patched TX/RX; do not infer interoperability from matching frequency alone.
- **[H13]** Espressif, [pinned ESP-CRAB example](https://github.com/espressif/esp-csi/blob/8633d67152db2808f141cc1595970aa9cf406045/examples/esp-crab/README.md). Local tree includes a circuit-explanation PDF; [issue 259](https://github.com/espressif/esp-csi/issues/259) requests fuller schematic/BOM/layout and scaling details. The issue is a user's report, not proof that no hardware PDF exists or that 5 GHz is currently broken. Retail procurement and full reproducible PCB availability remain unverified.
- **[H14]** Espressif, [ESP32-S3 datasheet](https://documentation.espressif.com/esp32-s3_datasheet_en.pdf): 1T1R, 2.4 GHz HT20/40 and antenna diversity.
