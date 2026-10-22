# Dual-role firmware and two-board validation

## Setup and scope

User-reported setup: two ESP-WROOM-32 boards approximately 0.70 m apart on the desk, roughly in the same plane. Antenna orientation, precise coordinates and activity are not surveyed. Retain original verified full-flash backups and the previous successful TX-only/RX-only images.

Implement independent probe TX and CSI RX capabilities, retaining protocol v2 and native CSI export. Add a two-participant leader/responder schedule to demonstrate alternation without a shared boot clock. This is not the full N-node adaptive scheduler or M7 completion.

## Steps

1. Driver/policy review (subagent): CSI/sniffer/ESP-NOW coexistence, send cancellation, guard lifetimes and bounded reception.
2. Host-testable `csi-node` pair policy (subagent): peer/schedule checks, guard and expiry, duplicate/old sequence rejection, wrapping sequence, reboot handling.
3. Firmware integration (parent): allow TX+RX, keep capture enabled; split/drain ESP-NOW; await sends to completion; independent capability reporting. Compile-time bench mode/peer configuration with archived build inputs. Preserve TX-only/RX-only builds.
4. Session audit (subagent): directed links, metadata, role/counter snapshots, contamination, unique CSI and finalized sequence loss.
5. Verify current board MACs and backup hashes; generate archived images and manifest; flash and verify both boards.
6. Iterate 30–60 s smoke captures until both directed links carry valid peer CSI with correct roles/PHY, no self-captures, no capture/frame gaps or queue/export drops after startup. Report probe losses separately, never call driver acceptance over-air delivery.
7. Run a longer capture and restart one board programmatically while recording. Confirm epoch change, automatic turn-taking recovery, and continued operation. Preserve all failed attempts.
8. Independent review and final checks: host workspace tests/no_std/clippy/fmt, all firmware feature combinations, verified session hashes and replay. Update docs/logbook with observed results and remaining gates.

## Initial schedule

Known MAC pair; lexicographically lower MAC leads. Leader sends at most once per 100 ms; responder sends once per fresh leader probe immediately, with no fixed guard, and skips triggers older than 30 ms. Both remain in capture mode. Nonzero schedule id distinguishes this policy from unscheduled periodic probes. A versioned turn tag in the over-air probe padding identifies request/response, leader boot epoch and round. The leader waits for that exact round response and adapts its timeout from RTT (bounded backoff on loss). The 100 ms interval is a rate cap, not a fixed inter-node guard. Peer/schedule/epoch/sequence are checked before responding; no transmissions are triggered by ambient or self traffic. ESP-NOW contention/driver delays mean these are best-effort turns, not guaranteed RF time slots. No sleep/channel/role reinitialization between turns.

## Acceptance and limits

Require both directed links, tx=true/rx=true Hello from both, fixed channel 6 / HT20 / MCS0 / long GI and unchanged 256-byte native layout, correctly attributed probes, zero recorded self-captures, bounded buffers and healthy counters. Target 10 probes/s per node for this first schedule, report actual rates and misses. Initial serial attachment can reject one partial frame; report it separately from subsequent corruption.

Two nodes cannot observe the same third-party probe on both receivers, so this does not validate shared-probe cross-receiver residuals. Gain extraction, callback timing/RAM, 90-minute wrap, controlled sensing experiments and N-node adaptive scheduling remain separate gates.

Iteration 1: bidirectional CSI confirmed at ~10 Hz. Review tightened responder expiry to 30 ms (minimum RTO) and added a 2 s pending-send watchdog which resets the node without cancelling a live send future. Final hardware validation uses firmware 0.2.1.

## Completion record

All planned stages completed for the two-board scope. Two subagents implemented/reviewed policy and audit tooling; review findings drove tighter stale-response rejection and the send-completion watchdog. Final firmware 0.2.1 is flashed/verified on both boards. The final baseline, leader-reset recovery and post-recovery captures pass directed-link, role, PHY, integrity and health checks; see README.md and summary.json. The observed restart capture gap was 623.923 ms. All 117 host tests and the four firmware capability configurations pass. Remaining full-M5/M7 gates are listed in the README; no global clock, arbitrary-N scheduling or activity-classification claim is made.
