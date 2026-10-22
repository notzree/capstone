# Learning, room adaptation, and evaluation plan

Research checked 2026-10-19. This note distinguishes published evidence, static code inspection, and **proposed capstone experiments**. No model was trained or dataset downloaded for this note. The supplied [proposal snapshot](proposal-snapshot.md) was read; the papers and repositories below were checked independently where indicated.

Direction update: new hardware is explicitly available and there is no hard demo deadline. [final-system.md](final-system.md) governs hardware selection and finished-system requirements; the ESP32 baseline below is optional and does not gate richer acquisition.

## Recommendation

Build a reproducible, single-person, room-installed fall research prototype with three competing baselines: interpretable temporal features with a small classifier, a compact neural temporal model, and ENetFall on its native data. Evaluate room transfer before adding model complexity. The defensible initial product claim is a **possible-fall detector within measured coverage**, with a quality/unreliable state. A generic environment-agnostic elderly-care detector remains an experiment.

Use the available ESP32s to test acquisition and affordable sensing. In parallel, obtain one working multi-antenna CSI reference setup if the hardware investigation can establish a supported combination. A CNN cannot reconstruct motion components never captured, resolve every coverage hole, or make an arbitrary antenna/firmware combination equivalent to the training device. Treat model choice, sensing geometry, and radio quality as separate decisions.

The root agent has identified actual local ESP32 logs, rather than solely synthetic experiments. Their reported achieved rate is around 69–71 frames/s for the inspected recordings. Consult [local evidence](local-evidence.md) for exact commands and limitations. Those logs do not contain fall labels; they establish an acquisition starting point, not fall feasibility.

## What the strongest relevant sources establish

### ENetFall: reproducible fall baseline with a material transfer gap

The IEEE Access 2023 paper uses Intel 5300, one TX antenna and three RX antennas, generally 1,000 packets/s. Its five-second examples become `3 × 625 × 30` tensors. On new-room/new-person living-room A, modified B0 reports **69.0% accuracy, 54.6% precision, 90.0% recall** (Table 5); this differs from the pooled random-split headline. The tested NodeMCU/TTGO ESP32 captures showed residual noise overlapping activity signatures (§II-A). That is evidence about those tested systems, not a universal impossibility result. Tables 5–6 and §V-C discuss room, person, and off-dominant-path losses. [Published PDF, printed pp. 83766–83776](https://eprints.whiterose.ac.uk/id/eprint/202278/8/Deep_Learning_Based_Fall_Detection_Using_WiFi_Channel_State_Information.pdf).

The [author repository](https://github.com/yc541/ENetFall) provides `train.py`, `test.py`, and a dataset/model download pointer. Static inspection of [`test.py`](https://github.com/yc541/ENetFall/blob/main/test.py), lines 40–50, confirms test-collection-wide maximum/mean normalization and `shuffle=True, drop_last=True`. Rewrite the evaluation wrapper to consume every held-out event and use documented normalization; preserve the original script as the reproduction comparator. Repository availability is not confirmation that its weights/dataset have been downloaded successfully or reproduced.

### FallAware: physical features plus a small network, with stronger false-alarm evidence

FallAware combines differential autocorrelation of CSI power with a **21k-parameter LSTM**. It uses NXP 88W8997 2×2 radios, 5.2 GHz/40 MHz, normally 3,000 Hz; independent TX–RX pairs extend coverage. It reports **94.1% average detection in unseen environments**, about 2,400 staged falls, and 21 aggregate months of false-alarm observation. Long-term data were motion-triggered recordings processed offline; single-occupant homes without pets averaged **4.2 false alarms/month**. The paper assumes no other significant motion during a fall and flags pets/fans as concerns. This is not prospective natural-fall sensitivity. Its classifier sees the previous eight seconds each second, followed by a five-second moving-probability decision window; compute time is not onset-to-alert delay. [Full paper, §§IV, VI–VIII](https://www.researchgate.net/publication/388348926_FallAware_An_Explainable_Learning_Approach_to_Robust_Fall_Detection_With_WiFi), [DOI](https://doi.org/10.1109/JSAS.2024.3520517).

**Inference:** a compact temporal model on deliberately chosen features is a credible first neural experiment. FallAware's hardware, rate, augmentation, and data cannot be replaced by a low-rate single-antenna stream while retaining its reported performance.

### SHARP: reusable phase/Doppler processing, not a fall result

SHARP reports over **95% worst-case average activity accuracy** across its tested person/day/environment changes. Its released data use an ASUS RT-AC86U monitor with Nexmon, 5.21 GHz/80 MHz, and three volunteers. Activities include walking/running/jumping, stationary sitting/standing, repeated sit-to-stand, and arm movements; **falls are not a class**. Code contains phase sanitization, Doppler computation, and neural training/evaluation entrypoints, with pretrained activity networks. Reuse it as a feature-processing/reference-data experiment rather than a pretrained fall detector. [Paper](https://arxiv.org/abs/2103.09924), [official repository](https://github.com/francescamen/SHARP).

### Widar3.0: invariance can require geometry and hardware

Widar3.0 estimates body-coordinate velocity profiles (BVP), reporting **82.6–92.4% cross-domain gesture accuracy**. The MobiSys paper requires at least three receivers; its experiments use six receivers with three antennas each and one 1,000-packet/s transmitter. User position/orientation are prerequisites, and only the gesture performer is assumed present. These are gesture results, not fall or multi-person validation. The physical representation is useful inspiration, but adopting BVP would introduce a larger localization/geometry project. [Paper, §§6.1, 6.4](https://cswu.me/papers/mobisys19_widar3_paper.pdf), [author dataset/code portal](https://tns.thss.tsinghua.edu.cn/widar3.0/index.html).

### Few-shot learning: label budgets are not installation minutes

FewSense reports five-shot recognition of novel classes/domains at **90.3%, 96.5%, and 82.7%** on SignFi, Widar, and WiAR respectively. These are gesture/activity benchmarks, not evidence that five target-room fall examples suffice. The [author code](https://github.com/Guolin-Yin/FewSense) includes pretraining/fine-tuning scripts, but its README has little execution guidance and the repository listing exposes no license file. [Paper](https://arxiv.org/abs/2203.02014), [accepted manuscript, cross-dataset figures](https://livrepository.liverpool.ac.uk/3166054/1/manuscript_TMC_2022_FewSense.pdf). No verified source in this note establishes a universal **2-, 5-, or 10-minute fall-detector adaptation** cost. Measure that curve locally.

An older fall-specific adversarial-augmentation paper itself reports only slight, nonsignificant unseen-domain improvements in its abstract. More elaborate augmentation is therefore a hypothesis to test, not a substitute for held-out rooms. [Primary paper](https://arxiv.org/abs/2005.11932).

### 2026 follow-up: promising, unreplicated; read the scenario, not just the abstract

[Wang et al., arXiv:2605.00869v1](https://arxiv.org/html/2605.00869v1) reuses ENetFall's 757 events. Its **98.8%** is B+D training → C-left testing; C-right is 92.9%. B+C → previously unseen A/D yields **88.3%/91.7%**, respectively (§IV-B). Pooled 80/20 testing is also reported, despite a general leave-environment-out statement. NLoS Table IV's “trained with the entire dataset” wording leaves its independent-test construction unclear. Table V ablates components on the reported target rooms, so a separately untouched final domain is still desirable. The live Intel 5300 demonstration has **100 staged events**, 94% overall accuracy, and no duration; its ADL rows imply four errors among 50 ADLs, inconsistent with interpreting “near-zero false alarms” as a measured operational rate. Five-second windows, 0.5-second strides and an unspecified consecutive-history alert rule precede the reported 12.4 ms processing. No implementation repository link was found in the paper. Its normalization/variance/augmentation ideas merit individual ablations; the headline does not establish whole-home safety or a validated low alarm burden.

## Three models to build, in order

Everything in the remainder is a **proposed experiment or design**, unless linked to a source for a statistical method.

| Candidate | Input and implementation | Why keep it | Gate before expansion |
|---|---|---|---|
| A. Interpretable baseline | Trailing-window normalized amplitude differences, robust variance, band-power/entropy, event duration and rise/decay features; logistic regression and a small random forest | Reveals whether captured signals contain information beyond room noise; easy to inspect false alarms | Must beat a constant predictor and motion-only detector on complete held-out sessions |
| B. Compact temporal model | A small 1D CNN/TCN over subcarrier summaries or an LSTM over a lag-by-time autocorrelation representation | Tests learned time structure with a manageable data/compute burden | Outperform A on validation-room recall versus alarms/hour, not only random-window accuracy |
| C. ENetFall reference | Reproduce native Intel5300 tensor/model pipeline, then compare with corrected grouped evaluation | Concrete public fall model and reference dataset | Obtain data, verify contents/splits, document baseline reproduction before transferring to our radio |

Start B with tens to low hundreds of thousands of parameters as an engineering budget, not a scientifically optimal size. Compare equal input durations and the same train/validation/test groups. Parameter count, CPU latency, memory, and actual achieved packet rate belong in the result table. Keep inference on a laptop initially; MCU deployment is a later optimization after predictive value is demonstrated.

For single-antenna ESP32, use amplitude-only A/B first. Do not fabricate three antenna channels by copying one stream and call that an ENetFall reproduction. Independent ESP32 nodes are useful links but do not automatically provide simultaneous, phase-coherent antenna measurements. For genuine multi-antenna capture, evaluate amplitude and sanitized phase/ratio/Doppler variants separately. Maintain independent per-device preprocessing front ends and document their sample semantics.

Try training-only amplitude scaling, modest noise injection, temporal shift within labeled context, and channel dropout after a baseline exists. Verify label preservation: reversing time changes a fall's evolution; severe smoothing/time stretching can erase the feature being learned. Synthetic “NLoS” is an augmentation, not a physical measurement of a wall's effect. Record every augmentation as an ablation.

## Install-time adaptation experiment

First train on source rooms only. Before inspecting the final room, fix the global model, adaptation procedure, stopping rules, thresholds, and the contents of a timed installation script using development rooms. All variants use an independent later test session in the target room, with target-room falls reserved for evaluation except in the explicitly supervised research-only arm.

| Arm | Target-room data allowed | What changes | What it can establish |
|---|---|---|---|
| Z0: strict zero-shot | None | Nothing | Generalization to a new room without installation data |
| C2/C5/C10: room calibration | Nested first 2/5/10 minutes of empty-room plus scripted normal activities | Quality baseline, allowed normalizers, candidate-event noise threshold; classifier weights and global decision policy frozen | Whether cheap non-fall setup reduces noise/background shift |
| A2/A5/A10: non-fall adaptation | Same nested timed clips, with activity labels or no labels declared explicitly | Optional frozen-encoder normal-activity prototypes or constrained adapter; use source replay to avoid collapsing to “non-fall” | Whether adaptation adds value beyond threshold calibration |
| S: supervised upper comparator | Separate 1/3/5 labeled independent staged fall events plus matched hard ADLs per new room | Frozen encoder + regularized head; optionally small adapter | Label-efficiency ceiling; not a feasible elderly-resident installation requirement |

Proposed normal-activity script: start with quiet empty-room measurement, then enter/exit, walk through near/middle/far coverage points, sit/stand/lie down, bend, open/close a door, and finish with a quiet seated period. State how much of each fits the 2/5/10 minute prefix. Use only safe ordinary actions. **No resident should be asked to fall for setup.** Approved staged-fall collection is a distinct research activity with its own protocol.

Time the entire installation: physical placement, troubleshooting, usable recording minutes, annotation effort, adaptation compute, and validation walk-through. “Five-shot” describes samples per class, not five seconds/minutes; overlapping windows from one physical action are one event. If supervised fitting needs one person in one position repeatedly, do not describe it as room-general adaptation.

Measure the same held-out test for all arms. Report paired differences in event sensitivity and false-alert rate, per-room results and negative transfer. Re-run after furniture/door changes and moving a radio. Freeze or bound online normalization updates; otherwise a prolonged fall/immobility condition or broken radio can silently become the new baseline. Do not optimize on test labels or repeatedly select the best adaptation epoch on the test room.

## Causal streaming pipeline

1. Log device timestamps, host receive timestamps, source identity, packet sequence, channel/rate/bandwidth, RSSI/noise/AGC information where available, antenna mapping and raw complex CSI. Mark fields unavailable rather than synthesizing them. Establish actual unique-sample rate, jitter and long gaps.
2. Validate/subcarrier-mask deterministically; keep a coverage/quality mask. Dropped packets, reconnects and antenna permutations are system events, not activity labels.
3. Apply causal despiking/filters. Fit fixed PCA/scalers only on training data. A trailing rolling normalizer is permitted if its update rule is frozen, reset behavior documented, and input history actually available at decision time.
4. If uniform sampling is required, use bounded delay: interpolation between two already received samples may introduce an explicit small delay; never use a later observation to claim an earlier decision. Do not interpolate through long outages. Low-pass before decimation where needed and report usable bandwidth at the measured rate.
5. Evaluate trailing 2/4/6-second contexts with proposed 0.25/0.5-second strides as development ablations. Generate spectral/ACF features from completed trailing buffers. Avoid centered smoothing, whole-record normalization, `filtfilt`, or padding that accesses future data in an allegedly real-time score. Entire completed trailing-window normalization is causal, but differs from sample-level online normalization and must match training.
6. Produce probability, quality and candidate-event evidence. Aggregate causally into one event; record candidate, classification, alert-policy and delivery timestamps separately. Save evidence surrounding alerts and a random sample of non-alert periods for audit.

A motion gate must be tested end to end, including falls it fails to pass. High speed is not mandatory for all relevant falls: slow collapse and low-height slide are separate test strata. Do not let “stationary” mean “empty,” and do not erase a fall event merely because motion later resumes.

## Dataset design that tests the real claim

Use immutable raw recordings plus a manifest with `recording_id`, physical `event_id`, participant, room/home, date/session, layout revision, radio/firmware revision, node geometry, fall/ADL subtype, coverage point, concurrent motion, and label provenance. Timestamp annotation uncertainty. A small opt-in ground-truth camera or observer can support laboratory labels; it need not become an input to the deployed detector.

**Split raw recordings before window creation.** Keep all overlapping windows, antenna streams, links, augmented samples, and repeated representations of the same physical fall in the same partition. No normalization, feature selection, SMOTE, pretrained fine-tuning, or threshold tuning may use the untouched test partition. If public events lack participant/session identity, mark those evaluations as event/room splits; do not manufacture a subject-independent claim.

Use nested domain splits: source training rooms; a separate validation room/person/session for thresholds and hyperparameters; final held-out rooms and later days. Report these tests separately:

- Later session, same room/person/placement.
- New person, same room (with source/test participant identities disjoint).
- New room, overlapping people allowed and disclosed.
- New room **and** new people.
- Changed furniture/door state and changed radio placement.
- Different device/toolchain as a separate transfer problem.

With few rooms, leave-one-room-out estimates are exploratory; never tune on each held-out room and report the average as untouched-test performance. Show every room and coverage location, not only the pooled best number. Collect ordinary activity sessions where all classes share the same radio setup; otherwise the model can learn that a mattress/capture configuration means “fall.” Randomize activity order and include transition sequences rather than only centered isolated clips.

Proposed first collection tranche after acquisition is stable: three accessible rooms, at least two days/layout conditions, and several participants, with final room and people withheld from model development. Start safe normal-activity collection immediately; determine staged-fall sample counts with the supervisor's approved protocol. Public datasets can support model reproduction while local labels accrue. Do not use unsafe falls as a convenient way to expand the training set.

## Event metrics and continuous evaluation

Pre-register a temporal event-matching tolerance relative to annotated fall onset/impact and an allowed post-event delay. Match at most one alert to each fall. Duplicate alerts are reported; unmatched alerts are false alerts. A long refractory interval can hide a second fall, so include repeated-event sequences and count misses during cooldown. A rejected low-quality fall is still an end-to-end miss; quality-based abstention additionally reduces reported availability.

| Measurement | Denominator and interpretation |
|---|---|
| End-to-end fall sensitivity | Detected ground-truth falls / all ground-truth falls, including gating and transport failures |
| Event precision | True matched alerts / all alerts; report staged-set prevalence because it differs sharply from real life |
| False alerts per occupied hour | Unmatched alert episodes / monitored occupied hours, not number of non-fall windows |
| False alerts per room-day | Unmatched alert episodes / valid monitored room-days; show empty and occupied exposure separately |
| Availability | Valid monitored time / intended monitoring time, plus count/duration of gaps |
| Delay | Onset/impact → candidate → decision → delivered alert; median/p95 and missed-event count |
| Coverage | Per-location sensitivity and quality, including bed/chair/floor/doorway and off-dominant paths |
| Subgroups | Per-room/person/layout/fall subtype, concurrent motion, and device results |

Maintain both scripted hard-negative trials and long uninterrupted ordinary sessions. Include rapid sitting/lying, exercise, picking up objects, dropped objects, doors, adjacent-room walking, fans, device touches, reconnects and staff entering. Begin a shadow-mode stream as soon as capture is stable, with adjudication of every generated alert. Review randomly sampled non-alert time too; “no notification” alone cannot establish no missed falls.

Use Wilson or exact binomial intervals for event sensitivity with independent events, and show uncertainty per room/person; repeated windows are not extra independent falls. [NIST proportion intervals](https://www.itl.nist.gov/div898/handbook/prc/section2/prc241.htm). A useful calculated example: **95/100 independent falls detected gives a 95% Wilson interval of about 88.8–97.8%**, so the point estimate does not establish 95% population sensitivity. Room/session clustering makes a simple binomial interval optimistic; use cluster bootstrap or show per-domain ranges when there are too few independent domains.

For a homogeneous Poisson false-alert process with zero alerts over `T` valid room-days, the one-sided 95% upper rate is `-ln(0.05)/T ≈ 2.996/T` alerts/day. Consequently, zero in three days permits almost one/day; zero in 30 days permits about 0.10/day (about three per 30-day month). A zero-count upper bound below one per 30-day month needs roughly **90 room-days**, not a short demo. This is a calculated bound under a rate model, not proof across varied homes; correlated alerts/nonstationary occupancy require per-home/day reporting. [NIST constant-rate confidence bounds](https://itl.nist.gov/div898/handbook/apr/section4/apr451.htm).

Choose acceptable sensitivity, alert burden and delay with the care partner before model selection. For capstone engineering gates, publish the exact achieved counts/exposure and confidence bounds even when targets are missed. Do not turn a few controlled successes into an elderly-care deployment claim.

## Multi-person limit and scope control

A single-link classifier observes superposed channel changes; it does not inherently assign a fall to one person. Counting, localization and identity association each require independent labels and evaluation. Test one person falling while another walks or assists, ordinary two-person transfers, and adjacent-room motion. Record those as an explicit stress-test distribution; abstain or mark unvalidated conditions rather than assuming help will always arrive from the second person.

Do not suppress alerts solely because a count model predicts two people. A count error would then become a fall miss, and a second person may be unable to assist. Likewise, stationary presence is not established by having a good motion classifier. Keep a meaningful unreliable state and retain the fall record after later movement.

The first useful scientific output is the **zero-shot versus timed calibration curve at a fixed, measured sensing quality and alert policy**. If calibration works but strict zero-shot fails, ship the research prototype with a documented install step. If both fail at coverage points despite reliable radio capture, change geometry/device/features before training a larger CNN. If only a few-shot model succeeds with target-room falls, report that limitation plainly and keep it out of the proposed resident installation workflow.
