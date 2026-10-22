# Experiment: temporal sampling versus spatial diversity

Status: proposed experiment. Recorded 2026-10-19 from the team's proposed design. **Updated 2026-10-20:** after the [limits review](limits/README.md) the two-tier mesh is the primary architecture, so this experiment now chooses the mesh's **minimum node count, tier layout, fusion method and airtime split**. It still keeps the high-rate link and star as controls that can overturn that choice.

## Question and hypothesis

**At a fixed airtime and hardware budget, does observing more TX–RX paths at a lower sampling rate per path improve fall detection compared with observing fewer paths at a higher sampling rate?**

The hypothesis is that complementary perspectives improve coverage and robustness enough to offset reduced temporal detail on each link. The competing hypothesis is that sampling loss, poorer individual measurements, extra export traffic or fusion complexity outweigh the additional perspectives. The preferred operating point may be a selected subset of links rather than a full mesh.

With broadcast probes and adaptive scheduling this is not a fixed tradeoff (see "Adaptive schedule" below): low-rate sampling everywhere and high-rate sampling on the links crossing the person can run together. The sharper question is whether that adaptive allocation matches uniform high-rate sampling at a fraction of the airtime.

Here, temporal sampling means **usable CSI observations per second per directed TX–RX link**, including their actual gap/jitter distribution. Spatial diversity means **different radio path geometries**. Per-packet CSI detail—subcarriers, antenna chains, bandwidth, quantization and scaling—is a separate variable. More temporal samples do not automatically mean higher per-packet CSI resolution.

## Proposed architecture

- Every sensing node can act as both transmitter and CSI receiver, taking scheduled turns transmitting while the other nodes listen.
- Each receiver extracts CSI locally and forwards **CSI measurements plus metadata**, not merely the received application payload. Forwarding ordinary payload packets alone does not preserve the measured radio channel.
- Nodes perform capture, timestamping, framing, bounded buffering and export. Signal processing, feature extraction, fusion and inference run at one logical central processor.
- The processor may be local or in the cloud. Capture-time device timestamps remain authoritative for signal processing; cloud arrival time is an additional transport measurement.
- The sensing links form a mesh. Measurement export can still use a star through an ordinary AP, wired gateway or direct connections. Multi-hop mesh routing and distributed inference are not prerequisites.

Keep deployment hardware open. A homogeneous ESP32 setup can isolate topology effects; a richer receiver configuration is a separate complete-system comparison. Do not attribute hardware/antenna/band changes to topology alone.

## Current implementation constraints

The implementation plan uses one Rust workspace with TX/RX/TX+RX firmware entrypoints and a Linux collector: [rust-workspace.md](rust-workspace.md). Sensing TX is embedded; RX can be embedded `no_std` or Linux `std`. All scientific processing runs on a Linux OS, locally or in the cloud. In option 2 the central computer may also contribute supported CSI reception; Linux sensing transmission is optional rather than a prerequisite. This narrows the earlier generic processor wording without selecting a topology.

## Research-backed expectation for option 2

A subagent reviewed the historical evidence against the proposed architecture on 2026-10-19. **Inference:** extra edge-to-edge paths may improve sensitivity where the central receiver's paths observe a fall weakly. Expect the largest gain in worst-position coverage, not an automatic improvement everywhere. Correlated/noisy paths may add little; a selected subset may preserve better rates than full mesh. More receivers do not provide independent cross-node phase without additional synchronization evidence.

FreeFall's separately evaluated receiver positions demonstrate placement sensitivity, not a fusion win; see [the historical evidence mapping](evidence-to-design.md#3-placement-is-already-known-to-matter). FallAware uses independent receiver systems for coverage and identifies receiver fusion as future work (Sections VI and VIII). Its 3,000 versus 1,000 Hz ablation also found lower-rate degradation varied by environment (Section VII-B). Neither result establishes that mesh diversity offsets reduced sampling. [FallAware primary paper](https://www.researchgate.net/publication/388348926_FallAware_An_Explainable_Learning_Approach_to_Robust_Fall_Detection_With_WiFi).

Keep compatible central rich-RX streams in option 2 when possible, then test whether edge measurements add value. A rich Linux receiver versus original ESP32 receivers changes antenna/layout quality as well as topology; use homogeneous hardware for the topology control and label the richer-hardware comparison separately. Real wireless export can erase a fusion advantage visible in recorded-input tests, so both information and actual system comparisons below remain necessary.

## Conditions to compare

### Additional direct mesh evidence found 2026-10-19

- [Less is More, v3 preprint](https://arxiv.org/html/2602.10823v3): nine ESP32-C3 nodes, 72 directed links, centralized processing. Single-link AUC 0.541 versus full-mesh 0.489 for occupancy, both near chance; not fall detection or proof all fusion fails. Concatenation and burst scheduling limit interpretation. [Companion code](https://github.com/brunobastosrodrigues/less-is-more-wifi-sensing) exists; not audited/built here. Its timing defaults differ from the paper, so distinguish packet rate, burst gaps and cycle rate.
- [OpenCSI preprint](https://arxiv.org/html/2607.26665v1), Section IV-A: eight ESP32 nodes, 56 directed links, UDP collection at a central host; binary occupancy evaluation. Direct feasibility precedent, without a fall-detection or star-versus-mesh result.
- [Device-Free RF Human Body Fall Detection and Localization in Industrial Workplaces](https://www.researchgate.net/publication/309688893_Device-Free_RF_Human_Body_Fall_Detection_and_Localization_in_Industrial_Workplaces): experimentally tested a 12-node full mesh for falls using IEEE 802.15.4 RSSI. This is relevant adjacent evidence, not Wi-Fi CSI validation.

### Sampling correction: listening does not itself divide probe airtime

Under ideal broadcast capture, adding listeners to an existing transmitter consumes no extra sensing-probe transmissions. For four nodes A/B/C/D and a hypothetical 1,000 probes/s total, A/B/C can each send about 333/s to D. If those same A/B/C also capture one another during their listening turns, the same probes support nine directed streams at about 333/s each, rather than three. D need not become a sensing TX. Equal turns for all four would instead yield twelve directed streams at 250/s each. These counts assume mutual reception and unchanged packet airtime, and exclude export/control/retry costs.

Therefore, compare both **enabling additional receivers on the existing probe schedule** and **allocating probe airtime to additional transmitters**. Wireless export and half-duplex export interruptions can reduce usable sampling in either case. A claim that mesh inherently divides sampling by its directed-link count would be incorrect: one broadcast can generate several receivers' CSI measurements.

| Condition | Sensing arrangement | Purpose |
| --- | --- | --- |
| High-rate link | One TX–RX link with a larger temporal sampling budget | Establish the benefit of temporal detail on one view |
| Star | One TX with several RX, or several TX with one RX; record which | Establish a constrained multi-view baseline |
| Scheduled full mesh | Every node takes TX turns; other nodes capture usable direct links | Test wider path diversity at lower per-directed-link sampling |
| Selected mesh | Only a chosen subset of transmitters/links is used | Locate a better tradeoff than either full mesh or star |
| Adaptive mesh (added 2026-10-20) | All transmitters at a background rate; the hub boosts those whose links cross the tracked person | Test whether attention by geometry matches uniform high-rate sampling at lower airtime |
| Tier ablation (added 2026-10-20) | Floor tier only, upper tier only, both tiers with and without cross-tier links | Measure what each tier contributes to lying-on-floor detection and fall vs fast-sit separation |

Use the same node positions and radio hardware for topology-isolation experiments where possible. State the complete hardware/installation cost in system-level comparisons. A disabled sensing role does not require physically removing its node and changing the room geometry.

For four mutually reachable nodes, a star has three node-pair geometries; the full mesh has up to six geometries and twelve directed measurement streams. Reverse directions share endpoints and are not twelve independent spatial views. Raw phase across independently clocked nodes is not automatically coherent.

Illustrative ideal arithmetic only: under a 1,000-transmission/s total probe budget, one broadcast TX could supply each of three RX with 1,000 observations/s. Four equal-turn TX nodes could supply each directed link with 250 observations/s. Both yield up to 3,000 receiver observations/s in total, distributed differently. This assumes every listening node receives every probe, equal packet airtime and no export/management overhead. It is not a promised device rate or a complete network budget.

### Adaptive schedule (added 2026-10-20)

Because one broadcast probe gives a measurement at every listening node, the schedule unit is a transmitter, not a link. Proposed starting point, to be measured:

- **Background:** every transmitter at about 10 Hz, enough for tomography of a walking person (crossing a link's ~0.8 m sensitive zone at 1 m/s takes under a second). 24 nodes cost about 240 probes/s.
- **Boost:** the transmitters whose links cross the tracked person at about 200–300 Hz, covering fall dynamics at 2.4 GHz (≥100 Hz for the torso, ~160 Hz with limbs; ~200–300 Hz to match the FallDeFi rate ablation scaled from 5.2 GHz). Four boosted transmitters add about 1,200 probes/s.
- **Budget:** about 1,440 probes/s in total, under a rough 1–2k/s practical estimate and the ~4,600/s airtime bound for short HT20 probes. 1,000 probes/s is about 12% of a channel that facility Wi-Fi may share; keep listen-before-talk on.
- **Control:** the spatial stage sends schedule requests; the hub enforces the budget and distributes them, effective at a stated cycle number ([sensing plan](../sensing/PLAN.md), control path). The boost follows the tracked person, so it is normally in place before a fall starts; export batches ≤100 ms keep it current.

## Two complementary comparisons

**Information comparison:** collect simultaneous multi-link observations, then compare star subsets, selected subsets and all available links from the same physical events at matched input sampling. Include node-count subsets (for example 24, 16, 12, 8 nodes) and tier subsets to find the minimum installation that keeps held-out performance. This tests whether additional views add predictive information. Choose links/hyperparameters using training/validation data only.

**System comparison:** actually run star, selected-mesh and full-mesh transmission schedules under the same total airtime budget, including management, retries and CSI export when they share the channel. This measures the real tradeoff after contention, half-duplex operation and backpressure. Discarding links offline does not reclaim their airtime, so offline subsets alone cannot answer this question.

Include an export-batching sweep (proposed 20/100/250/500 ms) while preserving per-observation capture timestamps and native CSI. The team accepts roughly 500 ms accumulation if useful; enforce earlier memory/size flushes and stagger/pace exports across nodes. Hold batching policy fixed in topology comparisons, then compare policies separately. Check whether fewer packets reduce gaps or whether larger export bursts worsen contention. Delivery twice per second does not mean sensing at 2 Hz. [Batching implementation contract](rust-workspace.md#csi-export-batching-configurable-initially-500-ms).

Also sweep the probe budget to plot performance against achieved per-link rate and network cost. Packet counts alone are not equal airtime when PHY, size or retransmission behavior changes. If equal-rate-per-link comparisons require different total airtime, report that as a separate condition rather than calling both budgets matched.

## Data contract and controls

Each exported measurement needs receiver/node ID, observed transmitter ID, sender sequence when available, capture timestamp/clock epoch, session/layout ID, PHY/channel/bandwidth, antenna/subcarrier map, raw complex CSI or native bytes plus scaling, validity flags and quality counters. Distinguish sender attempts, received CSI callbacks, node queue drops, exported records and processor decode failures.

Use capture timestamps and measured clock alignment for cross-node feature windows. Do not silently interpolate across outages or treat a missing link as no movement. Preserve raw data and source/configuration provenance. Use bounded queues and expose incomplete recordings when export falls behind.

Hold carrier frequency, bandwidth, frame type, power, packet size and model context duration fixed for the topology-isolation comparison. Include a declared fusion method and missing-link mask. The primary fusion is the spatial stage (a 3D tomography forward model with fade-level weights and quiet links as negative evidence) plus fall dynamics taken only from links crossing the person; compare it against the best single link, late voting and feature concatenation. "Less is More" found its concatenated mesh no better than one link, so concatenation is a control, not a candidate. Collect the same fall/ordinary-activity protocol across room positions, then hold out people, days, rooms and furniture layouts as specified in [generalization.md](generalization.md). One human fall observed by twelve streams remains one event.

## Measurements and decision rule

Primary result: **event sensitivity and per-position coverage at the same validation-selected false-alert budget**, with event counts and uncertainty. Report continuous false alerts per observed room-day and occupied hour, observation-to-alert latency, intended-time sensing availability, accepted samples/s and gaps per link, node/export losses, airtime/backhaul usage and compute/memory cost.

Inspect whether improvements are concentrated at previously weak bedside/chair/floor/corner positions. Include hard non-falls, adjacent-room motion and a missing/restarted node. Test quiet occupancy and multi-person conditions separately from the core single-person fall result.

Keep the mesh only if its held-out performance or coverage benefit survives the actual transmission/export schedule at an acceptable resource and installation cost. Choose the smallest node count and tier layout that matches the full mesh. If the high-rate star/link wins, retain that architecture and update the build plan. This experiment is intended to decide, not assume, the outcome.

## Local versus cloud processor

Use identical recorded inputs and the same DSP/model pipeline to compare processor placement separately from topology. Measure capture-to-decision latency, jitter, upload bandwidth, backlog, reconnect/replay behavior and unavailable decision time during network loss. Distinguish historical analysis of recovered buffered data from timely live alerts. Cloud compute may be viable; its suitability remains an experiment rather than a requirement for or against this sensing layout.

## Implementation boundary

This is compatible with the [Rust acquisition/Python DSP boundary](software-boundary.md): Rust firmware/acquisition delivers identifiable CSI batches and timing/quality metadata; Python performs signal processing and fusion. The [esp-csi-rs verification](esp-csi-rs-verification.md) establishes successful software builds for original ESP32/S3, not live mesh throughput. Prove a direct hardware link before running the multi-node experiment.
