# Acquisition mode comparison

Per-run observations:

| Condition | Repeat | Direction | TX requested Hz | TX driver accepted Hz | CSI Hz | Missing interior % | Interval p95 ms | Interval p99 ms | Max gap ms | RSSI mean dBm |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|
| star-a-10 | 1 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 10.00 | 10.00 | 9.81 | 1.79 | 104.68 | 199.04 | 200.74 | -69.03 |
| star-b-10 | 1 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 10.00 | 10.00 | 9.70 | 2.90 | 104.32 | 202.52 | 299.93 | -70.83 |
| independent-10 | 1 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 10.00 | 10.00 | 9.68 | 3.12 | 105.97 | 201.07 | 305.05 | -68.98 |
| independent-10 | 1 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 10.00 | 10.00 | 9.90 | 0.89 | 104.43 | 108.78 | 300.55 | -68.99 |
| pair-10 | 1 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 9.91 | 9.91 | 9.70 | 2.02 | 106.16 | 200.90 | 212.29 | -70.22 |
| pair-10 | 1 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 10.00 | 10.00 | 9.90 | 0.89 | 104.07 | 107.29 | 202.67 | -68.64 |
| star-a-50 | 1 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 49.79 | 49.79 | 47.51 | 4.57 | 27.01 | 42.51 | 63.74 | -70.79 |
| star-b-50 | 1 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 49.79 | 49.79 | 48.02 | 3.54 | 25.19 | 41.03 | 61.72 | -73.42 |
| independent-50 | 1 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 49.74 | 49.74 | 49.05 | 1.39 | 24.51 | 38.40 | 80.96 | -65.39 |
| independent-50 | 1 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 49.74 | 49.74 | 48.93 | 1.61 | 24.23 | 39.22 | 45.78 | -65.58 |
| pair-50 | 1 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 49.12 | 49.12 | 48.83 | 0.63 | 25.62 | 46.76 | 114.31 | -59.48 |
| pair-50 | 1 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 49.42 | 49.42 | 49.14 | 0.59 | 23.93 | 29.28 | 113.79 | -58.37 |
| pair-50 | 2 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 48.73 | 48.73 | 48.37 | 0.78 | 26.55 | 50.15 | 59.05 | -59.72 |
| pair-50 | 2 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 49.24 | 49.24 | 48.75 | 1.04 | 24.45 | 48.60 | 54.47 | -58.24 |
| independent-50 | 2 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 49.74 | 49.74 | 49.39 | 0.67 | 24.31 | 27.97 | 44.53 | -58.38 |
| independent-50 | 2 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 49.73 | 49.73 | 49.00 | 1.48 | 24.67 | 38.97 | 63.99 | -58.57 |
| star-b-50 | 2 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 49.79 | 49.79 | 48.96 | 1.66 | 24.20 | 40.24 | 51.45 | -57.46 |
| star-a-50 | 2 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 49.81 | 49.81 | 48.41 | 2.78 | 24.86 | 40.84 | 63.03 | -57.22 |
| pair-10 | 2 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 9.77 | 9.77 | 9.65 | 1.14 | 107.56 | 200.16 | 298.75 | -58.00 |
| pair-10 | 2 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 10.00 | 10.00 | 9.77 | 2.23 | 104.67 | 199.28 | 201.80 | -57.00 |
| independent-10 | 2 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 10.00 | 10.00 | 9.88 | 1.11 | 104.15 | 195.99 | 204.32 | -58.07 |
| independent-10 | 2 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 10.00 | 10.00 | 9.72 | 2.68 | 104.83 | 201.78 | 203.13 | -57.02 |
| star-b-10 | 2 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 10.00 | 10.00 | 9.81 | 1.79 | 104.24 | 199.62 | 203.45 | -57.10 |
| star-a-10 | 2 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 10.00 | 10.00 | 9.77 | 2.23 | 105.73 | 200.44 | 201.68 | -57.21 |

Aggregate rates and loss (mean [min, max] across observed link runs):

| Condition | Direction | Repeats | TX requested Hz | TX driver accepted Hz | CSI Hz | Missing interior % |
|---|---|---:|---:|---:|---:|---:|
| independent-10 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 2 | 10.00 [10.00, 10.00] | 10.00 [10.00, 10.00] | 9.81 [9.72, 9.90] | 1.78 [0.89, 2.68] |
| independent-10 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 2 | 10.00 [10.00, 10.00] | 10.00 [10.00, 10.00] | 9.78 [9.68, 9.88] | 2.12 [1.11, 3.12] |
| independent-50 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 2 | 49.74 [49.73, 49.74] | 49.74 [49.73, 49.74] | 48.97 [48.93, 49.00] | 1.55 [1.48, 1.61] |
| independent-50 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 2 | 49.74 [49.74, 49.74] | 49.74 [49.74, 49.74] | 49.22 [49.05, 49.39] | 1.03 [0.67, 1.39] |
| pair-10 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 2 | 10.00 [10.00, 10.00] | 10.00 [10.00, 10.00] | 9.83 [9.77, 9.90] | 1.56 [0.89, 2.23] |
| pair-10 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 2 | 9.84 [9.77, 9.91] | 9.84 [9.77, 9.91] | 9.68 [9.65, 9.70] | 1.58 [1.14, 2.02] |
| pair-50 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 2 | 49.33 [49.24, 49.42] | 49.33 [49.24, 49.42] | 48.95 [48.75, 49.14] | 0.81 [0.59, 1.04] |
| pair-50 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 2 | 48.93 [48.73, 49.12] | 48.93 [48.73, 49.12] | 48.60 [48.37, 48.83] | 0.71 [0.63, 0.78] |
| star-a-10 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 2 | 10.00 [10.00, 10.00] | 10.00 [10.00, 10.00] | 9.79 [9.77, 9.81] | 2.01 [1.79, 2.23] |
| star-a-50 | a0:b7:65:dd:e9:3c -> b0:b2:1c:0b:07:1c | 2 | 49.80 [49.79, 49.81] | 49.80 [49.79, 49.81] | 47.96 [47.51, 48.41] | 3.67 [2.78, 4.57] |
| star-b-10 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 2 | 10.00 [10.00, 10.00] | 10.00 [10.00, 10.00] | 9.76 [9.70, 9.81] | 2.34 [1.79, 2.90] |
| star-b-50 | b0:b2:1c:0b:07:1c -> a0:b7:65:dd:e9:3c | 2 | 49.79 [49.79, 49.79] | 49.79 [49.79, 49.79] | 48.49 [48.02, 48.96] | 2.60 [1.66, 3.54] |

Health counts (counter drops use the counter-report interval):

| Condition | Repeat | Stream/decode first 1s / later | Missing frames | Missing captures | Pool drops | Export drops | Sealed integrity |
|---|---:|---:|---:|---:|---:|---:|---|
| star-a-10 | 1 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| star-b-10 | 1 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| independent-10 | 1 | 1 / 0 | 0 | 0 | 0 | 0 | True |
| pair-10 | 1 | 1 / 0 | 0 | 0 | 0 | 0 | True |
| star-a-50 | 1 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| star-b-50 | 1 | 2 / 0 | 0 | 0 | 0 | 0 | True |
| independent-50 | 1 | 2 / 0 | 0 | 0 | 0 | 0 | True |
| pair-50 | 1 | 1 / 0 | 0 | 0 | 0 | 0 | True |
| pair-50 | 2 | 2 / 0 | 0 | 0 | 0 | 0 | True |
| independent-50 | 2 | 1 / 0 | 0 | 0 | 0 | 0 | True |
| star-b-50 | 2 | 1 / 0 | 0 | 0 | 0 | 0 | True |
| star-a-50 | 2 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| pair-10 | 2 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| independent-10 | 2 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| star-b-10 | 2 | 0 / 0 | 0 | 0 | 0 | 0 | True |
| star-a-10 | 2 | 1 / 0 | 0 | 0 | 0 | 0 | True |

Interpretation limits:

- Acquisition pilot; two repetitions per condition; no activity labels inferred.
- Forward/reverse profile order; target rates are per transmitter, achieved load reported separately.
- Descriptive pilot only; repeated windows on the same desk are not independent room/person/day trials.
- Compare modes at the same per-node offered rate. Bidirectional modes expose two sensing links; star exposes one.
- Probe missing fraction uses observed sequence interiors only; excludes recording boundaries and cannot attribute causes to RF collision.
- Counter deltas cover first-to-last counters reports; UART/frame/capture errors are reported separately.
- Transmitter requested/accepted rates use that transmitter epoch and its own counter interval. Driver acceptance is not proof of over-the-air delivery.
- Pair follower transmission is conditional on receiving leader requests. Its sequence increments only when it attempts a response, so a low reverse missing fraction alone cannot establish better sampling coverage.
- Timing uses receiver node timestamps, not UART arrival times. Interval statistics include loss gaps.
- First-second stream/decode errors are separated from later errors; attaching to an active UART can start mid-frame, but timing alone does not prove an error cause.
- No motion labels are supplied. Results do not measure sensing accuracy, presence, or fall detection.
- Aggregate timing statistics summarize per-run quantiles; they are not quantiles of pooled intervals.
