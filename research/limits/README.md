# What limits Wi-Fi CSI sensing, and what can be engineered

2026-10-20. Synthesis of four parallel literature reviews (subagents with web access and the local research files). Numbers are author-reported or derived from first principles; nothing here was measured by the team. Estimates are labelled as such in the underlying reports.

| File | What it answers |
| --- | --- |
| [radar.md](radar.md) | How radar (FMCW mmWave, UWB, CW Doppler, MIT SDR systems, Wi-Fi passive radar) gets its capability, and what blocks Wi-Fi CSI from following each approach. |
| [rf-networks.md](rf-networks.md) | Networked non-radar RF sensing (RSS radio tomography, CSI meshes, UWB CIR, Bluetooth Channel Sounding, FTM, 802.11bf) and lessons for an ESP32 mesh. |
| [first-principles.md](first-principles.md) | Derived bounds: dynamic-to-static power, resolution, Fresnel zones, degrees of freedom, impairments, SNR, airtime, room dependence, plus a paper-audit checklist. |
| [paper-audit.md](paper-audit.md) | 26 sources classified by binding limitation, with every ablation that directly measures a limit. |

## Bottom line

ESP32 systems fall short mainly because of **too few data points, poor geometry and the wrong statistic**, all engineerable. What remains even on the best hardware is **domain shift** (new room, new people, moved furniture), which is a data and evaluation problem. Bandwidth and antenna aperture are fixed by physics at 2.4 GHz, but they only bind for multi-person separation, pose and heart rate.

## What radar and RF networks have that ESP32 CSI lacks

1. **A shared clock between transmitter and receiver.** Radar keeps phase coherent across measurements, so it sees Doppler sign, breathing and people standing still. A 2.4 GHz single-chip CW radar measured heart rate in 2004 ([Droitcour](https://doi.org/10.1109/tmtt.2004.823552)), so the band is not the problem. ESP32 phase is effectively random per packet. Fixes: share a 40 MHz crystal between ESP32s ([ESPARGOS](https://arxiv.org/html/2502.09405)), divide CSI from two coherent receivers on one chip (FarSense ratio), or round-trip CSI ([ML-Track](https://discovery.ucl.ac.uk/id/eprint/10203112/1/ML_Track%20Paper_accepted.pdf): phase error 0.96 to 0.04 rad with single-antenna radios).
2. **Bandwidth for range bins.** Radar sweeps 1.4–7 GHz and resolves centimetres. 20 MHz Wi-Fi resolves about 17 m of path length; the whole 2.4 GHz band about 3.6 m. Fundamental for ESP32. Hardware routes: AX210 at 160 MHz (about 1.9 m), UWB at 500 MHz (DLR: 10 nodes, 45 links resolved into 123 multipath components, 80% of errors under 0.6 m).
3. **Density plus a physical forward model.** Successful RSS tomography used 24–34 nodes about 1 m apart (276–870 links) and a link-to-floor model with quiet links as negative evidence. Ghostline had 21 node pairs. "Less is More" concatenated 792 features into a classifier at 1.4 Hz per link, which explains its mesh doing no better than one link.
4. **A fast, regular sampling schedule.** Ghostline used about 1% of the channel's airtime (bound about 4,600 probes/s per channel). The team's 71 frames/s matches the 921,600-baud serial ceiling. Firmware, not physics.

## What the papers were actually limited by

- **Falls:** domain shift is the largest measured drop (ENetFall 96.8% to 69.0% in a new room with new people; RT-Fall sensitivity 89% to 57% with furniture blocking the main path; FallDeFi false positives 14.9% to 27.9% in a later session). Coverage next: FreeFall detection ranged 77.7–98.4% by receiver alone in one room.
- **Sample rate:** FallDeFi held steady down to 600 Hz at 5.2 GHz; scaled to 2.4 GHz that is roughly 200–300 Hz per link (estimate). Physics: at least 100 Hz for the torso, about 160 Hz with limbs. Every ESP32 mesh in the set ran at 20 Hz per link or less.
- **Lying still:** untested anywhere. Doppler features cannot see it (SHARP says so explicitly). OpenCSI reached F1 ≥ 0.959 for static occupancy at 20 Hz with per-link quiet baselines. [Mager et al. 2013](https://span.ece.utah.edu/uploads/mager-pimrc2013.pdf) (nodes at 17 and 140 cm, cross-level links essential) detected 100% of falls with no false alarms, on 2 subjects.
- **Pose needs resolution:** GoPose went from 4.7 cm to about 19 cm with 1 subcarrier instead of 30, and to 8 cm with 1 receiver instead of 4. Not reachable on single-antenna ESP32.
- **Vitals need coherence:** FarSense at 5 m detected breathing 100% of the time with the antenna ratio versus 1.4% with amplitude alone.
- **Thermal SNR is not binding for falls or breathing.** At 4 m and 20 dBm, in-room RSSI should be −35 to −50 dBm. The team's −70 to −80 dBm suggests 20–40 dB of avoidable loss.

## Design changes this research implies

- **Gain control on original ESP32.** The [`esp_csi_gain_ctrl` README](https://components.espressif.com/components/espressif/esp_csi_gain_ctrl/versions/0.1.5/readme) lists the original ESP32 as supported; only the `csi_recv_router` example leaves it disabled. Test before buying. Forcing gain can cause packet loss.
- **Second radio per node: same channel and shared crystal, not a different channel.** One HT20 frame already spans about 2 independent fades and HT40 about 4–5 (estimate from 22–31 ns measured indoor delay spread). The tenfold multi-channel gain was measured on narrowband RSS and has not been repeated with CSI. A coherent pair unlocks Doppler sign, breathing and stationary detection. ESPARGOS found the ESP32 PLL phase re-randomises after every reset or channel change, so this needs a fixed channel and a bench test.
- **Node count and height.** 16–24 nodes for a 20–30 m² room. Floor tier about 0.3 m; consider ceiling corners (about 2–2.4 m) for the upper tier, which projects about twice the vertical fall velocity onto each link compared with 1.2 m.
- **Adaptive airtime.** 24 nodes at 10 Hz each for tomography is 240 probes/s; boosting the 4 transmitters nearest the person to 300 Hz adds 1,200/s; about 1,440/s total, under a rough 1–2k/s practical estimate. 1,000 probes/s is about 12% of a channel that facility Wi-Fi may share.
- **Statistics.** Per-subcarrier attenuation against a gain-compensated baseline for stationary people, variance or Doppler for motion, a 3D forward model with fade-level weights and quiet-link negative evidence, and a state machine that holds a person who stops moving.

## Cheapest experiments first

1. Binary export over USB or Ethernet; capture fall-like motion at 300 Hz or more on one link; downsample offline to 200, 120, 70 and 20 Hz. Not done on ESP32 at 2.4 GHz by any paper in the set.
2. Read and force receiver gain on original ESP32; check that a person standing still on the line of sight gives stable attenuation.
3. Explain the −70 to −80 dBm in-room RSSI.
4. Two ESP32s on one 40 MHz crystal, same channel: is their phase difference stable?
5. Floor, 1 m and ceiling links for lying versus standing.
6. On one recording: best single link vs late fusion vs the tomography model.
