# Networked non-radar RF sensing: lessons for the ESP32 CSI mesh

2026-10-20. Literature review by a research subagent (web search, primary papers, datasheets). Estimates are labelled as inference. Index: [README.md](README.md).

## 1. Summary

The RSS-based radio tomographic imaging (RTI) systems that worked did not succeed because of their measurement; RSS is cruder than CSI. They succeeded because of four system choices:

- **Density.** They used 24–34 nodes spaced about 0.6–1.1 m apart around the room, giving 276–870 links.
- **A physical forward model.** A regularised linear inversion means each link only votes on the area it physically crosses, and quiet links count as evidence that the area is empty.
- **A statistic matched to the person's state.** Mean attenuation or histogram distance sees people who are standing still. Variance only sees motion. These were paired with online baseline updates and a state machine that "holds" a person who stops moving.
- **Several frequency channels, weighted by fade level** (whether a link sits in a constructive or destructive multipath interference state). This cut narrowband error by about 10x.

The Wi-Fi CSI meshes so far (Ghostline, Less is More) used 7–9 radios, one channel, 1.4–8 frames/s per link, and variance features or classifier feature-concatenation. Their failures follow from those choices: standing people are lost, and the mesh does no better than a single link. Most of this is fixable by engineering.

The closest precedent for falls is two-level 3D RTI, with nodes at 17 cm and 140 cm and using the diagonal links between levels. It separated falls from deliberate lying down, on a small dataset. That matches a two-height plan, but only if the statistic can see a motionless person.

Channel diversity should help wideband CSI less than narrowband RSS. One 20/40 MHz CSI frame already spans several coherence bandwidths, so per-subcarrier fade weighting and HT40 should come before multi-radio nodes.

What engineering cannot recover on ESP32 is delay resolution. UWB (500 MHz bandwidth) and coherent multi-channel phase (Bluetooth Channel Sounding, Chronos) separate multipath into extra "virtual links". Single-antenna 20/40 MHz Wi-Fi CSI with phase that is not coherent across packets cannot do that.

## 2. Comparison table

Category key: (a) physics or regulation, (b) fix by buying hardware, (c) fix by system engineering, (d) not worth fixing.

| Approach | Nodes / links | Band / BW | Per-link rate | Measurement | Best result | Key enabler | Blocker for ESP32 CSI mesh | Cat. | Lever |
|---|---|---|---|---|---|---|---|---|---|
| Shadowing RTI, Wilson & Patwari 2010 | 28 nodes, 378 links; 0.9 m spacing at 0.9 m height | 802.15.4 2.4 GHz, 1 ch | ~3.3 Hz | RSS mean vs empty room | 1–2 standing people imaged (41 m², outdoor) | Dense perimeter + regularised inverse | Automatic gain control (AGC) steps and per-frame normalisation erase absolute attenuation | c | Lock or compensate gain; mean-shift statistic |
| Variance RTI (VRTI), 2011 | 34 nodes, through-wall, 72 m², torso height | 1 ch | 50-sample buffer | RSS variance | 0.63 m mean tracking error | Variance is robust when there is no line of sight | Stationary people invisible (Ghostline repeats this) | c | Use only as the motion channel |
| Kernel-distance RTI (KRTI), Zhao & Patwari 2013 | 34 nodes | ch 26 | ~3 Hz | Distance between short- and long-term RSS histograms | Stationary 0.71 m; moving RMSE ≤0.81 m (VRTI: 2.1 m on a windy day) | Distribution statistic + slowly updated long-term histogram | Needs gain-stable amplitudes | c | Port to per-subcarrier dB |
| Channel-diversity RTI, Kaltiokallio 2012 | 30 nodes at 1 m, 70 m², 5 ch | 75 MHz span | 87 ms cycle per channel | RSS + fade level | 0.10 m stationary (single channel: 1.08–2.13 m) | Selecting channels in anti-fade | One radio; CSI is already partly diverse | c (partly a) | Subcarrier fade weighting; HT40 |
| Fade-level spatial model, TMC 2014 | 30–33 nodes, 4–5 ch | — | 2.87 Hz per channel | RSS; per-link impact area depends on fade level | ≤0.30 m tracking: open room, apartment, through-wall | Per-link spatial model | Needs per-link fade estimate | c | Estimate fade per subcarrier |
| Multi-target RTI, Bocca 2014 | 30–33 nodes (5 interior in office) at 1 m | 4–5 ch | ~100 ms cycle per channel | RSS | Up to 4 people, ≤0.55 m mean error, no miscounts | Link count, rate, tracking on image sequences | 7–9 nodes at 1–8 Hz | c | ≥20 nodes, ≥10 Hz |
| "Follow @grandma", 2012 | 33 nodes, 58 m² apartment, 1 week | 4 ch | 2.5 Hz per link per channel | RSS vs moving-average reference | 0.23 m RMSE; all area transitions correct in a 2 h test | Online recalibration + state machine | Baseline policy | c | Selective update + hold state |
| 3D-RTI fall detection, Mager 2013 | 24 nodes at 17 and 140 cm, 3×3 m | 4 ch | 3.4 Hz per link per channel | RSS, 3D voxels, hidden Markov model | 100% falls, 0 false alarms (2 subjects, 54 trials) | Floor layer + cross-level links | Statistic must see a lying person | c | Two layers + diagonals |
| Fingerprint DFL, Mager 2015 | 30 nodes at 28 and 132 cm, 870 links, 8 ch | — | 1.2 Hz per link per channel | RSS + random forest | 1.4–1.6% error with correlation-based channel selection | Channel choice that is stable as the room changes | Per-room training; error doubles per ~6 furniture changes | d | Avoid fingerprinting |
| Wi-CaL, 2022 | 4 ESP32 pairs, deliberately crossing | HT20 | 100 Hz | CSI amplitude features + ML | Count error 0.35–0.41 people; 4-zone accuracy 91–98% | Placed links, high rate | Per-room ML | c/d | — |
| Ghostline, 2026 | 7 radios, 36 directed links, 48 m²; most at 0.2 m | ch 1 | ≤8 Hz | Temporal std of log-amplitude | 0.85 m median from path; standing people lost; two walkers not separated | Node-to-node links (90%→19% tracked without them) | Variance statistic, few links, one channel | c | See section 4 |
| Less is More, 2026 | 9 nodes, 72 links | ch 11 HT20 | 1.4 Hz | Variance features concatenated into an MLP | Area under curve (AUC) 0.54 single link vs 0.49 mesh (both near chance) | — | 80 ms slots; feature concatenation | c | Forward-model fusion |
| OpenCSI, 2026 | 8 nodes, 56 links per room | HT20 / HE20 | 20 Hz | \|Ā−μ\|/σ_quiet per link | Occupancy F1 up to 0.99 zero-shot across rooms and chips | Per-link quiet-period normalisation | σ-normalisation drops magnitude | c | Keep as presence layer |
| UWB CIR (DW1000, DLR) | 10 nodes at 0.8 m, 45 links → 123 paths | 3.9 GHz, 500 MHz | — | Power per multipath component | 80% of errors <0.6 m (line-of-sight links only: 4.4 m) | ~1 ns CIR taps make reflections into extra links | 20/40 MHz gives 15/7.5 m delay resolution | a (b) | Add UWB anchors |
| Bluetooth 6.0 Channel Sounding | 1:1 pairs | 72 × 1 MHz, 2.4 GHz | Per procedure | Two-way tone phase + round-trip time | Fall feasibility study: sensitivity 96.9%, specificity 90.6–100% (2 nodes, 6 people) | Two-way exchange cancels oscillator phase, giving coherent ~80 MHz | ESP32 phase is incoherent across packets and channels | b (c, hard) | nRF54L15 nodes |
| 802.11mc/az FTM | Initiator–responder | 20 MHz recommended | Bursts | Round-trip time | ESP32-S3: 0.52 m mean error line-of-sight; ~28 m without | Hardware timestamps | Device-based; original ESP32 has no FTM | b/d | Possible body-blockage feature |
| 802.11bf (2025) | AP-led, multistatic allowed | 1–7.125 GHz | — | Standardised CSI reports | Standard only | CSI exchange across vendors | No ESP32 silicon found | b/d | — |
| LoRa sensing | 1 TX + software radio RX | Sub-GHz narrowband | — | Phase/amplitude | Breathing at 25 m (15 m through a wall) | Range and sensitivity | Wrong problem; needs a software radio | d | — |
| RFID tag walls | Reader + tag arrays | UHF | — | RSSI + phase | 0.24 m median (WallSense) | Many cheap passive "nodes" | Separate reader infrastructure | b/d | — |
| Diffraction tomography (Wiffract, Rytov models) | Dense receiver grid / robot-moved antennas | 2.4/5 GHz | — | RSSI with wave models | Still-object imaging; 86.7% letter reading | Wave-model inversion, dense sampling | Needs dense known sampling and coherence | a/d | — |

## 3. The five design questions

**Q1. Does channel diversity help wideband CSI as much as narrowband RSS?**

Probably not as much. Established results:

- For narrowband RSS, single channels gave 1.08–2.13 m error. Choosing among 5 channels by fade level gave 0.21 m with one channel selected and 0.10 m with four (Kaltiokallio 2012).
- The mechanism is that the sign and spatial extent of the change a person causes depend on the link's fade level. Anti-fade links drop when the line is crossed; deep-fade links can rise, and also respond far from the line (TMC 2014).
- Measured indoor RMS delay spread at 2.4 GHz is 22–31 ns in a 7.8 × 9.95 m room (Wysocki & Zepernick).

Inference from those numbers:

- The 50%-correlation coherence bandwidth is about 1/(5τ), roughly 6–9 MHz.
- An HT20 CSI frame (about 16 MHz of usable subcarriers) therefore holds about 2 independent fade states, and HT40 about 4–5. That is a pool comparable to Kaltiokallio's 5 channels.
- Wi-CaL's finding that 13 of 52 subcarriers performed about as well as all 52 is consistent with this subcarrier correlation.
- So per-subcarrier fade-level weighting on HT40 may capture much of the narrowband gain. Channels 1/6/11 add decorrelated blocks across about 70 MHz, but the return should be smaller than 10x.

No experiment was found comparing single- and multi-channel CSI for RTI on commodity hardware. The only multi-channel CSI device-free localisation paper found (MDPI Sensors 2018) is simulation only.

Phase-coherent stitching across channels does help a lot, but for delay resolution, not fade diversity. Chronos reached 0.47 ns time-of-flight error by stitching Wi-Fi bands. Splicer showed that amplitude stitches easily while phase needs careful error removal.

**Q2. What density and rate did successful systems need?**

- For sub-metre localisation: 24–34 nodes over 9–72 m², spaced about 0.6–1.1 m, with 276–561 node pairs and 4–5 channels.
- Per link: about 2.3–11 Hz per channel. These networks used about 2.9–3.1 ms per transmission and 74–100 ms per full cycle.
- For counting up to 4 people: 32 nodes over 67 m², 27 on the perimeter and 5 inside, using 5 channels (Bocca).
- Wilson's error bounds fall as node density rises.
- The @grandma node-removal simulation found that a smaller, well-placed subset can still localise accurately.
- For comparison, Ghostline has 21 node pairs over 48 m², about 20x fewer than Bocca's ~496.

Inference: for a 20–30 m² room, aim for 16–24 nodes split across two heights and at least 5–10 Hz per link. Ghostline's 8 frames/s cap was a choice, and Less is More's 80 ms slots are not a hardware limit; Wi-CaL ran ESP32 links at 100 Hz.

**Q3. Has anyone used multiple heights to detect lying or posture?**

Yes:

- **Mager et al., PIMRC 2013.** Nodes at 17 cm and 140 cm, processed as separate 2D layers ("dual-level"), produced too many false positives because crouching and lying looked alike. Full 3D RTI including the diagonal cross-level links separated standing, mid-position (lying on a bed, sitting, crouching) and lying on the floor. A hidden Markov model plus a time threshold (best range 1.26–2.15 s) gave 100% detection with no false alarms, but only on 2 subjects in an uncluttered room.
- **Mager et al., JSAC 2015.** Nodes at 28 cm and 132 cm. Localising a standing person with lower links only gave 26.8% error, upper links only 5.9%, and all links 3.5%. So the floor layer adds little for standing localisation; its value is for lying detection.
- Chinese multi-level and vertical RTI posture papers (CCC 2016; Journal of Ambient Intelligence and Humanized Computing, 2015) also exist; only their abstracts were reviewed.

**Q4. How did RTI handle stationary people, baseline drift, and long-term use?**

- **Empty-room calibration** sees still people but drifts. In @grandma, images became wrong within a day. In the KRTI work, moving one node after calibration created a false hotspot.
- **Variance** never sees a still person.
- **KRTI's slowly updated histogram** sees a still person for minutes, after which they fade into the baseline.
- **@grandma** combined a moving-average reference with a state machine that holds the last area while image intensity is low. It ran for one week. Neighbours moving outside the apartment caused occasional false transitions.
- **Placement hygiene mattered:** in @grandma and Bocca, keeping antennas a few centimetres off walls and away from marble fixed poorly localised areas.
- **Fingerprints decay** with furniture changes (error doubles per ~6 changes). Choosing channels by correlation made them more robust.
- **Ghostline's baseline** (10th percentile over 60 s) loses a standing person within seconds.
- **OpenCSI** recovered occupancy F1 from 0.25 to 0.98 after 30 s of fresh quiet data.

**Q5. Why might a fused CSI mesh underperform a single link?**

Established points from the papers:

- Less is More concatenated 792 features into an MLP and evaluated with day-level splits.
- Its full-mesh rate was 1.4 Hz, and its variance features detected only 38% of sedentary activity.
- Several nodes sat in "dead corners".
- Both the single link and the mesh scored near chance, so the paper compares two failures.

Inference adds a further cause: correlated noise. Links that share a receiver share its AGC steps and local interference, and all links on one channel share interference bursts, so fusion that assumes independence overweights common-mode noise.

How successful systems avoided this:

- A forward model (each link maps to an ellipse of floor area) replaced a learned link-selection problem.
- Regularisation and a noise model.
- Fade-level weighting of unreliable links.
- High per-link rates.
- Quiet links counted as negative evidence.
- Common-mode normalisation; Ghostline used a room-level ratio normalisation.

Ghostline and Wi-CaL both found that more crossing links help localisation. "Less is more" applies to classifier fusion for occupancy, not to tomography.

## 4. Ranked levers for an ESP32-style mesh

Gain and difficulty estimates are inference unless a source is noted.

1. **A statistic that sees stationary attenuation (c).** Use the per-subcarrier dB mean shift against a reference, or KRTI-style kernel distance, with gain compensation or locked gain. Espressif's `esp_csi_gain_ctrl` provides both and lists the original ESP32; check that the original ESP32 actually reports AGC/FFT gain.
   - Ghostline subtracts the mean log-amplitude per LTF block. That removes the broadband shadowing RTI relies on, on top of using variance.
   - Gain: essential for detecting someone lying on the floor. Difficulty: medium.
2. **3D tomographic fusion with fade-level weights and negative evidence (c),** instead of concatenation or per-link voting. Gain: high. Difficulty: medium.
3. **Two-height geometry with about 1 m spacing and all diagonal links (c).** Mount antennas at least 5 cm off walls (@grandma, Bocca). Gain: high for falls, counting and localisation. Difficulty: low to medium.
4. **Tight broadcast TDMA with wired sync, a few ms per slot, for at least 5–10 Hz per link (c).** Gain: medium to high. Difficulty: medium.
5. **Per-subcarrier fade-level weighting, then try HT40 (c).** Gain: medium. Difficulty: low.
6. **Baseline management (c):** update the reference only for links not explained by a tracked person, hold state per zone (bed vs floor), take snapshots when the room is known empty, and track calibration maturity as OpenCSI does. Gain: required for 24/7 use. Difficulty: medium.
7. **Multi-radio channels 1/6/11 in lockstep (c).** Gain: moderate; more independent measurements and interference robustness. Difficulty: high.
   - Risk to measure: leakage from simultaneous adjacent-channel transmissions into co-located receivers.
8. **Two-way reciprocal CSI exchange to cancel oscillator phase (c, hard; possibly d for a capstone).** This is the Bluetooth Channel Sounding / Chronos principle and would enable coherent phase and band stitching.
9. **Hardware additions (b):** AX210 with FeitCSI (2 RX chains, up to 160 MHz, FTM) as anchors, UWB DW3000 nodes for CIR, or nRF54L15 Bluetooth Channel Sounding nodes.
   - DW3000 cannot transmit and receive at the same time, so radar-style use needs two chips.
10. **Not worth pursuing (d):** LoRa, RFID walls, diffraction imaging, and waiting for 802.11bf on ESP32.

Fundamental limits (a): the 2.4 GHz band gives about 15/7.5 m delay resolution at 20/40 MHz, and the first Fresnel zone of a 4 m link is about 0.5 m in radius.

## 5. Sources

**RSS radio tomography**

- Wilson & Patwari, RTI, TMC 2010: https://span.ece.utah.edu/uploads/RTI_version_3.pdf
- Wilson dissertation, with VRTI experiment details: https://span.ece.utah.edu/uploads/DFL-JoeyWilson.pdf ; VRTI paper DOI: https://doi.org/10.1109/tmc.2010.175
- Zhao et al., KRTI, IPSN 2013: https://span.ece.utah.edu/uploads/ipsn13-camera_ready.pdf
- Kaltiokallio et al., channel diversity, MASS 2012: https://span.ece.utah.edu/uploads/MASS2012_camera_ready_new.pdf
- Kaltiokallio et al., fade-level spatial model, TMC 2014: https://span.ece.utah.edu/uploads/TMC-2012-10-0498.R1-main.pdf
- Bocca et al., multiple target tracking, TMC 2014: https://arxiv.org/abs/1302.4720
- Kaltiokallio et al., Follow @grandma: https://span.ece.utah.edu/uploads/kaltiokallio2012follow-grandma.pdf
- Mager et al., fall detection with RF sensor networks, PIMRC 2013: https://span.ece.utah.edu/uploads/mager-pimrc2013.pdf
- Mager, Lundrigan & Patwari, fingerprint DFL in changing environments, JSAC 2015: https://span.ece.utah.edu/uploads/main.pdf
- Patwari et al., Breathfinding: https://arxiv.org/abs/1302.3820
- Multi-level RTI posture sensing (CCC 2016): https://doi.org/10.1109/chicc.2016.7554699
- RTI body-pose fall detection: https://link.springer.com/article/10.1007/s12652-014-0243-x

**Wi-Fi CSI meshes and CSI-based tomography**

- Ghostline: https://arxiv.org/abs/2610.18764
- OpenCSI: https://arxiv.org/abs/2607.26665
- Less is More: https://arxiv.org/abs/2602.10823
- Wi-CaL: https://doi.org/10.1109/ACCESS.2022.3155812
- CSI-RTI with image fusion: https://doi.org/10.1109/iceiec.2018.8473507
- CSI-MIMO RTI: https://doi.org/10.1109/pimrc.2018.8580720
- LiFS: https://oar.princeton.edu/bitstream/88435/pr17p0q/1/LiFS.pdf
- Multi-channel CSI DFL (simulation only): https://www.mdpi.com/1424-8220/18/9/3110
- Indoor delay spread at 2.4 GHz (Wysocki & Zepernick): https://doi.org/10.26636/jtit.2000.3-4.23
- Splicer: https://www.sigmobile.org/mobicom/2015/papers/p53-xieA.pdf
- Chronos: https://www.usenix.org/conference/nsdi16/technical-sessions/presentation/vasisht
- `esp_csi_gain_ctrl`: https://components.espressif.com/components/espressif/esp_csi_gain_ctrl/versions/0.1.5/readme?language=en

**UWB and mm-wave multipath RTI**

- DLR multipath-enhanced DFL with DW1000: https://digital-library.theiet.org/doi/pdf/10.1049/mia2.12373
- UWB CIR device-free localisation (Sensors 2022): https://www.mdpi.com/1424-8220/22/16/6255
- COTS multistatic UWB tracking: https://doi.org/10.1109/lawp.2022.3141869
- UWB CIR sensing dataset: https://pmc.ncbi.nlm.nih.gov/articles/PMC9587989/
- DW3000 accumulator sizes (driver header): https://android.googlesource.com/kernel/google-modules/uwb/+/refs/heads/android-gs-bluejay-5.10-android13-qpr3-beta1/kernel/drivers/net/ieee802154/dw3000_core.h
- Qorvo forum, DW3000 cannot TX and RX simultaneously: https://forum.qorvo.com/t/how-to-get-the-raw-data-and-transmit-through-usb/21254
- Multipath-RTI at mm-wave: https://doi.org/10.1109/access.2024.3379317

**Bluetooth Channel Sounding**

- Bluetooth Core 6.0 feature overview: https://www.bluetooth.com/core-specification-6-feature-overview/
- Device-free sensing with Channel Sounding (2025): https://doi.org/10.1145/3714394.3754369
- Channel Sounding fall-detection feasibility (Sensors 2026): https://doi.org/10.3390/s26061930
- Channel Sounding IQ dataset (nRF54L15): https://zenodo.org/records/17347695

**802.11 FTM and 802.11bf**

- ESP-IDF FTM example and supported chips: https://github.com/espressif/esp-idf/blob/master/examples/wifi/ftm/README.md
- ESP32-S3 FTM accuracy study: https://biblioteca.sbrt.org.br/articles/5121
- FeitCSI: https://feitcsi.kuskosoft.com/
- IEEE 802.11bf-2025: https://standards.ieee.org/ieee/802.11bf/11574/
- NIST overview of the 802.11bf procedure: https://tsapps.nist.gov/publication/get_pdf.cfm?pub_id=935175
- Infineon ACW741x announcement: https://www.infineon.com/technology-news/2026/infcss202601-039

**LoRa, RFID, diffraction tomography**

- LoRa through-wall sensing (IMWUT 2020): https://doi.org/10.1145/3397326 ; follow-up: https://doi.org/10.1145/3478080
- WallSense: https://mdpi-res.com/d_attachment/sensors/sensors-19-00068/article_deploy/sensors-19-00068.pdf
- TagFall: https://eudl.eu/doi/10.4108/eai.22-7-2015.2260072
- Wiffract: https://web.ece.ucsb.edu/~ymostofi/papers/PallaproluKoranyMostofi_MobiCom2022.pdf
- Rytov-model Wi-Fi imaging: https://web.ece.ucsb.edu/~ymostofi/papers/TVT15_DepatlaBucklandMostofi.pdf

**Caveats**

- Sources read only as abstracts: the CSI-RTI papers, the vertical RTI posture papers, and the first Channel Sounding sensing paper.
- "No ESP32 802.11bf silicon" rests on the RuView project docs plus the absence of any Espressif announcement found.
- No hardware prices were verified.
