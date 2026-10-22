# What to build next

**Recommendation:** develop a finished room-installed **possible-fall monitoring system**, with dedicated sensing hardware, continuous quality monitoring, a measured installation procedure, reliable event handling and sustained operation.

**Updated 2026-10-20 after the [limits review](limits/README.md):** the primary sensing architecture is a **two-tier mesh of embedded ESP32-class TX+RX nodes** (a floor tier and an upper tier) exporting over wired links to Linux. Processing runs in three stages: per-link conditioning against a gain-compensated baseline, a spatial stage (tomography and tracking from surveyed node positions, no training), then fall classification on room-independent quantities. The ESP32 gap for falls is mostly too few samples and too little coverage, both engineerable; bandwidth and phase coherence did not limit any fall result, and rich-receiver systems still lost accuracy in new rooms. **NXP 88W8997** and **AX210/FeitCSI** receivers therefore become optional references rather than the first purchase. Original ESP32 versus ESP32-C5 nodes is decided by a same-link bench comparison. Compare new-room transfer against timed non-fall calibration, then select final hardware and the minimum node count from measured results.

**Confirmed direction:** there is no hard demo deadline; progress should be as fast as practical toward the finalized project. The existing board family is original ESP32. “MacBook antennas” means the MacBook's built-in Wi-Fi. The inspected capture stacks do not provide a supported native macOS/built-in-MacBook CSI route, so use the MacBook for development, analysis and UI, with dedicated CSI capture hardware. See [final-system.md](final-system.md) for the updated system requirements and development-to-deployment path.

Research checked 2026-10-19 using three focused subagents and extended 2026-10-20 with four more ([limits/](limits/README.md)), the current [Google research document](https://docs.google.com/document/d/1LvhWgVKfX9y7fNDREqT2WzxCtPXiJItZiGMJGuQFGZ8/edit?tab=t.0), primary papers, official documentation and source inspection. This is a plan and static evidence review; no new fall experiment or model training was performed.

**Implementation direction:** one Rust workspace with embedded sensing TX, embedded or Linux sensing RX, and all DSP/ML on a Linux processor. Shared contracts still support both the TX star and distributed TX+RX; the distributed mesh is the primary target and the star a comparison. [rust-workspace.md](rust-workspace.md) is the current software structure; earlier matched Linux-TX recommendations are acquisition references rather than the selected deployment plan.

## Read these in order

| File | What it answers |
| --- | --- |
| [limits/README.md](limits/README.md) | What bounds CSI sensing fidelity: radar and RF-network comparison, first-principles limits, a 26-paper limitation audit, and which limits are engineerable. |
| [rust-workspace.md](rust-workspace.md) | Cargo packages, thin TX/RX/TX+RX firmware entrypoints, no_std/std boundaries, Linux hub, radio ownership and build order. |
| [experiment-sensing-mesh.md](experiment-sensing-mesh.md) | Proposed all-node TX/RX experiment: temporal sampling per link versus multiple perspectives, with local or cloud central processing. |
| [node-topology.md](node-topology.md) | Assessment of multiple ESP32 TX/single rich RX, router versus NIC capture, and alternative sensing layouts with centralized computation. |
| [esp-csi-rs-verification.md](esp-csi-rs-verification.md) | Pinned Rust CSI library audit, 24 passing host tests, ESP32/S3 firmware builds and the remaining live-hardware gate. |
| [software-boundary.md](software-boundary.md) | Rust acquisition ownership, firmware languages, the batched Python SDK contract and work possible before hardware arrives. |
| [evidence-to-design.md](evidence-to-design.md) | How the historical Drive papers determine the hardware, feature pipelines, segmentation, calibration and evaluation choices. |
| [final-system.md](final-system.md) | Finalized system direction, confirmed constraints, installation, reliable operation and evidence needed to freeze hardware. |
| [build-plan.md](build-plan.md) | Concrete architecture, first 14 working days, eight-week milestones, dataset collection, model comparisons, calibration experiment and go/no-go rules. |
| [local-evidence.md](local-evidence.md) | What already exists locally, measured capture rates, likely serial bottleneck and reusable code. |
| [hardware.md](hardware.md) | Ranked hardware choices, exact chip/toolchain constraints, estimated CAD allowances and acquisition gates. |
| [repositories.md](repositories.md) | Existing GitHub code/data, checked commits, licensing/access issues and realistic reuse. |
| [generalization.md](generalization.md) | Quantified paper evidence, unseen-room limitations, timed adaptation and defensible evaluation. |
| [proposal-snapshot.md](proposal-snapshot.md) | Text-only snapshot of research tab `t.0`, with source revision. The stakeholder Questions tab was read but is not copied here. Follow the original document for its native links/formatting. |

## Decisions supported by the evidence

**There is an acquisition starting point.** Five saved non-synthetic ESP32 recordings exist. Empty/seated sessions delivered about 69.3/71.0 frames/s. Their verbose records would nearly saturate the tool's default 921,600-baud serial connection at those rates. Actual baud is not in the session manifests, so verify it before attributing the limit. First measure and improve export, then assess the radio. There are no labeled falls in these captures. [Local audit](local-evidence.md).

**Environment robustness is feasible to investigate, but cannot be assumed.** A useful capstone result is an adaptation curve showing accuracy/alert burden versus installation effort across held-out rooms and people. Test a fixed source model, two minutes of known-empty calibration, and five/ten minutes of scripted ordinary activities. Those durations are proposed experiments, not published guarantees. Non-fall adaptation cannot establish how unseen target-room falls look. [Learning and evaluation](generalization.md).

**A bigger CNN is not the first hardware decision.** Independent ESP32s are separate sensing links; they do not create a coherent multi-antenna array. An AX210 offers two receive chains, whereas the common Intel 5300 fall datasets use three. A generic router, passive antenna or a reshaped tensor does not bridge that difference. Keep hardware-specific input adapters and collect local data. [Hardware decision](hardware.md).

**Public implementations are useful, with bounded reuse.** Start with Espressif for capture, ENetFall for a native-data model reproduction and CSI-Bench for harder-domain benchmarking. The current CSI-Bench repository reports corrected Version 12 reruns: ResNet18 fall accuracy is about 95.0% on `test_id` but 70.8% on `test_hard`; inspect split construction before calling that a specific room-transfer result. These are upstream results, not our reproduction. [Pinned benchmark results](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/RESULTS.md), [repository audit](repositories.md).

**Counting is stretch work; detecting a stationary person is not.** A person lying still after a fall is the core case, so stationary detection (gain-compensated attenuation against a baseline, with floor-tier links) is part of fall detection. Do not make counting a prerequisite or use an unreliable count to suppress possible falls. Plan for multi-link coverage from the start: coverage was the second-largest measured failure after new-room drop, and a single link at one height cannot see a body on the floor. Use single links for bench work only. Multi-room identity and simultaneous-person fall attribution remain separate research problems. [Limits review](limits/README.md).

**Classify falls on room-independent quantities.** Per-link CSI patterns encode the room's fixed multipath, which plausibly explains drops such as ENetFall's 96.8% to 69.0% in a new room. Feed the fall classifier with position, height-layer occupancy, drop speed and duration from the spatial stage, plus high-rate dynamics from only the links crossing the person. Physics-based or body-referenced features transferred better in FreeFall (80% to 93%), Widar3.0 (+15 to +52 points across rooms) and OpenCSI (zero-shot cross-room occupancy F1 ≥ 0.978). The only direct fall evidence for mesh tomography is one small RSS study (Mager 2013, 2 subjects), so this is a well-supported inference to test, not a result. Keep the FallDeFi, FallAware and ENetFall per-link pipelines as baselines on the same recordings.

**If accuracy is the overriding goal regardless of modality, keep a radar comparison available.** TI documents a fall-demo route for IWR6843AOPEVM. This is a practical contingency to compare on the same test protocol, not evidence of guaranteed radar accuracy. The plan keeps this optional and distinguishes it from a CSI-only result. [Contingency and scope](build-plan.md#9-sensor-contingency-when-fall-accuracy-is-the-governing-requirement).

## Start here

1. **One link on existing ESP32s.** Confirm board count. Binary export over USB or Ethernet; instrument sender, callback, queue and export rates; read and force receiver gain; explain the −70 to −80 dBm in-room RSSI. Capture fall-like motion at ≥300 Hz per link and downsample offline (200/120/70/20 Hz). Re-run the read-only audit with `python3 research/tools/audit_existing_capture.py`.
2. **2–4 ESP32-C5s on the same link:** original ESP32 at 2.4 GHz vs C5 at 2.4 GHz vs C5 at 5 GHz on the same fall-like motions. This decides which board the fleet uses.
3. **8-node single-tier mesh:** tomography, tracking and baseline management, with clock alignment from shared probes. It should clearly beat Ghostline before scaling.
4. **16–24 nodes in two tiers** with adaptive scheduling, the fall/ADL protocol and the new-room adaptation experiments in the build plan. Find the minimum node count from offline link subsets.
5. In parallel, verify access/provenance for the public datasets and reproduce one simple classifier on its native inputs. Bring in an NXP/AX210 reference only if step 1 shows falls confused with fast sitting in a way that higher sample rates fix.

The GitHub remote was empty when cloned. Its metadata is installed at `/Users/notzree/code/capstone/.git`; existing demo/research files were preserved. This work creates local research artifacts only; no commit or push was made, no hardware was flashed, and the Google document was not edited. Budget, available native Linux hosts, exact board count and partner alert requirements remain open. No hard deadline applies.
