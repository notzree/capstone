//! CSIs/1 control, independent of acquisition protocol v1/v2. All integers LE.
//! Header: magic[0..4], version[4], kind[5], body length[6..8], kit[8..12],
//! sender MAC/epoch[12..22], destination MAC/epoch[22..32], correlation[32..36].
//! A CRC32 follows the exact body. Unknown versions/kinds and trailing bytes fail closed.
//! Roster snapshots are paginated (one entry per envelope, total=0 for an empty roster).
use crate::{DecodeError, EncodeError, NodeId, Probe, cobs, crc::crc32};
pub const MAGIC: &[u8; 4] = b"CSIs";
pub const VERSION: u8 = 1;
pub const MAX_LEN: usize = 250;
pub const HEADER_LEN: usize = 36;
pub const MAX_NODES: usize = 8;
pub const CAP_TX: u16 = 1;
pub const CAP_RX: u16 = 2;
pub const TAG_LEN: usize = 17;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Identity {
    pub mac: NodeId,
    pub epoch: u32,
}
impl Identity {
    pub const BROADCAST: Self = Self {
        mac: NodeId([0xff; 6]),
        epoch: 0,
    };
    pub fn valid(self) -> bool {
        self.epoch != 0 && self.mac.0 != [0; 6] && self.mac.0[0] & 1 == 0
    }
}

macro_rules! codes {
    ($name:ident { $($variant:ident = $value:literal),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        #[repr(u8)]
        pub enum $name { $($variant = $value),* }
        impl $name { pub fn decode(v:u8)->Result<Self,DecodeError> { match v { $($value=>Ok(Self::$variant),)* _=>Err(DecodeError::InvalidField(stringify!($name))) } } }
    }
}
codes!(Kind { Discover=1, DiscoverAck=2, Start=3, CommandAck=4, Grant=5, Keepalive=6, Stop=7, StatusRequest=8, StatusReply=9,
    Attach=16, GetRoster=17, GetStatus=18, HostHeartbeat=19, RxReady=32, NodeSeen=33, RosterSnapshot=34, DiscoveryComplete=35,
    CommandResult=36, RunState=37, DrainComplete=38, GrantOutcome=39 });
codes!(State { Idle=0, Discovering=1, Ready=2, Starting=3, Measuring=4, Stopping=5, Fault=6, Armed=7 });
codes!(ResultCode { Ok=0, Busy=1, Invalid=2, Stale=3, Incompatible=4, Full=5, MissingAck=6, Conflict=7, Exhausted=8, LeaseExpired=9, Duration=10, Reboot=11 });

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeInfo {
    pub identity: Identity,
    pub build: [u8; 8],
    pub capabilities: u16,
    pub protocol: u8,
    pub channel: u8,
    pub state: State,
    pub owner: Identity,
    pub run: u64,
}
impl Default for NodeInfo {
    fn default() -> Self {
        Self {
            identity: Identity::default(),
            build: [0; 8],
            capabilities: 0,
            protocol: VERSION,
            channel: 6,
            state: State::Idle,
            owner: Identity::default(),
            run: 0,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Start {
    pub attachment: u64,
    pub run: u64,
    pub schedule: u32,
    pub rate: u16,
    pub duration_ms: u32,
    pub lease_ms: u16,
    pub count: u8,
    pub nodes: [Identity; MAX_NODES],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    Discover {
        nonce: u32,
        round: u8,
        channel: u8,
    },
    DiscoverAck {
        nonce: u32,
        round: u8,
        info: NodeInfo,
    },
    Start(Start),
    CommandAck {
        run: u64,
        command: Kind,
        result: ResultCode,
        state: State,
        last_probe: u32,
        attempts: u64,
    },
    Grant {
        run: u64,
        token: u32,
        schedule: u32,
    },
    Keepalive {
        run: u64,
        lease_ms: u16,
    },
    Stop {
        attachment: u64,
        run: u64,
    },
    StatusRequest,
    StatusReply(NodeInfo),
    Attach {
        token: u64,
    },
    GetRoster,
    GetStatus,
    HostHeartbeat {
        token: u64,
    },
    RxReady {
        token: u64,
        info: NodeInfo,
    },
    NodeSeen(NodeInfo),
    RosterSnapshot {
        index: u8,
        total: u8,
        info: NodeInfo,
    },
    DiscoveryComplete {
        nonce: u32,
        count: u8,
        rejected: u32,
    },
    CommandResult {
        command: Kind,
        result: ResultCode,
        missing: u8,
    },
    RunState {
        run: u64,
        schedule: u32,
        state: State,
        selected: u8,
        acked: u8,
        reason: ResultCode,
    },
    DrainComplete {
        run: u64,
        capture: u64,
        export: u32,
        captured: u64,
        exported: u64,
        dropped: u64,
        selected: u8,
        confirmed: u8,
    },
    GrantOutcome {
        run: u64,
        token: u32,
        tx: Identity,
        probe_seq: u32,
        capture_seq: u32,
        result: ResultCode,
    },
}
impl Body {
    pub fn kind(self) -> Kind {
        match self {
            Self::Discover { .. } => Kind::Discover,
            Self::DiscoverAck { .. } => Kind::DiscoverAck,
            Self::Start(_) => Kind::Start,
            Self::CommandAck { .. } => Kind::CommandAck,
            Self::Grant { .. } => Kind::Grant,
            Self::Keepalive { .. } => Kind::Keepalive,
            Self::Stop { .. } => Kind::Stop,
            Self::StatusRequest => Kind::StatusRequest,
            Self::StatusReply(_) => Kind::StatusReply,
            Self::Attach { .. } => Kind::Attach,
            Self::GetRoster => Kind::GetRoster,
            Self::GetStatus => Kind::GetStatus,
            Self::HostHeartbeat { .. } => Kind::HostHeartbeat,
            Self::RxReady { .. } => Kind::RxReady,
            Self::NodeSeen(_) => Kind::NodeSeen,
            Self::RosterSnapshot { .. } => Kind::RosterSnapshot,
            Self::DiscoveryComplete { .. } => Kind::DiscoveryComplete,
            Self::CommandResult { .. } => Kind::CommandResult,
            Self::RunState { .. } => Kind::RunState,
            Self::DrainComplete { .. } => Kind::DrainComplete,
            Self::GrantOutcome { .. } => Kind::GrantOutcome,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    pub kit: u32,
    pub sender: Identity,
    pub destination: Identity,
    pub correlation: u32,
    pub body: Body,
}

struct Put<'a> {
    bytes: &'a mut [u8],
    at: usize,
}
impl Put<'_> {
    fn bytes(&mut self, b: &[u8]) -> Result<(), EncodeError> {
        let end = self.at + b.len();
        self.bytes
            .get_mut(self.at..end)
            .ok_or(EncodeError::BufferTooSmall)?
            .copy_from_slice(b);
        self.at = end;
        Ok(())
    }
    fn u8(&mut self, v: u8) -> Result<(), EncodeError> {
        self.bytes(&[v])
    }
    fn u16(&mut self, v: u16) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }
    fn u64(&mut self, v: u64) -> Result<(), EncodeError> {
        self.bytes(&v.to_le_bytes())
    }
    fn identity(&mut self, v: Identity) -> Result<(), EncodeError> {
        self.bytes(&v.mac.0)?;
        self.u32(v.epoch)
    }
    fn info(&mut self, v: NodeInfo) -> Result<(), EncodeError> {
        self.identity(v.identity)?;
        self.bytes(&v.build)?;
        self.u16(v.capabilities)?;
        self.u8(v.protocol)?;
        self.u8(v.channel)?;
        self.u8(v.state as u8)?;
        self.identity(v.owner)?;
        self.u64(v.run)
    }
}
struct Get<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Get<'_> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let end = self.at + N;
        let b = self.bytes.get(self.at..end).ok_or(DecodeError::Truncated)?;
        let mut out = [0; N];
        out.copy_from_slice(b);
        self.at = end;
        Ok(out)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(u8::from_le_bytes(self.bytes()?))
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.bytes()?))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn identity(&mut self) -> Result<Identity, DecodeError> {
        Ok(Identity {
            mac: NodeId(self.bytes()?),
            epoch: self.u32()?,
        })
    }
    fn info(&mut self) -> Result<NodeInfo, DecodeError> {
        Ok(NodeInfo {
            identity: self.identity()?,
            build: self.bytes()?,
            capabilities: self.u16()?,
            protocol: self.u8()?,
            channel: self.u8()?,
            state: State::decode(self.u8()?)?,
            owner: self.identity()?,
            run: self.u64()?,
        })
    }
}
impl Packet {
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut buf = [0; MAX_LEN];
        let mut w = Put {
            bytes: &mut buf,
            at: 0,
        };
        w.bytes(MAGIC)?;
        w.u8(VERSION)?;
        w.u8(self.body.kind() as u8)?;
        w.u16(0)?;
        w.u32(self.kit)?;
        w.identity(self.sender)?;
        w.identity(self.destination)?;
        w.u32(self.correlation)?;
        match self.body {
            Body::Discover {
                nonce,
                round,
                channel,
            } => {
                w.u32(nonce)?;
                w.u8(round)?;
                w.u8(channel)?;
            }
            Body::DiscoverAck { nonce, round, info } => {
                w.u32(nonce)?;
                w.u8(round)?;
                w.info(info)?;
            }
            Body::Start(s) => {
                w.u64(s.attachment)?;
                w.u64(s.run)?;
                w.u32(s.schedule)?;
                w.u16(s.rate)?;
                w.u32(s.duration_ms)?;
                w.u16(s.lease_ms)?;
                w.u8(s.count)?;
                for n in s.nodes {
                    w.identity(n)?;
                }
            }
            Body::CommandAck {
                run,
                command,
                result,
                state,
                last_probe,
                attempts,
            } => {
                w.u64(run)?;
                w.u8(command as u8)?;
                w.u8(result as u8)?;
                w.u8(state as u8)?;
                w.u32(last_probe)?;
                w.u64(attempts)?;
            }
            Body::Grant {
                run,
                token,
                schedule,
            } => {
                w.u64(run)?;
                w.u32(token)?;
                w.u32(schedule)?;
            }
            Body::Keepalive { run, lease_ms } => {
                w.u64(run)?;
                w.u16(lease_ms)?;
            }
            Body::Stop { attachment, run } => {
                w.u64(attachment)?;
                w.u64(run)?;
            }
            Body::Attach { token } | Body::HostHeartbeat { token } => w.u64(token)?,
            Body::StatusReply(info) | Body::NodeSeen(info) => w.info(info)?,
            Body::RxReady { token, info } => {
                w.u64(token)?;
                w.info(info)?;
            }
            Body::RosterSnapshot { index, total, info } => {
                w.u8(index)?;
                w.u8(total)?;
                w.info(info)?;
            }
            Body::DiscoveryComplete {
                nonce,
                count,
                rejected,
            } => {
                w.u32(nonce)?;
                w.u8(count)?;
                w.u32(rejected)?;
            }
            Body::CommandResult {
                command,
                result,
                missing,
            } => {
                w.u8(command as u8)?;
                w.u8(result as u8)?;
                w.u8(missing)?;
            }
            Body::RunState {
                run,
                schedule,
                state,
                selected,
                acked,
                reason,
            } => {
                w.u64(run)?;
                w.u32(schedule)?;
                w.u8(state as u8)?;
                w.u8(selected)?;
                w.u8(acked)?;
                w.u8(reason as u8)?;
            }
            Body::DrainComplete {
                run,
                capture,
                export,
                captured,
                exported,
                dropped,
                selected,
                confirmed,
            } => {
                w.u64(run)?;
                w.u64(capture)?;
                w.u32(export)?;
                w.u64(captured)?;
                w.u64(exported)?;
                w.u64(dropped)?;
                w.u8(selected)?;
                w.u8(confirmed)?;
            }
            Body::GrantOutcome {
                run,
                token,
                tx,
                probe_seq,
                capture_seq,
                result,
            } => {
                w.u64(run)?;
                w.u32(token)?;
                w.identity(tx)?;
                w.u32(probe_seq)?;
                w.u32(capture_seq)?;
                w.u8(result as u8)?;
            }
            Body::StatusRequest | Body::GetStatus | Body::GetRoster => {}
        }
        let len = w.at;
        buf.get_mut(6..8)
            .ok_or(EncodeError::BufferTooSmall)?
            .copy_from_slice(&((len - HEADER_LEN) as u16).to_le_bytes());
        let crc = crc32(buf.get(..len).ok_or(EncodeError::BufferTooSmall)?);
        let mut w = Put { bytes: out, at: 0 };
        w.bytes(buf.get(..len).ok_or(EncodeError::BufferTooSmall)?)?;
        w.u32(crc)?;
        Ok(w.at)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() > MAX_LEN {
            return Err(DecodeError::Oversized);
        }
        let mut r = Get { bytes, at: 0 };
        if &r.bytes::<4>()? != MAGIC {
            return Err(DecodeError::BadMagic);
        }
        let version = r.u8()?;
        if version != VERSION {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        let raw = r.u8()?;
        let kind = Kind::decode(raw).map_err(|_| DecodeError::UnknownKind(raw))?;
        let length = usize::from(r.u16()?) + HEADER_LEN;
        if bytes.len() != length + 4 {
            return Err(if bytes.len() < length + 4 {
                DecodeError::Truncated
            } else {
                DecodeError::TrailingBytes
            });
        }
        let mut crc = Get { bytes, at: length };
        if crc.u32()? != crc32(bytes.get(..length).ok_or(DecodeError::Truncated)?) {
            return Err(DecodeError::BadCrc);
        }
        let kit = r.u32()?;
        let sender = r.identity()?;
        let destination = r.identity()?;
        let correlation = r.u32()?;
        let body = match kind {
            Kind::Discover => Body::Discover {
                nonce: r.u32()?,
                round: r.u8()?,
                channel: r.u8()?,
            },
            Kind::DiscoverAck => Body::DiscoverAck {
                nonce: r.u32()?,
                round: r.u8()?,
                info: r.info()?,
            },
            Kind::Start => {
                let mut s = Start {
                    attachment: r.u64()?,
                    run: r.u64()?,
                    schedule: r.u32()?,
                    rate: r.u16()?,
                    duration_ms: r.u32()?,
                    lease_ms: r.u16()?,
                    count: r.u8()?,
                    nodes: [Identity::default(); MAX_NODES],
                };
                if s.count == 0 || usize::from(s.count) > MAX_NODES {
                    return Err(DecodeError::InvalidField("count"));
                }
                for n in &mut s.nodes {
                    *n = r.identity()?
                }
                Body::Start(s)
            }
            Kind::CommandAck => Body::CommandAck {
                run: r.u64()?,
                command: Kind::decode(r.u8()?)?,
                result: ResultCode::decode(r.u8()?)?,
                state: State::decode(r.u8()?)?,
                last_probe: r.u32()?,
                attempts: r.u64()?,
            },
            Kind::Grant => Body::Grant {
                run: r.u64()?,
                token: r.u32()?,
                schedule: r.u32()?,
            },
            Kind::Keepalive => Body::Keepalive {
                run: r.u64()?,
                lease_ms: r.u16()?,
            },
            Kind::Stop => Body::Stop {
                attachment: r.u64()?,
                run: r.u64()?,
            },
            Kind::StatusRequest => Body::StatusRequest,
            Kind::GetStatus => Body::GetStatus,
            Kind::GetRoster => Body::GetRoster,
            Kind::Attach => Body::Attach { token: r.u64()? },
            Kind::HostHeartbeat => Body::HostHeartbeat { token: r.u64()? },
            Kind::StatusReply => Body::StatusReply(r.info()?),
            Kind::NodeSeen => Body::NodeSeen(r.info()?),
            Kind::RxReady => Body::RxReady {
                token: r.u64()?,
                info: r.info()?,
            },
            Kind::RosterSnapshot => {
                let index = r.u8()?;
                let total = r.u8()?;
                if usize::from(total) > MAX_NODES || (total != 0 && index >= total) {
                    return Err(DecodeError::InvalidField("snapshot"));
                }
                Body::RosterSnapshot {
                    index,
                    total,
                    info: r.info()?,
                }
            }
            Kind::DiscoveryComplete => Body::DiscoveryComplete {
                nonce: r.u32()?,
                count: r.u8()?,
                rejected: r.u32()?,
            },
            Kind::CommandResult => Body::CommandResult {
                command: Kind::decode(r.u8()?)?,
                result: ResultCode::decode(r.u8()?)?,
                missing: r.u8()?,
            },
            Kind::RunState => Body::RunState {
                run: r.u64()?,
                schedule: r.u32()?,
                state: State::decode(r.u8()?)?,
                selected: r.u8()?,
                acked: r.u8()?,
                reason: ResultCode::decode(r.u8()?)?,
            },
            Kind::DrainComplete => Body::DrainComplete {
                run: r.u64()?,
                capture: r.u64()?,
                export: r.u32()?,
                captured: r.u64()?,
                exported: r.u64()?,
                dropped: r.u64()?,
                selected: r.u8()?,
                confirmed: r.u8()?,
            },
            Kind::GrantOutcome => Body::GrantOutcome {
                run: r.u64()?,
                token: r.u32()?,
                tx: r.identity()?,
                probe_seq: r.u32()?,
                capture_seq: r.u32()?,
                result: ResultCode::decode(r.u8()?)?,
            },
        };
        if r.at != length {
            return Err(DecodeError::TrailingBytes);
        }
        Ok(Self {
            kit,
            sender,
            destination,
            correlation,
            body,
        })
    }
    pub fn encode_stream(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut raw = [0; MAX_LEN];
        let n = self.encode(&mut raw)?;
        let n = cobs::encode(raw.get(..n).ok_or(EncodeError::BufferTooSmall)?, out)?;
        *out.get_mut(n).ok_or(EncodeError::BufferTooSmall)? = 0;
        Ok(n + 1)
    }
    pub fn air_valid(&self, source: [u8; 6], kit: u32, own: Identity) -> bool {
        self.kit == kit
            && self.sender.valid()
            && self.sender.mac.0 == source
            && (self.destination == own || self.destination == Identity::BROADCAST)
    }
}
/// Appended after CSIp; intentionally different from legacy CSIt/CSIb.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeTag {
    pub run: u64,
    pub grant: u32,
}
impl ProbeTag {
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, EncodeError> {
        let mut w = Put { bytes: out, at: 0 };
        w.bytes(b"CSIg")?;
        w.u8(1)?;
        w.u64(self.run)?;
        w.u32(self.grant)?;
        Ok(w.at)
    }
    pub fn find(payload: &[u8]) -> Option<Self> {
        payload.windows(TAG_LEN).find_map(|b| {
            let mut r = Get { bytes: b, at: 0 };
            if &r.bytes::<4>().ok()? != b"CSIg" || r.u8().ok()? != 1 {
                return None;
            }
            Some(Self {
                run: r.u64().ok()?,
                grant: r.u32().ok()?,
            })
        })
    }
    pub fn sensing(payload: &[u8]) -> Option<(Probe, Self)> {
        Some((Probe::find(payload)?, Self::find(payload)?))
    }
}
