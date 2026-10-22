# Build on the Drive research: evidence → implementation

Rechecked 2026-10-19 against the current [Capstone Scoping + Research](https://docs.google.com/document/d/1LvhWgVKfX9y7fNDREqT2WzxCtPXiJItZiGMJGuQFGZ8/edit?tab=t.0), tab `t.0`, revision `AHj4eMRGlJ8Vez1Aq0T1JEebSMJAlF7x-Plzdjyn4by1EbVRfa6FvL9gIIKAvHE8w6t1QKHA5Lwz3MNhWRS8IYAqiB1Xb1E-Jj1_Hy8npC3E`. Its historical results are the foundation for this plan. Recent repository checks supplement that foundation.

Paper numbers and thesis page references below follow the Drive document. Metrics are author-reported results summarized there, not measurements reproduced by this team. The detailed source account remains in [proposal-snapshot.md](proposal-snapshot.md); this file explains which progress to reuse.

## 1. What prior work already gives us

| Prior work in Drive | Concrete progress | Carry into our implementation | Remaining project question |
| --- | --- | --- | --- |
| **Anti-Fall / RT-Fall [R2–R3]** | Multi-antenna phase differences support event segmentation; amplitude/phase features feed an SVM. Room-trained RT-Fall reports about 91% sensitivity/92% specificity. Transfer and furniture changes degrade it substantially. | A separate event segmenter and classifier; phase-derived features only on hardware that actually supports usable simultaneous chains. Include fast sitting/lying and changed-furniture tests. | Can our exact capture chain retain useful discrimination after a layout change, with inexpensive recalibration? These related systems are not independent deployment replications. |
| **FallDeFi [R4; thesis §14 D–E]** | A concrete amplitude → denoising → PCA → STFT → event/features → SVM pipeline, with released MATLAB code. Robust selected features include duration, band entropies and curve shape. | Port a bounded, interpretable baseline from the implementation and verify each stage against reference traces. Do not begin with an arbitrary CNN and an undefined input representation. | Which features survive our hardware and changed rooms? What local threshold calibration is actually needed? |
| **FreeFall [thesis §14 F–H]** | Improves activity separation inside mixed walking/fall/recovery events and uses physical-motion-inspired features plus random forest. Lab-to-other-room accuracy/TPR/FPR is 92.98/88.59/6.14%. | Compare compound-event versus sub-activity classification. Test thesis-inspired duration, spectral/shape features and a random forest alongside FallDeFi. | Do these gains survive a causal implementation and new hardware? No public FreeFall implementation was verified in our repo audit; label any port accordingly. |
| **ENetFall / Chu et al. [R5]** | Released EfficientNet training/test code and data/weights pointer; hardware experiments explain why a controlled injection/monitor setup was selected. New-room/new-person accuracy can be 69.0% despite 96.8% random-split accuracy. | Reproduce the native-data baseline and its difficult transfer setup. Preserve a paper-compatible result separately from corrected grouped evaluation. Use five-second temporal context as a tested starting point. | Does new hardware need retraining/input changes, and can short normal-activity adaptation improve the hard transfer case without losing fall recall? |
| **FallAware [R6]** | Differential autocorrelation with a compact LSTM; 94.1% unseen-environment detection and much longer non-fall observation than most earlier systems. NXP 2×2, typically 3,000 Hz, differs from our ESP32. | Prioritize an autocorrelation representation plus a small temporal model as a serious candidate. Use its ordinary-activity evaluation approach and NXP hardware lineage to inform acquisition. | Can we obtain the capture chain and reproduce the representation? Public author code/weights were not located. Its offline, motion-triggered long-term observations are not a deployed online guarantee. |
| **CSI-Bench [R7]** | Thousands of fall/non-fall examples and explicit hardware/difficulty differences. | Use a versioned public benchmark for model development and hardware-specific stress tests. | Current corrected Version 12 results must remain separate from the original figures in Drive; neither is our reproduction. |
| **SHARP [R14]** | A concrete phase-cleaning/Doppler route to person/day/environment activity transfer using multi-antenna, wideband Nexmon hardware. | An optional representation comparison if the chosen radio supports it, with reusable source pointers in [generalization.md](generalization.md). | It does not contain a fall class; transferring its representation to falls is our experiment, not an already validated fall detector. |
| **TCD-FERN, Wi-CaL, WiMANS [R9, R11, R15]** | Prior room-presence models, multi-link counting features and labeled multi-user activities. | Keep separate room-context/counting tasks and reuse suitable labels/features once fall sensing is stable. | Moving-crowd count or room occupancy does not establish stationary count, identity tracking or who fell. |

## 2. Hardware should follow measurement requirements

The research supports richer measurement before arbitrary model scaling, but it does not identify one universally best retail radio.

- **NXP is a task-evidence candidate:** FallAware and CSI-Bench motivate investigating its exact acquisition stack. Their different sample rates also show that there is no universal fall-sensing rate to copy blindly.
- **Intel 5300 is a faithful-reproduction option:** it most closely matches FallDeFi/FreeFall/ENetFall measurement assumptions. An obtainable working kit is worth considering for reference experiments. Its legacy toolchain and maintenance burden still need evaluation for a finished installation; do not assume an AX210 is a drop-in replacement.
- **AX210/FeitCSI is an engineering-access candidate:** it provides a documented modern capture path. Choosing it is our engineering inference, not a historical paper demonstrating superior AX210 fall accuracy. Establish native fall data and phase quality before adopting paper-derived models.
- **Nexmon/RT-AC86U is a representation candidate:** SHARP makes it relevant if we prioritize reproducing cleaned phase/Doppler processing. Its non-fall activity results alone do not select the final fall hardware.

Choose the complete radio/host/firmware/driver/packet-source combination by whether it supplies the selected representation reproducibly and sustains unattended capture. Budget a reference setup separately from compact final installation hardware if necessary. New equipment is authorized in principle; current ownership does not determine the architecture.

## 3. Placement is already known to matter

The thesis's FreeFall experiments use one TX and three RX locations, but evaluate the links separately. One poorly covered kitchen position has about 70% event detection and 90% subsequent classification—roughly 63% combined. That is direct motivation for a coverage map and additional receiver locations, not a guarantee that fusion works.

Compare one link against two complementary links, and use a third only if coverage evidence warrants it. Match physical node count and per-link packet budget when comparing one-TX/multiple-RX with multiple-TX/one-RX. Keep all observations of one physical fall in the same data split. An independent receiver is a spatially different link, not automatically a phase-coherent antenna of the first receiver.

## 4. Implementation sequence grounded in that progress

1. **Establish a reference:** verify data access, build the ENetFall native-data reproduction, and inspect FallDeFi's released intermediate processing. Save original and repaired evaluation separately. This can proceed while richer hardware is acquired.
2. **Reproduce representations:** implement a bounded FallDeFi-inspired amplitude/spectral feature baseline and a FallAware-inspired autocorrelation baseline on our chosen capture stream. Explicitly label adaptations from the source methods. Verify sample rate, units, chain mapping and completed-window behavior.
3. **Address the known segmentation failure:** compare a simple fixed temporal window with FreeFall-inspired activity splitting on walking → fall → recovery and ordinary fast transfers. Preserve the continuous event timeline; no centered offline filter may masquerade as immediate inference.
4. **Compare models on the same split:** spectral features + SVM/RF, autocorrelation + small LSTM/TCN, and an ENetFall-style deep reference where physically meaningful. Add generic CNN variants only with a clear hypothesis. Threshold all candidates on development data at the same operational false-alert budget.
5. **Measure the unresolved part:** zero-shot transfer versus timed ordinary-activity calibration; new people/days/layouts; corner/bed/chair coverage; continuous alert burden; runtime reliability. Those gaps, rather than proving CSI changes with motion again, define our contribution.

## 5. Preserve the lessons about rate, calibration and alerts

FallDeFi's downsampling ablation supports roughly 600–1,000 Hz for that feature pipeline, with degradation at 400 Hz and below. That is not a live-network experiment or a universal minimum. Its STFT settings are sample-rate dependent. FreeFall's high-frequency noise estimate and feature bands likewise cannot be copied onto low-rate measurements; resample/redesign explicitly and compare against a faithful reference.

The thesis distinguishes classifier training, local event/noise thresholds and physical coverage. Keep those three installation costs separate. The proposed 2/5/10-minute onboarding curve is our experiment; the Drive papers do not establish those durations. Moving furniture must trigger measured drift/recommissioning behavior rather than an unsupported claim of automatic invariance.

FreeFall's 30-versus-60-second post-fall cancellation rule is inconsistent across the source and is not a validated basis for suppressing a caregiver alert. Retain the detection after later motion; evaluate acknowledgement/resolution separately. This is a deliberate design change justified by the source limitation.

The finalized project therefore builds on existing detection, feature and segmentation methods, while supplying an integrated, evaluated installation and operating process. It does not need to rediscover every historical result or promise the same percentages on a different radio.
