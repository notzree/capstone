# Radar approaches versus Wi-Fi CSI

2026-10-20. Literature review by a research subagent (web search, primary papers, datasheets, TI app notes). Wi-Fi resolution numbers, the 0.65 s window and the ~50 dB processing-gain figure are the subagent's calculations, not measured values. Gains and costs in section 4 are estimates unless cited. Index: [README.md](README.md).

The single biggest gap between radar and Wi-Fi CSI is a shared clock between transmitter and receiver. Bandwidth, antenna count, packet rate and geometry follow. Of these, clock coherence and packet rate can be engineered around on ESP32s, and angle can be partly recovered. Bandwidth can only be partly recovered, and pose-level imaging is not realistic.

## 1. Summary

Ranked by how much each matters for falls, a person lying still, localization and counting:

1. **One oscillator drives both the transmit and receive sides.** Radar phase stays coherent from chirp to chirp, so Doppler, breathing and stationary people are all visible. A 2.4 GHz single-chip CW radar measured heart rate at 1 m and respiration at 2 m ([Droitcour 2004](https://doi.org/10.1109/tmtt.2004.823552)), so 2.4 GHz itself is not the problem. Separate Wi-Fi devices have carrier frequency offset (CFO), sampling frequency offset (SFO) and packet-detection delay, which randomise phase on every packet. A single-antenna ESP32 cannot even form the cross-antenna ratio that cancels these. This is why Ghostline loses standing people within seconds and why "lying on the floor after a fall" is hard.
2. **Bandwidth gives range gating.** Radar sweeps 1.4–7 GHz and gets 2–10 cm range bins. Wi-Fi channels are 20–160 MHz, which is about 18 m to 1.9 m of path-length resolution. Without range bins you cannot separate a person from the wall behind them, separate two people, or measure height.
3. **Coherent antenna aperture gives angle.** Radar uses 12 to 64+ virtual elements and gets 5–15° resolution. ESP32 has 1 antenna, AX210 has 2, Nexmon routers have 4.
4. **A controlled, uniform, fast waveform.** Radar chirps at kHz rates on a fixed schedule. CSI arrives irregularly, at whatever rate the traffic allows (Ghostline: at most 8 frames/s per transmitter). That aliases any fall-speed Doppler.
5. **Geometry and dynamic range.** A monostatic radar range-gates out the wall before integrating. A bistatic Wi-Fi link is dominated by the line-of-sight path, has about 40 dB of usable dynamic range ([WiDeo](https://www.usenix.org/system/files/conference/nsdi15/nsdi15-paper-joshi.pdf)), and ESP32 reports CSI as int8 behind an automatic gain control (AGC).

## 2. Table

Key equations:

- Range resolution, monostatic: \(\Delta R=c/2B\). Bistatic CSI gives path-length resolution \(\Delta L=c/B\).
- Angular resolution: \(\Delta\theta\approx0.886\lambda/L\) for array length \(L\).
- Velocity resolution: \(\lambda/2T\) monostatic, or path-rate resolution \(\lambda/T\) for CSI.
- Maximum unambiguous path rate at packet rate \(F\): \(\lambda F/2\).

Categories: **a** = physics or regulation, **b** = fixable by buying different Wi-Fi hardware, **c** = fixable by system engineering, **d** = not worth fixing.

| Quantity | Radar value | Wi-Fi CSI value (ESP32 / AX210 / Nexmon) | Blocker | Cat. | Lever + citation |
|---|---|---|---|---|---|
| Bandwidth → range | TI IWR6843 fall design 1.78 GHz → 8.4 cm; WiTrack 1.69 GHz → 8.8 cm; X4 1.4 GHz; Soli 7 GHz → ~2 cm | HT20 (~16.6 MHz occupied) → ~18 m path; HT40 (114 subcarriers ≈ 35.6 MHz) → ~8.4 m / 160 MHz → ~1.9 m / 80 MHz → ~3.75 m | Channel width limit; SFO and packet-detection delay destroy absolute time of flight | a per channel; b; c by stitching | Chronos stitched 35 bands in 84 ms → 0.47 ns ToF error; Splicer cut ranging error 7.1 → 1.63 m |
| Aperture → angle | TI 3TX×4RX = 12 virtual, 14.5° azimuth; Vayyar 24TX/22RX, ~5.9°; RF-Capture 4TX×16RX | 1 / 2 / 4 receive chains | Too few elements; per-chain phase offsets (on new cards these don't cancel) | b, c | ESPARGOS (shared 40 MHz clock + phase reference, 8 elements/board); ArrayTrack 16 antennas; Phaser; SpotFi |
| Shared TX/RX oscillator | Same oscillator for TX and RX; range correlation suppresses phase noise | Independent oscillators → CFO, SFO, detection delay | Separate devices are not phase-coherent; ESP32 has no second chain for a ratio | c | CSI ratio (FarSense, 8 m breathing range); conjugate multiplication (Widar2.0); two-way reciprocity product (Chronos); monostatic AX211 |
| Repetition rate → max velocity | TI chirp period 89 µs → 4.58 m/s; Soli 1–10 kHz | Ghostline ≤8/s → <0.5 m/s path rate at 2.4 GHz | Contention-based access, low traffic, firmware caps | c | Scheduled injection. UCL measured busy-office APs at ~100 frames/s and off-hours at 4–5/s |
| Coherent time → velocity resolution | TI 25.7 ms → 0.095 m/s at 60 GHz | At 2.44 GHz, 0.65 s gives the same path-rate resolution | Only works if phase is coherent across packets; longer wavelength | a (λ), c | UCL passive radar: 0.5 s → 2 Hz, 5 s → 0.2 Hz Doppler resolution |
| Clutter rejection / dynamic range | Range gating + roughly 50 dB coherent processing gain (estimate for TI) | int8 CSI, AGC, line-of-sight dominates | Weak human reflections land near the quantization floor | c (partly a) | Fixed gain (esp_csi_gain_ctrl); directional antennas; MIMO nulling (Wi-Vi); analog cancellation (WiDeo) |
| Geometry | Monostatic, polar coverage from one box | Bistatic ellipses; sensitivity concentrated near the line of sight | No range per link, so "disturbed link" votes are only metre-scale | c | WiTrack2.0 intersects 25 ToF ellipses; Ghostline's node-to-node links raised tracked updates from 19% to 90% |
| Wavelength → displacement sensitivity | 60 GHz: 1 mm → 0.8π rad (Soli) | 2.4 GHz: 1 mm → ~0.1 rad (monostatic-equivalent) | Physics | a; d for heartbeat | 5–6 GHz (ESP32-C5 / AX210) gives about 2.2–2.6× more sensitivity |

## 3. Detail by approach

**FMCW mmWave radar (TI, Vayyar, Soli).**

- TI's IWR6843 fall-detection app note ([TI](https://www.ti.com.cn/cn/lit/an/zhcaa43/zhcaa43.pdf)):
  - Waveform: 3TX/4RX time-multiplexed MIMO (12 virtual elements), 1.78 GHz, 96 chirps × 3 transmitters, 14 Hz frames.
  - Result: 8.4 cm range resolution, 4.58 m/s maximum velocity, 0.095 m/s velocity resolution. It deliberately lengthened the frame to get finer velocity resolution.
  - Fall rule: the point-cloud height drops more than 0.25 m compared with 20 frames earlier. Sensor mounted at 2 m with a 30° downtilt.
- TI's tracking demo keeps tracking people "as they sit or lie down and remain stationary" ([TI guide](https://dev.ti.com/tirex/explore/content/radar_toolbox_4_00_00_05/source/ti/examples/Industrial_and_Personal_Electronics/People_Tracking/3D_People_Tracking/docs/3d_people_tracking_user_guide.html)). This is exactly the case Ghostline fails.
- Vayyar's 60 GHz fall sensor uses 24 Tx / 22 Rx antennas: 7.5 cm range resolution at 2 GHz and about 5.9° angular resolution from a 4.6 cm aperture ([FCC user guide](https://fcc.report/FCC-ID/2AHIS-V60G-HOME/4678774.pdf)).
- Soli uses 7 GHz of bandwidth (~2 cm) but relies mainly on 1–10 kHz repetition rates and phase. At 60 GHz, 1 mm of motion is 0.8π rad ([Soli](http://www.ivanpoupyrev.com/wp-content/uploads/2017/01/siggraph_final.pdf)).
- Fall signatures come from micro-Doppler time-frequency maps ([Amin 2016](https://doi.org/10.1109/msp.2015.2502784)). Adding range cuts false alarms, because fast sitting looks like a slow fall in Doppler alone ([Erol & Amin 2016](https://doi.org/10.1109/radar.2016.7485176)).

**Shaker group (supervisor).** FMCW radar work, not CW Doppler:

- 77 GHz AWR1443 vital signs, with 94% (breathing) and 80% (heart rate) correlation against a reference sensor ([Alizadeh et al. 2019](https://doi.org/10.1109/access.2019.2912956)).
- Long-term-care monitoring with mmWave MIMO FMCW and GRU models, including fall and fall-recovery detection ([IEEE Sensors 2022](https://doi.org/10.1109/sensors52175.2022.9967327), [URSI 2023](https://doi.org/10.46620/ursigass.2023.3686.dmdw1546)).

**UWB impulse radar.**

- Novelda X4: 7.29 or 8.748 GHz center, 1.4–1.5 GHz bandwidth, 23.3 GS/s direct RF sampling, 1536 range bins (~9.9 m), pulse rate up to 40.5 MHz ([datasheet](https://dev.novelda.com/X4F103/_static/x4_datasheet_RevA.pdf)).
- Walabot Developer: 18 antennas, 3.3–10 GHz in the US ([tech brief](https://cdn.sparkfun.com/assets/learn_tutorials/7/2/4/walabot-tech-brief-416.pdf)).
- Same enablers as FMCW: about 10 cm range bins and a coherent single-chip transceiver.

**CW / Doppler radar.** The [Droitcour](https://doi.org/10.1109/tmtt.2004.823552) result is the key one. A 2.4 GHz, 1 mW CMOS radar measured heart rate and respiration because the transmit and receive sides share one oscillator, which cancels phase noise at short range. An I/Q receiver avoided the demodulation nulls. The blocker for Wi-Fi vitals is coherence, not the band.

**MIT SDR systems.** All of these are custom FMCW front ends near 5.4–7.25 GHz with about 1.7–1.8 GHz of bandwidth and sub-mW power.

- [WiTrack](https://www.usenix.org/system/files/conference/nsdi14/nsdi14-paper-adib.pdf): 1 TX / 3 RX, 2.5 ms sweeps. 10–13 cm horizontal and 21 cm vertical error. Fall detection from elevation change gave 96.9% precision and 93.9% recall, on simulated falls in a padded room.
- [WiTrack2.0](https://witrack.csail.mit.edu/witrack2-paper.pdf): 5 TX × 5 RX, all on one external clock. 12.1 cm error for 4 moving people; 11.2 cm for up to 5 static people, located by their breathing over 10 s windows.
- [RF-Capture](https://people.csail.mit.edu/hongzi/content/publications/RFcapture-TOG.pdf): 4 TX + 16 RX in a 60×18 cm T-shaped array.
- [RF-Pose](https://openaccess.thecvf.com/content_cvpr_2018/papers/Zhao_Through-Wall_Human_Pose_CVPR_2018_paper.pdf) and [RF-Pose3D](https://people.csail.mit.edu/mingmin/papers/rfpose3d-sigcomm-zhao.pdf): about 10 cm depth and 15° angle resolution; 4–5 cm average keypoint error.
- [Wi-Vi](https://www.mit.edu/~fadel/papers/wivi-paper.pdf) is the exception: 20 MHz at 2.4 GHz, 2 clock-synchronized TX + 1 RX, MIMO nulling of static reflections, and inverse synthetic aperture tracking. It counts people and gives relative position but no fine localization.

**Wi-Fi passive radar.** These use Wi-Fi signals but radar processing:

- UCL ([real-time system](https://discovery.ucl.ac.uk/id/eprint/1540858/1/A%20Wireless%20Passive%20Radar%20System%20for%20Real%20Time%20Through-Wall%20Movement%20Detection.pdf)) uses two USRP N210s clocked together over a MIMO cable: a reference channel aimed at the access point and a surveillance channel aimed at the room. They cross-correlate the two (cross-ambiguity function), cancel the direct signal, and use 0.5–5 s integration windows.
- In the [stand-alone access point paper](https://doi.org/10.1109/tgrs.2020.3006387), indoor use was Doppler-only because Wi-Fi range resolution is too coarse. Beacons occupy only 0.42% of airtime, and real traffic was ~100 frames/s at peak and 4–5 frames/s off-hours.
- Sapienza (Colone, Lombardo; [TAES 2012](https://doi.org/10.1109/taes.2012.6178049), [Martelli thesis](https://iris.uniroma1.it/retrieve/e383531d-01a5-15e8-e053-a505fe0a3de9/Tesi_dottorato_Martelli.pdf)): range resolution 11–18 m; 0.25 s integration gives 4 Hz Doppler resolution; inverse synthetic aperture processing recovers cross-range resolution on moving targets.
- The lesson is that the reference channel supplies the coherence a CSI receiver lacks.

**Making commodity Wi-Fi radar-like.**

- *Stitching.* [Chronos](https://www.usenix.org/system/files/conference/nsdi16/nsdi16-paper-vasisht.pdf) hopped 35 bands (about 1 GHz combined, 2–3 ms per hop) on an Intel 5300. It cancelled the random phase from each re-tune by multiplying the A→B and B→A CSI. It measured device-to-device ToF, not reflections. [Splicer](https://www.cs.cityu.edu.hk/~zhenjili/2015-MobiCom-Splicer.pdf) did the same on Atheros cards.
- *Coherent arrays.*
  - [ArrayTrack](https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final51.pdf): 16 antennas on two WARP boards with shared oscillators, calibrated with a CW tone.
  - [Phaser](https://jon.thesquareplanet.com/papers/mobicom14-phaser.pdf): two NICs joined through a shared antenna and splitter, 20° median phase error.
  - [SpotFi](https://conferences.sigcomm.org/sigcomm/2015/pdf/papers/p269.pdf): joint angle and ToF estimation with 3 antennas, 40 cm error.
- *Ratio and conjugate tricks.* [FarSense](https://ar5iv.labs.arxiv.org/html/1907.03994) divides CSI from two antennas to cancel shared offsets, extending breathing range from 2–4 m to 8 m. [Widar2.0](https://tns.thss.tsinghua.edu.cn/~qiankun/files/MobiSys18_Widar2.0_paper.pdf) jointly estimates ToF, angle and Doppler on one link, fusing coarse 1 ns ToF with fine Doppler for 0.75 m median error.
- *Caveat for AX210 and Nexmon.* On new-generation cards, including the AX200, each antenna has its own RF front end, and the inter-antenna phase difference no longer fully cancels the offsets ([Yi et al. 2023](https://hal.science/hal-04783864/file/Enabling%2BWiFi%2BSensing%2Bon%2BNew-generation%2BWiFi%2BCards.pdf)).
- *Monostatic attempts.* [WiDeo](https://www.usenix.org/system/files/conference/nsdi15/nsdi15-paper-joshi.pdf) used WARP radios with full-duplex analog cancellation. A newer paper ([arXiv 2508.02799](https://arxiv.org/html/2508.02799v1)) did monostatic range-Doppler on a stock AX211 laptop card: 160 MHz at 6.3 GHz, using the TX-to-RX leakage as its phase reference. It got 5 cm range error for a hand within 40 cm, so long range is unproven.
- *ESP32 coherence.* [ESPARGOS](https://arxiv.org/html/2502.09405) builds a phase-coherent array from ESP32-family receivers, using a shared 40 MHz clock plus a distributed Wi-Fi phase reference. It found the ESP32's PLL phase changes after every reset or channel change. Four synchronized arrays with ceiling transmitters localized a passive human with 0.32 m mean error by triangulation ([arXiv 2504.09924](https://arxiv.org/abs/2504.09924)). The [manual](https://espargos.net/espargos-one/espargos-one-manual.pdf) has an optional radar transmit mode and notes bistatic works better than monostatic.
- *802.11bf-2025* (published Sept 2025) standardizes sensing measurement setup, scheduled NDP sounding and multistatic sensing ([IEEE](https://standards.ieee.org/ieee/802.11bf/11574/)). No evidence found of ESP32 support.

## 4. Ranked levers for an ESP32 multi-node system

Gains and costs are the subagent's estimates unless a citation is attached.

1. **Frequency-lock pairs of ESP32s to one 40 MHz crystal** (ESPARGOS-style; add the phase-reference injection only if absolute angle is needed).
   - *Gain:* largest. The two-chain CSI ratio makes phase usable, which brings back Doppler sign, breathing and stationary or lying-person detection (FarSense range). With calibration it also gives angle. ESPARGOS reports phase differences stay stable with frequency lock alone, so Doppler and breathing don't need the full phase calibration.
   - *Cost:* medium. Custom PCB or coax clock distribution. Commercial ESPARGOS units are EU-professional-only and were out of stock on 2026-10-20, so plan to build.
2. **Raise and regularize the packet rate.** Use slotted broadcasts at a fixed period; one broadcast feeds N−1 links at once.
   - *Gain:* high for falls. Matching TI's 4.58 m/s needs about 150 frames/s at 2.4 GHz and about 340 at 5.5 GHz. Uniform timing makes Doppler FFTs valid.
   - *Cost:* low, firmware only. Ghostline's 8/s looks like a firmware cap rather than an airtime limit.
3. **Fix the receiver gain and change antennas and geometry.** Lock or compensate AGC with [esp_csi_gain_ctrl](https://components.espressif.com/components/espressif/esp_csi_gain_ctrl/versions/0.1.5/readme). Use U.FL modules with directional or cross-polarized patches so the line of sight dominates less. Mount nodes high and looking down so height changes show up.
   - *Gain:* medium. Weak reflections move further above the int8 floor.
   - *Cost:* low.
4. **Two-way reciprocity products** (Chronos): multiply A→B and B→A CSI captured within a few ms of each other.
   - *Gain:* phase-stable links without shared clocks. Untested on ESP32, and it doubles the phase.
   - *Cost:* low to medium.
5. **Channel stitching for coarse ToF.**
   - *Gain:* moderate for localization and counting, low for falls. 2.4 GHz channels 1–13 span about 80 MHz (~3.75 m path resolution). ESP32-C5's 5 GHz span of about 700 MHz could reach sub-metre in theory.
   - *Cost:* high. Every hop resets PLL phase (ESPARGOS), so lever 4 or a phase reference is needed. ESP32 channel-switch time is undocumented.
6. **Hybrid anchor.** Add one AX210 node (160 MHz, monostatic as in the AX211 paper, ~0.93 m range bins), or one mmWave radar per room for falls.
   - *Gain:* high. *Cost:* goes against "cheap ESP32" but is the lowest-risk route to reliable fall detection.
7. **Not worth chasing (category d):** skeletons or pose (RF-Pose needs ~1.8 GHz plus 2D arrays), heart rate on bistatic ESP32 links (heartbeat is 0.1–0.5 mm of chest motion, about 0.01–0.05 rad at 2.4 GHz), and waiting for 802.11bf on ESP32.

## 5. Sources

- TI: [IWR6843 fall detection app note](https://www.ti.com.cn/cn/lit/an/zhcaa43/zhcaa43.pdf); [3D people tracking guide](https://dev.ti.com/tirex/explore/content/radar_toolbox_4_00_00_05/source/ti/examples/Industrial_and_Personal_Electronics/People_Tracking/3D_People_Tracking/docs/3d_people_tracking_user_guide.html); [vital signs lab](https://e2e.ti.com/cfs-file/__key/communityserver-discussions-components-files/1023/vitalSigns_5F00_lab_5F00_user_5F00_guide_5F00_v1.2UPDATE.pdf)
- Vayyar: [FCC user guide](https://fcc.report/FCC-ID/2AHIS-V60G-HOME/4678774.pdf); Soli: [SIGGRAPH 2016](http://www.ivanpoupyrev.com/wp-content/uploads/2017/01/siggraph_final.pdf)
- UWB: [Novelda X4 datasheet](https://dev.novelda.com/X4F103/_static/x4_datasheet_RevA.pdf); [Walabot tech brief](https://cdn.sparkfun.com/assets/learn_tutorials/7/2/4/walabot-tech-brief-416.pdf)
- Fall radar: [Amin 2016](https://doi.org/10.1109/msp.2015.2502784); [Erol & Amin 2016](https://doi.org/10.1109/radar.2016.7485176)
- CW Doppler: [Droitcour 2004](https://doi.org/10.1109/tmtt.2004.823552)
- Shaker group: [Alizadeh 2019](https://www.weizmann.ac.il/math/yonina/sites/math.yonina/files/Remote_Monitoring_of_Human_Vital_Signs_Using_mm-Wave_FMCW_Radar.pdf); [LTC monitoring 2022](https://doi.org/10.1109/sensors52175.2022.9967327); [URSI 2023](https://doi.org/10.46620/ursigass.2023.3686.dmdw1546)
- MIT: [WiTrack](https://www.usenix.org/system/files/conference/nsdi14/nsdi14-paper-adib.pdf); [WiTrack2.0](https://witrack.csail.mit.edu/witrack2-paper.pdf); [RF-Capture](https://people.csail.mit.edu/hongzi/content/publications/RFcapture-TOG.pdf); [RF-Pose](https://openaccess.thecvf.com/content_cvpr_2018/papers/Zhao_Through-Wall_Human_Pose_CVPR_2018_paper.pdf); [RF-Pose3D](https://people.csail.mit.edu/mingmin/papers/rfpose3d-sigcomm-zhao.pdf); [Wi-Vi](https://www.mit.edu/~fadel/papers/wivi-paper.pdf)
- Passive Wi-Fi radar: [UCL real-time](https://discovery.ucl.ac.uk/id/eprint/1540858/1/A%20Wireless%20Passive%20Radar%20System%20for%20Real%20Time%20Through-Wall%20Movement%20Detection.pdf); [UCL stand-alone AP](https://doi.org/10.1109/tgrs.2020.3006387); [Colone/Lombardo TAES 2012](https://doi.org/10.1109/taes.2012.6178049); [Martelli thesis](https://iris.uniroma1.it/retrieve/e383531d-01a5-15e8-e053-a505fe0a3de9/Tesi_dottorato_Martelli.pdf)
- Commodity Wi-Fi: [Chronos](https://www.usenix.org/system/files/conference/nsdi16/nsdi16-paper-vasisht.pdf); [Splicer](https://www.cs.cityu.edu.hk/~zhenjili/2015-MobiCom-Splicer.pdf); [ArrayTrack](https://www.usenix.org/system/files/conference/nsdi13/nsdi13-final51.pdf); [Phaser](https://jon.thesquareplanet.com/papers/mobicom14-phaser.pdf); [SpotFi](https://conferences.sigcomm.org/sigcomm/2015/pdf/papers/p269.pdf); [FarSense](https://ar5iv.labs.arxiv.org/html/1907.03994); [Widar2.0](https://tns.thss.tsinghua.edu.cn/~qiankun/files/MobiSys18_Widar2.0_paper.pdf); [WiDeo](https://www.usenix.org/system/files/conference/nsdi15/nsdi15-paper-joshi.pdf); [AX211 monostatic](https://arxiv.org/html/2508.02799v1); [new-gen cards](https://hal.science/hal-04783864/file/Enabling%2BWiFi%2BSensing%2Bon%2BNew-generation%2BWiFi%2BCards.pdf)
- ESPARGOS: [paper](https://arxiv.org/html/2502.09405); [manual](https://espargos.net/espargos-one/espargos-one-manual.pdf); [passive channel charting](https://arxiv.org/abs/2504.09924); [product page](https://espargos.net/espargos-one/)
- 802.11bf: [IEEE](https://standards.ieee.org/ieee/802.11bf/11574/); [overview](https://ar5iv.labs.arxiv.org/html/2207.04859)
- Hardware: [ESP32 CSI docs](https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-guides/wifi-driver/wifi-vendor-features.html); [ESP32-C5 CSI](https://docs.espressif.com/projects/esp-idf/en/v5.5.4/esp32c5/api-guides/wifi.html); [esp_csi_gain_ctrl](https://components.espressif.com/components/espressif/esp_csi_gain_ctrl/versions/0.1.5/readme); [FeitCSI](https://github.com/KuskoSoft/FeitCSI); [Nexmon CSI](https://github.com/seemoo-lab/nexmon_csi)
- [Ghostline (arXiv 2610.18764)](https://arxiv.org/html/2610.18764)
