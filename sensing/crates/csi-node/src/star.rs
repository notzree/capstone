//! Bounded, fake-clock-testable star policies. Times are local monotonic milliseconds.
use csi_protocol::{Probe, star::*};
pub const DISCOVERY_MS: u64 = 10_000;
pub const STALE_MS: u64 = 6_000;
pub const HOST_LEASE_MS: u64 = 2_000;
pub const TX_LEASE_MS: u16 = 3_000;
pub const ACK_MS: u64 = 500;
pub const GRANT_MS: u64 = 20;
pub const QUIET_MS: u64 = 10;
const QUEUE: usize = 64;
#[derive(Debug, Clone, Copy)]
pub enum Action {
    Air(Packet),
    Usb(Packet),
}
struct Actions {
    items: [Option<Action>; QUEUE],
    head: usize,
    len: usize,
    pub drops: u64,
}
impl Actions {
    const fn new() -> Self {
        Self {
            items: [None; QUEUE],
            head: 0,
            len: 0,
            drops: 0,
        }
    }
    fn push(&mut self, a: Action) {
        if self.len == QUEUE {
            self.drops += 1;
            return;
        }
        self.items[(self.head + self.len) % QUEUE] = Some(a);
        self.len += 1;
    }
    fn pop(&mut self) -> Option<Action> {
        if self.len == 0 {
            return None;
        }
        let a = self.items[self.head].take();
        self.head = (self.head + 1) % QUEUE;
        self.len -= 1;
        a
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub info: NodeInfo,
    pub first_seen: u64,
    pub last_seen: u64,
    pub csi_observed: bool,
}
#[derive(Debug, Clone, Copy)]
pub struct Grant {
    pub tx: Identity,
    pub token: u32,
    pub issued: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    Current,
    Late,
    Unissued,
    WrongRun,
}

/// 16 response slots x 20 ms plus 0..5 ms jitter. Round and entropy change collisions.
pub fn response_delay(id: Identity, nonce: u32, round: u8, entropy: u32) -> u64 {
    let mut h = nonce ^ id.epoch ^ entropy ^ u32::from(round).wrapping_mul(0x9e3779b9);
    for b in id.mac.0 {
        h = (h ^ u32::from(b)).wrapping_mul(16777619)
    }
    u64::from(h % 16) * 20 + u64::from((h >> 8) % 6)
}
/// Conservative bounds: 30% serial reserve and one worst-case 30 ms opportunity per slot.
/// Fewer than five nodes can request higher per-link rates within both budgets.
/// Clean responses normally complete before timeout. Default 5x10 is admitted with a
/// 20ms grant deadline; timeout recovery can degrade achieved rates, never raise them.
pub fn admit_profile(
    count: usize,
    rate: u16,
    baud: u32,
    max_frame_bytes: usize,
) -> Result<(), ResultCode> {
    if count == 0 || count > 5 || rate == 0 || max_frame_bytes == 0 {
        return Err(ResultCode::Invalid);
    }
    let demand = count as u64 * u64::from(rate);
    if demand * max_frame_bytes as u64 * 100 > u64::from(baud) / 10 * 70 || demand * GRANT_MS > 1000
    {
        return Err(ResultCode::Invalid);
    }
    Ok(())
}

pub struct Receiver {
    pub identity: Identity,
    pub kit: u32,
    pub channel: u8,
    pub build: [u8; 8],
    pub state: State,
    pub roster: [Option<Entry>; MAX_NODES],
    pub run: Option<Start>,
    pub selected: u8,
    pub acked: u8,
    pub outstanding: Option<Grant>,
    pub rejected: u32,
    pub grant_timeouts: u64,
    pub late: u64,
    pub unissued: u64,
    actions: Actions,
    nonce: u32,
    round: u8,
    discovery_start: u64,
    attachment: u64,
    host_seen: u64,
    command: u32,
    host_command: u32,
    attempt: u8,
    deadline: u64,
    next_keepalive: u64,
    next_status: u64,
    generation: u32,
    next_grant: u32,
    next_slot: u64,
    slot: usize,
    recent: [Option<Grant>; 16],
    recent_at: usize,
    stop_reason: ResultCode,
    draining: bool,
}
impl Receiver {
    pub fn new(
        identity: Identity,
        kit: u32,
        channel: u8,
        build: [u8; 8],
        nonce: u32,
        now: u64,
    ) -> Self {
        Self {
            identity,
            kit,
            channel,
            build,
            state: State::Discovering,
            roster: [None; MAX_NODES],
            run: None,
            selected: 0,
            acked: 0,
            outstanding: None,
            rejected: 0,
            grant_timeouts: 0,
            late: 0,
            unissued: 0,
            actions: Actions::new(),
            nonce: nonce.max(1),
            round: 0,
            discovery_start: now,
            attachment: 0,
            host_seen: now,
            command: 0,
            host_command: 0,
            attempt: 0,
            deadline: 0,
            next_keepalive: 0,
            next_status: 0,
            generation: 0,
            next_grant: 0,
            next_slot: 0,
            slot: 0,
            recent: [None; 16],
            recent_at: 0,
            stop_reason: ResultCode::Ok,
            draining: false,
        }
    }
    pub fn pop(&mut self) -> Option<Action> {
        self.actions.pop()
    }
    pub fn control_drops(&self) -> u64 {
        self.actions.drops
    }
    fn packet(&self, destination: Identity, body: Body, correlation: u32) -> Packet {
        Packet {
            kit: self.kit,
            sender: self.identity,
            destination,
            correlation,
            body,
        }
    }
    fn air(&mut self, to: Identity, body: Body) {
        self.actions
            .push(Action::Air(self.packet(to, body, self.command)))
    }
    fn usb(&mut self, body: Body) {
        self.actions.push(Action::Usb(self.packet(
            Identity::BROADCAST,
            body,
            self.host_command,
        )))
    }
    fn result(&mut self, k: Kind, r: ResultCode) {
        self.usb(Body::CommandResult {
            command: k,
            result: r,
            missing: self.selected & !self.acked,
        })
    }
    fn state_event(&mut self) {
        self.usb(Body::RunState {
            run: self.run.map_or(0, |r| r.run),
            schedule: self.generation,
            state: self.state,
            selected: self.selected,
            acked: self.acked,
            reason: self.stop_reason,
        })
    }
    pub fn info(&self) -> NodeInfo {
        NodeInfo {
            identity: self.identity,
            build: self.build,
            capabilities: CAP_RX,
            protocol: VERSION,
            channel: self.channel,
            state: self.state,
            owner: Identity::default(),
            run: self.run.map_or(0, |r| r.run),
        }
    }
    fn snapshot(&mut self) {
        let total = self.roster.iter().flatten().count() as u8;
        if total == 0 {
            self.usb(Body::RosterSnapshot {
                index: 0,
                total,
                info: NodeInfo::default(),
            })
        } else {
            let mut index = 0;
            for i in 0..MAX_NODES {
                if let Some(e) = self.roster[i] {
                    self.usb(Body::RosterSnapshot {
                        index,
                        total,
                        info: e.info,
                    });
                    index += 1;
                }
            }
        }
    }
    fn command_next(&mut self) -> Result<(), ResultCode> {
        self.command = self.command.checked_add(1).ok_or(ResultCode::Exhausted)?;
        Ok(())
    }
    pub fn host(&mut self, p: Packet, now: u64) {
        if p.kit != self.kit || p.destination != self.identity {
            return;
        }
        self.host_command = p.correlation;
        match p.body {
            Body::Attach { token } if token != 0 => {
                if self.run.is_some() {
                    self.stop(now, ResultCode::LeaseExpired);
                }
                self.attachment = token;
                self.host_seen = now;
                self.usb(Body::RxReady {
                    token,
                    info: self.info(),
                });
                self.snapshot();
                self.state_event();
            }
            Body::HostHeartbeat { token } if token == self.attachment && token != 0 => {
                self.host_seen = now
            }
            Body::GetRoster => self.snapshot(),
            Body::GetStatus => self.state_event(),
            Body::Discover { nonce, .. } => {
                if self.run.is_some() {
                    self.result(Kind::Discover, ResultCode::Busy)
                } else {
                    self.nonce = nonce.max(1);
                    self.discovery_start = now;
                    self.round = 0;
                    self.roster = [None; MAX_NODES];
                    self.rejected = 0;
                    self.state = State::Discovering;
                    self.state_event();
                }
            }
            Body::Start(s) => {
                // Retries of the applied host request cannot start a second run.
                if self.run.is_some() {
                    self.result(Kind::Start, ResultCode::Busy);
                    return;
                }
                let result = self.start(s, now);
                if let Err(e) = result {
                    self.result(Kind::Start, e)
                }
            }
            Body::Stop { attachment, run } if attachment == self.attachment && attachment != 0 => {
                if self.run.is_some_and(|r| run != 0 && r.run != run) {
                    self.result(Kind::Stop, ResultCode::Stale)
                } else if self.run.is_none() {
                    self.result(Kind::Stop, ResultCode::Ok)
                } else {
                    self.stop(now, ResultCode::Ok)
                }
            }
            _ => self.result(p.body.kind(), ResultCode::Invalid),
        }
    }
    fn start(&mut self, mut s: Start, now: u64) -> Result<(), ResultCode> {
        if self.state != State::Ready {
            return Err(ResultCode::Busy);
        }
        if s.attachment == 0
            || s.attachment != self.attachment
            || now.saturating_sub(self.host_seen) >= HOST_LEASE_MS
        {
            return Err(ResultCode::Stale);
        }
        if s.duration_ms == 0 || s.duration_ms > 7_200_000 {
            return Err(ResultCode::Invalid);
        }
        admit_profile(
            usize::from(s.count),
            s.rate,
            921600,
            csi_protocol::stream::MAX_ENCODED_LEN,
        )?;
        for i in 0..usize::from(s.count) {
            let id = s.nodes[i];
            if !id.valid() || s.nodes[..i].contains(&id) {
                return Err(ResultCode::Invalid);
            }
            let e = self
                .roster
                .iter()
                .flatten()
                .find(|e| e.info.identity == id)
                .ok_or(ResultCode::Stale)?;
            if now.saturating_sub(e.last_seen) >= STALE_MS {
                return Err(ResultCode::Stale);
            }
            if e.info.capabilities & CAP_TX == 0
                || e.info.protocol != VERSION
                || e.info.channel != self.channel
            {
                return Err(ResultCode::Incompatible);
            }
            if e.info.run != 0 {
                return Err(ResultCode::Busy);
            }
        }
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ResultCode::Exhausted)?;
        self.command_next()?;
        s.schedule = self.generation;
        s.run = (u64::from(self.identity.epoch) << 32) | u64::from(self.generation);
        s.lease_ms = TX_LEASE_MS;
        self.run = Some(s);
        self.selected = ((1u16 << s.count) - 1) as u8;
        self.acked = 0;
        self.state = State::Starting;
        self.attempt = 1;
        self.deadline = now + ACK_MS;
        self.next_slot = now;
        self.slot = 0;
        self.next_grant = 0;
        self.recent = [None; 16];
        self.outstanding = None;
        self.stop_reason = ResultCode::Ok;
        self.draining = false;
        self.air(Identity::BROADCAST, Body::Start(s));
        self.state_event();
        Ok(())
    }
    pub fn air_input(&mut self, p: Packet, source: [u8; 6], now: u64) {
        if !p.air_valid(source, self.kit, self.identity) {
            return;
        }
        match p.body {
            Body::DiscoverAck { nonce, round, info }
                if self.state == State::Discovering
                    && now.saturating_sub(self.discovery_start) < DISCOVERY_MS
                    && nonce == self.nonce
                    && round < self.round
                    && info.identity == p.sender =>
            {
                self.upsert(info, now);
            }
            Body::StatusReply(info) if info.identity == p.sender => {
                if let Some(e) = self
                    .roster
                    .iter_mut()
                    .flatten()
                    .find(|e| e.info.identity.mac == p.sender.mac)
                {
                    if e.info.identity.epoch != p.sender.epoch && self.run.is_some() {
                        return;
                    }
                    e.info = info;
                    e.last_seen = now;
                    self.usb(Body::NodeSeen(info));
                }
            }
            Body::CommandAck {
                run,
                command,
                result,
                state,
                ..
            } if self.run.is_some_and(|s| s.run == run) && p.correlation == self.command => {
                let s = self.run.unwrap();
                let Some(i) = s.nodes[..usize::from(s.count)]
                    .iter()
                    .position(|id| *id == p.sender)
                else {
                    return;
                };
                if (self.state == State::Starting && command == Kind::Start)
                    || (self.state == State::Stopping && command == Kind::Stop)
                {
                    // Preserve TX-originated ACK identities/counters in the USB control journal.
                    self.actions.push(Action::Usb(p));
                    let applied = result == ResultCode::Ok
                        && state
                            == if command == Kind::Start {
                                State::Armed
                            } else {
                                State::Idle
                            };
                    if applied {
                        self.acked |= 1 << i;
                        let info = self
                            .roster
                            .iter_mut()
                            .flatten()
                            .find(|e| e.info.identity == p.sender)
                            .map(|entry| {
                                entry.last_seen = now;
                                entry.info.state = state;
                                entry.info.owner = if command == Kind::Start {
                                    self.identity
                                } else {
                                    Identity::default()
                                };
                                entry.info.run = if command == Kind::Start { run } else { 0 };
                                entry.info
                            });
                        if let Some(info) = info {
                            self.usb(Body::NodeSeen(info));
                        }
                    } else if self.state == State::Starting {
                        let result = if result == ResultCode::Ok {
                            ResultCode::Invalid
                        } else {
                            result
                        };
                        self.result(Kind::Start, result);
                        self.stop(now, result);
                        return;
                    }
                    if self.acked == self.selected {
                        if self.state == State::Starting {
                            self.state = State::Measuring;
                            self.deadline = now + u64::from(s.duration_ms);
                            self.next_keepalive = now;
                            self.next_slot = now;
                            self.result(Kind::Start, ResultCode::Ok);
                            self.state_event();
                        } else {
                            self.draining = true;
                        }
                    }
                }
            }
            _ => self.rejected = self.rejected.saturating_add(1),
        }
    }
    fn upsert(&mut self, info: NodeInfo, now: u64) {
        let at = self
            .roster
            .iter()
            .position(|e| e.is_some_and(|e| e.info.identity.mac == info.identity.mac))
            .or_else(|| self.roster.iter().position(Option::is_none));
        let Some(at) = at else {
            self.rejected = self.rejected.saturating_add(1);
            self.result(Kind::Discover, ResultCode::Full);
            return;
        };
        let first = self.roster[at]
            .filter(|e| e.info.identity == info.identity)
            .map_or(now, |e| e.first_seen);
        if self.roster[at].is_some_and(|e| e.info.identity.epoch != info.identity.epoch) {
            self.result(Kind::Discover, ResultCode::Reboot)
        }
        self.roster[at] = Some(Entry {
            info,
            first_seen: first,
            last_seen: now,
            csi_observed: false,
        });
        self.usb(Body::NodeSeen(info));
    }
    pub fn stop(&mut self, now: u64, reason: ResultCode) {
        if self.state == State::Stopping || self.run.is_none() {
            return;
        }
        self.outstanding = None;
        self.state = State::Stopping;
        self.acked = 0;
        self.attempt = 1;
        self.deadline = now + ACK_MS;
        self.stop_reason = reason;
        if self.command_next().is_err() {
            self.draining = true;
            self.stop_reason = ResultCode::Exhausted;
            return;
        }
        self.air(
            Identity::BROADCAST,
            Body::Stop {
                attachment: 0,
                run: self.run.unwrap().run,
            },
        );
        self.state_event();
    }
    /// Call only after every retained CSI before this boundary is serialized on USB.
    pub fn drain_complete(
        &mut self,
        capture: u64,
        export: u32,
        captured: u64,
        exported: u64,
        dropped: u64,
    ) {
        if !self.draining {
            return;
        }
        let run = self.run.take().unwrap();
        self.usb(Body::DrainComplete {
            run: run.run,
            capture,
            export,
            captured,
            exported,
            dropped,
            selected: self.selected,
            confirmed: self.acked,
        });
        self.result(
            Kind::Stop,
            if self.acked == self.selected {
                ResultCode::Ok
            } else {
                ResultCode::MissingAck
            },
        );
        // Don't assume idle after an unconfirmed stop. Status refresh must establish it.
        for e in self.roster.iter_mut().flatten() {
            if run.nodes[..usize::from(run.count)].contains(&e.info.identity) {
                let index = run.nodes[..usize::from(run.count)]
                    .iter()
                    .position(|id| *id == e.info.identity)
                    .unwrap();
                if self.acked & (1 << index) == 0 {
                    e.info.run = run.run;
                    e.info.state = State::Armed;
                }
            }
        }
        self.state = State::Ready;
        self.draining = false;
        self.state_event();
    }
    pub fn needs_drain(&self) -> bool {
        self.draining
    }
    pub fn observe(
        &mut self,
        probe: Probe,
        tag: ProbeTag,
        source: [u8; 6],
        capture: u32,
        now: u64,
    ) -> Observation {
        let Some(s) = self.run else {
            return Observation::WrongRun;
        };
        if self.state != State::Measuring
            || s.run != tag.run
            || s.schedule != probe.schedule
            || probe.tx.0 != source
        {
            return Observation::WrongRun;
        }
        let matches = |g: Grant| {
            g.token == tag.grant
                && g.tx
                    == Identity {
                        mac: probe.tx,
                        epoch: probe.epoch,
                    }
        };
        if let Some(g) = self.outstanding.filter(|g| matches(*g)) {
            if now.saturating_sub(g.issued) > GRANT_MS {
                self.late += 1;
                return Observation::Late;
            }
            self.outstanding = None;
            self.remember(g);
            if let Some(e) = self
                .roster
                .iter_mut()
                .flatten()
                .find(|e| e.info.identity == g.tx)
            {
                e.last_seen = now;
                e.csi_observed = true;
            }
            self.usb(Body::GrantOutcome {
                run: s.run,
                token: g.token,
                tx: g.tx,
                probe_seq: probe.seq,
                capture_seq: capture,
                result: ResultCode::Ok,
            });
            Observation::Current
        } else if self.recent.iter().flatten().any(|g| matches(*g)) {
            self.late += 1;
            Observation::Late
        } else {
            self.unissued += 1;
            Observation::Unissued
        }
    }
    fn remember(&mut self, g: Grant) {
        self.recent[self.recent_at] = Some(g);
        self.recent_at = (self.recent_at + 1) % self.recent.len();
    }
    pub fn tick(&mut self, now: u64) {
        if self.run.is_some()
            && matches!(self.state, State::Starting | State::Measuring)
            && now.saturating_sub(self.host_seen) >= HOST_LEASE_MS
        {
            self.stop(now, ResultCode::LeaseExpired)
        }
        match self.state {
            State::Discovering => {
                if now.saturating_sub(self.discovery_start) >= DISCOVERY_MS {
                    self.state = State::Ready;
                    self.usb(Body::DiscoveryComplete {
                        nonce: self.nonce,
                        count: self.roster.iter().flatten().count() as u8,
                        rejected: self.rejected,
                    });
                    self.snapshot();
                    self.state_event();
                } else if self.round < 20
                    && now >= self.discovery_start + u64::from(self.round) * 500
                {
                    self.air(
                        Identity::BROADCAST,
                        Body::Discover {
                            nonce: self.nonce,
                            round: self.round,
                            channel: self.channel,
                        },
                    );
                    self.round += 1;
                }
            }
            State::Ready => {
                if now >= self.next_status {
                    self.next_status = now + 2000;
                    if self.command_next().is_ok() {
                        self.air(Identity::BROADCAST, Body::StatusRequest)
                    }
                }
            }
            State::Starting | State::Stopping => {
                if self.draining {
                    return;
                }
                if now >= self.deadline {
                    if self.attempt < 3 {
                        self.attempt += 1;
                        self.deadline = now + ACK_MS;
                        if self.state == State::Starting {
                            self.air(Identity::BROADCAST, Body::Start(self.run.unwrap()))
                        } else {
                            self.air(
                                Identity::BROADCAST,
                                Body::Stop {
                                    attachment: 0,
                                    run: self.run.unwrap().run,
                                },
                            )
                        }
                    } else if self.state == State::Starting {
                        self.result(Kind::Start, ResultCode::MissingAck);
                        self.stop(now, ResultCode::MissingAck)
                    } else {
                        self.draining = true
                    }
                }
            }
            State::Measuring => {
                let s = self.run.unwrap();
                if now >= self.deadline {
                    self.stop(now, ResultCode::Duration);
                    return;
                }
                if now >= self.next_keepalive {
                    self.next_keepalive = now + 500;
                    self.air(
                        Identity::BROADCAST,
                        Body::Keepalive {
                            run: s.run,
                            lease_ms: TX_LEASE_MS,
                        },
                    );
                }
                if let Some(g) = self.outstanding {
                    if now.saturating_sub(g.issued) >= GRANT_MS {
                        self.outstanding = None;
                        self.remember(g);
                        self.grant_timeouts += 1;
                        self.next_slot = self.next_slot.max(now + QUIET_MS);
                        self.usb(Body::GrantOutcome {
                            run: s.run,
                            token: g.token,
                            tx: g.tx,
                            probe_seq: 0,
                            capture_seq: 0,
                            result: ResultCode::MissingAck,
                        });
                    } else {
                        return;
                    }
                }
                if now >= self.next_slot {
                    let Some(token) = self.next_grant.checked_add(1) else {
                        self.stop(now, ResultCode::Exhausted);
                        return;
                    };
                    self.next_grant = token;
                    let tx = s.nodes[self.slot];
                    self.slot = (self.slot + 1) % usize::from(s.count);
                    self.next_slot = now + (1000 / (u64::from(s.count) * u64::from(s.rate))).max(1);
                    self.outstanding = Some(Grant {
                        tx,
                        token,
                        issued: now,
                    });
                    self.air(
                        tx,
                        Body::Grant {
                            run: s.run,
                            token,
                            schedule: s.schedule,
                        },
                    );
                }
            }
            _ => {}
        }
    }
}

/// TX never has an autonomous sensing timer. A valid fresh grant consumes one sequence.
pub struct Transmitter {
    pub info: NodeInfo,
    kit: u32,
    run: Option<Start>,
    lease_until: u64,
    duration_until: u64,
    pub probe_seq: u32,
    pub attempts: u64,
    last_grant: u32,
    cache: [Option<(Packet, Body)>; 8],
    cache_at: usize,
    owner_high: Option<(Identity, u32)>,
    ended: Option<(Identity, u64)>,
}
impl Transmitter {
    pub fn new(identity: Identity, kit: u32, channel: u8, build: [u8; 8]) -> Self {
        Self {
            info: NodeInfo {
                identity,
                build,
                capabilities: CAP_TX,
                channel,
                ..NodeInfo::default()
            },
            kit,
            run: None,
            lease_until: 0,
            duration_until: 0,
            probe_seq: 0,
            attempts: 0,
            last_grant: 0,
            cache: [None; 8],
            cache_at: 0,
            owner_high: None,
            ended: None,
        }
    }
    pub fn tick(&mut self, now: u64) {
        if self.run.is_some() && (now >= self.lease_until || now >= self.duration_until) {
            self.clear()
        }
    }
    fn clear(&mut self) {
        if self.run.is_some() {
            self.ended = Some((self.info.owner, self.info.run));
        }
        self.run = None;
        self.info.state = State::Idle;
        self.info.owner = Identity::default();
        self.info.run = 0;
    }
    fn ack(&self, command: Kind, run: u64, result: ResultCode) -> Body {
        Body::CommandAck {
            run,
            command,
            result,
            state: self.info.state,
            last_probe: self.probe_seq.wrapping_sub(1),
            attempts: self.attempts,
        }
    }
    pub fn command(&mut self, p: Packet, source: [u8; 6], now: u64) -> Option<Body> {
        self.tick(now);
        if !p.air_valid(source, self.kit, self.info.identity) {
            return None;
        }
        match p.body {
            Body::Discover {
                nonce,
                round,
                channel,
            } if channel == self.info.channel => Some(Body::DiscoverAck {
                nonce,
                round,
                info: self.info,
            }),
            Body::StatusRequest => Some(Body::StatusReply(self.info)),
            Body::Keepalive { run, lease_ms }
                if self.info.owner == p.sender
                    && self.info.run == run
                    && self.run.is_some()
                    && lease_ms > 0
                    && lease_ms <= TX_LEASE_MS =>
            {
                self.lease_until = now + u64::from(lease_ms);
                None
            }
            Body::Start(s) => {
                if !s.nodes[..usize::from(s.count).min(MAX_NODES)].contains(&self.info.identity) {
                    return None;
                }
                if let Some((old, result)) = self
                    .cache
                    .iter()
                    .flatten()
                    .find(|(old, _)| old.sender == p.sender && old.correlation == p.correlation)
                {
                    return Some(if *old == p {
                        *result
                    } else {
                        self.ack(Kind::Start, s.run, ResultCode::Conflict)
                    });
                }
                let result = if self.run.is_some() {
                    ResultCode::Busy
                } else if self
                    .owner_high
                    .is_some_and(|(o, id)| o == p.sender && p.correlation <= id)
                {
                    ResultCode::Stale
                } else if s.run == 0
                    || s.schedule == 0
                    || s.lease_ms == 0
                    || s.lease_ms > TX_LEASE_MS
                    || s.duration_ms == 0
                    || s.duration_ms > 7_200_000
                    || s.count > 5
                    || s.rate == 0
                {
                    ResultCode::Invalid
                } else {
                    self.run = Some(s);
                    self.info.owner = p.sender;
                    self.info.run = s.run;
                    self.info.state = State::Armed;
                    self.lease_until = now + u64::from(s.lease_ms);
                    self.duration_until = now + u64::from(s.duration_ms);
                    self.last_grant = 0;
                    self.owner_high = Some((p.sender, p.correlation));
                    ResultCode::Ok
                };
                let ack = self.ack(Kind::Start, s.run, result);
                self.cache[self.cache_at] = Some((p, ack));
                self.cache_at = (self.cache_at + 1) % 8;
                Some(ack)
            }
            Body::Stop { run, .. } => {
                if let Some((old, result)) = self
                    .cache
                    .iter()
                    .flatten()
                    .find(|(old, _)| old.sender == p.sender && old.correlation == p.correlation)
                {
                    return Some(if *old == p {
                        *result
                    } else {
                        self.ack(Kind::Stop, run, ResultCode::Conflict)
                    });
                }
                if self.run.is_some() {
                    if self.info.owner != p.sender || self.info.run != run {
                        return None;
                    }
                } else if self.ended != Some((p.sender, run)) {
                    return None;
                }
                self.clear();
                self.owner_high = Some((p.sender, p.correlation));
                let ack = self.ack(Kind::Stop, run, ResultCode::Ok);
                self.cache[self.cache_at] = Some((p, ack));
                self.cache_at = (self.cache_at + 1) % 8;
                Some(ack)
            }
            _ => None,
        }
    }
    pub fn grant(
        &mut self,
        p: Packet,
        source: [u8; 6],
        received_us: u64,
        now_us: u64,
    ) -> Option<(Probe, ProbeTag)> {
        self.tick(now_us / 1000);
        if !p.air_valid(source, self.kit, self.info.identity)
            || p.destination != self.info.identity
            || p.sender != self.info.owner
            || now_us.checked_sub(received_us)? > 10_000
        {
            return None;
        }
        let Body::Grant {
            run,
            token,
            schedule,
        } = p.body
        else {
            return None;
        };
        let s = self.run?;
        if run != s.run || schedule != s.schedule || token == 0 || token <= self.last_grant {
            return None;
        }
        self.last_grant = token;
        self.info.state = State::Measuring;
        let probe = Probe {
            tx: self.info.identity.mac,
            epoch: self.info.identity.epoch,
            schedule,
            seq: self.probe_seq,
        };
        self.probe_seq = self.probe_seq.wrapping_add(1);
        self.attempts += 1;
        Some((probe, ProbeTag { run, grant: token }))
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    const RX: Identity = Identity {
        mac: csi_protocol::NodeId([2; 6]),
        epoch: 1,
    };
    const TX: Identity = Identity {
        mac: csi_protocol::NodeId([4; 6]),
        epoch: 2,
    };
    fn p(sender: Identity, destination: Identity, correlation: u32, body: Body) -> Packet {
        Packet {
            kit: 1,
            sender,
            destination,
            correlation,
            body,
        }
    }
    fn settings() -> Start {
        let mut nodes = [Identity::default(); 8];
        nodes[0] = TX;
        Start {
            attachment: 99,
            run: 123,
            schedule: 1,
            rate: 10,
            duration_ms: 10000,
            lease_ms: 3000,
            count: 1,
            nodes,
        }
    }
    fn tx() -> Transmitter {
        Transmitter::new(TX, 1, 6, [1; 8])
    }
    fn ready() -> Receiver {
        let mut r = Receiver::new(RX, 1, 6, [1; 8], 7, 0);
        r.tick(0);
        let info = tx().info;
        r.air_input(
            p(
                TX,
                RX,
                0,
                Body::DiscoverAck {
                    nonce: 7,
                    round: 0,
                    info,
                },
            ),
            TX.mac.0,
            9900,
        );
        r.tick(10000);
        r.host(
            p(Identity::default(), RX, 1, Body::Attach { token: 99 }),
            10000,
        );
        while r.pop().is_some() {}
        r
    }
    #[test]
    fn discovery_deduplicates_reboots_and_ignores_stale_nonce_full_table() {
        let mut r = Receiver::new(RX, 1, 6, [1; 8], 7, 0);
        r.tick(0);
        for i in 0..9 {
            let mut info = tx().info;
            info.identity.mac.0[5] = i;
            r.air_input(
                p(
                    info.identity,
                    RX,
                    0,
                    Body::DiscoverAck {
                        nonce: 7,
                        round: 0,
                        info,
                    },
                ),
                info.identity.mac.0,
                1,
            );
        }
        assert_eq!(r.roster.iter().flatten().count(), 8);
        assert!(r.rejected > 0);
        let info = r.roster[0].unwrap().info;
        r.air_input(
            p(
                info.identity,
                RX,
                0,
                Body::DiscoverAck {
                    nonce: 7,
                    round: 0,
                    info,
                },
            ),
            info.identity.mac.0,
            2,
        );
        assert_eq!(r.roster.iter().flatten().count(), 8);
        let mut reboot = info;
        reboot.identity.epoch += 1;
        r.air_input(
            p(
                reboot.identity,
                RX,
                0,
                Body::DiscoverAck {
                    nonce: 7,
                    round: 0,
                    info: reboot,
                },
            ),
            reboot.identity.mac.0,
            3,
        );
        assert_eq!(r.roster[0].unwrap().info.identity, reboot.identity);
        r.tick(10000);
        r.air_input(
            p(
                info.identity,
                RX,
                0,
                Body::DiscoverAck {
                    nonce: 7,
                    round: 0,
                    info,
                },
            ),
            info.identity.mac.0,
            10001,
        );
        assert_eq!(r.roster[0].unwrap().info.identity, reboot.identity);
    }
    #[test]
    fn tx_idle_owned_dedup_conflict_grant_fencing_and_leases() {
        let mut t = tx();
        let start = p(RX, Identity::BROADCAST, 1, Body::Start(settings()));
        assert_eq!(t.info.state, State::Idle);
        let ack = t.command(start, RX.mac.0, 0).unwrap();
        assert_eq!(t.info.state, State::Armed);
        assert_eq!(t.command(start, RX.mac.0, 100), Some(ack));
        assert_eq!(t.lease_until, 3000);
        let mut conflict = start;
        let Body::Start(mut s) = start.body else {
            unreachable!()
        };
        s.rate = 11;
        conflict.body = Body::Start(s);
        assert!(matches!(
            t.command(conflict, RX.mac.0, 100),
            Some(Body::CommandAck {
                result: ResultCode::Conflict,
                ..
            })
        ));
        let mut foreign = start;
        foreign.sender.epoch = 3;
        assert!(matches!(
            t.command(foreign, RX.mac.0, 100),
            Some(Body::CommandAck {
                result: ResultCode::Busy,
                ..
            })
        ));
        let grant = p(
            RX,
            TX,
            2,
            Body::Grant {
                run: 123,
                token: 1,
                schedule: 1,
            },
        );
        let (probe, tag) = t.grant(grant, RX.mac.0, 100_000, 100_001).unwrap();
        assert_eq!((probe.seq, tag.grant), (0, 1));
        assert!(t.grant(grant, RX.mac.0, 100_000, 100_002).is_none());
        let g = p(
            RX,
            TX,
            3,
            Body::Grant {
                run: 123,
                token: 2,
                schedule: 1,
            },
        );
        assert!(t.grant(g, RX.mac.0, 100_000, 110_001).is_none());
        assert_eq!(t.probe_seq, 1);
        t.tick(3000);
        assert_eq!(t.info.state, State::Idle);
        assert!(t.grant(g, RX.mac.0, 3_001_000, 3_001_001).is_none());
    }
    #[test]
    fn complete_start_grants_timeout_quiet_stop_drain_and_no_automatic_resume() {
        let mut r = ready();
        let mut t = tx();
        r.host(
            p(Identity::default(), RX, 2, Body::Start(settings())),
            10000,
        );
        assert_eq!(r.state, State::Starting);
        let start = loop {
            if let Some(Action::Air(p)) = r.pop()
                && matches!(p.body, Body::Start(_))
            {
                break p;
            }
        };
        let ack = t.command(start, RX.mac.0, 10000).unwrap();
        r.air_input(p(TX, RX, start.correlation, ack), TX.mac.0, 10001);
        assert_eq!(r.state, State::Measuring);
        r.tick(10001);
        let grant = r.outstanding.unwrap();
        let s = r.run.unwrap();
        let probe = Probe {
            tx: TX.mac,
            epoch: TX.epoch,
            schedule: s.schedule,
            seq: 0,
        };
        let tag = ProbeTag {
            run: s.run,
            grant: grant.token,
        };
        assert_eq!(
            r.observe(probe, tag, TX.mac.0, 0, 10002),
            Observation::Current
        );
        assert_eq!(r.observe(probe, tag, TX.mac.0, 1, 10003), Observation::Late);
        r.tick(10101);
        assert!(r.outstanding.is_some());
        r.tick(10121);
        assert!(r.outstanding.is_none());
        r.tick(10130);
        assert!(r.outstanding.is_none());
        r.tick(10201);
        assert!(r.outstanding.is_some());
        r.stop(10132, ResultCode::Ok);
        assert!(r.outstanding.is_none());
        r.tick(10632);
        r.tick(11132);
        r.tick(11632);
        assert!(r.needs_drain());
        r.drain_complete(1, 2, 1, 1, 0);
        assert_eq!(r.state, State::Ready);
        assert!(r.run.is_none());
    }
    #[test]
    fn partial_start_rolls_back_before_any_grant_and_host_expiry_cancels_renewal() {
        let mut r = ready();
        r.host(
            p(Identity::default(), RX, 2, Body::Start(settings())),
            10000,
        );
        for now in [10500, 11000, 11500] {
            r.tick(now);
        }
        assert_eq!(r.state, State::Stopping);
        assert!(r.outstanding.is_none());
        let mut r = ready();
        r.host(
            p(Identity::default(), RX, 2, Body::Start(settings())),
            10000,
        );
        r.tick(12000);
        assert_eq!(r.state, State::Stopping);
        assert_eq!(r.stop_reason, ResultCode::LeaseExpired);
    }
    #[test]
    fn rate_admission_defaults_serial_limits_and_backoff_diversity() {
        assert!(admit_profile(5, 10, 921600, 339).is_ok());
        assert!(admit_profile(5, 50, 921600, 339).is_err());
        assert!(admit_profile(0, 10, 921600, 339).is_err());
        assert!(admit_profile(6, 1, 921600, 339).is_err());
        let delays = (0..20)
            .map(|r| response_delay(TX, 7, r, 123))
            .collect::<std::collections::BTreeSet<_>>();
        assert!(delays.len() > 8);
        assert!(delays.iter().all(|d| *d <= 305));
    }
    #[test]
    fn generation_exhaustion_and_wrong_kit_source_cannot_start() {
        let mut r = ready();
        r.generation = u32::MAX;
        r.host(
            p(Identity::default(), RX, 2, Body::Start(settings())),
            10000,
        );
        assert!(r.run.is_none());
        let mut t = tx();
        let s = p(RX, Identity::BROADCAST, 1, Body::Start(settings()));
        assert!(t.command(s, [6; 6], 0).is_none());
        let mut wrong = s;
        wrong.kit = 2;
        assert!(t.command(wrong, RX.mac.0, 0).is_none());
    }
}
