# Repositories and datasets worth building on

Checked 2026-10-19. This is a static source audit, not a successful reproduction. Five upstream repositories were cloned into `/private/tmp` and inspected without running their code or deserializing their model files. ENetFall/FallDeFi external datasets and CSI-Bench Kaggle data were not downloaded. Counts below distinguish human events, files, packets, and derived windows.

## Decision

Start with **ENetFall as a small offline fall-model reproduction**, **CSI-Bench FallDetection as the broader hardware/domain stress test**, and **Espressif's official acquisition tooling for our own ESP32 recordings**. Keep a simple feature classifier beside a small CNN. FallDeFi offers an interpretable signal-processing reference. Community ESP32 demos provide capture and integration examples, but their existence does not establish useful fall sensitivity in unseen homes.

Pretraining and fine-tuning are plausible experiments. None of these sources supplies a verified universal model that can be copied from Intel/NXP measurements onto our ESP32s and deployed without a local evaluation.

## Reuse matrix

| Candidate | Radio/data and artifacts actually found | Best use | Important gap |
|---|---|---|---|
| [ENetFall](https://github.com/yc541/ENetFall) | Intel 5300; training/testing scripts; external data/weights pointer; Apache-2.0 code | First reproducible deep fall baseline | Assumes three receive channels × 30 subcarriers; evaluation code needs repair |
| [CSI-Bench, authors' repository](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark) | Multiple radios, mostly NXP for fall; loaders/models/configs/results; separate Kaggle dataset | More diverse fall baseline; hardware-specific evaluation | Vendor capture utilities; data/code license inconsistency; no raw data/weights in inspected checkout |
| [FallDeFi](https://github.com/dmsp123/FallDeFi) | Intel 5300 MATLAB processing, example, feature `.mat` files; external MEGA raw-data pointer; MIT notice | Interpretable amplitude → PCA/STFT → SVM reference | Old MATLAB/toolbox dependencies and manually configured paths; raw archive unverified |
| [esp32_csi_sensing](https://github.com/md-asaduzzaman-shahin/esp32_csi_sensing) | Two-board firmware, CSV recording, RF models, 80 CSV recordings in v4 | Concrete low-cost pipeline reference | No documented held-out room/person result; class/session confounding and unclear root license |
| [csi-pose](https://github.com/sel00000/csi-pose) | Six ESP32-S3 boards, synchronized recording, pose training, rule-based fall demo; MIT code | Borrow clock alignment/logging and event-state ideas | No checked-in training recordings/checkpoint; one-person/one-room demo; pose task adds scope |
| FallAware | Paper verified through [author publication listings](https://guozhen-jenn-zhu.github.io/publications/), DOI [10.1109/JSAS.2024.3520517](https://doi.org/10.1109/JSAS.2024.3520517) | Physics-informed feature/temporal-policy research | No public author code, weights, or fall dataset located in the bounded search; do not schedule it as a ready implementation |

## 1. ENetFall: best first offline reproduction

Pinned revision: [`215d48b8d53c7c18d8b992ddc87918c43948103e`](https://github.com/yc541/ENetFall/tree/215d48b8d53c7c18d8b992ddc87918c43948103e).

The authors describe **321 fall and 436 non-fall events, 22 volunteers, four indoor environments**. The [README](https://github.com/yc541/ENetFall/blob/215d48b8d53c7c18d8b992ddc87918c43948103e/README.md) links one [Google Drive archive for data and trained networks](https://drive.google.com/file/d/1ehX8mjbZfNzXGzT7BrzHFB2F6ifoU9dv/view). The archive's contents, public accessibility, size, and separate data/weight terms remain unverified. The Git checkout itself contains only README, license, and two scripts. This makes the acquisition check a bounded first task rather than an assumed dependency.

The implementation expects `dataset_CSI_t` and `dataset_labels` inside room-specific MATLAB files. It reshapes each event to **3 × 625 × 30**, assigning interleaved channels to three receive antennas. This is physically different from three independently clocked ESP32 receivers. See [train.py, lines 183–202](https://github.com/yc541/ENetFall/blob/215d48b8d53c7c18d8b992ddc87918c43948103e/train.py#L183).

Before reproduction:

- Replace the random event split at [train.py:211](https://github.com/yc541/ENetFall/blob/215d48b8d53c7c18d8b992ddc87918c43948103e/train.py#L211) with saved manifests. Retain a paper-compatible random split for comparison, then add room/person/session grouping if the archive supplies those identifiers. If identifiers are missing, say so; filenames alone do not reconstruct people reliably.
- Evaluate every item: training and validation use `drop_last=True` at lines 223–225. `test.py` also shuffles and drops the incomplete batch at [line 53](https://github.com/yc541/ENetFall/blob/215d48b8d53c7c18d8b992ddc87918c43948103e/test.py#L53).
- Save training-fitted normalization. The training script computes its maximum/mean from the training subset; the separate test script recomputes them from the test collection at [lines 48–51](https://github.com/yc541/ENetFall/blob/215d48b8d53c7c18d8b992ddc87918c43948103e/test.py#L48).
- Compute global TP/FP/TN/FN, then precision/recall. Current [train.py:53–101](https://github.com/yc541/ENetFall/blob/215d48b8d53c7c18d8b992ddc87918c43948103e/train.py#L53) averages per-batch ratios and can divide by zero. Report seeds and settings; the script repeatedly initializes until its validation target is reached.

**Reuse outcome:** a tested dataset adapter, baseline checkpoint, confusion matrix, grouped split manifest, and exact environment file. Reusing the EfficientNet idea on our radios still requires a new input adapter and our own recordings; resizing tensors alone does not establish transfer.

## 2. CSI-Bench: broader data, with an important current update

Pinned revision: [`b1beac7b0fa46e01748901195ecfbe309dfc0180`](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/tree/b1beac7b0fa46e01748901195ecfbe309dfc0180). The [authors' project page](https://ai-iot-sensing.github.io/projects/project.html) links this repository and [Kaggle dataset](https://www.kaggle.com/datasets/guozhenjennzhu/csi-bench); third-party forks are unnecessary.

The [NeurIPS paper, Appendix A.5](https://proceedings.neurips.cc/paper_files/paper/2025/file/f7c68372b3d39e8d7a093eb2edaaad87-Paper-Datasets_and_Benchmarks_Track.pdf) describes **2,770 falls and 3,930 non-fall activities**, 17 adults, six environments; primarily NXP 88W8997 2×2, 58 subcarriers, 5.18 GHz/40 MHz, 100 Hz, plus a smaller 2.4 GHz single-antenna ESP32-S3 subset. Its specialist fall split is 70/15/15 with easy/medium/hard tiers. The paper's hard tier includes ESP32 measurements. These are distinct from its multitask cross-user/environment/device evaluations. Its acquisition uses vendor-supplied CSI firmware/drivers and proprietary utilities (Section 3.3); a dataset does not make that collection stack publicly reproducible.

**Newer than the proposal:** the current [README](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/README.md) says the authors fixed data issues and reran benchmarks on **Kaggle Version 12**. Their [RESULTS.md](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/RESULTS.md) reports ResNet18 fall `test_id` accuracy **95.02 ±0.57%**, easy **97.30 ±0.29%**, medium **76.47 ±0.00%**, hard **70.78 ±5.52%**; hard F1 **69.81 ±6.29%**, over seeds 42/43/44. These are author reruns, not our observed results. Keep them separate from the earlier paper's 94.88% headline and 68.84% hard accuracy. Do not mix old split/data releases with current code.

Useful code anchors:

- [scripts/local_runner.py](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/scripts/local_runner.py) and [configs/csi_bench_local_config.json](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/configs/csi_bench_local_config.json): local training entry/config. Start with one task/model/seed, not the cloud sweep.
- [load/supervised/benchmark_dataset.py:295–418](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/load/supervised/benchmark_dataset.py#L295): HDF5 `CSI_amps` loading, per-sample normalization, then clipping/zero-padding to 500 ×232. Padding does not manufacture antenna information for ESP32.
- The same loader returns `None` for absent/corrupt samples, which collation can drop. Add an explicit preflight manifest and assert evaluated sample counts so a partial download cannot silently improve a test result.

No `.h5`, `.pth`, or `.pt` data/checkpoint files were found in the inspected Git checkout. Kaggle page resolves, but download/authentication and archive integrity are untested. First fetch only the fall task, record release/version and hashes, inventory class/user/environment/device/session coverage, and inspect split overlap. Proposed adaptation experiments require enough independent target-room recordings; benchmark windows are not independent rooms.

**License ambiguity:** the pinned [`LICENSE`](https://github.com/guozhen-jenn-zhu/CSI-Bench-Real-WiFi-Sensing-Benchmark/blob/b1beac7b0fa46e01748901195ecfbe309dfc0180/LICENSE) says MIT while README says CC BY-NC-ND 4.0. Preserve provenance, inspect Kaggle's actual data terms, and resolve intended code-versus-data licensing before redistributing derived assets or adopting this as product code. Local source inspection and planning can proceed.

## 3. FallDeFi: interpretable reference, not an online package

Pinned revision: [`8b3fefe4407adae0c01427dded3e34c4df4e7080`](https://github.com/dmsp123/FallDeFi/tree/8b3fefe4407adae0c01427dded3e34c4df4e7080).

The repository has real MATLAB implementations and derived feature files. Its [Activity/Readme.txt](https://github.com/dmsp123/FallDeFi/blob/8b3fefe4407adae0c01427dded3e34c4df4e7080/Activity/Readme.txt) specifies MATLAB R2014b, Wavelet Toolbox 4.14, Intel CSI Tool MATLAB routines, and libsvm 3.22. The [example](https://github.com/dmsp123/FallDeFi/blob/8b3fefe4407adae0c01427dded3e34c4df4e7080/Activity/falldefi_example.m#L23) sequences interpolation, room extraction, DWT, PCA, STFT, feature extraction, and SVM. Classification scripts require manually selected feature matrices/settings; this is research code rather than a single reproducible service command.

The [raw-data pointer](https://github.com/dmsp123/FallDeFi/blob/8b3fefe4407adae0c01427dded3e34c4df4e7080/Link%20to%20the%20data%20set) is an old MEGA link. It could not be verified through the web tool; do not assume it is dead, available, or included in Git. The proposal's thesis review supplies the event counts and experimental caveats; those have not been recounted from raw data in this audit.

**Reuse outcome:** implement a small Python reference for amplitude quality checks, wavelet/PCA/STFT visualization and an SVM/RF baseline on our chosen radio. Match units, timebase, band and preprocessing; tune room thresholds using calibration data only. Do not spend the initial sprint porting every MATLAB comparison or assume FreeFall is included because it is a later thesis chapter.

## 4. A concrete two-ESP32 fall demo: useful artifacts, limited validation

Pinned revision of [md-asaduzzaman-shahin/esp32_csi_sensing](https://github.com/md-asaduzzaman-shahin/esp32_csi_sensing): [`1d02555e8d966b6b66da64e012abcb2fd6847b6a`](https://github.com/md-asaduzzaman-shahin/esp32_csi_sensing/tree/1d02555e8d966b6b66da64e012abcb2fd6847b6a).

The repository contains transmitter/receiver firmware and both older and v4 Python pipelines. The v4 data directory actually has **20 CSV files each** for `fall`, `move`, `none`, and `sit_down`: **80 recordings**, with **37,516 CSV data rows in total** (9,300 / 9,150 / 9,700 / 9,366 respectively; headers excluded). These are packet/log rows, not 37,516 independent falls. Trained `model_v4.joblib` and multiclass counterpart are present, but were not loaded.

The [v4 trainer](https://github.com/md-asaduzzaman-shahin/esp32_csi_sensing/blob/1d02555e8d966b6b66da64e012abcb2fd6847b6a/app/fall_detection_v4/train_model_v4.py#L15) uses 20-packet windows with stride 10, Savitzky–Golay filtering, a 150-tree RF (depth 10), and an 80/20 **file-level** split before making windows. That avoids placing overlapping windows from the same file directly in both sets. However, sequential files from one collection session can still share a domain. Filenames visibly cluster classes by date/session; no supplied person/room manifest establishes broader transfer. Every window inherits its file's label, which is also a weak assumption for an entire recording containing a transient fall. [Features](https://github.com/md-asaduzzaman-shahin/esp32_csi_sensing/blob/1d02555e8d966b6b66da64e012abcb2fd6847b6a/app/fall_detection_v4/csi_feature_extractor.py#L46) are per-subcarrier mean/std/max/min/mean absolute difference, not validated physical body velocity.

Its root README's license section names a predecessor but gives no clear permission terms for the added fall code/data. Component license files do not establish the root project's terms. **Use as an inspection/reference candidate; prefer official Espressif firmware as our maintained foundation.** No accuracy claim from this project is adopted here. Our own test must include complete session/person/room holds and continuous negatives.

## 5. csi-pose: strong logging ideas, excessive first-milestone scope

Pinned revision: [`02b1001590beea8e7d6daac57a4d8b4a345eb50f`](https://github.com/sel00000/csi-pose/tree/02b1001590beea8e7d6daac57a4d8b4a345eb50f).

This is actual firmware/host/training/runtime code for **3 TX +3 RX ESP32-S3 boards**, camera-derived 2D pose labels, and a fall state machine. The [README](https://github.com/sel00000/csi-pose/blob/02b1001590beea8e7d6daac57a4d8b4a345eb50f/README.md) transparently reports **10/11 staged falls with two false positives in one 720-second replay session**. Its pose evaluation is one subject, one room, one session with time-ordered 80/20 split. This is useful demo evidence; it cannot estimate a monthly alarm burden or environmental generalization.

The [split utility](https://github.com/sel00000/csi-pose/blob/02b1001590beea8e7d6daac57a4d8b4a345eb50f/train/split_session.py#L23) includes a three-second gap at the train/validation boundary. The [fall FSM](https://github.com/sel00000/csi-pose/blob/02b1001590beea8e7d6daac57a4d8b4a345eb50f/rt/csi_rt/fall.py#L25) uses inferred hip descent, aspect-ratio transition and head position; CSI stillness can be bypassed when uncalibrated. No `.h5`, `.pth`, `.pt`, or `.onnx` recording/checkpoint was found in this checkout. Reproduction therefore needs its own label collection/training or an author-provided artifact.

**Reuse outcome:** borrow MIT-licensed raw-log preservation, board clock fitting, sequence/gap/reboot handling, HDF5 session structure, and synchronized labeling after a focused review. Do not add 18-joint pose reconstruction as a dependency of binary fall detection. Nine independent links are not equivalent to a coherent 3×3 MIMO CSI matrix.

## NXP acquisition: a real lead, still a procurement gate

Current [NXP AN14011, Rev. 10.2, 25 June 2026](https://www.nxp.com.cn/docs/en/application-note/AN14011.pdf) explicitly lists 88W8997 CSI support and documents an `mlancsi` application, NXP driver/firmware, CSI configuration and capture commands. It says `mlancsi` v1.1 is included in the BSP from Linux 6.12.49. Some lower-level command details still direct readers to an NXP representative. This is stronger evidence than an unsupported assumption that every commercial router exports CSI, but it is not a verified turnkey GitHub implementation of FallAware/CSI-Bench.

Before selecting NXP, obtain the exact module/host combination, BSP image, firmware, driver, `mlancsi` utility and license terms, then prove timestamped complex CSI capture with the desired antenna count, traffic source and rate. We have not confirmed price, delivery, access to every required bundle, or a sustained local capture. It remains a conditional alternative to a verified open extraction stack.

## Concrete next implementation tasks

1. Make a data-access inventory: ENetFall archive availability/contents; CSI-Bench Version 12 fall subset and metadata; FallDeFi raw-data status. Save immutable hashes and licenses outside Git for large downloads. Stop any reproduction early if its essential labels/splits cannot be recovered.
2. Reproduce one native-hardware baseline before changing hardware/model simultaneously. Save sample IDs and all predictions, not only a percentage. Add the missing evaluation checks described above.
3. Implement one shared recording contract for our hardware: timestamps, sequence/source ID, radio configuration, antenna/subcarrier map, raw complex measurements, RSSI/gain metadata where exposed, session/person/room/layout IDs and synchronized event annotations.
4. Build a hardware-specific RF/SVM and small CNN on identical grouped splits. Compare amplitude normalization, derivative/spectrogram features, and masking rather than assuming ImageNet transfer is best.
5. Compare frozen source model, local threshold calibration, head-only fine-tuning, and full fine-tuning under a fixed target-room annotation budget. Target adaptation and final test must use separate sessions. Include training from scratch with the same target budget to establish whether pretraining helped.
6. Preserve a never-used person/room and continuous ADL recordings. Report fall-event recall, false alerts per occupied hour/room-day, event delay, and unusable-radio time. Window accuracy is an intermediate debugging measure.

No downloaded model was executed, no live CSI was measured, and no claimed fall accuracy was independently reproduced in this audit.
