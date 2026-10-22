# Proposal snapshot

Read on 2026-10-19 from https://docs.google.com/document/d/1LvhWgVKfX9y7fNDREqT2WzxCtPXiJItZiGMJGuQFGZ8
Revision: `AHj4eMRGlJ8Vez1Aq0T1JEebSMJAlF7x-Plzdjyn4by1EbVRfa6FvL9gIIKAvHE8w6t1QKHA5Lwz3MNhWRS8IYAqiB1Xb1E-Jj1_Hy8npC3E`
Scope: research tab `t.0`; stakeholder Questions tab read but omitted from this local research extract.

Wi-Fi CSI Fall Detection for Elderly Care Homes

Research first pass • supervisor discussion draft

This is the detailed research pass requested by the team. It intentionally retains paper-level evidence, competing hardware options, open questions, and proposed experiments. A later pass can extract a concise 1–2 page design document. Published results below are reported within their experimental scope; the proposed design and validation plan are our recommendations, not demonstrated capabilities of our prototype.

1. Problem, proposed approach, and project objectives

Problem statement: investigate whether a room-installed Wi-Fi sensing system can identify possible falls in elderly care homes and provide useful room context for staff. The proposed sensing modality is Wi-Fi channel state information (CSI). The radio topology, receiver platform, learning method, calibration procedure, and operational targets remain to be selected through literature review and experiments.

Proposed initial scope: a research prototype for one person in a known room, followed by a small multi-room setup. The intended outputs are occupancy, movement state, room estimate, a possible-fall event, and eventually a coarse occupant count. Staff workflow, acceptable alert burden, and the types of falls that matter most should be established through the care-home discussions already being organized in the Questions tab.

The five objectives, in the team's requested order

1. Presence detection: distinguish an empty room from an occupied room. Evaluate both moving and stationary occupants. A detector that only responds while someone moves has completed a motion-presence subtask, rather than the full occupancy objective. Include an unknown/unreliable state when evidence or radio coverage is insufficient.

2. Movement detection: for a single occupant, distinguish moving from stationary over a stated observation window. Define small motions, transfers, and ordinary walking separately during evaluation. Treat fans, doors, pets, staff movement, and movement through an adjacent wall as possible confounders.

3. Localization: track the occupied room and transitions of a single person across a small set of rooms, such as a bedroom and bathroom. Room-level localization is the initial target; continuous coordinates and personal identity are additional problems. Once several people are present, an occupancy map alone does not identify which person changed rooms.

4. Fall detection: distinguish a possible fall from ordinary and fall-like activities. Large movement is a candidate event, not a sufficient fall definition: sitting quickly, lying down, dropping an object, or a caregiver assisting a transfer can also produce strong changes. The test set should also address slower collapses or slides if the partner considers them important.

5. Counting: classify room occupancy into 1, 2, and 3+ people, with 0 retained for empty rooms. Report an unknown state when the count is unreliable. This stage is useful both as a separate capability and as a way to characterize when a single-person fall model is outside its evaluated operating conditions.

The order above is the presentation roadmap. Work on datasets, fall-like negative examples, and hardware limitations can proceed earlier. Counting need not block a first single-person fall experiment, and failure to observe motion must not automatically suppress every fall candidate.

What CSI measures and why architecture matters

CSI describes the received radio channel across OFDM subcarriers and, where supported, antenna pairs. A person's movement changes reflected propagation paths. The receiver estimates these changes from received packets; a transmitter supplies the traffic used for sensing. Subcarriers are frequency measurements, not physical anchors, and an ESP32's known location does not by itself turn it into a position beacon for a person. [H1] [H2]

Useful distinctions for this project: a sensing link is a transmitter–receiver pair; a radio node is a physical device; an antenna pair is a measurement channel within a link; and a network mesh is a data-forwarding arrangement. A paper with nine antenna-pair channels may still use only two physical nodes. The topology that transports logs does not have to be the topology that provides sensing coverage.

2. Literature: fall detection and evidence closest to deployment

[R1] WiFall: Device-free Fall Detection by Wireless Networks — INFOCOM 2014

Problem and setup: detect falls without a wearable using commercial 802.11n devices and CSI variability. The accessible primary abstract reports experiments in two indoor scenarios and layouts. Exact antenna counts and packet rates were not verified from the accessible material.

Result: the authors report average detection precision of 87% and a false-alarm rate of 18%. Preserve those metric labels; precision is not the same quantity as fall sensitivity.

Relevance and limit: an early feasibility baseline for commodity Wi-Fi. It does not establish a sustained alert rate in elderly homes. Treat it as a starting point for understanding signal variability, not a performance target for a deployed system.

[R2] Anti-Fall: A Non-intrusive and Real-time Fall Detector Leveraging CSI from Commodity WiFi Devices — 2015 preprint

Setup: one TP-Link WDR5300 transmitter antenna and a Dell laptop receiver with an Intel 5300 and two receive antennas; 5 GHz traffic at 100 packets/s. Experiments used a 3 × 4 m office and 6 × 6 m meeting room, six students, and 230 falls plus 510 fall-like activities collected over two weeks with camera ground truth.

Method and result: inter-antenna phase differences segment activities; amplitude and phase features feed classification. Reported fall-detection rate was 89%, with 13% false positives. Moving a sofa reduced detection from 83% to 76% and increased false positives from 9% to 34%.

Relevance: directly motivates layout-shift testing. It is related to the later RT-Fall research and should not be counted as an independent deployment replication.

[R3] RT-Fall: A Real-Time and Contactless Fall Detection System with Commodity WiFi Devices — IEEE TMC 2017

Setup: TP-Link WDR5300 TX and two-antenna Intel 5300 RX, 5 GHz, 100 packets/s. Six volunteers aged 21–32 performed staged falls across four settings; video and a protective mattress were used.

Method: phase-difference segmentation and amplitude/phase SVM features. Room-specific models achieved approximately 91% sensitivity and 92% specificity. Furniture blocking a dominant path reduced one test from 89% to 57% sensitivity and 92% to 59% specificity; retraining recovered much of the loss.

An office-trained model averaged 71% sensitivity and 72% specificity in other settings. This motivates held-out-room and layout tests; the falls were simulated by younger adults.

[R4] FallDeFi: Ubiquitous Fall Detection using Commodity Wi-Fi Devices — PACM IMWUT 2017

Setup now verified from the supplied thesis: two Linux/Intel 5300 laptops per sensing link, each with two external omnidirectional antennas, modified drivers/firmware, monitor mode and CSI Tool. Measurements use the 5.2 GHz band, approximately 1,000 packets/s, 100-byte payloads and 10 s traces. Three volunteers aged 27-30 perform 326 staged falls and 744 non-fall activities across corridor, bedrooms, kitchen, bathroom and lab settings. Bathroom/toilet positions with insufficient safe space receive only non-fall trials. [T1: pp. 85-92; PDF 104-111]

Method and results: amplitude interpolation, wavelet denoising, PCA, STFT, candidate-event detection and an RBF SVM. The above-93% familiar-setting headline refers to the richer Original Features variant. The thesis's robust Selected Features comparison reports same-dataset accuracy/TPR/FPR of 88.90/94.33/14.92%, later changed-session results of 80.90/90.48/27.87%, and lab-to-other-room results of 80.10/81.56/21.56%. These are staged-activity metrics, not alerts per month. Target-room spectrogram/event-threshold calibration remains necessary. [T1: pp. 89-108; PDF 108-127]

Relevance: a detailed interpretable fall baseline with concrete layout, day, furniture, person and coverage tests. Section 14 reviews the thesis account and its FreeFall successor, including preprocessing, rate ablations, exact comparison tables, and limitations. Original journal PDF access still has the reported certificate problem; the thesis provides primary author evidence for the details above. The implementation repository remains available [C4]. The thesis explicitly says live operation and long-term domestic/elderly validation were not implemented. [T1: p. 148; PDF 167]

[R5] Deep Learning-Based Fall Detection Using WiFi Channel State Information — IEEE Access 2023

Source status: the complete published PDF supplied by the team has now been reviewed, including the hardware experiments, room plans, and Tables 2–7. Publication page numbers below refer to the article's printed pages; the uploaded file includes a repository cover sheet.

Research question: can a fall classifier retain useful performance when the training data include varied people, propagation paths, and rooms? The study compares several capture arrangements before choosing its final hardware, making it especially relevant to our unresolved receiver choice.

Final capture setup (Section II-C, pp. 83769–83770): one Intel NUC D34010WYKH in injection mode transmits to one Lenovo ThinkPad R61 in monitor mode; both use Intel 5300 NICs. One TX antenna and three RX antennas supply three antenna-pair channels, each with 30 measured subcarriers: 90 amplitude streams. Traffic is typically 5.2 GHz at 1,000 packets/s. This is one fixed link between two computers, not a multi-room mesh or an ESP32 deployment.

Hardware experiments (Section II, pp. 83766–83769): the authors first tested pairs of ESP32-based NodeMCU and TTGO T8 boards at about four metres with line of sight and a configured 200 packets/s. They observed spikes and residual low-frequency fluctuations even after filtering, overlapping ordinary-motion signatures. Their tested implementation collected below roughly 200 packets/s. They judged these captures useful for detecting activity but inadequate for reliably distinguishing activities, including falls.

Interpretation for our hardware: those ESP32 findings apply to the boards, firmware, rates, interference, and processing evaluated in this study; they do not prove that all ESP32 fall classifiers are impossible. Reproduce empty-room noise, stationary occupancy, walking, and fall-like activity measurements on our exact board before selecting the fall model. Sampling rate alone is insufficient: downsampled clean multi-antenna CSI is not equivalent to noisy single-antenna CSI captured at the same nominal rate.

Router experiments: with an Intel 5300 receiver, Netgear WAC104 AC1200 traffic produced periodic fluctuations; the tested TP-Link AC1200 setup produced substantial residual noise. Sagemcom Fast 5364 AC1200 gave cleaner activity signatures, but AP/client capture still suffered disconnects requiring firmware reload. The authors therefore selected a two-computer injection/monitor arrangement. This supports testing specific radio/toolchain combinations instead of assuming a commercial router or an open router is interchangeable.

Dataset and labels (Section IV and Table 2, pp. 83772–83773): 757 manually labeled five-second events—321 falls and 436 non-falls—from 22 volunteers aged 24–43, seven women and fifteen men. Four environments yield five room-specific subsets because Home Lab C contains two living rooms. A GoPro video with timestamp overlay supplies ground truth. Raw recordings typically contain repeated activities over one to two minutes.

Room subsets: living room A, 248 events (91 falls/157 non-falls), with roughly 4.7 × 4.5 m floor-plan dimensions; meeting room B, 134 (58/76), roughly 7.9 × 6.5 m; Home Lab C left living room, 85 (40/45), and right living room, 85 (36/49); lecture room D, 205 (96/109), roughly 10.4 × 7.6 m. In C, the TX is in the entrance area and the RX is in the left living room; right-room activities lie off the dominant path. Some small furniture was repositioned between sessions.

Activities: forward, backward, left, and right staged falls onto a mattress, with varied preceding movements and comfortable speeds; non-falls include bending/picking up an object, sitting down and standing up from a mattress, walking, arm waving, and no activity. Living-room A falls also use a sofa. The study does not label the full five-stage roadmap: its main learned output is binary fall/non-fall, not occupant identity, room tracking, or count.

Model (Section III, pp. 83770–83771): five seconds of amplitude CSI are downsampled by a factor of eight, from 5,000 to 625 time samples. A 625 × 90 array is reorganized into 3 × 625 × 30, assigning one receive-antenna channel to each image channel. ImageNet-pretrained EfficientNet B0/B1 is adapted with fully connected layers, SiLU activation, and dropout for binary classification. Wavelet denoising was optional and made little difference in the reported comparison. Training uses PyTorch and an RTX 2070 GPU.

Headline result and split (Section V-B and Table 3, pp. 83774–83775): each training run randomly assigns 80% of the chosen dataset to training and 20% to validation. Modified B0 without denoising reports 96.8% accuracy, 96.0% precision, and 96.9% recall; modified B1 reports 96.1%, 94.8%, and 96.3%. This split does not hold out complete people, homes, or recording sessions. The near-99% results elsewhere must be read with their own training/test conditions.

Rooms represented during training (Table 4, p. 83776): after training with the entire collected dataset, modified B1 reports 97.5% accuracy for the grouped dominant-path rooms, 92.0% for the right living room C, and 95.5% for lecture room D. These results are not tests of a completely unseen environment. The paper's narrative sometimes says 'over 92%' while Table 4 gives 92.0%; retain the table value.

New room plus different people (Table 5): training uses meeting room B and both living rooms C. The six participants in living room A differ from the other rooms' participants. On A, modified B0 achieves 69.0% accuracy, 54.6% precision, and 90.0% recall; B1 achieves 67.5%, 54.2%, and 92.2%. High recall alongside low precision exposes the false-positive problem under this combined shift. On lecture room D, B1 reaches 78.5% accuracy. These are far more relevant to a generic-home claim than the pooled-data headline.

New environment with overlapping participant set (Table 6): train on meeting room B and lecture room D, then test Home Lab C. Modified B0 reaches 90.0% accuracy in C-left and 78.0% in C-right; B1 reaches 84.0% and 80.0%. For B1 in C-right, precision is 72.4% and recall 91.3%. This shows a meaningful transfer loss even when the people are represented in the training population. The article has inconsistent figure lettering in this table caption; the room labels and accompanying description establish the comparison.

Latency and compute (Table 7): the modified B0/B1 implementations report under one millisecond preprocessing and under one millisecond GPU inference per five-second segment, with 5.3 million and 7.8 million parameters. These timings omit the observation window, packet collection, communications, and alert aggregation. They do not demonstrate sub-millisecond end-to-end fall alerts or inference on an Arduino/ESP32.

Deployment limits stated by the authors (Section V-C): no older-adult fall recordings; no fall collection in bathrooms or kitchens; no falls from stairs, chairs, or beds above ground level; and limited room/participant diversity. Mattress-cushioned, self-paced simulated falls may differ from natural events. The study does not report sustained alarms/day or alarms/month in ordinary homes. Its precision is measured on the selected event dataset, not a continuous operational alert stream.

Reproducibility resources: the authors' ENetFall repository contains training/test scripts and points to a dataset/model download. I verified the repository listing and scripts; I have not downloaded the dataset or executed the models. The paper is therefore a practical baseline candidate, with reproduction still to be completed. [C1]

Code-level checks before reproduction: current train.py performs a random event split, drops incomplete validation batches, and averages batch precision/recall. Its batch-size/epoch defaults differ from the article. Use fixed, documented settings and global event-level confusion counts for our evaluation. This static code inspection does not establish which repository revision produced every published table. [C2]

The current test.py evaluates all selected datasets without independently selecting a held-out split; it uses shuffle=True and drop_last=True, and computes normalization from the selected test collection. It can therefore omit an incomplete batch and use information about the test collection's distribution. For our baseline, retain an untouched test set, evaluate every event, and reuse training-fitted normalization. This is an implementation audit point, not a claim that all of the paper's results are invalid. [C3]

Implication for the capstone: retain an inexpensive ESP32 presence/movement experiment, with an explicit quality gate before fall classification. In parallel, reproduce the binary classifier on its native dataset if feasible. Test room/person/layout transfer separately and include off-dominant-path activities. A richer receiver may be necessary if the selected ESP32 cannot supply stable discriminative measurements; network mesh routing alone does not solve that measurement problem.

[R6] FallAware: An Explainable Learning Approach to Robust Fall Detection With WiFi — IEEE JSAS 2025

One TX–RX pair uses NXP 88W8997 2 × 2 radios (5.2 GHz, 40 MHz, typically 3,000 Hz), autocorrelation features, and an LSTM. Approximately 2,400 falls from over 25 volunteers yielded 94.1% detection in unseen environments.

Six environments contributed 21 aggregate months of non-fall observations, including older-adult homes: fewer than five alarms/month in single-person homes. Motion-triggered recordings were processed offline.

This is not prospective natural-elderly-fall sensitivity or a continuously live 21-month classifier. Concurrent movement, pets, and fans remain concerns; extra receivers are independent pairs, not fused localization.

[R7] CSI-Bench: A Large-Scale In-the-Wild Dataset for Multi-task WiFi Sensing — NeurIPS 2025

Setup: the broader benchmark spans many environments and device types. Its fall subset contains 2,770 fall and 3,930 non-fall samples from 17 participants in six environments, principally using NXP 88W8997 2 × 2 CSI at 100 Hz, with a smaller ESP32-S3 subset. Participants in the dataset are adults aged 23–42.

Result: a ResNet18 fall baseline reports 94.88% overall accuracy. Performance varies sharply by difficulty: the hard ESP32 tier reports 68.84% accuracy and 68.08% weighted F1.

Evaluation caution: the specialist protocol uses five-second windows and a random 70/15/15 split. Separate cross-user/environment/device evaluations elsewhere in the benchmark should not be attributed automatically to that fall headline.

Relevance: a useful dataset and reproducibility starting point. Device-specific results and vendor-dependent extraction show why retail hardware interchangeability must be tested rather than assumed.

3. Literature: presence, movement, and stationary occupancy

[R8] WiDetect: Robust Motion Detection with a Statistical Electromagnetic Model — PACM IMWUT 2019

Setup: Intel Galileo Gen2 hosts with three transmit and three receive antennas, providing nine antenna-pair channels; 5.805 GHz and 40 MHz. The operational detector can use 30 Hz CSI with a two-second window; higher-rate data were also used for model validation.

Method and result: autocorrelation of CSI power and a statistical test identify motion. A house experiment of approximately 200 m² over one week reports 99.68% true-positive rate and zero observed false alarms; the paper also studies longer apartment measurements and coverage.

Relevance: an interpretable baseline before deep learning. It detects environmental motion, including non-human sources, rather than guaranteeing human occupancy. Zero observed alarms in a finite test is not a zero underlying false-alarm probability.

[R9] Time-Selective RNN for Device-Free Multi-Room Human Presence Detection Using WiFi CSI — TCD-FERN, 2023 preprint

Setup: TP-Link WDR4300 routers in fixed star configurations: one TX and one RX for two rooms, or one TX and two RXs for three rooms. Two antennas per node provide four antenna-pair channels per link; 2.447 GHz, 20 MHz, 56 subcarriers, and 10 Hz CSI.

Method and result: GRU-based temporal selection, subcarrier fusion, and voting classify room presence. Tests include walking and standing still in neighboring laboratories and a conference space divided into three rooms. Reported three-room presence accuracies are 97.35%, 98.45%, and 97.57%.

Relevance: directly addresses our first and third objectives. It predicts occupancy by room, not an identified person's continuous trajectory, and the limited evaluated layouts do not establish generic care-home calibration.

[R10] FarSense: Pushing the Range Limit of WiFi-based Respiration Sensing with CSI Ratio of Two Antennas — 2019

Setup: two Intel 5300 mini-PCs, one transmit antenna and two receive antennas, 5.24 GHz, approximately 100 packets/s. A complex CSI ratio between receive antennas suppresses shared measurement noise and exposes small periodic changes.

Result: controlled respiration tests include seated sensing at eight metres with error below 0.5 breaths/min, as well as bed-posture experiments.

Relevance: illustrates why a nearly stationary person can still affect CSI. However, a respiration estimator is not a demonstrated general occupancy detector. Weak reflections, unfavorable geometry, walls, and multiple people complicate sensing. Failure to recover a breathing trace should not be interpreted automatically as an empty room. The antenna-ratio method also requires suitable coherent multi-antenna CSI, which a single-chain ESP32 does not directly provide.

4. Literature: localization, counting, and representation learning

[R11] Wi-CaL: WiFi Sensing and Machine Learning Based Device-Free Crowd Counting and Localization — IEEE Access 2022

Setup: four paired ESP32 TX–RX links, eight boards total, spanning horizontal, vertical, and diagonal room paths; 100 Hz and 52 usable subcarriers. Rooms were 5.5 × 5.5 m and 11 × 5.5 m, with up to five and ten people respectively. Six-second windows overlap by three seconds; three different-day sessions support leave-one-session-out testing.

Method and result: feature-based LightGBM predicts count and the crowd's room quadrant. Reported count error is 0.35 and 0.41 people; importantly, the paper defines MAE as MEDIAN absolute error. Quadrant localization accuracy is 91.4% and 98.1%.

Relevance: concrete evidence for low-cost ESP32 counting. Participants walked, and quadrant localization describes a group distribution rather than tracking individuals between rooms. Stationary-person counting and performance with only one receiver remain open for our design.

[R12] Widar2.0: Passive Human Tracking with a Single Wi-Fi Link — ACM MobiSys 2018

Setup: one TX antenna and a three-antenna coherent linear-array RX, using Intel 5300 devices at 5.825 GHz and approximately 1,000 Hz. The paper also tests reduced sampling rates.

Method and result: combine angle of arrival, time of flight, and Doppler to track a moving person's reflected path. Reported position error is approximately 0.75 m across three environments and a 6 × 5 m area. The paper uses inconsistent mean/median wording around this summary, so the statistic should be rechecked before precise comparison.

Relevance: continuous position tracking is possible with richer measurements, but 'single link' does not mean single receive antenna. Two-person tracking was unsuccessful in the evaluated setup. This architecture is substantially different from labeling rooms using several inexpensive links.

[R13] Zero-Effort Cross-Domain Gesture Recognition with Wi-Fi — Widar3.0, ACM MobiSys 2019

Setup: one transmitter and six receivers in the experiments; Intel 5300 receivers have three antennas, with 5.825 GHz traffic at approximately 1,000 packets/s. Sixteen participants perform gestures in a roughly 2 × 2 m area.

Method and result: reconstruct body-coordinate velocity profiles from Doppler and classify them with neural networks. Reported gesture accuracy is 92.7% within a domain and approximately 82.6–92.4% across evaluated domains.

Relevance: motion representations may transfer better than raw room-specific amplitudes. These are single-performer gesture results, not fall sensitivity or stationary occupancy measurements. The receiver count and velocity reconstruction requirements make this an inspiration for feature selection, rather than an immediate ESP32 implementation recipe.

[R14] SHARP: Environment and Person Independent Activity Recognition with Commodity IEEE 802.11 Access Points — IEEE TMC 2023

Setup: Netgear X4S AC2600 traffic-generation routers and an ASUS RT-AC86U CSI monitor with four receive antennas, using Nexmon CSI, 80 MHz, and approximately 173 packets/s. Training and test conditions vary by person, day, and environment across bedroom, living-room, and laboratory settings.

Method and result: phase cleaning and Doppler-based representations classify empty, sitting, walking, running, and jumping. The worst reported mean accuracy for these five activities is approximately 95.99% across the evaluated settings.

Relevance: a useful model for testing reduced room-specific training. It is not a fall detector, and its multi-antenna wideband hardware is materially different from a classic ESP32 link. 'Environment independent' should be understood relative to the paper's test conditions.

[R15] WiMANS: A Benchmark Dataset for WiFi-based Multi-user Activity Sensing — 2024

Setup: an Intel 5300 transmitter–receiver computer pair, synchronized video labels, both 2.4 and 5 GHz data, and zero to five simultaneous users. The dataset contains 11,286 samples and more than 9.4 hours across three environments, with identity, location, and activity labels.

Relevance: provides a starting point for examining what breaks when multiple people move simultaneously, and for deriving a coarse counting task from annotated users. It is a dataset and baseline resource rather than a ready-to-deploy fall alert system.

Limit: its hardware and labeling assumptions may differ from our captures; cross-domain multi-user recognition remains difficult. Imported model weights should be evaluated against our measurements, and access conditions should be checked before committing to a reproduction.

5. Literature: optional pose estimation and an RF comparison boundary

[R16] DensePose From WiFi — 2023 preprint

Setup and method: nine channel measurements from three transmit and three receive antennas feed a model trained with camera-derived dense-body labels. This means nine antenna-pair channels, not nine receivers or six distributed room nodes.

Result: reported average precision falls from 43.5 in the same-layout evaluation to 27.3 under different layouts; dense-pose metrics also degrade. Rare poses and three or more people are challenging.

Relevance: shows that learned spatial representations can be rich, while layout transfer remains a problem. Average precision for pose estimation is not fall-detection accuracy. Full-body reconstruction would add substantial labeling and modeling work to our capstone and is not required for a first event classifier. Exact capture hardware should be checked before attempting reproduction.

[R17] GoPose: 3D Human Pose Estimation Using WiFi — 2022

Setup: five Intel 5300 laptops—one TX and four RXs—with three antennas per device and specially arranged receive antennas; 5.32 GHz, 40 MHz, and approximately 1,000 packets/s. Kinect supplies pose labels, with synchronization between sensing and ground truth.

Result: fourteen-joint pose estimation reports approximately 4.7 cm overall error across three rooms and roughly 4.8–5.2 cm in held-out-room tests.

Relevance: demonstrates a more demanding multi-receiver approach to spatial sensing. Joint-coordinate error is not an alarm metric, and the setup does not establish an ESP32 equivalent or sustained performance in elderly care homes. Consider pose estimation only as a later option if simpler fall and room classifiers prove insufficient.

[R18] Multi-Person Localization via RF Body Reflections — WiTrack2.0, USENIX NSDI 2015

Setup: a dedicated frequency-swept RF system operating across approximately 5.46–7.25 GHz, with antenna arrays and USRP-based processing. This is not commodity Wi-Fi CSI capture.

Result: the paper localizes up to five people, including stationary subjects using respiration-related reflections, with approximately 11.7 cm median error per horizontal coordinate.

Relevance: a useful comparison when discussing what richer RF measurements can provide. Its range resolution, bandwidth, and signal-generation architecture cannot be assumed for an ordinary router and ESP32. It should remain outside our proposed Wi-Fi hardware baseline unless the project intentionally changes sensing modality.

6. What the literature implies for this project

Evidence strength and comparison limits

For fall detection, prioritize both controlled fall sensitivity and sustained non-fall observations. FallAware is particularly useful for operational negative-data evaluation; CSI-Bench is useful for device and dataset diversity; RT-Fall exposes layout and transfer weaknesses. These complement one another rather than establish a single universal accuracy figure. [R3] [R6] [R7]

Reported precision, sensitivity, specificity, weighted F1, balanced activity accuracy, count error, pose error, and alarms/month answer different questions. Do not rank papers by their headline percentages without checking labels, test population, train/test split, room changes, hardware, and whether the unit of evaluation is a window or a distinct event.

Several high-performing systems use coherent multi-antenna receivers, wide bandwidth, or much higher sample rates than a minimal ESP32 capture. A published algorithm can inform our design without its performance transferring to our board. First reproduce the measurements the algorithm expects, then compare models.

Stationary people, furniture, and carpets

The presence objective needs its own stationary-person tests. Sitting, sleeping, and remaining still must not be treated as equivalent to an empty room merely because a motion score is small. Candidate approaches include room-presence classification, carefully controlled occupancy history, and micro-motion features where observable. Each should be tested independently rather than silently substituting motion for occupancy.

The core generalization question is whether the same pipeline remains usable after changes in furniture, node location, doors, and room geometry. Compare a model with no local adaptation against one with a short, explicitly documented calibration procedure. Report both results, rather than presenting recalibrated results as calibration-free transfer.

Carpet is a sensible experimental variable, but this review did not identify an isolated carpet-specific accuracy penalty or a validated carpet-invariant calibration method. Avoid assigning it an invented percentage. Test a controlled flooring/rug change while holding node geometry and the activity protocol fixed, and distinguish it from simultaneously changing the whole room.

Generic deployment across most elderly care homes should be a research aim, not a demonstrated requirement met by the current literature or our prototype. An achievable capstone claim is an evaluated operating envelope: specified rooms, installation rules, calibration cost, people, activities, and known failure cases.

Single-person assumptions and room context

Begin with a single-person model and deliberately test what happens when a second person enters. The output may need to become 'uncertain' while retaining the candidate event for analysis. Coarse count is not personal identification, and room occupancy is not proof of who fell.

Multi-room estimates should be evaluated against movement next door and through walls. A link can be affected by motion outside the room assigned to it. Use multi-link evidence and a transition model where useful, while measuring the cases where spatial filtering fails.

7. Hardware feasibility: evaluate the capture chain before choosing the router

The proposed router + Arduino RX + multiple ESP32 TX idea

This is a plausible experiment, but it is not yet a validated capture architecture. 'Open-source router' describes software availability, not automatic access to CSI. 'Arduino' describes a development ecosystem or board family, not a guaranteed CSI-capable radio. We need the exact receiver chip, driver/firmware, antenna arrangement, supported PHY, packet format, and extraction interface.

If 'Arduino RX' means an ESP32-class board programmed through the Arduino framework, first confirm that the chosen SDK integration exposes reliable CSI and metadata. If it means another Wi-Fi board, do not assume it can export channel estimates. A CSI-capable receiver can listen to suitable packets from an ordinary router; the transmitter need not always expose its own CSI. [H1]

Compatibility proof: choose one transmitter and one receiver; send identifiable packets on a fixed channel; confirm that the receiver obtains the expected CSI for those packets at the required rate. Repeat after reboot and with real room motion. Confirm direct transmitter identity rather than assuming an AP-forwarded packet still represents the original transmitter–receiver path.

[H1] Espressif ESP-CSI — official examples

Espressif provides csi_send, csi_recv, and csi_recv_router examples, plus motion/presence demonstration code. These support starting with an ESP32 receiver and either another ESP32 or a router providing traffic. They establish a development route, rather than the performance of a generic care-home fall system.

[H2] ESP-IDF Wi-Fi CSI documentation — capture details to retain

For the documented classic ESP32 API, CSI collection uses esp_wifi_set_csi_rx_cb, esp_wifi_set_csi_config, and esp_wifi_set_csi. Record the receive-control metadata along with the channel buffer. Buffer interpretation depends on PHY/LTF, bandwidth, and related settings; values are interleaved imaginary/real components, and first_word_invalid marks an invalid initial segment.

Engineering implication: pin the board, ESP-IDF version, firmware configuration, buffer interpretation, channel, and sample rate. Do not compare unnormalized raw layouts across devices. The selected documentation is for the original ESP32; capabilities should be rechecked for the exact ESP32 family member used.

[H3] FeitCSI — a richer PC receiver alternative

FeitCSI, by Miroslav Hutar, Peter Brida, and Juraj Machaj, supports CSI extraction and frame injection on Intel AX200/AX210 PCIe hardware under Linux, with broader PHY and bandwidth options than legacy Intel 5300 tooling.

Potential use: a higher-fidelity capture baseline if the team needs measurements unavailable on the selected ESP32. This review has not demonstrated an end-to-end ESP32-transmitter/FeitCSI-receiver link. Driver installation, packet compatibility, antenna access, and data-rate stability would be explicit feasibility work before adopting it.

[H4] Nexmon CSI — supported chip and firmware combinations

Nexmon CSI supports specific Broadcom chip/firmware combinations, including the ASUS RT-AC86U configuration used in relevant research. The project's compatibility list, rather than a router's retail name or OpenWrt support alone, should drive selection. Hardware revision and firmware version are part of the capture contract.

Potential use: reproduce richer router-based amplitude/phase measurements. Cost and maintenance effort should be compared with an ESP32 baseline and a PC-based receiver, using actual obtainable hardware before procurement.

[H5] Atheros CSI Tool for OpenWrt — packet-format compatibility

The documented Atheros collection workflow requires appropriate HT sounding traffic and patched tooling on the relevant endpoints. This demonstrates why an open router and arbitrary ESP32 packets cannot be assumed to form a working CSI link.

Potential use: reproduce older router-based research if an exact supported hardware/firmware chain is available. Treat it as a specific ecosystem, not a generic property of any open-source router.

8. Architecture options and the experiment that should choose between them

Recommendation: start with fixed sensing links and ordinary log transport. Compare sensing coverage first; introduce mesh routing only if it solves a demonstrated transport or coverage problem. This is an engineering proposal for the capstone, not a conclusion that one topology works universally.

Option A: several transmitters, one CSI receiver

Example comparison setup: three room-positioned ESP32 TXs and one central RX, four radio nodes in total. Label every packet by source and preserve per-link measurements. This follows the team's current idea and centralizes capture.

Questions to resolve: are packets from every room directly observable at the receiver; do walls or furniture cause weak links; can the receiver sample each source consistently; and does link-specific activity distinguish a person's room? Multiple transmitters add different propagation paths, but every path still terminates at the same receiver.

Option B: one transmitter, several CSI receivers

Example comparison setup: one TX and three room-positioned RXs, again four radio nodes. Each RX extracts its own CSI and sends timestamped logs to a laptop/hub. This can create measurements local to different rooms, at the cost of distributed capture and synchronization.

Questions to resolve: can one TX illuminate all rooms; which receivers remain useful with doors closed; what timing accuracy is needed for feature-level fusion; and how much installation effort is added? Compare this option with A at a matched node count and comparable per-link sampling budget.

Option C: room-specific TX–RX pairs

Place a link within each priority room and aggregate room-level features or predictions at a hub. This may simplify local coverage and failover, while requiring more nodes. Consider it when a central star leaves an important room poorly observed. The Wi-CaL setup illustrates a multi-link low-cost sensing arrangement, though its task and room geometry differ from ours. [R11]

Transport, timing, and packet budget

Record which path is actually being sensed. A network forwarding path is not automatically the human-sensing path. Fix channel, antenna orientation, node placement, and transmit schedule during initial experiments; test changes individually later.

Budget sample rate per link, rather than quoting only aggregate traffic. For example, 300 received packets/s evenly split across three transmitters is 100 samples/s per transmitter before losses. This is an illustrative allocation, not a validated fall-sensing rate. Log the distribution of inter-packet gaps, not just the average rate.

For feature-level fusion, timestamp alignment must be accurate relative to the feature window and measured latency. Shared timestamps or NTP do not make independent radios phase-coherent; do not combine their raw phase as if they were one coherent antenna array.

Compare A, B, and a limited version of C using per-room signal quality, packet loss, movement separability, stationary occupancy performance, cross-room confusion, and layout robustness. Select the smallest architecture that meets the evaluated coverage goal. Add a receiver or transmitter only when it measurably improves a weak area or capability.

Provisional prototype choice

First establish a two-node ESP32 capture and motion baseline using official examples. Expand to a matched star comparison if capture is stable. Keep Intel/FeitCSI or a supported Nexmon receiver as a branch if the experiment requires multi-antenna phase, wider bandwidth, or cleaner measurements than the chosen ESP32 supplies.

Do not commit the full project to an algorithm requiring coherent antenna ratios, angle of arrival, or wideband measurements before verifying that our receiver can supply them. This is the main hardware risk to resolve before buying a larger set of nodes.

9. Proposed data, preprocessing, and model plan

Capture and preprocessing

Store raw CSI and the metadata needed to reproduce it: node IDs, source MAC, packet sequence or equivalent marker, local timestamp, channel/bandwidth, PHY information, antenna/channel layout, RSSI, reported noise, firmware version, placement, room, activity labels, and quality flags. Keep a configuration manifest for every recording session.

Check invalid samples, changing buffer lengths, packet gaps, saturation, and subcarrier selection. Separate per-link streams before windowing. Compare amplitude-based baselines first, adding phase-derived features only when phase quality and sanitization have been verified. A model should not learn packet-format changes as human activity.

Expose quality to downstream tasks. If a link stops delivering useful CSI, mark the estimate uncertain rather than reporting a confident empty room. Measure restart behavior and recovery after a transmitter or receiver disconnects.

Candidate models, ordered by what each experiment resolves

Movement baseline: compare a simple CSI-power variability score with autocorrelation-based features. The purpose is to establish coverage, useful windows, and common confounders before adding a complex model. [R8]

Presence baseline: evaluate supervised empty/occupied features and a temporal model, including long stationary segments. Compare a current-window estimate with an occupancy-history model that uses confirmed entries/exits. Report where history becomes stale or ambiguous. [R9]

Room baseline: concatenate link-specific features, then classify room occupancy and use transition smoothing if it reduces flicker. Keep an unsmoothed result for comparison, and test whether smoothing hides rapid or unusual transitions.

Fall baseline: reproduce a feasible public-data classifier, then compare feature-based event classification with a compact temporal model on our own captures. Candidate representations include amplitude statistics, spectrograms, and autocorrelation/motion features. Model comparisons must use the same held-out recordings.

Counting baseline: train a classifier directly for 0/1/2/3+ rather than claiming exact counts from a small dataset. Compare one link with multiple links, moving people with stationary people, and room-specific training with transfer. Coarse bins match the team's goal and expose errors relevant to single-person assumptions.

Physics-informed motion representations are worth comparing with raw CSI when they are implementable on our hardware. They may reduce room dependence, but the comparison itself must demonstrate that improvement. Full pose reconstruction is an optional branch after the core objectives.

Calibration as a measured requirement

Proposed calibration modes: (A) no room-specific data beyond installation checks; (B) a short known-empty recording and a scripted walking path; (C) a small labeled occupied/empty session. Record actual time, staff instructions, needed labels, and retraining effort for each mode. These are experiment conditions, not a proven installation procedure.

Evaluate each mode on a new room before and after furniture movement. Distinguish a threshold reset, normalization update, classifier retraining, and adding labeled falls: they impose very different deployment costs. Prefer an approach that needs ordinary activities rather than falls to calibrate a new room, if the data support it.

Adaptive backgrounds need safeguards: a motionless occupant must not simply be absorbed into an 'empty' reference. Update a known-empty baseline only when emptiness is independently established in the evaluation, or explicitly measure the consequences of a weaker assumption. Log every adaptation event so that apparent transfer performance can be audited.

Proposed change detection: track link quality and sustained shifts in feature distributions. Test whether the system can request recalibration after relocation, furniture changes, or radio configuration changes. A drift flag is useful even if fully automatic recalibration is not achieved.

10. Validation: design tests that answer the real deployment questions

Data split and ground truth

Split by complete recording session before creating overlapping windows. Put a person's, day's, room's, or device's held-out recordings entirely into the corresponding test set. Nearby windows from the same staged event must not appear on both sides of a split.

Maintain separate tests for: within-room new sessions; unseen participant; unseen day; furniture/layout change; completely unseen room; changed device or radio settings; and multi-room or multi-person interference. A strong random-window result does not replace these tests.

Ground truth should label occupancy, movement, room transitions, activity start/end, possible-fall events, and ambiguous cases independently. Use consented temporary video or an equivalent annotation method for research where appropriate; a camera-free deployed sensing concept can still use video labels during controlled development.

Begin fall modeling with existing datasets. Any collected staged falls require a safe, approved protocol with suitable participants and supervision. Do not ask elderly residents to perform falls. An eventual partner evaluation can first collect ordinary non-fall activity and installation/coverage observations.

Environment and confounder matrix

Room and installation tests: bedroom-like space, bathroom-like space if available, hallway transition, open/closed doors, furniture in original and changed positions, bed/chair orientation, node placement and height, different link directions, and movement in an adjacent room.

Flooring test: compare rug/carpet conditions using the same people, actions, and node geometry. Avoid changing several factors at once if the purpose is to isolate the effect of flooring. Document when the practical room comparison cannot isolate a single factor.

Activity negatives: walking, sitting quickly, getting into or out of bed, lying down, crouching, picking up an object, kneeling, dropping objects, chair movement, assisted transfers, and a visitor entering during another person's activity. Select the final list with the supervisor and care-home partner.

Other motion tests: fans, HVAC-related movement, curtains, door movement, pets if relevant, cleaning activity, walkers/wheelchairs, and movement outside the target room. Capture ordinary background Wi-Fi traffic and test packet-rate loss or interference as a separate condition.

Stationary tests: a seated person, a person lying in bed, long quiet occupancy, an empty room immediately after departure, and two stationary occupants. State the duration covered by each test; a short pause is not evidence of all-night presence detection.

Metrics and alarm semantics

Presence: occupied sensitivity, empty specificity, precision, balanced accuracy, stationary-occupant recall, and duration of unknown output. Report room and participant breakdowns.

Movement: recall for each movement category, false motion triggers during empty and stationary periods, detection delay, and coverage by position. Separate human motion from nuisance motion.

Room localization: room confusion matrix, transition detection delay, missed transitions, time spent in the wrong room, and uncertainty. If coordinates are later attempted, report positional error separately.

Fall events: sensitivity = TP/(TP + FN), precision = TP/(TP + FP), and specificity = TN/(TN + FP) for a stated negative-event definition. Also report distinct false alerts per observed hour/day, event detection latency, and the activities generating those alerts.

Define event aggregation before evaluation: candidate-window merging, refractory period, threshold, and whether repeated outputs from one episode count as one or many alerts. Freeze these choices on a validation set, then apply them unchanged to the test set.

Illustration of why the unit matters: a continuous detector evaluating non-overlapping five-second windows makes 17,280 decisions/day. A 1% negative-window false-positive rate would produce about 173 positive windows/day before event merging, if every window were a negative. This arithmetic is not a conversion to 173 distinct alerts/day; temporal dependence and aggregation change that quantity.

Counting: confusion matrix for 0/1/2/3+, per-class recall, exact-bin accuracy, confusion between one and multiple people, and unknown duration. If numerical error is included, define explicitly whether it is mean or median absolute error.

Operational accounting: state observed hours per home, number of homes, number of sensor pairs, uptime, and whether processing was online or offline. Aggregate observation months across homes must not be described as one home observed continuously for that duration.

Sustained non-fall recording

After controlled classification tests, run a locked model on long ordinary recordings. Extend duration and diversity rather than only adding balanced staged examples. Log every alert with time, room estimate, quality state, and ground-truth review.

A zero-alert result should always include its observation duration and environment count. Select an acceptable alert burden with the partner before treating it as a success criterion. Do not silently tune away test-set false alarms and report the same recordings as an untouched evaluation.

Report staged-fall sensitivity and sustained non-fall alarm burden as separate evidence. Neither alone demonstrates prospective performance on natural elderly falls. If natural events are unavailable, retain that gap explicitly in the supervisor report.

11. Stage plan, completion evidence, and fallback paths

Stage 0 — capture feasibility and literature reproduction

Deliverable: one reliable link, raw-data logger, metadata/configuration manifest, quality plots, and a reproducible preprocessing baseline. Check direct packet attribution, packet rate/gaps, restarts, and response to a known walking path. Review the papers most relevant to the actual receiver.

Decision gate: if CSI cannot be collected consistently or the required representation is unavailable, change the receiver/platform before collecting a large dataset. Stage 0 is supporting work for the team's five objectives, not a replacement objective.

Stage 1 — presence

Deliverable: empty/occupied classifier with separate moving and stationary evaluation. Start with one room and then test a new day, person, and layout. Record false occupied outputs and false empty periods, not just window accuracy.

Decision gate: establish the supported stationary duration and positions. If reliable stationary sensing remains unresolved, report the achieved motion-presence capability explicitly and keep full occupancy as an open objective.

Stage 2 — movement

Deliverable: moving/stationary detector for one person, with nuisance-motion and small-motion tests. Compare simple statistics with a more robust representation at a matched sampling rate.

Decision gate: identify blind spots and common false triggers before using movement segmentation to feed the fall classifier. Revisit placement or add a sensing link when coverage is the limiting factor.

Stage 3 — room-level localization

Deliverable: a small multi-room occupancy/transition prototype and an A/B topology comparison. Measure wrong-room time, transition latency, door effects, and adjacent-room movement.

Decision gate: select node count and installation rules from observed coverage. Keep continuous coordinates and identity tracking outside the initial claim unless separately demonstrated.

Stage 4 — possible-fall classification

Deliverable: event detector evaluated on staged/public falls, demanding non-fall activities, a held-out room/person, and sustained non-fall recordings. Include alert aggregation and end-to-end latency.

Decision gate: report sensitivity, false alerts per time, and major misses together. If cross-room generalization is weak, narrow the supported installation/calibration envelope and make that limitation part of the result.

Stage 5 — coarse counting

Deliverable: 0/1/2/3+ room classifier with moving/stationary and multiple-link comparisons. Analyze how second-person activity affects stages 1–4.

Decision gate: distinguish a useful count estimate from a guarantee that a fall detector is safe under concurrent movement. Counting is a stretch goal if the fall/generalization work consumes the schedule; preserve it in the roadmap and state what was actually completed.

12. Proposed contribution and supervisor decisions

Proposed capstone contribution: an affordable, reproducible Wi-Fi CSI prototype evaluated for staged capabilities, cross-room/layout robustness, and operational false-alert burden. The research value can come from a careful architecture and calibration comparison, even if the final fall classifier uses established methods.

Questions to resolve with the supervisor and partner: Which setting comes first—single-resident bedroom, bathroom, assisted-living unit, or long-term-care room? Which events and response information are most valuable? What alert burden and detection delay are acceptable? How often do layouts change? What stationary duration is important? How often are visitors or staff concurrently present? What installation, power, networking, and data-retention constraints apply?

Project choices still open: exact budget and boards; supported receiver/toolchain; access to public datasets; available test rooms; approval and labeling protocol; target operating envelope; quantitative acceptance targets; and whether the first multi-room design uses shared TX, shared RX, or local pairs. Avoid inventing commitments until these are resolved.

Suggested next experiments: (1) verify one ESP32 capture chain; (2) compare empty, walking, and quiet occupancy over several positions; (3) perturb furniture and repeat without retraining; (4) compare two star topologies at matched node count; (5) establish a public-data fall baseline and evaluate difficult non-fall activities on the selected hardware.

For the later 1–2 page supervisor version, retain the problem, five objectives, proposed initial operating envelope, evidence for the key hardware/generalization risks, architecture decision experiment, and stage gates. Move this paper inventory and the full evaluation protocol to an appendix or supporting research note.

13. Reading order and unresolved evidence

Priority reading: the supplied thesis [T1] for presence, calibration, event segmentation and placement; FallAware for false-alarm evaluation; CSI-Bench for device diversity; RT-Fall for layout shifts; Wi-CaL for ESP32 links; TCD-FERN for room presence; WiDetect for interpretable motion; and ESP-CSI/ESP-IDF for the first capture experiment. Then read Widar2.0, Widar3.0 and SHARP if richer localization or transfer representations are needed.

Remaining evidence gaps: exact WiFall antenna/rate setup and full-text access; original FallDeFi journal PDF access (its setup and results are now substantially covered by the supplied author's thesis); dataset download/reuse terms; direct ESP32-to-selected-router/PC receiver capture; natural elderly-fall sensitivity; isolated carpet effects; stationary counting; care-home calibration/installation cost; and the thesis's unresolved split, metric-denominator and alert-timing details. Both the complete 2023 deep-learning fall paper and the 184-page 2019 thesis have now been reviewed from supplied PDFs.

This is a targeted literature and tooling review, not an exhaustive systematic review or a hardware compatibility demonstration. Paper-specific metrics above retain their task and test context. Candidate engineering steps are labeled as proposals; no device has been purchased, flashed, or tested as part of this research pass.

14. Detailed review of the supplied 2019 thesis

Source and reading scope: the supplied 184-page PhD thesis, Fall Detection using Channel State Information from WiFi Devices, contains three related contributions: nonlinear presence detection (Chapter 4), FallDeFi (Chapter 5), and the enhanced FreeFall system (Chapter 6). The introductory theory, literature review, experiments, and final limitations were reviewed across three parallel readers and reconciled against the source; key numerical tables and floor plans were visually checked. This is the author's thesis account of FallDeFi, rather than a claim that the inaccessible journal PDF was retrieved. Main-body printed page n corresponds to PDF page n + 19. Citations below give both numbers. [T1]

Why this source matters: it exposes the engineering behind the headline accuracy—supported CSI capture, signal preprocessing, local calibration, event segmentation, fall position, ordinary activity confounders, and incomplete deployment work. It provides useful precedents for presence, motion, and fall detection. Room tracking, occupant identity, and 1/2/3+ counting are discussed as other research problems or future extensions; they are not delivered by the thesis's fall classifier. The three contributions should retain separate hardware, datasets, and metrics.

A. CSI extraction, physical assumptions, and hardware

The receiver estimates complex channel responses from incoming OFDM packets. CSI exposes selected subcarrier channel measurements; RSSI is an aggregate signal-strength descriptor. The historical Intel 5300 tool exports 30 subcarrier groups at either 20 or 40 MHz. The thesis's monitor-mode capture uses modified Intel 5300 drivers/firmware and injected 802.11n HT frames at the transmitter, with a compatible receiver. Its report that client/AP configurations were less stable applies to that historical software stack. It does not mean every modern transmitter needs a CSI extractor. Current router/ESP32 compatibility must be established with the exact radio, firmware, extractor, and packet format. [T1: pp. 28-31; PDF 47-50]

Raw phase contains packet-detection, carrier/sampling-frequency offset, and IQ-imbalance effects. The thesis uses amplitude for its presence and fall contributions. Its Doppler-related amplitude features lose movement direction and should not be read as signed human trajectories. The channel model splits unaffected paths from human-affected paths, assumes no other moving objects and a simplified specular reflection, and excludes direct-link obstruction in its initial validation. A reported model correlation of 0.9 excludes the final 0.5 s where shadowing makes the model diverge; it is a limited channel-model result, not localization or fall accuracy. [T1: pp. 30-36, 69; PDF 49-55, 88]

Presence hardware: two laptops, ACER 5740 and Lenovo ThinkPad L520, Ubuntu 14.04, Intel IWL 5300 cards and three external antennas on each endpoint. Three TX antennas by three RX antennas give nine antenna-pair channels, each with 30 subcarrier groups: 270 amplitude dimensions per packet from two physical nodes. FallDeFi hardware changes to two external omnidirectional antennas on each of two Linux/Intel 5300 laptop endpoints per sensing link. FreeFall reuses that fall hardware. The chapters' AP labels refer to these experimental radio locations; they do not establish extraction from ordinary unmodified household routers. [T1: pp. 38, 56, 87, 127; PDF 57, 75, 106, 146]

Our hardware implication: an Arduino label does not specify a CSI receiver. Select and demonstrate the actual Wi-Fi chip and exposed capture interface before committing to the router + Arduino RX + ESP32 TX chain. Neither the laptop benchmark nor the coherent multi-antenna measurements establish equivalent ESP32/MCU performance. Prove one usable TX-RX link, then measure per-source sample rate, gaps, antenna/subcarrier layout, timestamps, and response to known movement before adding nodes. This is a proposed capstone sequence.

B. Presence detection: a useful baseline with a limited stationary test

The Chapter 4 detector interpolates CSI amplitude streams, applies Gaussian kernel PCA (or polynomial kernel PCA for comparison), selects an informative principal component, and thresholds its eigenvalue. Ordinary linear PCA is the baseline. Gaussian kernel width changes with the batch's median squared Euclidean distances to suppress empty-room noise while retaining occupied-room variation. The chosen component is PC2 in LoS and PC3 in blocked LoS; the first component can be dominated by noise. The detection threshold is the maximum selected-component eigenvalue across empty-room reference intervals. These are room-calibrated settings, not universal constants. [T1: pp. 47-54, 57, 64; PDF 66-73, 76, 83]

Final detector setup: a furnished 5 x 8 m research lab, 5 m radio link, 5.32 GHz channel 64, 20 MHz, 1,000 packets/s and 15 dBm TX power. Compare direct LoS with LoS blocked by desktop monitors, approximately 2.26 dB mean attenuation. Each link condition includes 12 stationary positions, two walking conditions, and two empty reference recordings, each 60 s: the execution narrative describes 32 recordings and 32 minutes total. Table 4.1 instead lists 15 experiments; retain that discrepancy. The earlier exploratory single-subcarrier tests use a different 6 m link at 100 packets/s, so their settings should not be merged with this detector evaluation. [T1: pp. 40, 55-58; PDF 59, 74-77]

The stationary subject deliberately performs subtle movements with regular pauses to resemble a working person. This is relevant to quiet occupancy, but does not establish detection of completely motionless, sleeping, or breathing-only occupants. Walking trials include crossing and not crossing the direct link. Each 60 s trace is divided into twenty 3 s windows of 3,000 packets. The reported TP percentages average detected windows across 14 occupied scenarios; they are not fall accuracy or a long-term occupancy metric. [T1: pp. 55-59; PDF 74-78]

Table 4.2 results, TP/FP: LoS linear PCA 88.3%/0%, Gaussian kernel PCA 90.6%/0%, polynomial kernel PCA 62.6%/0%; blocked LoS linear PCA 83.3%/5%, Gaussian kernel PCA 88.3%/0%, polynomial kernel PCA 62.6%/5%. Both walking conditions are detected fully in the scenario plots, while stationary results vary with position and are generally stronger near the sensing link. Zero observed FP comes from brief empty-reference data and a threshold based on the largest empty-reference eigenvalue; the thesis does not clearly establish an independent later-day false-alarm test. Do not translate this into zero false alarms in a care home. [T1: Table 4.2, pp. 58-62; PDF 77-81]

Calibration and compute limits: the presence scheme explicitly needs prior room knowledge to choose kernel parameters, principal components, and thresholds. No multi-occupant presence/counting evaluation is reported. The kernel matrix scales with observation count squared: a 3,000 x 3,000 dense float64 matrix alone is about 72 MB before eigensolver overhead—our implementation estimate, not a measured thesis RAM benchmark. Larger observation windows may improve detection in a shown scenario but add memory, computation, and response delay. The thesis does not benchmark embedded execution. [T1: pp. 57, 63-64; PDF 76, 82-83]

Proposed Stage 1 tests: separate empty, walking, subtle movement, still seated, and still lying conditions; map positions around the room and through blockages. Fit calibration only from training sessions, then test later sessions and layout changes. Report occupied-window recall, empty-room false detections, time until a quiet occupant is lost, and an unknown state. Stage 2 should independently measure moving versus stationary; absence of detected motion is insufficient evidence that a room is empty.

C. FallDeFi: exact setup, staged data, and composite environment changes

Each fall sensing link uses the two modified Intel 5300 laptops described above. Activities are captured in 10 s traces at approximately 1,000 packets/s using 100-byte payloads: 10,000 packets per trace and 30 subcarrier values per antenna pair per packet. The stated 800 kb/s is payload traffic, not total channel airtime or CSI-log transport. Experiments use the 5.2 GHz band. The exact experimental fall-channel bandwidth is not separately established by the Chapter 5/6 setup text; do not copy the Chapter 4 presence experiment's 20 MHz setting into it. [T1: pp. 87-89, 127; PDF 106-108, 146]

Tested links: kitchen 4 m LoS/cluttered; corridor 9 m LoS/uncluttered; lab 7 m nLoS/cluttered; bedrooms 5 m through one insulated/decoupled plaster partition; bathroom/toilet 5 m through two plaster partitions. The apartment floor plan shows three TX placements and two RX placements, five radios, with separate room-link analyses. Simultaneous capture, scheduling, joint fusion, and mesh routing are not established. Falls plus non-falls are performed at yellow-marked positions; blue-marked confined toilet/bathroom positions receive non-fall activities only for safety. Bathroom evidence therefore does not establish fall coverage of every bathroom/toilet position or every wall material. [T1: Fig. 5.10, pp. 85-87; PDF 104-106]

Table 5.2 contains 326 staged falls and 744 non-fall traces, 1,070 activities total, from three volunteers aged 27-30 with different physiques. Room fall/non-fall totals are corridor 82/135, bedroom 51/161, kitchen 69/189, bathroom 75/173, lab 49/86. Falls include simulated trips, slips, loss of balance, and loss of consciousness, with forward, backward, lateral, and on-position variants. Negatives include walking, jumping, sitting/standing on chair or floor, bending/picking an object, door movement, raising hands, and no event. At most two people are present, with the second person stationary or doing ordinary activity. This is controlled young-adult evidence, not natural elderly falls or demonstrated 3+ occupancy. [T1: Tables 5.2-5.3, pp. 88-92; PDF 107-111]

A/B sessions create meaningful combined shifts: corridor data are separated by seven days and TX moves 0.5 m; bedroom by 33 days with another occupant; kitchen by 33 days with furniture moved and another occupant; bathroom by ten days with TX moved 0.5 m and another occupant. Lab data provide the cross-room training environment. Because day, furniture, TX placement, and people sometimes change together, these are composite generalization tests rather than isolated causal furniture or carpet measurements. [T1: Table 5.2, pp. 88-89; PDF 107-108]

D. FallDeFi processing, feature selection, sampling, and calibration

Pipeline: linear interpolation of amplitude -> discrete wavelet denoising -> linear PCA over the 30 subcarrier streams of each antenna pair -> retain enough PCs to explain 95% variance -> STFT and averaged spectrogram -> noise/morphology processing -> power burst curve candidate-event detection -> features -> RBF SVM. This fall pipeline uses linear PCA, distinct from Chapter 4's kernel-PCA presence detector. Denoising is intended to preserve high-frequency motion while making the first PC usable. Selected implementation parameters include symlet with three coefficients, ten levels, soft thresholding, and per-level threshold rescaling; the four-level drawing is an illustration. [T1: Algorithm 2, pp. 67-79, 90; PDF 86-98, 109]

STFT settings: 512-sample Hamming window, 256-sample overlap at 1,000 Hz, about 1.95 Hz frequency spacing. Those settings imply a 0.512 s window and 0.256 s hop; the text's approximately 0.1 s time-resolution claim is arithmetically inconsistent. The power burst curve sums the 5-25 Hz spectrogram band to identify candidate activity intervals. It detects motion-like events; a large burst alone is not a fall decision. These particular settings are reproduction starting points, not verified ESP32 defaults. [T1: pp. 76, 79-81; PDF 95, 98-100]

Original features include extreme and torso frequency statistics, their ratio, spectral entropy across frequency bands, fractal/Hausdorff dimension of the extreme-frequency curve, event duration, and energy ratios. The robust selected-feature variant retains event duration, entropy in 1-10 and 10-30 Hz, and fractal dimension. High-frequency information weakens in distant/nLoS cases, which can make falls resemble ordinary nearby fast sitting or standing. Sequential forward selection seeks accuracy across different datasets; RBF SVM parameters use cross-validation. The thesis also uses SMOTE to address small samples/class imbalance, but does not fully specify selection-domain separation or whether oversampling occurs only inside training folds. These are reproduction questions, not proof that leakage occurred. [T1: pp. 80-85, 93; PDF 99-104, 112]

Pretrained does not mean locally calibration-free. Wavelet/PCA settings are selected once, but FallDeFi's spectrogram segmentation and event thresholds depend on the testing room. It estimates a noise threshold from several activity spectrograms in that environment and uses a median value for testing. Separate the labor for classifier training, per-link noise calibration, and physical coverage assessment in our installation requirements. [T1: pp. 89-91; PDF 108-110]

Sampling ablation uses recorded 1,000 packets/s traces downsampled offline to 200/400/600/800/1,000 packets/s. Performance is maintained around 600-1,000 and deteriorates at 400 and below in this particular feature pipeline. It is not a live transmitter-rate/network-contention experiment and does not prove all fall systems require 600 Hz. For our ESP32 route, test the actual achieved unique-sample rate, jitter, gaps, and classifier degradation rather than copying a requested TX rate. Higher carrier frequency produces larger Doppler frequency for the same motion, while penetration and interference also matter; the 5.2 GHz results do not automatically transfer to 2.4 GHz. [T1: pp. 95-96, 109; PDF 114-115, 128]

E. FallDeFi results: preserve feature variants and test protocols

The richer Original Features variant produces the familiar above-93% same-condition headline, but loses more under changes. The exact robust Selected Features results in Table 5.7 are preferable for comparing deployment transfer. A,A & B,B means 70/30 train/test within each room/session dataset; A,B trains on the earlier session and tests the later changed session of that room; Lab,A+B trains on lab data and tests other room datasets. The first protocol is not unseen-person or unseen-home validation. Numbers are author-reported averages, not a single pooled confusion matrix. [T1: pp. 96-100, 108; PDF 115-119, 127]

FallDeFi Selected Features, Table 5.7, percentages in the order accuracy / F1 / precision / TPR / FPR: same-dataset 88.90 / 90.24 / 86.65 / 94.33 / 14.92; later changed session 80.90 / 82.34 / 76.12 / 90.48 / 27.87; lab-to-other-rooms 80.10 / 80.15 / 78.95 / 81.56 / 21.56. The higher headline and these values describe different feature variants and conditions, rather than interchangeable accuracy claims. FPR is FP/(FP+TN) among sampled non-fall activities; it is not false caregiver alerts per day or month. [T1: Table 5.7, p. 108; PDF 127]

Thesis baseline comparisons on the same data: CARM accuracy/TPR/FPR is 77.85/80.32/24.63 for same-dataset, 75.68/81.70/30.34 for later-session, and 64.26/53.07/24.55 for lab transfer. RT-Fall is 80.96/85.20/24.00, 59.35/62.81/45.52, and 51.14/67.82/65.91 respectively. These are the thesis's reimplementations: CARM uses its candidate-event prescreener and a binary classification adaptation; RT-Fall uses downsampled phase-difference processing. They should not replace the original papers' own results or be mixed with a different dataset. [T1: pp. 103-108; PDF 122-127]

Spatial/person shifts remain important. The thesis shows reduced performance for different-person/different-room tests, with roughly ten percentage points lower accuracy than the same-person/same-room condition in the plotted comparison. Weak/distant links lose high-frequency content; ordinary events near strong links can resemble falls. Chapter 5 explicitly regards about 20% missed falls and about 20% sampled non-falls misclassified under transfer as inadequate for practical use. This motivates both the later FreeFall work and our need for long recordings of ordinary activities. [T1: pp. 98-102, 107; PDF 117-121, 126]

Compute timing: Table 5.8 reports 0.622 s CPU per second of CSI for FallDeFi, 0.604 s for CARM, and 0.341 s for RT-Fall on a 2.7 GHz Core i7 with 16 GB RAM. FallDeFi extraction accounts for 0.282 s and feature/classification for 0.300 s. This supports laptop processing throughput feasibility; it does not establish end-to-end alert delay, MCU runtime, or a working online service. The final chapter explicitly states real-time operation was still unimplemented. [T1: pp. 107-108, 148; PDF 126-127, 167]

F. FreeFall: separating an event from the activities inside it

FreeFall addresses a weakness of FallDeFi: one detected event can contain walking, a fall, and subsequent movement. Classifying the entire mixture can hide the fall. It improves amplitude sanitation/PC selection, constructs a Doppler-related velocity curve, splits the event into separate activities, and adds physically motivated features. The velocity curve is an instantaneous-frequency/path-change proxy without direction; attenuation, person position, and moving body parts can affect it. It is not independently validated ground-truth torso velocity, human height, or room coordinates. [T1: pp. 110-115, 121-126; PDF 129-134, 140-145]

FreeFall retains wavelet denoising but chooses middle-ranked PCs by retaining eigenvalues between the first and third quartiles. Its shown ranks such as 8-23 are illustrative, not fixed indices. This reduces domination by low-frequency artifacts in the largest PCs and by noise in the smallest PCs. It averages STFT spectrograms, estimates noise from amplitudes above 180 Hz, and uses a 5-25 Hz power burst curve for candidate events. The noise-only high-frequency assumption and adaptive threshold are part of the algorithm and need checking on new hardware/interference conditions. [T1: pp. 117-121, 127-129; PDF 136-140, 146-148]

The velocity proxy uses a 95th-percentile frequency measure and LOWESS smoothing (first-degree local polynomial, f=0.1). Peaks/troughs define activity boundaries, with adjacent troughs enclosing an activity. Features include entropy up to 180 Hz and in 10 Hz bins from 40-180 Hz, entropy before/after the peak, spectral flux, peak velocity proxy, values 0.5/1/1.5 s after the peak, integrated displacement proxy, peak-to-peak acceleration, duration, and duration times peak velocity. The rationale assumes typical 2-3 s staged falls. Slow collapses or low-height slides remain a separate validation need. [T1: pp. 122-126; PDF 141-145]

Classifier: random forest, 50 trees, minimum leaf size five, with class priors/costs adjusted to penalize missed falls. The authors choose it after comparing logistic regression, naive Bayes, SVM, and random forest; SMOTE is again used. Default same-dataset split is 70/30. Participant/session grouping, oversampling order, and an independent final test domain after cross-environment feature selection are not fully specified. Our reproduction should fit normalization, synthetic sampling, feature selection, and tuning solely on training/validation groups, then freeze them before held-out tests. [T1: pp. 124-129; PDF 143-148]

G. FreeFall datasets and topology: coverage is a sensing requirement

Table 6.1 Dataset I reuses the five room environments with one evaluated radio link per environment: corridor 82 falls/243 ADLs, bedrooms 51/218, kitchen 69/355, bathroom 75/324, lab 49/232, totaling 326 falls and 1,372 ADLs. The earlier FallDeFi table lists the same fall total but only 744 non-falls; the later ADL denominator and kitchen clutter label differ without a clear reconciliation in these chapters. Keep table-specific counts and avoid claiming identical event denominators across both results. [T1: Tables 5.2 and 6.1, pp. 88, 127-129; PDF 107, 146-148]

Dataset II explicitly uses AP0 as one transmitter and AP1/AP2/AP3 as three receivers: four physical nodes and three sensing links per room. Kitchen is 18.9 m2 with 3.5/4/4.5 m links and 50 falls/131 ADLs; break room 64.9 m2 with 7/6.5/8 m links and 100 falls/148 ADLs; seminar room 66.49 m2 with 4/9/10 m links and 120 falls/232 ADLs. That is 270 staged falls and 511 ADLs, observed over three links; three observations do not become three independent human falls. Ten falls are performed at each numbered mattress position. Three volunteers aged 27-30 perform the trials; one- and two-person cases are included. [T1: Table 6.1, Fig. 6.10, pp. 128-133; PDF 147-152]

Table 6.2 kitchen links AP1/AP2/AP3, percentages in the order event detection / classification accuracy / TPR / FPR: 92/92.31/91.11/7.20; 100/96.00/97.96/4.64; 100/99.44/98.00/0.00. Break room: 96/85.89/78.13/9.21; 98/88.94/81.63/5.04; 99/95.58/94.95/4.05. Seminar room: 94.17/88.53/77.67/4.19; 95/88.34/80.53/7.51; 100/94.75/98.36/6.65. These are separately evaluated links; no joint-fusion or mesh-routing metric is supplied. [T1: Table 6.2, p. 135; PDF 154]

The source separates prescreener coverage from classification metrics, so classifier TPR should not automatically be presented as unequivocal end-to-end sensitivity. It explicitly gives one position example: kitchen position 4 on AP0-AP1 has 70% event detection and 90% subsequent fall classification, about 63% combined detection. The full table's classifier denominators are not transparent enough to reconstruct exact end-to-end counts by multiplying every row. Our evaluation should count all ground-truth falls, including ones never reaching the classifier. [T1: pp. 130, 134-137; PDF 149, 153-156]

AP0-AP3, the longest/diagonal link, performs best in each of the three tested layouts. Placement, obstruction, and fall position vary together; this is a useful coverage result, not a universal rule that longer links always work better. The narrow 1.1 m corridor with radios about 1 m above the floor is weaker; the author suggests investigating height and shadowing. Map sensing coverage at realistic floor, chair, bed, and doorway locations. Receiving Wi-Fi packets reliably does not prove that a person's fall perturbs a link sufficiently. [T1: pp. 134-138, 142; PDF 153-157, 161]

H. FreeFall transfer results and the wearable comparison

Table 6.4 FreeFall same-place accuracy/TPR/FPR: 94.70% / 96.33% / 5.63%. Lab-trained to other rooms: 92.98% / 88.59% / 6.14%. Comparator lab-transfer values are FallDeFi 80.10/81.56/21.56, CARM 64.26/53.07/24.55, and RT-Fall 51.14/67.82/65.91. These results support improved transfer in the tested settings. An accuracy near 93% still coexists with roughly 11% missed falls under the reported TPR; accuracy alone conceals that tradeoff. The classifier, prescreener, and post-event alert policy must retain distinct evaluation roles. [T1: Table 6.4, pp. 136-140; PDF 155-159]

Table 6.3 individual lab-transfer room results, accuracy/TPR/FPR: corridor 88.50/79.54/9.87; kitchen 92.58/100.00/8.73; bedrooms 92.19/82.35/5.50; bathroom 97.20/88.57/0.92. The bathroom's high accuracy therefore does not mean every fall was detected. Some surrounding prose swaps TPR/FPR labels, and the reported pooled nLoS TPR does not transparently aggregate the room values; use the clearly labeled individual rows rather than reverse-engineering unsupported counts. No independent carpet-material result is supplied. [T1: Table 6.3, pp. 136-140; PDF 155-159]

A controlled kitchen/break-room comparison uses a waist-mounted smartphone accelerometer, sampled at 200 Hz then downsampled to 50 Hz, with filtered acceleration/posture/velocity/displacement features and a random forest. The thesis reports accelerometer accuracy/TPR/FPR 100/100/0 versus FreeFall 97.4/95.97/2.3 for that comparison. These selected staged trials do not establish perfect wearable performance in homes. The author proposes future fusion because wearables are less dependent on room-link coverage while adherence to wearing them is a practical limitation. The fused performance is not demonstrated here. [T1: pp. 139-142; PDF 158-161]

I. Alert cancellation: a material algorithm and policy limitation

FreeFall's Algorithm 3 and classification text wait 60 s after a classified fall, then cancel it if the prescreener finds another activity. The comparison/discussion instead describe a 30 s wait. The source therefore leaves the precise implemented cancellation delay unresolved. It treats later activity as evidence of consciousness; the final discussion proposes identifying the subsequent activity and providing an assistance button. Report the 30-versus-60-second discrepancy explicitly rather than choosing a convenient delay. [T1: Algorithm 3, pp. 116, 126, 139, 142-143; PDF 135, 145, 158, 161-162] Acquisition is described as 10 s per activity, while this policy needs 30/60 s of later observation; the availability of that additional interval is not clearly explained. Retain this as a reproduction question rather than evidence that the rule was not executed.

Our alert-policy inference: a person moving after a fall may still be injured or need help, and a second occupant could produce the later movement. Preserve a detected-fall record and evaluate cancellation-related missed alerts instead of automatically adopting this rule. Measure time from fall onset to candidate detection, classification, policy decision, and delivered notification separately. The supervisor and care-home discussions should determine what subsequent evidence warrants escalation or resolution; the thesis does not validate this policy clinically.

J. What the thesis leaves open for our five objectives

Presence and movement: Chapter 4 provides a room-calibrated baseline for a mostly stationary person and walking, but not sleeping occupancy throughout a day. The fall prescreener detects candidate activity, rather than proving ongoing occupancy. Room localization: the final chapter proposes fusing range/location information and refined phase/Doppler direction to improve falls away from the link; it does not implement bedroom-to-bathroom identity tracking. Counting: the presence discussion predicts stronger perturbations from more people, but supplies no 1/2/3+ counting results. Two-person interference tests are not count or identity validation. [T1: pp. 64, 129, 147; PDF 83, 148, 166]

Deployment gaps explicitly acknowledged: real-time capture/transfer/server processing is still to be implemented; a long-term domestic network deployment was beyond the PhD's resources; validation in environments where elderly people live is still required; low-speed/low-height falls such as falling from a chair need further experiments; 2.4 GHz coexistence/interference needs analysis. Volunteers are aged 27-30. The thesis estimates long natural-event collection from prior accelerometer research, but does not itself collect prospective elderly falls. Its online feasibility and controlled transfer results should remain research-prototype evidence. [T1: pp. 142, 146-148; PDF 161, 165-167]

Environment requirements to carry forward: specify accepted wall types, furniture/door conditions, resident positions, concurrent staff/visitor activity, and available radio placement. Measure how much empty-room/reference activity data each new room requires and how often moved furniture or radios trigger recalibration. Carpet appears only within the broad environment concern; no isolated carpet accuracy effect or generic-home calibration cost is established. A frozen classifier, an adaptive noise threshold, and adequate sensing coverage are three separate requirements.

K. Additional literature leads and proposed implementation work

The thesis literature chapter identifies additional leads: Electronic Frog Eye (INFOCOM 2014), Freecount (GLOBECOM 2017), LiFS (MobiCom 2016), Dynamic-MUSIC/MaTrack (UbiComp 2016), CARM, WiDance, and Sensing-Fi. Their roles include count estimation, device-free localization, motion direction, and Wi-Fi/floor-vibration fusion. Chapter 2 warns that crowd-count approaches often depend on sustained motion and constrained spaces, and that localization calibration or sensor density can add installation effort. These are thesis-reported reading leads; their original full texts have not been independently reviewed in this addition, so no new headline metric from them is treated as primary evidence. [T1: pp. 10-13, 21-24; PDF 29-32, 40-43]

Proposed capstone work from this review: implement a reproducible interpretable baseline alongside the deep-learning baseline already described; keep quality checks and event prescreening separate from fall classification; compare per-link coverage before fusing links; test mixed activity sequences, distant falls, quiet occupancy, ordinary fast transfers, doors, and second-person motion. Use training-only preprocessing/SMOTE/feature tuning, later-session and held-out-person/room tests, and continuous non-fall recordings. Report false alerts per occupied hour and per room-day as well as staged-fall sensitivity. Obtain low-height/slow-fall examples from suitable existing datasets or an approved safe protocol.

Architecture decision remains experimental: the thesis's one-TX/three-RX arrangement is a useful comparison against our several-TX/one-RX idea, at matched physical node count and comparable per-link sampling. Test bedroom/bathroom boundaries, wall penetration, coverage holes, scheduling, and association of packets to sources. Multi-link fusion may improve redundancy, but the thesis supplies no guarantee of its gains. Select sensing geometry from measured coverage and room confusion; choose mesh/backhaul separately based on installation and log transport.

15. Linked primary papers and hardware references

[R1] WiFall: Device-free Fall Detection by Wireless Networks (INFOCOM 2014; primary university publication record)

[R2] Anti-Fall: A Non-intrusive and Real-time Fall Detector Leveraging CSI from Commodity WiFi Devices (2015)

[R3] RT-Fall: A Real-Time and Contactless Fall Detection System with Commodity WiFi Devices (IEEE TMC 2017)

[R4] FallDeFi: Ubiquitous Fall Detection using Commodity Wi-Fi Devices (PACM IMWUT 2017)

[R5] Deep Learning-Based Fall Detection Using WiFi Channel State Information (IEEE Access 2023; author repository)

[R6] FallAware: An Explainable Learning Approach to Robust Fall Detection With WiFi (IEEE JSAS 2025; full paper)

[R7] CSI-Bench: A Large-Scale In-the-Wild Dataset for Multi-task WiFi Sensing (NeurIPS 2025)

[R8] WiDetect: Robust Motion Detection with a Statistical Electromagnetic Model (PACM IMWUT 2019)

[R9] Time-Selective RNN for Device-Free Multi-Room Human Presence Detection Using WiFi CSI (2023)

[R10] FarSense: Pushing the Range Limit of WiFi-based Respiration Sensing with CSI Ratio of Two Antennas (2019)

[R11] Wi-CaL: WiFi Sensing and Machine Learning Based Device-Free Crowd Counting and Localization (IEEE Access 2022)

[R12] Widar2.0: Passive Human Tracking with a Single Wi-Fi Link (ACM MobiSys 2018)

[R13] Zero-Effort Cross-Domain Gesture Recognition with Wi-Fi / Widar3.0 (ACM MobiSys 2019)

[R14] SHARP: Environment and Person Independent Activity Recognition with Commodity IEEE 802.11 Access Points (IEEE TMC 2023)

[R15] WiMANS: A Benchmark Dataset for WiFi-based Multi-user Activity Sensing (2024)

[R16] DensePose From WiFi (2023)

[R17] GoPose: 3D Human Pose Estimation Using WiFi (2022)

[R18] Multi-Person Localization via RF Body Reflections / WiTrack2.0 (USENIX NSDI 2015)

[H1] Espressif ESP-CSI — official repository and examples

[H2] ESP-IDF 5.5.1 Wi-Fi CSI documentation for classic ESP32

[H3] FeitCSI — Intel AX200/AX210 CSI tooling (Hutar, Brida, Machaj)

[H4] Nexmon CSI — supported Broadcom chips and firmware

[H5] Atheros CSI Tool for OpenWrt — Collect CSI workflow

[C1] ENetFall — authors' repository, dataset/model pointer, and reproduction entry point

[C2] ENetFall train.py — published training and validation implementation

[C3] ENetFall test.py — published evaluation implementation

[C4] FallDeFi — authors' MATLAB implementation and dataset pointer

[A1] Sameera Palipana — author publication page with the related 2019 thesis reviewed in Section 14

[T1] Fall Detection using Channel State Information from WiFi Devices — PhD thesis, Cork Institute of Technology, 2019 (184-page supplied PDF)
