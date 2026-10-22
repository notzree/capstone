# Node distribution: multiple lightweight TX, one CSI RX, central computation

2026-10-19. Analysis of the team's proposed topology, using Sections 7–8 and 14G of the current [Drive research](https://docs.google.com/document/d/1LvhWgVKfX9y7fNDREqT2WzxCtPXiJItZiGMJGuQFGZ8/edit?tab=t.0), plus primary capture documentation. Proposed architectures and experiment criteria below are not measured project results.


Implementation: [rust-workspace.md](rust-workspace.md) defines shared TX/RX/TX+RX role logic, embedded firmware entrypoints and the Linux collector. Scientific processing stays on Linux; an embedded receiver captures and exports CSI without doing DSP/ML.

## Assessment

**Multiple ESP32 transmitters → one richer CSI receiver → Linux processing is a sensible candidate, especially within one room.** It minimizes distributed CSI export, centralizes received timestamps and permits multiple receive-chain measurements at the expensive receiver. Existing evidence does not establish it as the most accurate fall topology or sufficient for a whole suite through walls.

Separate three decisions: where the radio measures CSI, how measurements reach the computer, and where signal processing runs. Centralized computation is compatible with all architectures below. A distributed receiver can remain a lightweight capture/export node.

## What “router → Linux RX” must mean

Two valid acquisition arrangements:

```text
ESP32 transmitters --Wi-Fi--> CSI-capable router --CSI records/Ethernet--> Linux DSP
                             actual sensing RX                        compute host

ESP32 transmitters --Wi-Fi--> Linux host's CSI-capable NIC --> local DSP
                             actual sensing RX
```

In the first arrangement, the router's radio/firmware extracts CSI. The Linux machine receives measurement records; it is not the RF sensing endpoint. In the second, the Linux NIC extracts CSI directly, and a router may be unnecessary or serve only an association/management role.

An ordinary router forwarding payload packets does not export CSI automatically. If the Linux NIC measures only an AP's retransmitted downlink, it observes the AP→Linux radio channel, not the original ESP32→AP channel—even if the IP packet originated on the ESP32. A monitor NIC that directly overhears the ESP32's uplink is a different, potentially valid measurement. Verify the over-air transmitter identity and measured hop explicitly.

[Nexmon CSI](https://github.com/seemoo-lab/nexmon_csi) documents specific chip/firmware pairs, per-core/stream reports, source filtering and UDP export. Stock OpenWrt support is not equivalent to that capability. [FeitCSI's record format](https://feitcsi.kuskosoft.com/csi_format/) includes RX/TX dimensions, subcarrier count, source MAC, receive timestamp and PHY flags; such fields support separating incoming sources, but actual ESP32 compatibility still requires a capture test.

## Why the proposed star is attractive

- Only the central radio requires the richer extraction toolchain. Lightweight transmitters need scheduled identifiable traffic, not a signal-processing pipeline.
- Different transmitter positions create multiple propagation paths through the room. Preserve them as separate links rather than mixing samples into one stream.
- All arrivals use one receiver clock, simplifying temporal feature alignment. Independent transmitter oscillators still prevent treating their raw phase as a coherent transmitting array.
- A multi-chain receiver can measure several receive-antenna channels for each single-stream ESP32 transmitter. Multiple ESP32 boxes do not become simultaneous MIMO transmit streams; preserve that distinction in the tensor schema.
- CSI output can reach Linux by Ethernet rather than requiring every peripheral node to stream raw CSI over Wi-Fi.

## Constraints that could determine the outcome

**Geometry.** All links terminate at one physical location. Several transmitter positions can improve coverage, but a weakly perturbed corner/bed/floor location may remain poorly observed. A strong RSSI or a high packet count establishes communication, not fall observability. Reversing the same star's TX/RX roles does not automatically improve its geometry; the endpoint locations are unchanged. Hardware quality, traffic fanout and radio behavior can nevertheless favor one direction.

**Band and streams.** Original ESP32 transmitters are 2.4 GHz, 802.11n, 20/40 MHz devices with one radio chain. A richer receiver cannot convert their traffic into the 5.2 GHz/80 MHz measurements of another paper, nor add missing transmit streams. The negative ESP32-pair results in ENetFall do not isolate transmitter versus receiver limitations, so they do not by themselves disqualify an ESP32 TX plus richer RX. Compare actual captures. [Chu et al., Section II](https://eprints.whiterose.ac.uk/id/eprint/202278/8/Deep_Learning_Based_Fall_Detection_Using_WiFi_Channel_State_Information.pdf).

**Per-link sampling.** Three transmitters requesting 1,000 packets/s each demand about 3,000 transmissions/s before other traffic, retries and export overhead. An aggregate received rate of 900/s split evenly among three sources is only 300/s per link. Different links may have different loss/jitter. Schedule/stagger controlled traffic, fix the channel/PHY and record sender counters and receive times. Planned time slots are not a guarantee against outside interference.

**Channel and frame compatibility.** A single fixed-channel radio cannot monitor several independent channels simultaneously. Verify ESP-NOW versus ordinary associated 802.11 traffic, HT mode, source attribution and the extractor's supported packet formats. Successful transmission by the Rust library does not prove a Linux/router receiver extracts CSI from that frame type.

**Failure scope.** The central receiver can become a common failure point. A stopped source must appear as a missing link; do not normalize it into a quiet/empty room. Multiple receivers introduce their own clocks and export links, so redundancy requires an explicit health/fusion policy.

## Research precedents and their actual implications

| Source | Evaluated sensing arrangement | What it supports for this decision |
| --- | --- | --- |
| ENetFall [Drive R5] | One controlled Intel 5300 TX, one three-antenna Intel 5300 RX, host-based classification | Central richer reception and host compute are established research choices. It does not validate multiple original ESP32 transmitters or the same accuracy on them. |
| FreeFall [Drive §14G] | One TX, three physically separate RX locations; links scored separately | Receiver position materially changes coverage. One kitchen-position example yields about 70% event detection followed by 90% classification, roughly 63% combined. No demonstrated multi-link fusion gain or universal superiority over a reversed star. |
| FallAware [Drive R6] | A TX can serve multiple RX, each TX–RX pair treated independently | Distributed reception is used to extend coverage. The paper explicitly says there is no information exchange between the RX systems; a fusion gain would be our experiment. [Section VI-A](https://www.researchgate.net/publication/388348926_FallAware_An_Explainable_Learning_Approach_to_Robust_Fall_Detection_With_WiFi). |
| TCD-FERN [Drive R9] | One TX/one RX for two rooms, or one TX/two RX for three rooms | A concrete distributed-reception precedent for room presence, not fall validation or resident identity. |
| Wi-CaL [Drive R11] | Four paired ESP32 links, eight nodes, crossing a room | Evidence for multi-link counting/quadrant context. It is moving-crowd evaluation, not evidence of fall accuracy or stationary counting. [Paper](https://doi.org/10.1109/ACCESS.2022.3155812). |

## Alternatives worth comparing

The team also proposes every node supporting TX and RX, with CSI forwarded to a central local/cloud processor. [experiment-sensing-mesh.md](experiment-sensing-mesh.md) specifies this as a test of temporal sampling versus spatial diversity. It is an additional candidate, not an adopted final topology.

| Architecture | Sensing nodes | Principal advantage | Principal cost |
| --- | --- | --- | --- |
| **A. Proposed TX star** | 3 TX →1 richer RX | Cheap peripheral nodes; one CSI stack/clock/backhaul | All paths share a receiver location; separate transmissions consume airtime |
| **B. RX star** | 1 TX →3 RX →Linux | A broadcast can be observed by several receivers; spatially separated measurements | Each RX needs supported capture/export and clock alignment; cheap single-chain RX may lose the richer receiver's information |
| **C. Room-local links** | TX/RX inside each priority room →Linux | Local coverage, installation checks and fault isolation per room | More devices/power/backhaul; adjacent-room motion still needs testing |
| **D. Small hybrid** | 2 TX ×2 RX →Linux | Up to four distinct links and fewer paths tied to one endpoint | Two CSI capture locations and more acquisition complexity; four usable links are not guaranteed |

All four can run every DSP/model computation on one Linux machine. More RX does not require training or inference on each RX. A radio needing a Linux PCIe host does impose host hardware at that location, whereas an ESP32 receiver can export directly; count this complete-system cost.

For one room, compare the star/hybrid baselines with the scheduled full/selected mesh in the experiment above after one-link capture works. For a bedroom/bathroom or several rooms, also compare room-local links. B is useful as a research control and can be attractive when good, inexpensive receiver nodes exist. Keep model/measurement quality and actual airtime budgets explicit across these comparisons.

## Experiment to select the finalized topology

1. Prove one original-ESP32 transmitter→candidate rich receiver link. Measure the real PHY, layout, source identity, accepted rate, gaps and empty/motion behavior. Keep richer compatible transmitters as an alternative if the original radio restricts the selected representation too much.
2. Map a single room's bed/chair/floor/corners and ordinary confusers. Add transmitters at positions that create complementary paths, then add a second receiver only as a separately scored topology condition.
3. Compare A/B/D at a stated node/installation budget. Report both same accepted samples/link and same airtime-budget views where feasible; broadcast fanout means those budgets need not be identical. Disclose changed receiver hardware so a comparison is not mislabeled as geometry alone.
4. Keep participant/session/room splits fixed across model comparisons, and score every physical event once even when several links observe it. Compare per-position recall at the same false-alert budget, calibration effort, latency, availability and sustained capture stability.
5. Repeat with a closed-door second room, moved furniture, adjacent-room motion and a later day. If a central receiver only works on strong through-path locations, select room-local or hybrid reception rather than asking a CNN to infer absent measurements.

**Proposed decision:** keep central Linux computation. Treat the TX star as a strong candidate, while leaving the capture interface and final installation open to a second receiver or room-local links. Hardware proof and the coverage experiment should select the sensing topology.
