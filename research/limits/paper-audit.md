# Paper audit: what limited each system

2026-10-20. Audit by a research subagent of 26 sources: the papers in `drive_research.md` (R1–R18 plus the FreeFall thesis), the mesh preprints, the RTI papers, PhaseBeat, and the team's own ESP32 logs. Primary text was read for 18 of them plus the FallDeFi journal's downsampling section. Index: [README.md](README.md).

## Limitation codes

- **T1** Temporal sampling: per-link packet rate, jitter, gaps, max unambiguous Doppler
- **T2** Per-measurement resolution: bandwidth, antennas/aperture, observation time, subcarrier count
- **T3** SNR / dynamic range / quantization / AGC gain artifacts
- **T4** Phase coherence: CFO/SFO/timing offsets, cross-device incoherence, need for multi-antenna on one chip
- **T5** Spatial coverage / geometry: number of links, placement, Fresnel-zone coverage, blind spots, vertical dimension
- **T6** Too few independent measurements vs unknowns (multi-person, pose, scene)
- **T7** Environment dependence / domain shift (room, furniture, person, day, hardware)
- **T8** Data, labels, evaluation design (dataset size, staged falls, random vs grouped splits, short observation, no continuous false-alarm measurement)
- **T9** Confounders: multiple people, pets, fans, adjacent-room motion, interference
- **T10** Operational/system: export bottleneck, hardware/toolchain availability, cost, real-time constraints

Fix classes: **FUND** (fundamental physics/regulatory), **HW** (fixable by buying different hardware), **ENG** (fixable by system engineering), **ML/DATA** (modelling, labels, evaluation).

## 1. Summary

**Falls.** The largest measured failures come from two causes:

- **Domain shift and evaluation design (T7/T8).**
  - ENetFall falls from 96.8% to 69.0% accuracy (54.6% precision) on a new room with new people.
  - FallDeFi's false-positive rate goes from 14.9% to 27.9% after a later session.
  - RT-Fall's sensitivity drops from 89% to 57% when furniture blocks the main path.
- **Coverage (T5).** In one FreeFall room, true-positive rate ranges from 77.7% to 98.4% depending only on which receiver is used. A poorly covered spot detects only about 63% of falls end to end.

**Sampling rate (T1)** has a measured limit, but a forgiving one. Two results are relevant:

- FallDeFi holds up down to 600 Hz at 5.2 GHz and degrades from 400 Hz. Fall Doppler scales with carrier frequency, so that is very roughly 280 Hz at 2.4 GHz (estimate).
- ENetFall puts fall Doppler at 2.4 GHz at 32–56 Hz, with most energy below 14 Hz.

Every ESP32 mesh in the set ran at 20 Hz per link or less: Ghostline at most 8, Less is More 1.4, OpenCSI 20. The team's ~71 Hz exactly matches its UART ceiling. So the ESP32-specific gap is mostly too few data points (T1, T5), and that part is engineerable.

**Resolution (T2)** limits only multi-person separation, pose and model-based single-link tracking. No fall system used delay or angle resolution.

**Lying still on the floor is the weak spot.** SHARP states that a static person in sub-6 GHz Doppler is "hardly distinguished from the empty room". Less is More detected stationary presence 38% of the time. However, OpenCSI's per-link quiet-baseline Z-scores at 20 Hz reached F1 ≥ 0.959 for static occupancy. That suggests sensitivity to small amplitude changes (T3) plus floor-level coverage (T5) is the real constraint. No paper tested a person lying on the floor.

**How much is engineerable:** sampling rate, export, coverage, channel diversity and calibration are fixable. The domain-shift gap that remains even with Intel 5300 hardware at 1 kHz is a data/modelling problem.

## 2. Per-paper table

Verified column: Y means the primary text was read. Partial means spot-checked. N means taken from the local summaries.

| Paper | Hardware / band / BW / antennas (TX×RX) | Rate | Links / geometry / environments | Task, headline result, split | Limitations (T-code, class; later fix, if any) | Verified |
|---|---|---|---|---|---|---|
| WiFall (2014) | Intel 5300 laptops, 802.11n; antennas and BW not verified | n/v | 1 link; 2 scenarios, several layouts | Falls: 87% precision, 18% false alarms | T8 ML (precision, not sensitivity; staged falls); T7 ML; amplitude only (T4 HW). RT-Fall's phase difference later added 14 points sensitivity and 10 points specificity | N |
| Anti-Fall (2015) | TP-Link WDR5300 1×, Intel 5300 ×2; 5 GHz | 100 Hz | 1 link; office 3×4 m, meeting room 6×6 m; 6 students | Falls: 89% detection, 13% false positives; within room | T7 ML: moving a sofa took detection from 83% to 76% and false positives from 9% to 34%; T1 ENG (well below FallDeFi's rate limit); T8 (young, staged) | N |
| RT-Fall (TMC 2017) | Same as Anti-Fall | 100 Hz | 1 link, 4 settings; 6 volunteers aged 21–32 | Falls: ~91% sensitivity, 92% specificity, room-specific | T5/T7: a blocked main path took sensitivity from 89% to 57% and specificity from 92% to 59% (ENG: more links; ML: retrain). Office model in other rooms: 71/72. The FallDeFi thesis reimplementation transfers at 51.1% accuracy with 65.9% false positives; FreeFall reaches 92.98% | N |
| FallDeFi (IMWUT 2017) | Intel 5300, 2×2 omni; 5.2 GHz | 1000 Hz | 5 links (4–9 m; line-of-sight and through 1–2 walls), each evaluated alone; 3 volunteers aged 27–30; 326 falls, 744 non-falls | Falls: >93% (original features). Selected features: accuracy 88.9%, true-positive rate 94.3%, false-positive rate 14.9%; 70/30 split within a session | T7 ML: later session 80.9% / 27.9% false positives; lab to other rooms 80.1% / 21.6%. T3/T5 ENG: distant or obstructed links lose high frequencies. T1 measured. T8: SMOTE order unclear. T10: never run live | Partial (rate ablation from the journal; the rest from local thesis notes) |
| FreeFall (thesis ch. 6) | Same as FallDeFi | 1000 Hz | 1 TX + 3 RX per room (18.9–66.5 m²); links not fused | Falls: same place 94.7% accuracy / 96.3% true positives / 5.6% false positives; lab to other rooms 92.98% / 88.59% / 6.14% | T5 ENG: per-link true-positive rate 77.7–98.4% in the same room; ~1 m-high corridor radios weak; no fusion tested. T8: 30-second vs 60-second alert-cancel rule is inconsistent. Fixes FallDeFi's transfer loss through segmentation and physics-based features | N |
| ENetFall (Access 2023) | Intel 5300 injection to monitor, 1×3; 5.2 GHz; ESP32 NodeMCU/TTGO and routers also tried | 1000 Hz, decimated ×8 to 125 Hz for the model | 1 link; 5 rooms; 22 people aged 24–43; 321 falls, 436 non-falls | Falls: 96.8% on a random 80/20 split | T7/T8 ML: new room plus new people gives 69.0% accuracy, 54.6% precision, 90% recall. T1/T3: ESP32 below 200 Hz judged inadequate. FUND: fall Doppler is 32–56 Hz at 2.4 GHz vs 69–121 Hz at 5.2 GHz | Partial |
| FallAware (JSAS 2025) | NXP 88W8997 2×2; 5.2 GHz, 40 MHz | 3000 Hz | 1 pair per test; ~3 receivers at different heights, evaluated independently; 6 environments including older-adult homes | Falls: 94.1% detection in unseen environments at 5% false-alarm rate; under 5 alarms/month in single-person homes over 21 aggregate months; ~2,400 falls | T9: pets, fans, concurrent motion. T8: motion-triggered offline processing; softer falls detected less often. T5: receivers not fused | Y |
| CSI-Bench (NeurIPS 2025) | Mostly NXP 2×2, 40 MHz, 5 GHz, plus an ESP32-S3 subset | 100 Hz | 6 environments, 17 users | Falls: 95.02% overall vs 70.78 ± 5.52% on the hard tier (Version 12); random 70/15/15 split | T3/HW: the hard tier reflects signal quality and device diversity. T7: out-of-distribution splits drop sharply. T8: random split | Partial |
| WiDetect (IMWUT 2019) | 3×3; 5.805 GHz, 40 MHz | 30 Hz (1 kHz used for model validation) | 1 link; office, apartment, 6-room house; more than 5 weeks | Motion: 99.68% detection, 0 false alarms observed | T9 FUND: a fan, a Roomba and a cat all register as motion. T5: far rooms have empirical detection probability as low as 0.83. T1: the statistic needs high Fs | Y |
| TCD-FERN (TIM 2024) | TP-Link WDR4300 2×2; 2.447 GHz, 20 MHz | 10 Hz | Star layout, 1 TX + 1–2 RX covering 2–3 rooms | Room presence 97.35–98.45% | T7/T8: few layouts. T5: wall partitions, mitigated by voting (~+3%) | N |
| FarSense (2019) | Intel 5300 1×2; 5.24 GHz | ~100 Hz | 1 link, up to 9 m | Breathing: 100% detection at 5 m; MAE 0.28 bpm at 6 m, 0.64 at 9 m | T4 HW: needs two coherent receive chains on one chip (classic ESP32 cannot). T9: multiple people | Y |
| PhaseBeat (ICDCS 2017) | Intel 5300; phase difference across 2 receive antennas; directional antenna for heart rate | 400 Hz | 1 link; lab, corridor, through-wall | Breathing ~0.25 bpm median error; heart rate ~1 bpm | T1 (measured, see ablations). T3/T4 HW: heart rate needs phase difference plus a directional antenna | Y |
| Wi-CaL (Access 2022) | ESP32 1×1; 2.4 GHz, 52 subcarriers | 100 Hz | 4 crossing TX–RX pairs (8 boards); 5.5×5.5 m and 11×5.5 m rooms | Counting: median absolute error 0.35/0.41 people; quadrant accuracy 91.4/98.1%; leave-one-session-out | T6: walking crowds only. T8: their "MAE" is a median; quadrant is a crowd distribution, not individuals | Y |
| Widar2.0 (MobiSys 2018) | Intel 5300 1×3 linear array; 5.825 GHz, 20 MHz | 1000 Hz (needs ≥ 200) | 1 link, 2–6 m long; 10–30 m² rooms | Tracking: 0.75 m mean error (1 link), 0.63 m (2 orthogonal links) | T2 HW: two-person tracking failed ("only 3 antennas", "only 20 MHz"). T5: long links produce small Doppler. Needs line of sight. WiTrack2.0 handles multiple people with 1.79 GHz and 25 antenna pairs | Y |
| Widar3.0 (MobiSys 2019) | Intel 5300, 3 antennas per receiver; 5.825 GHz | 1000 Hz | 1 TX + 6 RX (≥ 3 required); ~2×2 m area | Gestures: 92.7% in-domain; cross-location 89.7, cross-orientation 82.6, cross-environment 92.4 | T5/T6: orientations where the body blocks links lose more than 10%. Needs a known location and orientation | Y |
| SHARP (TMC 2023) | 1 TX antenna to ASUS RT-AC86U Nexmon monitor with 4 receive antennas; 5 GHz, 80 MHz | 173 Hz | 1 link plus monitor; bedroom, living room, lab; 3 people | Dynamic activities: worst-case above 95% across person, day and environment changes | FUND (stated): a static person is hardly distinguishable from an empty room using Doppler features. T2: one antenna still above 80% | Y |
| WiMANS (2024) | Intel 5300 3×3; channel 12 (2.4 GHz) and channel 64 (5 GHz) | 1000 Hz | 1 link; 3 rooms; 0–5 users; 11,286 samples of 3 s | Multi-user identity / location / activity, e.g. 99.2 / 83.1 / 61.8 (5 GHz classroom); random 80/20 | T6/T9: more users degrade results (see ablations). T3: 2.4 GHz noisier (packet loss 4.52% vs 2.31%) | Y |
| DensePose (2023) | 3×3 antennas; 2.4 GHz (±20 MHz), 30 subcarriers | 100 Hz | 1 link; 16 layouts; 1–5 subjects (8 total) | Dense pose: AP 43.5 same layout (random 80/20) vs 27.3 unseen layout | T7 ML. T6/T9: three or more people fail. T8: camera pseudo-labels | Y |
| GoPose (IMWUT 2022) | Intel 5300; TX 3 linear antennas; 4 RX each with an L-shaped 3-antenna array; 5.32 GHz, 40 MHz | 1000 Hz | 1 TX + 4 RX; 3 home rooms; 10 people; Kinect labels at 10 Hz | 3D pose: 4.7 cm; unseen rooms 4.8–5.2 cm; leave-one-person-out | T6: tested with 1–2 people only. T8 (inferred): Kinect label error is comparable to the reported error | Y |
| WiTrack2.0 (NSDI 2015) | Custom FMCW radio, 5.46–7.25 GHz (1.79 GHz sweep); 5×5 directional antennas; shared clock | 2.5 ms sweeps | 25 antenna pairs on one 2×1 m plane; 5×7 m room | Multi-person: 7–18 cm median; up to 4 moving / 5 static people in line of sight | T2: path resolution c/B ≈ 17 cm vs 7.5 m at 40 MHz (FUND for Wi-Fi). T10: custom radio. Static people need 10 s windows | Y |
| Wilson & Patwari RTI (TMC 2010) | TelosB 802.15.4, received signal strength (RSS), 2.4 GHz | Token passing | 28 nodes around 41 m² | Attenuation imaging with MSE bounds | T5 (measured, see ablations). T3: RSS shadowing only | Partial |
| Kaltiokallio et al. (MASS 2012) | CC2531, RSS, 5 channels | Multi-Spin token passing | 30 nodes, ~70 m²; open and cluttered/through-wall | Stationary person localization: 0.10 m best | T5/T3: fixed by channel diversity (see ablations). T8: 10 positions, 1 person | Y |
| Less is More (2026) | 9 ESP32-C3 1×1; 2.4 GHz | 1.4 Hz full-mesh cycle | 72 links in a living room, nodes at convenient spots; 312 h over 12 days | Occupancy: AUC 0.541 (1 link) vs 0.489 (72 links); day-level forward-chaining | T1 ENG (stationary presence detected 38% of the time). ML (inferred): the "dilution" comes from concatenating 792 features into an MLP. T5: links in dead corners | Y |
| OpenCSI (2026) | 8 ESP32 per room: S3/C3 HT20 (52 subcarriers), C6 HE20 (246) | 20 Hz | 56 links around the room perimeter; 3 rooms | Binary occupancy: zero-shot cross-room F1 ≥ 0.978; static occupancy F1 ≥ 0.959 | T3 HW: the C3's noise floor compresses Z-scores (C3 to S3′ transfer F1 0.524). ML: MLP 0.675 vs random forest 0.978. T8: 12-minute sessions, 1 person; magnitude does not transfer | Y |
| Ghostline (2026) | 6 ESP32-S3 + router; 2.4 GHz channel 1 | ≤ 8 frames/s per transmitter | 36 links over 21 node pairs; 48 m² room | 1-person tracking: 0.85 m off-path median, 1.56 m vs reference; one pilot session | T1/T10. Stationary person mostly lost (tracked at 4 of 7 stops). T9: two walkers fail; recurring false tracks near one fixed point | Y |
| Team ESP32 logs | ESP32-WROOM-32, ESP-NOW, channel 11 HT40, int8 | 69–71 frames/s achieved | 1 link | Breathing visible, heart rate not | T10 ENG: matches the 921,600-baud UART ceiling (71.1 lines/s). T3: RSSI −70 to −80 dBm. Gaps up to ~374 ms | Local logs |

## 3. Ablation table (the most direct evidence)

| Paper | Variable changed | Values | Effect | T-code |
|---|---|---|---|---|
| FallDeFi | Packet rate (offline downsampling, 5.2 GHz) | 1000, 800, 600, 400, 200 | Flat down to 600; deteriorates from 400. Authors give fall Nyquist rate under 350 Hz | T1 |
| ENetFall | Split type | Random 80/20 vs new room + new people | Accuracy 96.8% vs 69.0%; precision 96.0% vs 54.6%; recall 96.9% vs 90.0%. Other unseen rooms: 72.5–90.0% | T7/T8 |
| FallAware | Packet rate | 3000 vs 1000 Hz | 3000 Hz better; ROC curves only, gap varies by environment | T1 |
| FallAware | Autocorrelation lag | 0.01 vs 0.03 s | Detection at 5% false alarms: 89.8% to 94.4% (Apt2); 92.2% to 93.4% (Office1) | T1/T2 |
| FallAware | Antenna-permutation augmentation | On vs off (2×2 gives 9 copies) | Without it the model failed in Office1 | T7/T8 |
| FreeFall | Which receiver (same room, same falls) | Receivers 1/2/3 | True-positive rate: kitchen 91.1/98.0/98.0; break room 78.1/81.6/95.0; seminar room 77.7/80.5/98.4. Longest diagonal link best everywhere. One kitchen spot: 70% detection × 90% classification ≈ 63% | T5 |
| FallDeFi / FreeFall | Transfer condition | Same session / later session / lab to other rooms | FallDeFi accuracy 88.9 / 80.9 / 80.1, false positives 14.9 / 27.9 / 21.6. FreeFall lab transfer: 92.98% accuracy, 6.14% false positives | T7 |
| RT-Fall / Anti-Fall | Furniture | Moved or blocking | Sensitivity 89% to 57%, specificity 92% to 59%; detection 83% to 76% with false positives 9% to 34% | T7/T5 |
| Ghostline | Remove node-to-node links (36 down to 6 router links) | Same recording, offline replay | Walk updates tracked 90% to 19.2%; within 1 m 50.0% to 11.2%; free walk 96% to 60% | T5 |
| Ghostline | RSS vs CSI evidence | RTI on each | The RSS baseline drifted (100% false tracks in the final empty period) | T3 |
| Kaltiokallio | Channels used per link (open room) | 1 fixed channel vs selection by fade level | 1.08–2.13 m vs 0.21 m (1 channel chosen by fade level) and 0.10 m (4 channels) | T5/T3 |
| Kaltiokallio | Same, cluttered room | Same | 1.81–2.68 m single channel vs 0.96 m best | T5/T3 |
| Wilson & Patwari | Node density and layout | Square / front-back / random | MSE bound falls with density; front-back layout leaves cells with no crossing links | T5 |
| Less is More | Links (features concatenated) | 1 to 72 | AUC 0.541 to 0.489 (Cohen's d = 0.86; 7 of 10 folds; p = 0.053). One random link: 0.504. Median single-link AUC 0.633 | T5 vs ML |
| Less is More | Motion state at 1.4 Hz | Empty / moving / stationary | Detected 98% / 100% / 38% | T1/T3 |
| OpenCSI | Per-link calibration | Calibrated Z-score vs warmup-normalized amplitude | Cross-room F1 ≥ 0.978 vs 8–20 points lower | T7 |
| OpenCSI | Learner | Random forest vs MLP | 0.978 vs 0.675 | ML |
| Widar2.0 | Packet rate | 1000 vs 250 Hz | Error about unchanged; theory requires ≥ 200 Hz (Doppler within ±100 Hz) | T1 |
| Widar2.0 | Links | 1 vs 2 orthogonal | 0.75 m vs 0.63 m | T5 |
| Widar3.0 | Packet rate | 1000 vs 250 Hz | About 4 points lower; above 85% throughout | T1 |
| Widar3.0 | Links | 6 down to 2 | Gradual decline to 3 links, sharp drop at 2 | T5/T6 |
| Widar3.0 | Feature | Body-velocity profile vs Doppler profile vs raw CSI | +15 and +52 points cross-domain | T7 |
| GoPose | Receivers | 1 / 2 / 3 / 4 | 8 / 6 / 5.2 / 4.7 cm | T5/T6 |
| GoPose | TX antennas | 1 / 2 / 3 | 8.5 / 6.8 / 4.7 cm | T2 |
| GoPose | Subcarriers | 1 vs 30 | ~19 vs 4.7 cm | T2 |
| GoPose | Angle of arrival | 1D vs 2D | 10.1 vs 4.7 cm | T2 |
| GoPose | Packet rate | 250 / 500 / 1000 | 5.7 / 5.3 / 4.7 cm | T1 |
| GoPose | Receiver spacing | 2.5 / 3 / 3.5 m | 4.7 / 5.1 / 5.8 cm | T3/T5 |
| GoPose | Users | 1 vs 2 | 4.7 vs 6.0 / 6.3 cm | T6 |
| PhaseBeat | Packet rate | 20 vs 400 Hz | Heart rate accuracy 88% vs 95%; breathing ~98% at every rate | T1 |
| FarSense | Antenna ratio vs single-antenna amplitude | Detection at 5 m | 100% vs 1.4% (HRD) / 7.7% (FullBreathe); range above 8 m vs under 3–4 m | T3/T4 |
| WiMANS | Band | 2.4 vs 5 GHz | 5 GHz better (e.g. location 78.8 vs 83.1; ST-RF identity 66.3 vs 89.4); combining bands did not help | T3 |
| WiMANS | Maximum users | 1 vs 5 | Identity −2.3 to −5.7 points; location −14.6 to −15.4; activity −25.7 to −25.9 | T6/T9 |
| DensePose | Layout | Same vs unseen | AP 43.5 vs 27.3 (AP@50: 87.2 vs 51.8) | T7 |
| DensePose | Phase input | Amplitude only vs with phase | AP 39.5 vs 40.3 | T4 |
| SHARP | Monitor antennas | 1 to 4 | Rises monotonically; above 80% even with 1 | T2/T5 |
| WiDetect | Rate / window length / subcarriers | 30–180 Hz; window T; subcarrier count F | ROC improves with rate; longer windows raise detection; more subcarriers cut false-alarm variance only | T1/T2 |
| Wi-CaL | Subcarriers; observation length | 4–52; 30–120 s | Subcarriers: little effect. Length: error 0.47 to 0.41 | T2/T8 |
| CSI-Bench | Difficulty tier | Overall vs hard | 95.02% vs 70.78% | T3/T7 |

## 4. Synthesis by goal

**Falls.**

- **Binding constraints, in order:** T7/T8 (12–28-point drops, false-positive rates roughly doubling), then T5 (up to 20 points between receivers in one room), then T1.
- **T2 is not binding.** No fall system resolved delay or angle.
- **T3/T4 shows up indirectly:**
  - distant or obstructed links lose high-frequency fall content (FallDeFi);
  - ENetFall rejected ESP32 captures for noise as well as low rate.
- **On an ESP32 mesh,** T1 and T10 are the first wall.
  - The team's 71 Hz is UART-bound: at 921,600 baud, one node gets about 230 binary 400-byte records/s across all incoming links.
  - Moving export to Ethernet/USB and putting radios on separate channels in parallel is pure engineering.
  - About 120 Hz or more per link at 2.4 GHz would keep peak fall Doppler from aliasing (inference from ENetFall's 32–56 Hz).
- **T5 is engineerable** with more links that cross the room, including floor-level links.
- **T7/T8 is not fixed by hardware**, but several techniques reduce it: FreeFall's physics-based features (lab transfer 80% to 93%), FallAware's augmentation plus large negative datasets, and OpenCSI-style per-link calibration.
- **Lying still** is a T3/T5 problem; Doppler-type features see nothing once the person is motionless.

**Localization and counting.**

- **Single person:** T5/T6 is binding and engineerable.
  - Ghostline loses almost all tracking without the crossing links.
  - Kaltiokallio improves about tenfold through channel diversity.
  - Ghostline gets 0.85–1.56 m with six ESP32-S3 boards.
- **Multiple people:** T6 plus T2.
  - Widar2.0's single link fails for two people because of 3 antennas and 20 MHz.
  - WiMANS location drops about 15 points at 5 users.
  - WiTrack2.0 needed 25 antenna pairs and 1.79 GHz.
- **What a mesh can and cannot buy:** it adds independent measurements, but not delay resolution. Path resolution at 40 MHz is c/B ≈ 7.5 m, which is a physical limit for ESP32-class hardware.
- **Stationary counting** is untested; Wi-CaL counted walking people only.

**Pose.**

- **T2 is binding**: subcarriers, antenna aperture and TX antennas (GoPose), plus T5 (number of receivers).
- **Learned approaches add T7/T8**: DensePose drops 16 AP on an unseen layout.
- **Full pose on 1T1R ESP32 is not engineerable.**
- **Coarse sit-vs-stand posture** from attenuation patterns at two heights is plausible but untested. SHARP explicitly excludes static postures.

**Vitals.**

- **Binding constraints: T3/T4.**
  - FarSense's range comes from the ratio between two coherent receive chains.
  - PhaseBeat's heart rate needed phase difference and a directional antenna.
- **T1 is not binding for breathing.** PhaseBeat was flat at ~98%, and CSI-Bench samples breathing at 30 Hz. It does matter for heart rate: 88% at 20 Hz vs 95% at 400 Hz.
- **The team's result fits this pattern:** breathing was visible on the ESP32, heart rate was not.
- **Heart rate likely needs different hardware:** AX210 or NXP 2×2.

**Data points vs resolution vs domain shift.**

- **Falls and single-person localization:** too few data points (T1/T5) is the engineerable gap on ESP32. Domain shift and data (T7/T8) is the remaining gap even on the best hardware.
- **Multi-person sensing, pose and heart rate:** limited by resolution (T2/T3/T4).

## 5. Gaps a mesh could test cheaply

1. **Fall-rate ablation at 2.4 GHz on ESP32.** Capture fall-like motion at the highest per-link rate that can be exported, then downsample to 200, 120, 70, 50 and 20 Hz. No paper has done this at 2.4 GHz or on ESP32.
2. **How links are fused, for falls.** Compare the best single link, late fusion (vote or max) and concatenation. FreeFall never fused its links, and Less is More only concatenated. This would settle whether "dilution" is physics or a modelling artifact.
3. **Node height.** Compare floor-level (≤ 30 cm), ~1 m and ceiling links for detecting a person lying down and for sit vs stand. No paper isolates height; FreeFall's author flags it.
4. **Channel diversity for CSI.** Kaltiokallio's tenfold gain was with RSS on 802.15.4. Multiple radios per node could test choosing the channel or subcarriers where each link is not in a deep fade.
5. **Long static and lying episodes.** Run calibrated Z-scores plus a breathing-band detector over many minutes, and measure how quickly the quiet baseline goes stale. OpenCSI used 12-minute sessions.
6. **Effect of TDMA gaps and jitter on Doppler features.** The mesh creates structured gaps; nobody has quantified their effect.
7. **AGC and int8 quantization effects** on small-motion sensitivity, for example fixed gain vs automatic gain.
8. **Rejecting non-human motion by location**, such as a fan or pet. Ghostline's false tracks clustered at one fixed spot.
9. **HT20 vs HT40 on ESP32** for presence and falls.
10. **Phase stability between two radios on one node** that share a crystal. If stable, the FarSense ratio method might be possible on cheap hardware. Speculative.
11. **Grouped (leave-room/leave-person-out) splits plus continuous false alerts per room-day**, built into the evaluation from the start.

## 6. Sources

- Ghostline: https://arxiv.org/html/2610.18764
- Less is More: https://arxiv.org/html/2602.10823v3
- OpenCSI: https://arxiv.org/html/2607.26665v1
- Kaltiokallio et al.: https://span.ece.utah.edu/uploads/MASS2012_camera_ready_new.pdf
- Wilson & Patwari RTI: https://span.ece.utah.edu/uploads/RTI_version_3.pdf
- PhaseBeat: https://ieeexplore.ieee.org/document/7980063
- WiFall: https://doi.org/10.1109/infocom.2014.6847948
- Anti-Fall: https://arxiv.org/abs/1507.01057
- RT-Fall: https://doi.org/10.1109/tmc.2016.2557795
- FallDeFi: https://dl.acm.org/doi/10.1145/3161183 (PDF: https://www.davidrojas.co.uk/wp-content/uploads/2017/10/falldefi_ubiquitous_fall_detection_wifi.pdf)
- FreeFall: Palipana PhD thesis, 2019 (local supplied PDF, summarized in `drive_research.md` §14)
- ENetFall: https://doi.org/10.1109/ACCESS.2023.3300726
- FallAware: https://www.researchgate.net/publication/388348926_FallAware_An_Explainable_Learning_Approach_to_Robust_Fall_Detection_With_WiFi
- CSI-Bench: https://arxiv.org/abs/2505.21866 (Version 12 results: https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark)
- WiDetect: https://cswu.me/papers/ubicomp19_widetect_paper.pdf
- TCD-FERN: https://doi.org/10.48550/arxiv.2304.13107
- FarSense: https://arxiv.org/abs/1907.03994
- Wi-CaL: https://doi.org/10.1109/ACCESS.2022.3155812
- Widar2.0: https://tns.thss.tsinghua.edu.cn/~qiankun/files/MobiSys18_Widar2.0_paper.pdf
- Widar3.0: https://cswu.me/papers/mobisys19_widar3_paper.pdf
- SHARP: https://arxiv.org/abs/2103.09924
- WiMANS: https://arxiv.org/html/2402.09430
- DensePose from WiFi: https://arxiv.org/abs/2301.00250
- GoPose: https://par.nsf.gov/servlets/purl/10416011
- WiTrack2.0: https://www.usenix.org/system/files/conference/nsdi15/nsdi15-paper-adib.pdf
- Team evidence: `logbook.md`, `research/local-evidence.md`, `research/hardware.md`
