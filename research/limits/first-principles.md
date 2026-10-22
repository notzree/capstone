# What Wi-Fi CSI can and cannot carry about a person in a room

2026-10-20. First-principles derivation by a research subagent, with cited constants and empirical values. Values marked "estimate" are the subagent's; check them against the team's own logs where noted. Index: [README.md](README.md).

## 1. Summary

On ESP32-class hardware, five constraints set what the team can achieve, in this order.

1. **Packet rate per link.** This is an engineering limit, not physics. Earlier ESP32 tests ran at ~80 Hz on one link, and Ghostline ran at ~8 Hz per link. The 802.11 airtime bound is roughly 4,600 probes/s per channel. At those rates, fall-velocity Doppler aliases completely, and only coarse "energy burst" information survives.
2. **No phase coherence on a single-chain ESP32, plus uncorrected automatic gain control (AGC) and scaling.** Without coherent phase, there is no Doppler sign, small motions fall into amplitude blind spots, and averaging cannot be coherent. AGC gain steps are about the same size as the human signal.
3. **Geometry.** The human scatter power falls off as 1/(R_t² R_r²), where R_t and R_r are the person's distances to the transmitter and receiver. Links at a single height are nearly blind to a body on the floor and to vertical (falling) velocity.
4. **A motionless person and dependence on the room.** A person lying still after a fall produces no Doppler. The only evidence left is breathing micro-motion or attenuation relative to an empty-room baseline, and that baseline drifts.
5. **Bandwidth.** Path-length resolution c/B is about 17 m at 20 MHz, and the whole 2.4 GHz band can do no better than about 3.6 m. Time-of-flight ranging is therefore out, and localization has to come from link geometry.

Thermal SNR is **not** the binding limit for detecting falls or breathing when the person is within about 1–2 m of some link. It **is** binding for heart rate and for people far from every link.

## 2. Master table

Columns: ESP32 = 1 link, HT20, ~80 Hz. 5300 = Intel 5300, 1×3, 40 MHz, 5 GHz, 1 kHz. AX210 = 2×2, 160 MHz, 5 GHz. Mesh = 10 nodes × 3 channels of ESP32.

| Limit | Equation | ESP32 | 5300 | AX210 | Mesh | Blocks | Cat. | Lever and expected gain |
|---|---|---|---|---|---|---|---|---|
| Human-to-static power ratio ρ | ρ = σd²/(4πR_t²R_r²) | −10 to −40 dB | same | same | best link per person ≈ −10 to −20 dB (est.) | vitals far from links; small motion | a | Geometry: put a link within ~1 m of every bed, chair and floor zone (+10–20 dB) |
| SNR per subcarrier | P_r − 10log N_sc + 119 − NF | ~20 dB at −75 dBm; ceiling ~30 dB | similar, +4.8 dB from 3-chain combining | +3 dB from 2 chains | per link same as ESP32 | heart rate; far breathing | c (b for the ceiling) | Get in-room RSSI to −40…−55 dBm (~+10 dB until the ceiling) |
| int8 quantization + AGC | SQNR ≈ 10log(6A²); a 1 dB step = 12% amplitude | 31–37 dB; gain not compensated in the official original-ESP32 example | int8, but AGC and RSSI are reported | int16 | 30 independent AGCs | lying/static detection via attenuation; vitals | b/c | S3/C5/C6 gain compensation or forcing; on ESP32, test `esp_csi_gain_ctrl` and a fixed shift |
| Delay resolution | Δℓ = c/B | 17 m (HT20), 8.4 m (HT40) | 8.4 m | 1.9 m | 17 m per radio; 4.3 m only if coherently stitched | time-of-flight ranging; separating people by range | a in 2.4 GHz (≥3.6 m), b at 5/6 GHz | Don't use ToF; use link geometry |
| Angular resolution | Δ(sinθ) ≈ 2/M | none | ~42° | ~90° (a single AoA only) | none per node | localization from one node | b/c | Coherent multi-ESP32 array (ESPARGOS-style) |
| Max unambiguous velocity | v_max = F_s·λ/4 | 2.5 m/s at 80 Hz | 13.6 m/s | 13.6 m/s at 1 kHz (rate set by the injector; assumed) | 6.2 m/s at 200 Hz/link; Ghostline 0.25 m/s | fall velocity; Doppler tracking; counting | c | Binary export, OFDM probes, scheduling (10–25× over Ghostline) |
| Velocity resolution | Δv = λ/(2T) | 0.21 m/s at T = 0.3 s | 0.09 m/s | 0.09 m/s | 0.21 m/s | fall vs. sit within short windows | a (b: 5 GHz is 2.25× better) | Longer windows help only for steady motion |
| Phase coherence | φ_n = 2πΔf_c t_n + θ | none (1 chain) | between chains | between chains | none across nodes or radios | Doppler sign; blind spots; coherent averaging; body-coordinate velocity | b/c | Shared 40 MHz clock with 2 co-channel radios (CSI ratio); round-trip CSI |
| Single-link blind regions | ∇ℓ = û_T + û_R, \|∇ℓ\| = 2cos(β/2) | r₁ = 0.35 m for a 4 m link | r₁ = 0.23 m | r₁ = 0.23 m | coverage set by geometry | localization; posture; lying | a per link, c for the mesh | ≥3 non-collinear links through every cell; two height tiers |
| Measurements vs. unknowns | M = L·C·(B/B_c)·A | 1–4 | 8–27 | 47–156 | 180–600 (≈45 independent for shadowing) | imaging; static multi-person | a/c | Sparse-target models; more nodes (L ∝ N²/2) |
| Airtime | DIFS + backoff + 36 + 4N_sym + 6 µs | ≤4,600 /s | n/a | n/a | ≤4,600 probes/s/ch, so ≤460 Hz/link | everything rate-dependent | a at the limit, c in practice | Keep listen-before-talk; ~1–2 k probes/s/ch realistic (est.) |
| Room dependence | cross-term 2Re{H_s*·H_d} | high | high | high | high, but multi-link enables velocity profiles | generalizing across rooms | a without calibration | Doppler or body-coordinate velocity; baseline-relative attenuation |
| Motionless person | ∂H/∂t = 0 apart from breathing | breathing or baseline only | same | same | low-tier links help | detecting someone lying after a fall; counting still people | a/c | Breathing detector + memory of the fall event + low-tier baseline |

Categories: **a** = fundamental physics or regulation, **b** = fixable by buying hardware, **c** = fixable by system engineering.

## 3. Derivations

### 3.1 What CSI is; static vs. dynamic components; quantization

What the receiver reports per packet n, subcarrier k (baseband index k·Δf, Δf = 312.5 kHz) and antenna m:

\[\hat H_{k,n,m}=g_n\,e^{-j(\phi_n+2\pi k\Delta f\,\tau_n)}\Big[\underbrace{\textstyle\sum_{l\in S}a_l e^{-j2\pi f_k\tau_l}}_{H_s\ (\text{room, furniture, nodes})}+\underbrace{\textstyle\sum_{l\in D}a_l(t)e^{-j2\pi f_k\tau_l(t)}}_{H_d\ (\text{body})}\Big]+w\]

Here g_n is the AGC/scale factor, φ_n the per-packet random phase, and τ_n the per-packet timing offset.

**Dynamic-to-static ratio.** Divide the bistatic radar equation by the line-of-sight (LoS) Friis equation:

\[\rho=\frac{P_d}{P_{LoS}}=\frac{\sigma\,d^2}{4\pi R_t^2R_r^2}\]

For a fixed radar cross-section σ, this does not depend on frequency. Dogaru's FDTD models give a human RCS averaged over aspect of −4 to 0 dBsm across frequencies up to X-band, and −10 to 0 dBsm from the front [1]. With σ = 0.5 m²:

- 4 m link, person 1 m off the midpoint (R = 2.24 m): ρ = 0.5·16/(4π·25) → **−15.9 dB**
- 4 m link, R_t = R_r = 3 m: **−21.0 dB**
- 1 m link (two nodes on the same wall), person 4 m away: **−38.1 dB**
- Person on the LoS: this is a shadowing regime instead. Januszkiewicz measured about +10 dB median path loss for one person standing between the antennas in a room [3].

Caveat: the body's far-field distance 2D²/λ ≈ 4 m at 2.4 GHz (D = 0.5 m), so in-room scattering is near-field. Measured short-range RCS at 5.8 GHz was only −45 to −20 dBsm at 0.5–2 m [2]. **Planning range: ρ ≈ −10 to −40 dB.**

**int8 quantization.** The quantization noise power is 1/6 LSB² per complex sample, so SQNR ≈ 10log₁₀(6A²), where A is the typical |H| in LSB. That gives 31 dB at A = 15 and 37 dB at A = 30. The value of A is an estimate; check it in the team's logs.

- At ρ = −30 dB the human component is about 1 LSB. It survives only because thermal noise (about 3 LSB rms at 20 dB SNR) dithers it.
- Deep spectral fades (−20 dB) have only ~17 dB SQNR.

**AGC.** A 1 dB gain step changes the amplitude of every subcarrier by 12%. A human at ρ = −20 dB modulates amplitude by at most ±10%. **AGC steps are therefore the same size as the signal.** ESP-IDF also applies an automatic digital shift unless `manu_scale` is set, and a subcarrier-smoothing filter (`channel_filter_en`) is on by default [13].

Espressif's `esp_csi_gain_ctrl` README lists the original ESP32 as a supported chip [14]. However, the official `csi_recv_router` example only enables gain control for S3, C3, C5, C6 and C61. **Test this on the team's hardware;** it could be a cheap fix.

### 3.2 Resolution bounds

**Delay / path length,** Δℓ = c/B_occupied:

| Configuration | Occupied bandwidth | Δℓ |
|---|---|---|
| ESP32 / 5300, HT20 (56 subcarriers) | 17.5 MHz | 17.1 m |
| ESP32 / 5300, HT40 (114 subcarriers) | 35.6 MHz | 8.4 m |
| AX210, 80 MHz | 77.8 MHz | 3.9 m |
| AX210, 160 MHz (1,992 subcarriers × 78.125 kHz [19]) | 155.6 MHz | 1.9 m |
| Channels 1/6/11 stitched coherently | 70 MHz span | 4.3 m |
| Entire 2.4 GHz ISM band (regulatory cap) | 83.5 MHz | 3.6 m |

Channel stitching only works with coherent phase across channels (as in Chronos [21]), and the sparse aperture produces sidelobes. A human's excess path in a 5×6 m room is 0–8 m, which is **inside a single 20 MHz resolution cell**.

**Angle.** For a uniform linear array with λ/2 spacing, the first null is at sinθ = 2/M:

- M = 1 (ESP32): no angle at all
- M = 2 (AX210): about 90°, enough to estimate one AoA but not to separate two sources
- M = 3 (Intel 5300): about 42°
- 4-element axis (ESPARGOS 4×2): about 30°

**Doppler.** The Doppler shift is f_D = (1/λ)·dℓ/dt, with |dℓ/dt| ≤ 2v.

- Resolution: Δv = λ/(2T).
- Wu found trunk velocity rises during the last 300–400 ms of a fall [4]. With T = 0.3 s that gives 0.21 m/s at 2.4 GHz (λ = 0.123 m) and 0.09 m/s at 5.5 GHz.
- Aliasing: amplitude-only CSI (single-chain ESP32) is a real-valued signal, so F_s > 2f_D,max and the sign is lost. Complex, coherent CSI needs F_s > the full span (−f_max to +f_max), which is also 2f_D,max. So **v_max = F_s·λ/4** in both cases.

**Human motion vs. required per-link rate.** Required F_s = 4v/λ.

| Motion | v (m/s) | f_D at 2.4 / 5.5 GHz | F_s needed at 2.4 / 5.5 GHz |
|---|---|---|---|
| Sitting, standing, daily activities | ≤1 [4] | 16 / 37 Hz | 33 / 73 Hz |
| Walking torso (typical) | 1–1.5 | ≤24 / 55 Hz | 49 / 110 Hz |
| Falling trunk (2–3× normal [4]; hip vertical impact 2.75 ± 0.42 m/s in young athletes falling onto mattresses [5]) | 3 | 49 / 110 Hz | 98 / 220 Hz |
| Limbs (estimate: 2–3× torso) | ≤5 | 81 / 183 Hz | 163 / 367 Hz |
| Breathing (5 mm at 0.25 Hz) | 0.004 | 0.06 Hz | this is a displacement problem, not Doppler |

**Displacement vs. wavelength.** Chest displacement is 4–12 mm for breathing and 0.2–0.5 mm for heartbeat [25]; Wang et al. report 4.2–5.4 mm for quiet breathing [6]. The phase change is Δφ = 2π·(2Δx)/λ:

- Breathing, 5 mm: 0.51 rad at 2.4 GHz, 1.15 rad at 5.5 GHz
- Heartbeat, 0.3 mm: 0.031 rad at 2.4 GHz, 0.069 rad at 5.5 GHz

### 3.3 Fresnel zones and what one link can see

The radius of the n-th Fresnel zone is r_n = √(nλd₁d₂/d). At the midpoint, r₁ = √(λd)/2:

- 2.4 GHz: 0.30 m (3 m link), 0.35 m (4 m), 0.39 m (5 m), 0.49 m (7.8 m, the diagonal of a 5×6 m room)
- 5.5 GHz: 0.23 m (4 m link)

The path-length gradient is ∇ℓ = û_T + û_R, with |∇ℓ| = 2cos(β/2), where β is the bistatic angle. This produces three kinds of blind spot:

- **On the LoS** (β = 180°), first-order Doppler is zero. The link only sees shadowing.
- **Motion tangent to the Fresnel ellipse** produces zero Doppler.
- **Amplitude-only sensing:** |H| ≈ |H_s| + |H_d|cos(φ₀ + Δφ). Sensitivity is proportional to |sin φ₀|, so it nulls at zone boundaries. Wang et al. verified this experimentally for breathing [6].

Using three channels helps against these nulls only when the excess path is at least c/(2·50 MHz) = 3 m. Near the LoS, every channel shares the same null, so the fix is a different link, not a different channel.

**2D localization to about 0.5 m.** At least 2–3 links must cross every cell at clearly different angles; Widar3.0 drops off sharply with only 2 links and used 6 [8].

- Crofton-formula estimate: in a 4×5 m room (perimeter 18 m), a person-sized sensitivity disk of ~0.8 m (perimeter 2.5 m) is crossed by about 0.14·L links.
- 10 nodes, L = 45 unique links: about 6 crossings in the middle of the room and 1–3 near walls and corners. That is ~0.5–1 m in the centre and poor at the edges.
- 20 nodes, L = 190: about 27 crossings.
- For comparison, the original radio tomographic imaging work used 28 nodes and 378 links around a 6.4 m square, at roughly 6× this link density [9].

**Vertical dimension.** Links at node height h are sensitive to z ∈ [h − r₁, h + r₁]. At h = 1 m that is 0.65–1.35 m, so a body lying on the floor (z < 0.3 m) sits outside the first Fresnel zone of every link.

Vertical velocity also projects onto ∇ℓ only through the term (z − h)(1/R_T + 1/R_R). For a torso 0.5 m off the link plane at R ≈ 2 m, that factor is ≈ 0.49, so a 3 m/s drop gives only about 12 Hz instead of 49 Hz.

**Fix:** two tiers of nodes.

- Low tier at ~0.3–0.4 m, to catch floor-level bodies.
- High tier at ~2–2.4 m (ceiling corners). Here the factor is 0.9–1.3, which gives strong sensitivity to vertical velocity.

### 3.4 Measurements vs. unknowns

The number of independent measurements per sweep is M = links × channels × (B/B_c) × antennas.

The coherence bandwidth is B_c ≈ 1/(5σ_τ) [11]. The IEEE 802.11n (TGn) channel models put residential and small-office RMS delay spread σ_τ at 15–50 ns (models B to D) [10], so B_c ≈ 13–4 MHz. That gives:

- 20 MHz: 1.3–4.4 independent values
- 40 MHz: 2.7–8.9
- 160 MHz: 12–39

Per sweep:

- ESP32, one link: 1–4 complex values
- Intel 5300 (1×3, 40 MHz): 8–27
- AX210 (2×2, 160 MHz): 47–156
- Mesh: 45 × 3 × (1.3–4.4) ≈ 180–600. However, shadowing is nearly flat in frequency, so only about 45 of these are independent for attenuation.

Unknowns:

- Imaging a 4×5 m room at 0.5 m resolution: 80 pixels, or about 400 voxels in 3D. **This is underdetermined:** the link-to-pixel weight matrix has rank ≤ 45 and needs regularization [9].
- Sparse targets: about 4–6 parameters per person (x, y, posture, velocity). For ≤3 people that is 6–18 unknowns, well below 45. **This is overdetermined,** provided the forward model (node geometry plus baseline) is known.

Time only adds new information when the person moves. For a static scene, more packets just average down noise.

### 3.5 Hardware impairments and what survives

- **Carrier frequency offset (CFO).** 802.11g allows ±25 ppm per device [16], so up to ±122 kHz between two devices at 2.437 GHz. With 10 ms between packets, the phase rotates thousands of radians, so packet-to-packet phase is uniformly random. This destroys Doppler sign and coherent averaging across packets on a single chain.
- **Timing offset and packet-detection delay.** A one-sample jitter (50 ns) adds a random phase slope of 2π·17.5 MHz·50 ns = 5.5 rad across the band. This destroys absolute time of flight. Removing a linear fit across subcarriers (SpotFi [23]) keeps relative delays.
- **Sampling-frequency offset.** It comes from the same crystal as the CFO and is absorbed into the timing offset.
- **AGC and scaling:** a common per-packet factor g_n.
- **I/Q imbalance:** mirror leakage between subcarriers ±k, estimate −25 to −35 dB. It is static and can be calibrated.
- **Phase noise:** a random common phase per packet plus an error floor. The ESP32 module datasheet lists TX EVM around −30 dB at MCS7 [12], which implies an estimated per-packet SNR ceiling of about 30 dB.

**Quantities that survive on a single-chain ESP32:**

- |H| (with g_n still in it)
- |H| normalized per packet (this removes g_n but also the absolute attenuation level)
- Phase pattern across subcarriers after linear-fit removal (within one packet)

**Quantities that need two chains on the same chip** (AX210, Intel 5300, Nexmon):

- CSI ratio H₁/H₂, as in FarSense [7]
- Conjugate product H₁H₂* [8]

These cancel CFO, timing offset, phase and shared AGC. They restore Doppler sign and remove blind spots.

**Making separate devices coherent:**

- **Shared 40 MHz reference clock.** This removes CFO and SFO drift. A random PLL phase remains after every relock or channel change, as ESPARGOS measured on ESP32 [20].
  - Within one node: two co-channel ESP32s sharing a crystal should have a constant phase offset between relocks. That is enough for a CSI ratio. This is a hypothesis, consistent with ESPARGOS.
  - Across nodes: also needs a reference signal for phase calibration [20].
- **Round-trip CSI.** Multiplying the forward and reverse CSI cancels CFO if the turnaround time is stable [21]. The residual error is 2π·Δf_c·σ_turnaround. Keeping it under 0.1 rad at Δf_c = 20 kHz needs <0.8 µs of jitter. ML-Track met this in firmware on Raspberry Pi (Nexmon): 0.96 → 0.04 rad of phase error, and 0.23 m median tracking error with single-antenna radios [22]. A software turnaround on ESP32 is probably too jittery (estimate).

### 3.6 SNR and estimation

**Link budget.** Noise per subcarrier is −174 + 10log(312.5 kHz) + NF ≈ −113 dBm, using an NF of ~6 dB. That NF is an estimate from the datasheet sensitivity of −93 dBm at MCS0 [12]: −101.8 dBm thermal noise, plus ~3 dB required SNR.

Signal per subcarrier = RSSI − 17.5 dB, which gives SNR per subcarrier of:

- 15.5 dB at −80 dBm
- 20.5 dB at −75 dBm
- 25.5 dB at −70 dBm
- about 30 dB at −50 dBm (capped by EVM and quantization)

Free-space loss over 4 m is 52 dB. At 20 dBm transmit power, an in-room link should therefore sit around −35 to −50 dBm. **The team's −70 to −80 dBm readings point to 20–40 dB of avoidable loss** (TX power setting, antenna orientation or polarization, enclosure). Moving from −75 to −55 dBm is worth about +10 dB before the ceiling.

**Breathing and heart rate** (coherent-phase bound, ρ = −20 dB, −75 dBm). With η = ρ·SNR = 1.1 per subcarrier per packet, combining K = 52 subcarriers gives 58, so per-packet phase error σ_φ = 1/√(2·58) = **0.093 rad**.

- Breathing (Δφ = 0.51 rad) is about 5× above the noise in a single packet. Even at 8 Hz this is thermally easy. Losing stationary people (Ghostline) is therefore a processing/AGC/geometry problem, not an SNR problem.
- Heart (Δφ = 0.031 rad), integrated over 10 s with SNR_out = N·A²/(2σ²):
  - 17 dB at 100 Hz
  - 6.5 dB at 8 Hz
  - 12.5 dB at −80 dBm and 100 Hz

  With amplitude only, subtract about 3 dB on average for the |sin φ₀| factor, with nulls in the worst case. In practice, heart rate is limited by breathing harmonics, body sway and AGC jitter, which matches the team's "breathing visible, heart rate not resolved".

**Doppler** (Rife–Boorstyn bound [24]): var(f) ≥ 6F_s²/((2π)²ηN(N²−1)). With η = 58, N = 30 and F_s = 100 Hz, σ_f = 0.03 Hz. The resolution (3.3 Hz) dominates. SNR only matters near the estimator threshold, Nη ≲ 10–20; for example ρ = −40 dB in a 0.3 s window gives Nη ≈ 17.

**Delay** (σ_τ ≥ 1/(2π·β_rms·√SNR_tot), with β_rms = B/√12):

- LoS path: 0.28 ns (8 cm)
- −20 dB human path: 2.8 ns (0.8 m)

This holds only before the unresolved LoS (inside the 17 m cell) and the timing offset are accounted for, and both make it far worse. At 160 MHz the bound improves by 8.9×.

**Processing gain:** 10log N for coherent averaging. Amplitude averaging is close to that only when per-sample SNR ≳ 0 dB.

### 3.7 Sampling and airtime budget

**Airtime per probe.** HT-mixed preamble is 36 µs (8+8+4+8+4+4) [17], plus a 6 µs signal extension at 2.4 GHz [16].

- 60-byte frame at MCS0 (26 bits/symbol): (16 + 480 + 6)/26 = 20 symbols = 80 µs. Total **122 µs**.
- MCS7: about 50 µs.
- ESP-NOW's default of 1 Mbps DSSS [15]: about 670 µs, and DSSS frames have no OFDM training fields. Set an OFDM/HT rate.

**Contention overhead.** DIFS is 28 µs (9 µs slot) or 50 µs (20 µs slot) [16]. The mean backoff is 7.5 slots (CWmin = 15), and broadcast frames get no ACK.

- Short slot: 28 + 67.5 + 122 = 217.5 µs, so about **4,600 probes/s per channel**.
- Long slot: about 3,100/s.
- MCS7 frames: about 6,900/s.
- Token-passing scheduling cuts collisions, not the radio's own carrier-sense overhead. Keep listen-before-talk on: in the EU, non-adaptive equipment at ≥10 dBm EIRP is limited to 10% medium utilization [26]. Probes share channels 1/6/11 with the facility's own Wi-Fi; 1,000 probes/s is already about 12% airtime per channel.

**Broadcast multiplication.** R probes/s per channel gives R(N−1) CSI records, and each directed link is sampled R/N times per second.

- With R = 2,000 (engineering estimate) and N = 10: 200 Hz per link, which gives v_max = 6.2 m/s at 2.4 GHz.
- Ghostline's 36 links × 8 Hz ≈ 290 CSI/s ≈ 48 probes/s, which is about **1% of the airtime bound**. It was limited by software or export.

**Export load.** In binary, one record is about 140 B (112 B of HT-LTF plus metadata).

- Per radio: 1,800/s, or about 250 kB/s. A 3 Mbaud UART is borderline; 921.6 kbaud fits only ~650/s.
- Whole mesh: 30 radios, about 7.6 MB/s (60 Mbit/s), which needs Ethernet or USB.
- Hypothesis for the earlier 70–90 fps: ASCII CSV of about 600–1,400 B per line at 921.6 kbaud caps out around 65–150 lines/s.

**Rate each goal needs per link** (2.4 GHz):

- Fall velocity profile: ≥100 Hz for the trunk, ≥160 Hz including limbs.
- Fall as an energy burst plus aftermath: ≥10–20 Hz. Aliasing preserves power, so the burst still shows, but sitting down hard looks the same.
- Tracking: ≥10 Hz with shadowing, ≥50 Hz with Doppler.
- Breathing: ≥2 Hz by Nyquist, 10–20 Hz in practice.
- Heart rate: ≥10 Hz by Nyquist, about 100 Hz for SNR.

### 3.8 Room dependence

The cross-term in |H|² = |H_s|² + |H_d|² + 2Re{H_s*·H_d} depends on the room's static phase at every subcarrier. Amplitude, phase and raw Doppler statistics are all room-dependent [8].

Representations that hold up better across rooms:

- **Doppler frequencies.** They depend only on dℓ/dt of body paths. Their power weighting still depends on location and orientation.
- **Body-coordinate velocity profile** (Widar3.0). It inverts Doppler from several links using known node positions and the person's location and orientation. Widar3.0 used Intel 5300 at 1 kHz with ≥3 receivers (6 links), and kept >90% accuracy when location error was ≤40 cm [8]. Single-chain ESP32 gives only |f_D|, so the inversion is more ambiguous and needs more links.
- **CSI ratio.** It removes hardware offsets and makes the ratio rotate at the Doppler rate. Its magnitude still depends on H_s.
- **Attenuation relative to an empty-room baseline.** LoS shadowing (~6–10 dB [3]) behaves mostly like body-plus-geometry. Off-LoS fading is room-specific noise.

What cannot be recovered without calibration:

- A motionless person, as distinct from a furniture change. Only breathing or baseline differencing can tell them apart.
- Fingerprint-style position mapping.
- Node positions; 20 MHz ToF cannot survey them.
- Per-device gain and phase offsets.

## 4. Diagnostic checklist for auditing papers

1. **Per-link rate.** Compute F_link (aggregate rate ÷ number of transmitters for broadcast) and v_max = F_link·λ/4. If v_max is below the task's velocity (≥3 m/s for a falling trunk), velocity features are aliased. Any result must then come from energy, timing or the post-fall state; check whether it confuses falls with sitting down hard. Confirm whether the reported rate is per link or aggregate, and whether it was ping-driven and jittery.
2. **Window.** Compute λ/(2T) against the velocity contrast between classes. Windows ≥1 s smear a 0.3–0.4 s fall.
3. **Bandwidth.** If localization is claimed from ToF at 20/40 MHz (Δℓ = 17 / 8.4 m), demand super-resolution, a high-SNR justification, and handling of the timing offset.
4. **Phase.** On a single-chain device, any use of phase across packets has to go through linear-fit removal, round-trip CSI or a shared clock. Otherwise the phase is an artefact. On multi-chain devices, check for CSI ratio or conjugate multiplication.
5. **Gain handling.** Look for gain lock or compensation, RSSI rescaling or per-packet normalization. If normalized, absolute attenuation claims are invalid.
6. **Geometry.** Compute r₁ and ρ for the test positions. If every position is within about 1 m of the LoS, the results are optimistic, since ρ ≈ −10 to −15 dB versus −25 to −35 dB elsewhere. Check node heights against claims about posture or lying on the floor.
7. **SNR.** From the reported RSSI, compute SNR per subcarrier and N·η·K for any vitals claim.
8. **Measurements vs. unknowns.** Unique links against pixels for imaging claims; links per person for counting claims.
9. **Generalization.** Was training and testing split across days, rooms or furniture changes? If not, assume the model learned H_s.
10. **Stationary-person tests.** Was a motionless person on the floor tested for more than a few minutes? Is detection based on remembering the fall event, on breathing, or on a baseline?
11. **Fall realism.** Most fall data is staged by young adults onto mattresses (≈2.75 m/s [5]). Real falls in care homes may be slower; treat that as an open question, not a given.
12. **Worked example (Ghostline [27]).** 8 Hz per link gives v_max = 0.25 m/s, so the walking tracks came from shadowing or variance. A still person produces no variance and is lost. The ~48 probes/s it used is about 1% of the airtime bound, so the limit was the software, not the radio.

## 5. Sources

1. Dogaru, Nguyen, Le, ARL-TR-4290 (2007): https://www.govinfo.gov/content/pkg/GOVPUB-D101-PURL-LPS110072/pdf/GOVPUB-D101-PURL-LPS110072.pdf
2. Human RCS measurements in a bio-radar scenario using a 5.8 GHz CW radar: https://vbn.aau.dk/ws/files/804742598/Accepted_Author_Manuscript.pdf
3. Januszkiewicz, Sensors 18(10):3412 (2018): https://pmc.ncbi.nlm.nih.gov/articles/PMC6211019/
4. Wu, J. Biomech. 33:1497 (2000): https://pubmed.ncbi.nlm.nih.gov/10940409/
5. van den Kroonenberg et al., J. Biomech. 29:807 (1996): https://doi.org/10.1016/0021-9290(95)00134-4
6. Wang H. et al., UbiComp 2016: https://doi.org/10.1145/2971648.2971744
7. Zeng et al., FarSense, IMWUT 2019: https://arxiv.org/abs/1907.03994
8. Zheng et al., Widar3.0, MobiSys 2019: https://cswu.me/papers/mobisys19_widar3_paper.pdf
9. Wilson & Patwari, Radio Tomographic Imaging, IEEE TMC 2010: https://span.ece.utah.edu/uploads/RTI_version_3.pdf
10. Erceg et al., TGn channel models (802.11-03/940): https://www.iitk.ac.in/mwn/papers/11-03-0940-01-000n-tgn-channel-models.pdf
11. Coherence bandwidth rule (Rappaport): https://sites.gatech.edu/ece-sarl/wp-content/uploads/sites/488/2016/09/22-CoherenceBandwidth.pdf
12. ESP32 datasheet: https://documentation.espressif.com/esp32_datasheet_en.pdf; module EVM: https://documentation.espressif.com/esp32-wrover-e_esp32-wrover-ie_datasheet_en.pdf
13. ESP-IDF Wi-Fi CSI documentation: https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-guides/wifi-driver/wifi-vendor-features.html
14. `esp_csi_gain_ctrl` README: https://components.espressif.com/components/espressif/esp_csi_gain_ctrl/versions/0.1.5/readme; example: https://github.com/espressif/esp-csi/blob/master/examples/get-started/csi_recv_router/main/app_main.c
15. ESP-NOW documentation: https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/network/esp_now.html
16. IEEE 802.11g-2003: https://hallaweb.jlab.org/tech/Detectors/public%5Fhtml/manuals/data_sheets-manuals/G-I/I/ieee/802/802-11/802.11g-2003.pdf
17. HT PPDU structure (MathWorks): https://www.mathworks.com/help/wlan/gs/ht-ppdu-structure.html
18. Halperin et al., Intel 5300 CSI Tool: https://dhalperi.github.io/linux-80211n-csitool/
19. FeitCSI: https://feitcsi.kuskosoft.com/csi_format/ and https://github.com/KuskoSoft/FeitCSI
20. Euchner & ten Brink, ESPARGOS: https://arxiv.org/html/2502.09405
21. Vasisht et al., Chronos, NSDI 2016: https://www.usenix.org/system/files/conference/nsdi16/nsdi16-paper-vasisht.pdf
22. ML-Track (multi-link round-trip CSI): https://discovery.ucl.ac.uk/id/eprint/10203112/1/ML_Track%20Paper_accepted.pdf; round-trip CSI Doppler: https://discovery.ucl.ac.uk/id/eprint/10161602/1/WCNC2023_submitted.pdf
23. Kotaru et al., SpotFi, SIGCOMM 2015: https://conferences.sigcomm.org/sigcomm/2015/pdf/papers/p269.pdf
24. Rife & Boorstyn, IEEE Trans. Inf. Theory 1974: https://doi.org/10.1109/tit.1974.1055282
25. W-band vital-signs radar (displacement values): https://www.mdpi.com/1424-8220/20/18/5209
26. ETSI EN 300 328 V2.2.2: https://www.etsi.org/deliver/etsi_en/300300_300399/300328/02.02.02_60/en_300328v020202p.pdf
27. Ghostline, arXiv 2610.18764: https://arxiv.org/html/2610.18764
