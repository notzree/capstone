//! Hardware-independent node policies. Relative receive times never imply synchronized clocks.
#![no_std]

use csi_protocol::{NodeId, Probe};

const RETIRED_EPOCHS: usize = 4;

pub const TURN_TAG_LEN: usize = 14;
const TURN_MAGIC: &[u8; 4] = b"CSIt";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnKind {
    Request,
    Response,
}

/// Over-air turn correlation appended after the existing `Probe` bytes.
///
/// Layout: `CSIt`, version byte 1, kind byte (request 0 / response 1), leader epoch
/// (u32 LE), round (u32 LE). It does not alter UART protocol v2 or the existing probe layout.
/// A response must echo the exact leader epoch and round; it does not transfer ownership of
/// subsequent rounds. Callers still validate sender identity and probe schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnTag {
    pub kind: TurnKind,
    pub leader_epoch: u32,
    pub round: u32,
}

impl TurnTag {
    pub const fn request(leader_epoch: u32, round: u32) -> Self {
        Self {
            kind: TurnKind::Request,
            leader_epoch,
            round,
        }
    }

    pub const fn response(leader_epoch: u32, round: u32) -> Self {
        Self {
            kind: TurnKind::Response,
            leader_epoch,
            round,
        }
    }

    /// Identity/schedule checks remain the caller's responsibility.
    pub fn is_request_for(&self, probe: &Probe) -> bool {
        self.kind == TurnKind::Request
            && self.leader_epoch == probe.epoch
            && self.round == probe.seq
    }

    pub fn is_response_to(&self, leader_epoch: u32, round: u32) -> bool {
        self.is_response_for(leader_epoch, round)
    }

    pub fn is_response_for(&self, leader_epoch: u32, round: u32) -> bool {
        self.kind == TurnKind::Response && self.leader_epoch == leader_epoch && self.round == round
    }

    pub fn encode(&self, out: &mut [u8]) -> Option<usize> {
        let out = out.get_mut(..TURN_TAG_LEN)?;
        out[..4].copy_from_slice(TURN_MAGIC);
        out[4] = 1;
        out[5] = match self.kind {
            TurnKind::Request => 0,
            TurnKind::Response => 1,
        };
        out[6..10].copy_from_slice(&self.leader_epoch.to_le_bytes());
        out[10..14].copy_from_slice(&self.round.to_le_bytes());
        Some(TURN_TAG_LEN)
    }

    pub fn find(payload: &[u8]) -> Option<Self> {
        payload.windows(TURN_TAG_LEN).find_map(|bytes| {
            if &bytes[..4] != TURN_MAGIC || bytes[4] != 1 {
                return None;
            }
            let kind = match bytes[5] {
                0 => TurnKind::Request,
                1 => TurnKind::Response,
                _ => return None,
            };
            Some(Self {
                kind,
                leader_epoch: u32::from_le_bytes(bytes[6..10].try_into().ok()?),
                round: u32::from_le_bytes(bytes[10..14].try_into().ok()?),
            })
        })
    }
}

/// Bounded local response deadline derived from completed round-trip observations.
///
/// Starts at 150 ms. Thereafter uses EWMA RTT (weight 1/8) plus four times EWMA absolute
/// deviation (weight 1/4), clamped to 30..500 ms. Samples are capped at 500 ms before
/// arithmetic, so a stalled/outlier round cannot create an unbounded recovery delay.
/// Feed only matching responses, and do not sample timed-out/retransmitted ambiguous rounds.
/// Timeouts double the current deadline up to 500 ms; a matching RTT observation clears
/// that recovery backoff and resumes the RTT-derived estimate.
/// This estimates a recovery deadline; it does not synchronize clocks or reserve RF airtime.
#[derive(Debug, Clone, Copy)]
pub struct AdaptiveTimeout {
    smoothed_us: Option<u64>,
    deviation_us: u64,
    backoff_us: u64,
}

impl Default for AdaptiveTimeout {
    fn default() -> Self {
        Self::new()
    }
}

impl AdaptiveTimeout {
    pub const fn new() -> Self {
        Self {
            smoothed_us: None,
            deviation_us: 0,
            backoff_us: 0,
        }
    }

    pub fn observe_rtt(&mut self, rtt_us: u64) {
        self.backoff_us = 0;
        let sample = rtt_us.clamp(1, 500_000);
        match self.smoothed_us {
            None => {
                self.smoothed_us = Some(sample);
                self.deviation_us = sample / 2;
            }
            Some(previous) => {
                self.deviation_us = (self.deviation_us * 3 + previous.abs_diff(sample)) / 4;
                self.smoothed_us = Some((previous * 7 + sample) / 8);
            }
        }
    }

    pub fn timeout_us(&self) -> u64 {
        let estimate = match self.smoothed_us {
            None => 150_000,
            Some(rtt) => (rtt + self.deviation_us * 4).clamp(30_000, 500_000),
        };
        estimate.max(self.backoff_us)
    }

    pub fn on_timeout(&mut self) {
        self.backoff_us = (self.timeout_us() * 2).min(500_000);
    }
}

/// One response entitlement per fresh, forward-moving probe from a configured leader.
///
/// This is a pair request/response policy, not an RF slot reservation. Wi-Fi contention and
/// driver latency can delay the actual transmission. The caller must discard an entitlement
/// if sending cannot start by `received_us + max_age_us`; do not queue catch-up responses.
/// Epochs are random boot identities, not ordered numbers. The four most recently retired
/// epochs are remembered to reject delayed traffic after a reboot. Older identities cannot
/// be distinguished from a new boot without a stronger protocol.
#[derive(Debug)]
pub struct PairResponder {
    peer: NodeId,
    schedule: u32,
    guard_us: u64,
    max_age_us: u64,
    current: Option<(u32, u32)>,
    retired: [Option<u32>; RETIRED_EPOCHS],
    retire_at: usize,
}

impl PairResponder {
    pub const fn new(peer: NodeId, schedule: u32, guard_us: u64, max_age_us: u64) -> Self {
        Self {
            peer,
            schedule,
            guard_us,
            max_age_us,
            current: None,
            retired: [None; RETIRED_EPOCHS],
            retire_at: 0,
        }
    }

    /// Consume a response entitlement and return its earliest local transmission time.
    ///
    /// Duplicate or backwards sequences are ignored, with wrapping sequence arithmetic.
    /// Forward movement must be less than half the sequence space. `received_us` and `now_us`
    /// must use this receiver's clock; the sender's startup time is irrelevant. If the guard
    /// has already elapsed, the returned time can be in the past. Rejecting stale input does
    /// not change the remembered leader identity or sequence.
    pub fn observe(&mut self, probe: Probe, received_us: u64, now_us: u64) -> Option<u64> {
        if probe.tx != self.peer
            || probe.schedule != self.schedule
            || self.guard_us > self.max_age_us
        {
            return None;
        }
        let age = now_us.checked_sub(received_us)?;
        if age > self.max_age_us {
            return None;
        }
        let due = received_us.checked_add(self.guard_us)?;
        // Ensure the caller can represent the expiry as well as the due time.
        received_us.checked_add(self.max_age_us)?;
        if let Some((epoch, seq)) = self.current {
            if probe.epoch == epoch {
                let forward = probe.seq.wrapping_sub(seq);
                if forward == 0 || forward >= 1 << 31 {
                    return None;
                }
            } else {
                if self.retired.contains(&Some(probe.epoch)) {
                    return None;
                }
                self.retired[self.retire_at] = Some(epoch);
                self.retire_at = (self.retire_at + 1) % RETIRED_EPOCHS;
            }
        }
        self.current = Some((probe.epoch, probe.seq));
        Some(due)
    }
}

/// Parse an exact six-octet, colon-separated MAC address, accepting either hex case.
pub fn parse_mac(text: &str) -> Option<NodeId> {
    let text = text.as_bytes();
    if text.len() != 17 {
        return None;
    }
    let mut mac = [0; 6];
    for (i, octet) in mac.iter_mut().enumerate() {
        let at = i * 3;
        *octet = hex(text[at])? * 16 + hex(text[at + 1])?;
        if i < 5 && text[at + 2] != b':' {
            return None;
        }
    }
    Some(NodeId(mac))
}

fn hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PEER: NodeId = NodeId([0xb0, 0xb2, 0x1c, 0x0b, 0x07, 0x1c]);
    fn probe(epoch: u32, seq: u32) -> Probe {
        Probe {
            tx: PEER,
            epoch,
            schedule: 1,
            seq,
        }
    }
    fn responder() -> PairResponder {
        PairResponder::new(PEER, 1, 5_000, 30_000)
    }

    #[test]
    fn turn_tag_golden_bytes_and_existing_probe_stay_compatible() {
        let tag = TurnTag {
            kind: TurnKind::Response,
            leader_epoch: 0x12345678,
            round: 0x9abcdef0,
        };
        let mut body = [0; 64];
        let probe = probe(7, 300);
        let n = probe.encode(&mut body).unwrap();
        assert_eq!(tag.encode(&mut body[n..]), Some(TURN_TAG_LEN));
        assert_eq!(
            &body[n..n + TURN_TAG_LEN],
            b"CSIt\x01\x01\x78\x56\x34\x12\xf0\xde\xbc\x9a"
        );
        assert_eq!(Probe::find(&body), Some(probe));
        assert_eq!(TurnTag::find(&body), Some(tag));
        let mut request = tag;
        request.kind = TurnKind::Request;
        assert_eq!(request.encode(&mut body), Some(TURN_TAG_LEN));
        assert_eq!(body[5], 0);
        assert_eq!(TurnTag::find(&body), Some(request));
    }

    #[test]
    fn turn_tag_rejects_malformed_candidates_and_finds_later_valid_tag() {
        let tag = TurnTag {
            kind: TurnKind::Request,
            leader_epoch: 7,
            round: 0,
        };
        let mut bytes = [0; TURN_TAG_LEN];
        tag.encode(&mut bytes).unwrap();
        for len in 0..TURN_TAG_LEN {
            assert_eq!(TurnTag::find(&bytes[..len]), None);
            assert_eq!(tag.encode(&mut bytes[..len]), None);
        }
        for (at, value) in [(0, b'X'), (4, 2), (5, 2)] {
            let mut bad = bytes;
            bad[at] = value;
            assert_eq!(TurnTag::find(&bad), None);
        }
        let mut prefixed = [0; TURN_TAG_LEN * 2 + 3];
        prefixed[..TURN_TAG_LEN].copy_from_slice(&bytes);
        prefixed[4] = 2;
        prefixed[TURN_TAG_LEN + 3..].copy_from_slice(&bytes);
        assert_eq!(TurnTag::find(&prefixed), Some(tag));
    }

    #[test]
    fn turn_response_matches_only_the_exact_outstanding_round() {
        let mut tag = TurnTag {
            kind: TurnKind::Response,
            leader_epoch: 7,
            round: u32::MAX,
        };
        assert!(tag.is_response_for(7, u32::MAX));
        assert!(!tag.is_response_for(7, 0));
        assert!(!tag.is_response_for(8, u32::MAX));
        tag.kind = TurnKind::Request;
        assert!(!tag.is_response_for(7, u32::MAX));
        assert!(TurnTag::request(7, 10).is_request_for(&probe(7, 10)));
        assert!(!TurnTag::request(7, 10).is_request_for(&probe(7, 11)));
        assert!(!TurnTag::request(8, 10).is_request_for(&probe(7, 10)));
        assert!(!TurnTag::response(7, 10).is_request_for(&probe(7, 10)));
        assert!(TurnTag::response(7, 10).is_response_to(7, 10));
    }

    #[test]
    fn adaptive_timeout_tracks_slowdown_and_recovers_without_unbounded_deadlines() {
        let mut timeout = AdaptiveTimeout::new();
        assert_eq!(timeout.timeout_us(), 150_000);
        for _ in 0..100 {
            timeout.observe_rtt(10_000);
        }
        assert_eq!(timeout.timeout_us(), 30_000);
        timeout.observe_rtt(100_000);
        assert!(timeout.timeout_us() > 100_000);
        for _ in 0..100 {
            timeout.observe_rtt(100_000);
        }
        assert!((99_000..=101_000).contains(&timeout.timeout_us()));
        for _ in 0..100 {
            timeout.observe_rtt(10_000);
        }
        assert_eq!(timeout.timeout_us(), 30_000);
    }

    #[test]
    fn adaptive_timeout_bounds_zero_and_maximum_integer_samples() {
        let mut timeout = AdaptiveTimeout::default();
        for sample in [u64::MAX, 0, u64::MAX, 500_001, 1, 0] {
            timeout.observe_rtt(sample);
            assert!((30_000..=500_000).contains(&timeout.timeout_us()));
        }
        for _ in 0..100 {
            timeout.observe_rtt(u64::MAX);
        }
        assert_eq!(timeout.timeout_us(), 500_000);
    }

    #[test]
    fn adaptive_timeout_backs_off_on_loss_and_resumes_after_matching_response() {
        let mut timeout = AdaptiveTimeout::new();
        timeout.on_timeout();
        assert_eq!(timeout.timeout_us(), 300_000);
        for _ in 0..100 {
            timeout.on_timeout();
        }
        assert_eq!(timeout.timeout_us(), 500_000);
        timeout.observe_rtt(10_000);
        assert_eq!(timeout.timeout_us(), 30_000);
        timeout.on_timeout();
        assert_eq!(timeout.timeout_us(), 60_000);
    }

    #[test]
    fn packet_driven_pair_turns_handle_loss_reboot_and_delayed_responses_without_a_guard() {
        let mut follower = PairResponder::new(PEER, 1, 0, 30_000);
        let mut deadline = AdaptiveTimeout::new();

        // These are follower-local receive times; the leader need not share this clock.
        let request = probe(7, 10);
        let tag = TurnTag::request(request.epoch, request.seq);
        assert!(tag.is_request_for(&request));
        assert_eq!(follower.observe(request, 900_000, 900_000), Some(900_000));
        let first_response = TurnTag::response(tag.leader_epoch, tag.round);
        assert!(first_response.is_response_to(7, 10));
        assert!(!first_response.is_response_to(7, 11));

        // A response is generated once, but lost on air. Recovery changes the deadline;
        // it does not allow a duplicate request to generate a second response.
        assert_eq!(
            follower.observe(probe(7, 11), 1_000_000, 1_000_000),
            Some(1_000_000)
        );
        assert_eq!(follower.observe(probe(7, 11), 1_001_000, 1_001_000), None);
        deadline.on_timeout();
        assert_eq!(deadline.timeout_us(), 300_000);
        let lost_response = TurnTag::response(7, 11);

        // The next round proceeds after recovery. A late previous response cannot
        // release this outstanding round; only its exact matching response can.
        assert_eq!(
            follower.observe(probe(7, 12), 1_400_000, 1_400_000),
            Some(1_400_000)
        );
        assert!(!lost_response.is_response_to(7, 12));
        assert!(TurnTag::response(7, 12).is_response_to(7, 12));
        deadline.observe_rtt(22_000);
        assert_eq!(deadline.timeout_us(), 66_000);

        // A queued receive event older than the minimum recovery deadline cannot
        // grant a turn after that deadline has passed.
        assert_eq!(follower.observe(probe(7, 13), 1_500_000, 1_530_001), None);

        // Leader restart grants sequence zero under the new boot identity. Neither
        // old requests nor responses can grant/release a turn in the restarted round.
        assert_eq!(
            follower.observe(probe(8, 0), 1_600_000, 1_600_000),
            Some(1_600_000)
        );
        assert_eq!(follower.observe(probe(7, 14), 1_601_000, 1_601_000), None);
        assert!(!TurnTag::response(7, 0).is_response_to(8, 0));
        assert!(!TurnTag::response(8, 0).is_response_to(8, 1));
        assert!(TurnTag::response(8, 0).is_response_to(8, 0));
    }

    #[test]
    fn response_uses_local_receive_time_and_lost_rounds_do_not_block() {
        let mut policy = responder();
        assert_eq!(
            policy.observe(probe(7, 300), 1_000_000, 1_000_200),
            Some(1_005_000)
        );
        // Missing rounds do not require retransmission or an acknowledgement.
        assert_eq!(
            policy.observe(probe(7, 305), 1_500_000, 1_506_000),
            Some(1_505_000)
        );
        assert_eq!(policy.observe(probe(7, 305), 1_500_001, 1_506_001), None);
        assert_eq!(policy.observe(probe(7, 304), 1_500_002, 1_506_002), None);
    }

    #[test]
    fn foreign_inputs_cannot_consume_a_valid_round() {
        let mut policy = responder();
        let mut foreign = probe(7, 20);
        foreign.tx = NodeId([1; 6]);
        assert_eq!(policy.observe(foreign, 100, 100), None);
        foreign = probe(7, 20);
        foreign.schedule = 2;
        assert_eq!(policy.observe(foreign, 100, 100), None);
        assert_eq!(policy.observe(probe(7, 20), 100, 100), Some(5_100));
    }

    #[test]
    fn sequence_wrap_is_forward_but_half_range_is_ambiguous() {
        let mut policy = responder();
        assert!(policy.observe(probe(7, u32::MAX), 100, 100).is_some());
        assert!(policy.observe(probe(7, 0), 200, 200).is_some());
        assert_eq!(policy.observe(probe(7, u32::MAX), 300, 300), None);
        assert_eq!(policy.observe(probe(7, 1 << 31), 300, 300), None);
        assert!(policy.observe(probe(7, 1), 300, 300).is_some());
    }

    #[test]
    fn reboot_accepts_zero_and_rejects_four_retired_epochs() {
        let mut policy = responder();
        for epoch in [90, 12, 400, 3, 80] {
            assert!(policy.observe(probe(epoch, 0), 100, 100).is_some());
        }
        for epoch in [90, 12, 400, 3] {
            assert_eq!(policy.observe(probe(epoch, 100), 200, 200), None);
        }
        assert!(policy.observe(probe(80, 1), 300, 300).is_some());
        assert!(policy.observe(probe(500, 0), 400, 400).is_some());
        assert_eq!(policy.observe(probe(80, 2), 500, 500), None);
    }

    #[test]
    fn stale_future_and_unrepresentable_times_do_not_change_identity() {
        let mut policy = responder();
        assert!(policy.observe(probe(7, 1), 100, 100).is_some());
        assert_eq!(policy.observe(probe(8, 0), 100, 30_101), None);
        assert_eq!(policy.observe(probe(8, 0), 200, 199), None);
        assert_eq!(
            policy.observe(probe(8, 0), u64::MAX - 10, u64::MAX - 10),
            None
        );
        assert!(policy.observe(probe(7, 2), 300, 30_300).is_some());
        assert!(policy.observe(probe(8, 0), 400, 400).is_some());
    }

    #[test]
    fn impossible_guard_never_returns_an_entitlement() {
        let mut policy = PairResponder::new(PEER, 1, 30_001, 30_000);
        assert_eq!(policy.observe(probe(7, 0), 100, 100), None);
    }

    #[test]
    fn mac_parser_is_strict_and_case_insensitive() {
        assert_eq!(parse_mac("b0:B2:1c:0B:07:1C"), Some(PEER));
        for bad in [
            "b0b21c0b071c",
            "b0-b2-1c-0b-07-1c",
            "b0:b2:1c:0b:07:1z",
            "b0:b2:1c:0b:07:1c ",
            "",
        ] {
            assert_eq!(parse_mac(bad), None, "{bad}");
        }
    }
}
