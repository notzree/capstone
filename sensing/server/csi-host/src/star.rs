//! One serial owner, typed commands, independent bounded sink workers and no hidden spool.
use crate::{
    Error, Record, RecordKind,
    delivery::{DeliveryHealth, Publisher},
    geometry::Geometry,
    manifest::{self, Manifest, SourceInfo},
    session::{SessionWriter, WriterOptions, wall_now_ns},
    trial::{self, Bundle, Labels, Stamp},
};
use csi_protocol::{
    Message, frame,
    star::{self as wire, Body, Identity, Kind, NodeInfo, Packet, ResultCode, Start, State},
    stream,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{broadcast, mpsc, oneshot},
    time::{interval, timeout},
};
use tokio_serial::SerialPortBuilderExt;
use uuid::Uuid;

fn fail(s: impl Into<String>) -> Error {
    Error::Config(s.into())
}
fn io_error(e: impl std::fmt::Display) -> Error {
    fail(e.to_string())
}
pub fn node_json(n: NodeInfo) -> Value {
    json!({"id":n.identity.mac.to_string(),"epoch":n.identity.epoch,"build_id":n.build.iter().map(|b|format!("{b:02x}")).collect::<String>(),"capabilities":n.capabilities,"protocol":n.protocol,"channel":n.channel,"state":format!("{:?}",n.state),"owner":n.owner.mac.to_string(),"owner_epoch":n.owner.epoch,"run":n.run})
}
pub fn packet_json(p: &Packet) -> Value {
    let fields = match p.body {
        Body::RxReady { token, info } => json!({"attachment":token,"node":node_json(info)}),
        Body::NodeSeen(info) | Body::StatusReply(info) => json!({"node":node_json(info)}),
        Body::RosterSnapshot { index, total, info } => {
            json!({"index":index,"total":total,"node":node_json(info)})
        }
        Body::DiscoveryComplete {
            nonce,
            count,
            rejected,
        } => json!({"nonce":nonce,"count":count,"rejected":rejected}),
        Body::CommandResult {
            command,
            result,
            missing,
        } => {
            json!({"command":format!("{command:?}"),"result":format!("{result:?}"),"missing_mask":missing})
        }
        Body::CommandAck {
            run,
            command,
            result,
            state,
            last_probe,
            attempts,
        } => {
            json!({"run":run,"command":format!("{command:?}"),"result":format!("{result:?}"),"state":format!("{state:?}"),"last_probe":last_probe,"attempts":attempts})
        }
        Body::RunState {
            run,
            schedule,
            state,
            selected,
            acked,
            reason,
        } => {
            json!({"run":run,"schedule":schedule,"state":format!("{state:?}"),"selected_mask":selected,"acked_mask":acked,"reason":format!("{reason:?}")})
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
            json!({"run":run,"capture_boundary_exclusive":capture,"last_export_seq":export,"captured":captured,"exported":exported,"dropped":dropped,"selected_mask":selected,"confirmed_mask":confirmed})
        }
        Body::GrantOutcome {
            run,
            token,
            tx,
            probe_seq,
            capture_seq,
            result,
        } => {
            json!({"run":run,"grant":token,"tx":tx.mac.to_string(),"tx_epoch":tx.epoch,"probe_seq":probe_seq,"capture_seq":capture_seq,"result":format!("{result:?}")})
        }
        _ => Value::Null,
    };
    json!({"kind":format!("{:?}",p.body.kind()),"sender":p.sender.mac.to_string(),"sender_epoch":p.sender.epoch,"correlation":p.correlation,"fields":fields,"body":format!("{:?}",p.body)})
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StartOptions {
    pub out: Option<PathBuf>,
    pub socket: Option<PathBuf>,
    pub geometry: Option<PathBuf>,
    pub plan: Option<PathBuf>,
    pub rate: u16,
    pub duration: u32,
    pub allow_long_run: bool,
    pub nodes: Vec<String>,
}
impl Default for StartOptions {
    fn default() -> Self {
        Self {
            out: None,
            socket: None,
            geometry: None,
            plan: None,
            rate: 10,
            duration: 3600,
            allow_long_run: false,
            nodes: vec![],
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Nodes,
    Status,
    Discover,
    Start(StartOptions),
    Stop,
    Begin {
        activity: String,
        position: Option<String>,
    },
    End,
    Note {
        text: String,
    },
    Shutdown,
}
struct Request {
    command: Command,
    reply: oneshot::Sender<Result<Value, Error>>,
}
#[derive(Clone)]
pub struct StarController {
    requests: mpsc::Sender<Request>,
    events: broadcast::Sender<Value>,
}
impl StarController {
    pub async fn request(&self, command: Command) -> Result<Value, Error> {
        let (tx, rx) = oneshot::channel();
        self.requests
            .send(Request { command, reply: tx })
            .await
            .map_err(|_| fail("star controller closed"))?;
        rx.await.map_err(|_| fail("star controller closed"))?
    }
    pub fn events(&self) -> broadcast::Receiver<Value> {
        self.events.subscribe()
    }
    /// Only USB-serial ports are probed, passively. No command is sent until a CSIs RX_READY.
    pub async fn open(device: &str) -> Result<Self, Error> {
        if device != "auto" {
            return Self::open_path(Path::new(device)).await;
        }
        let candidates = tokio_serial::available_ports()
            .map_err(io_error)?
            .into_iter()
            .filter(|p| matches!(p.port_type, tokio_serial::SerialPortType::UsbPort(_)))
            .map(|p| p.port_name)
            .collect::<Vec<_>>();
        let mut found = vec![];
        for path in candidates {
            if let Ok(serial) = open_serial(Path::new(&path))
                && let Ok((serial, ready)) = passive(serial).await
            {
                found.push((serial, ready));
            }
        }
        if found.len() != 1 {
            return Err(fail(format!(
                "found {} compatible star RXs; plug in one RX or use --device explicitly",
                found.len()
            )));
        }
        let (serial, ready) = found.pop().unwrap();
        Self::from_ready(serial, ready).await
    }
    async fn open_path(path: &Path) -> Result<Self, Error> {
        let serial = open_serial(path)?;
        let (serial, ready) = passive(serial).await?;
        Self::from_ready(serial, ready).await
    }
    /// Also used with PTYs/in-memory transports. No RF side effects before capability verification.
    pub async fn connect<T>(io: T) -> Result<Self, Error>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let (io, ready) = passive(io).await?;
        Self::from_ready(io, ready).await
    }
    async fn from_ready<T>(mut io: T, ready: Packet) -> Result<Self, Error>
    where
        T: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let Body::RxReady { info, .. } = ready.body else {
            return Err(fail("expected RX_READY"));
        };
        if info.capabilities != wire::CAP_RX
            || info.protocol != wire::VERSION
            || info.identity != ready.sender
            || !ready.sender.valid()
        {
            return Err(fail("incompatible star RX capability/identity"));
        }
        let token = (Uuid::now_v7().as_u128() as u64).max(1);
        let attach = Packet {
            kit: ready.kit,
            sender: Identity::default(),
            destination: ready.sender,
            correlation: 1,
            body: Body::Attach { token },
        };
        write_packet(&mut io, attach).await?;
        let (tx, rx) = mpsc::channel(16);
        let (events, _) = broadcast::channel(256);
        let controller = Self {
            requests: tx,
            events: events.clone(),
        };
        tokio::spawn(async move {
            Actor::new(io, ready, token, rx, events).run().await;
        });
        Ok(controller)
    }
}
fn open_serial(path: &Path) -> Result<tokio_serial::SerialStream, Error> {
    let serial = tokio_serial::new(path.to_string_lossy(), 921600)
        .preserve_dtr_on_open()
        .data_bits(tokio_serial::DataBits::Eight)
        .parity(tokio_serial::Parity::None)
        .stop_bits(tokio_serial::StopBits::One)
        .flow_control(tokio_serial::FlowControl::None)
        .open_native_async()
        .map_err(io_error)?;
    // serialport opens exclusively by default; repeating TIOCEXCL fails on macOS.
    Ok(serial)
}
async fn passive<T: AsyncRead + Unpin>(mut io: T) -> Result<(T, Packet), Error> {
    let ready = timeout(Duration::from_secs(3), async {
        let mut deframer = stream::Deframer::new();
        let mut bytes = [0; 2048];
        loop {
            let n = io.read(&mut bytes).await.map_err(io_error)?;
            if n == 0 {
                return Err(fail("serial EOF during star handshake"));
            }
            let mut b = &bytes[..n];
            while let Some(raw) = deframer.push(&mut b) {
                if let Ok(raw) = raw
                    && let Ok(p) = Packet::decode(raw)
                    && matches!(p.body, Body::RxReady { .. })
                {
                    return Ok(p);
                }
            }
        }
    })
    .await
    .map_err(|_| fail("no star-rx capability handshake (no commands sent)"))??;
    Ok((io, ready))
}
async fn write_packet<T: AsyncWrite + Unpin>(io: &mut T, p: Packet) -> Result<(), Error> {
    let mut bytes = [0; 254];
    let n = p.encode_stream(&mut bytes).map_err(io_error)?;
    // ESP32 UART has a 128-byte hardware FIFO. Pace the infrequent command
    // envelopes so a >128-byte START cannot overrun it during radio scheduling.
    for chunk in bytes[..n].chunks(64) {
        timeout(Duration::from_millis(250), io.write_all(chunk))
            .await
            .map_err(|_| fail("serial command write timeout"))?
            .map_err(io_error)?;
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    Ok(())
}

struct Known {
    info: NodeInfo,
    seen: Option<Instant>,
    observed: bool,
}
struct Active {
    sink: mpsc::Sender<SinkInput>,
    health: Arc<DeliveryHealth>,
    file: bool,
    socket: bool,
    session: Uuid,
    metadata: Value,
    rf: u64,
    started: bool,
    preparing: Instant,
    origin: Instant,
    received_hello: bool,
    stopping: Option<Instant>,
}
struct Prepared {
    sink: mpsc::Sender<SinkInput>,
    health: Arc<DeliveryHealth>,
    session: Uuid,
    metadata: Value,
}
enum SinkInput {
    Record(Arc<Record>),
    Label(Command, Stamp, oneshot::Sender<Result<Value, Error>>),
    Finish(Value, Stamp),
}
struct Actor<T> {
    io: T,
    rx: Identity,
    info: NodeInfo,
    kit: u32,
    token: u64,
    seq: u32,
    state: State,
    attached: bool,
    requests: mpsc::Receiver<Request>,
    events: broadcast::Sender<Value>,
    known: Vec<Known>,
    clock: Instant,
    active: Option<Active>,
    pending: Option<oneshot::Sender<Result<Value, Error>>>,
    stoppers: Vec<oneshot::Sender<Result<Value, Error>>>,
    prepare_tx: mpsc::Sender<Result<Prepared, Error>>,
    prepare_rx: mpsc::Receiver<Result<Prepared, Error>>,
    finish_tx: mpsc::Sender<Result<Value, Error>>,
    finish_rx: mpsc::Receiver<Result<Value, Error>>,
    starting: bool,
    finishing: bool,
    shutdown: bool,
    last: Value,
    records: u64,
    drops: u64,
    duration: u32,
    rate: u16,
    selected: Vec<Identity>,
}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Actor<T> {
    fn new(
        io: T,
        p: Packet,
        token: u64,
        requests: mpsc::Receiver<Request>,
        events: broadcast::Sender<Value>,
    ) -> Self {
        let Body::RxReady { info, .. } = p.body else {
            unreachable!()
        };
        let (pt, pr) = mpsc::channel(1);
        let (ft, fr) = mpsc::channel(1);
        Self {
            io,
            rx: p.sender,
            info,
            kit: p.kit,
            token,
            seq: 1,
            state: info.state,
            attached: false,
            requests,
            events,
            known: vec![],
            clock: Instant::now(),
            active: None,
            pending: None,
            stoppers: vec![],
            prepare_tx: pt,
            prepare_rx: pr,
            finish_tx: ft,
            finish_rx: fr,
            starting: false,
            finishing: false,
            shutdown: false,
            last: Value::Null,
            records: 0,
            drops: 0,
            duration: 0,
            rate: 0,
            selected: vec![],
        }
    }
    fn stamp(&self) -> Stamp {
        Stamp {
            mono_ns: self
                .active
                .as_ref()
                .map_or_else(|| self.clock.elapsed(), |a| a.origin.elapsed())
                .as_nanos() as u64,
            wall_ns: wall_now_ns(),
        }
    }
    fn emit(&self, v: Value) {
        let _ = self.events.send(v);
    }
    fn status(&self) -> Value {
        json!({"schema":"csi-star-status/1","receiver":node_json(self.info),"state":format!("{:?}",self.state),"attached":self.attached,"preparing":self.starting,"finishing":self.finishing,"records":self.records,"sink_input_drops":self.drops,
        "run":self.active.as_ref().map(|a|json!({"session_id":a.session,"rf_run":a.rf,"metadata":a.metadata,"started":a.started,"requested_per_tx_hz":self.rate,"requested_aggregate_hz":u32::from(self.rate)*self.selected.len() as u32,"delivery":*a.health.status.lock().unwrap()})),"last_run":self.last,"nodes":self.nodes()})
    }
    fn nodes(&self) -> Vec<Value> {
        self.known
            .iter()
            .map(|k| {
                let mut v = node_json(k.info);
                let stale = k.seen.is_none_or(|t| t.elapsed() >= Duration::from_secs(6));
                v["stale"] = stale.into();
                v["ready"] = (!stale
                    && k.info.run == 0
                    && k.info.capabilities & wire::CAP_TX != 0
                    && k.info.protocol == wire::VERSION
                    && k.info.channel == self.info.channel)
                    .into();
                v["csi_observed"] = k.observed.into();
                v
            })
            .collect()
    }
    async fn send(&mut self, body: Body) -> Result<(), Error> {
        self.seq = self
            .seq
            .checked_add(1)
            .ok_or_else(|| fail("host command IDs exhausted; reconnect"))?;
        write_packet(
            &mut self.io,
            Packet {
                kit: self.kit,
                sender: Identity::default(),
                destination: self.rx,
                correlation: self.seq,
                body,
            },
        )
        .await
    }
    async fn stop(&mut self, reason: &str) {
        let rf = self.active.as_ref().map_or(0, |a| a.rf);
        if let Some(a) = &mut self.active {
            if a.stopping.is_some() {
                return;
            }
            a.stopping = Some(Instant::now());
            a.metadata["stop_reason"] = reason.into();
        }
        let _ = self
            .send(Body::Stop {
                attachment: self.token,
                run: if rf == u64::MAX { 0 } else { rf },
            })
            .await;
    }
    fn queue(&mut self, record: Record) {
        let record = Arc::new(record);
        if let Ok((_, Message::Hello(_))) = frame::decode(&record.payload)
            && let Some(a) = &mut self.active
        {
            a.received_hello = true;
        }
        if let Some(a) = &self.active
            && a.sink.try_send(SinkInput::Record(record)).is_err()
        {
            self.drops += 1;
        }
    }
    async fn finish(&mut self, drain: Option<Packet>, reason: &str) {
        let stamp = self.stamp();
        let Some(mut a) = self.active.take() else {
            return;
        };
        self.finishing = true;
        a.metadata["drain"] = drain.as_ref().map_or(Value::Null, packet_json);
        if reason != "drained" || a.metadata["stop_reason"].is_null() {
            a.metadata["stop_reason"] = reason.into();
        }
        a.metadata["sink_input_drops"] = self.drops.into();
        if let Some(p) = drain
            && let Body::DrainComplete {
                selected,
                confirmed,
                captured,
                exported,
                dropped,
                ..
            } = p.body
        {
            a.metadata["stop_confirmed"] = (selected == confirmed).into();
            a.metadata["captured"] = captured.into();
            a.metadata["exported"] = exported.into();
            a.metadata["device_drops"] = dropped.into();
        }
        let result = timeout(
            Duration::from_millis(250),
            a.sink.send(SinkInput::Finish(a.metadata, stamp)),
        )
        .await;
        if !matches!(result, Ok(Ok(()))) {
            let _ = self.finish_tx.try_send(Err(fail(
                "sink failed before finalization; output remains incomplete",
            )));
        }
    }
    async fn command(&mut self, r: Request) {
        match r.command {
            Command::Nodes => {
                let _ = r.reply.send(Ok(json!(self.nodes())));
            }
            Command::Status => {
                let _ = r.reply.send(Ok(self.status()));
            }
            Command::Discover => {
                let result = if self.active.is_some() || self.starting || self.finishing {
                    Err(fail("stop the current run before discovery"))
                } else {
                    self.known.clear();
                    self.send(Body::Discover {
                        nonce: (Uuid::now_v7().as_u128() as u32).max(1),
                        round: 0,
                        channel: self.info.channel,
                    })
                    .await
                    .map(|_| json!({"discovering_seconds":10}))
                };
                let _ = r.reply.send(result);
            }
            Command::Start(options) => {
                let result = self.preflight(&options);
                if let Err(e) = result {
                    let _ = r.reply.send(Err(e));
                    return;
                }
                self.selected = result.unwrap();
                self.rate = options.rate;
                self.duration = options.duration * 1000;
                self.starting = true;
                self.pending = Some(r.reply);
                self.drops = 0;
                let metadata = json!({"schema":"csi-star-run/1","host_run_uuid":Uuid::now_v7(),"kit_id":self.kit,"receiver":node_json(self.info),"frozen_roster":self.selected.iter().map(|id|node_json(self.known.iter().find(|k|k.info.identity==*id).unwrap().info)).collect::<Vec<_>>(),"requested_per_tx_hz":options.rate,"duration_seconds":options.duration,"label_time_basis":"host_arrival_proxy","stop_confirmed":false});
                let tx = self.prepare_tx.clone();
                let finish = self.finish_tx.clone();
                let info = self.info;
                tokio::task::spawn_blocking(move || {
                    let result = prepare(options, metadata, info, finish);
                    let _ = tx.blocking_send(result);
                });
            }
            Command::Stop | Command::Shutdown => {
                if matches!(r.command, Command::Shutdown) {
                    self.shutdown = true;
                }
                if self.active.is_none() && !self.starting && !self.finishing {
                    let _ = r
                        .reply
                        .send(Ok(json!({"stopped":true,"last_run":self.last})));
                } else {
                    if self.stoppers.len() >= 16 {
                        let _ = r.reply.send(Err(fail("too many pending STOP requests")));
                        return;
                    }
                    self.stoppers.push(r.reply);
                    self.stop("operator").await;
                }
            }
            cmd @ (Command::Begin { .. } | Command::End | Command::Note { .. }) => {
                let stamp = self.stamp();
                if let Some(a) = &self.active
                    && a.started
                    && a.stopping.is_none()
                {
                    if let Err(
                        mpsc::error::TrySendError::Full(SinkInput::Label(_, _, reply))
                        | mpsc::error::TrySendError::Closed(SinkInput::Label(_, _, reply)),
                    ) = a.sink.try_send(SinkInput::Label(cmd, stamp, r.reply))
                    {
                        let _ = reply.send(Err(fail("label queue unavailable")));
                    }
                    return;
                }
                let _ = r.reply.send(Err(fail(
                    "labels require an active run and a surveyed --plan/--geometry",
                )));
            }
        }
    }
    fn preflight(&self, o: &StartOptions) -> Result<Vec<Identity>, Error> {
        if !self.attached
            || self.state != State::Ready
            || self.active.is_some()
            || self.starting
            || self.finishing
        {
            return Err(fail("RX must be attached and Ready with no current run"));
        }
        if o.out.is_none() && o.socket.is_none() {
            return Err(fail("START requires --out, --socket, or both"));
        }
        if o.duration == 0 || o.duration > 7200 || (!o.allow_long_run && o.duration > 3600) {
            return Err(fail(
                "duration must be 1..3600s; --allow-long-run permits up to 7200s",
            ));
        }
        if o.plan.is_some() && (o.geometry.is_none() || o.out.is_none()) {
            return Err(fail(
                "labelled runs require --out, --plan and surveyed --geometry",
            ));
        }
        let mut selected = vec![];
        for k in &self.known {
            let wanted = o.nodes.is_empty() || o.nodes.contains(&k.info.identity.mac.to_string());
            if wanted {
                if k.seen.is_none_or(|t| t.elapsed() >= Duration::from_secs(6))
                    || k.info.run != 0
                    || k.info.protocol != wire::VERSION
                    || k.info.capabilities & wire::CAP_TX == 0
                    || k.info.channel != self.info.channel
                {
                    if !o.nodes.is_empty() {
                        return Err(fail("selected node is stale, busy or incompatible"));
                    }
                    continue;
                }
                selected.push(k.info.identity);
            }
        }
        if !o.nodes.is_empty() && (selected.len() != o.nodes.len()) {
            return Err(fail("unknown or duplicate selected node IDs"));
        }
        if selected.is_empty() {
            return Err(fail(
                "no compatible live idle TX selected; finish discovery or check nodes",
            ));
        }
        csi_node::star::admit_profile(selected.len(), o.rate, 921600, stream::MAX_ENCODED_LEN)
            .map_err(|_| {
                fail("rate exceeds five-node, serial (30% reserve), or 20ms grant budget")
            })?;
        Ok(selected)
    }
    async fn packet(&mut self, p: Packet) {
        if p.kit != self.kit {
            return;
        }
        // Forwarded TX ACKs are provenance, never change the physical RX identity.
        if p.sender != self.rx {
            return;
        }
        match p.body {
            Body::RxReady { token, info } => {
                if p.sender != self.rx || info.identity != self.rx {
                    return;
                }
                if token == self.token {
                    self.attached = true;
                }
                self.info = info;
                self.state = info.state;
            }
            Body::NodeSeen(info) => {
                if let Some(k) = self
                    .known
                    .iter_mut()
                    .find(|k| k.info.identity.mac == info.identity.mac)
                {
                    if k.info.identity != info.identity {
                        k.observed = false;
                    }
                    k.info = info;
                    k.seen = Some(Instant::now());
                } else if self.known.len() < 8 {
                    self.known.push(Known {
                        info,
                        seen: Some(Instant::now()),
                        observed: false,
                    });
                }
            }
            Body::RosterSnapshot { total, info, .. } => {
                if total > 0
                    && !self
                        .known
                        .iter()
                        .any(|k| k.info.identity.mac == info.identity.mac)
                    && self.known.len() < 8
                {
                    self.known.push(Known {
                        info,
                        seen: None,
                        observed: false,
                    });
                }
            }
            Body::RunState {
                run,
                schedule,
                state,
                reason,
                ..
            } => {
                self.state = state;
                self.info.state = state;
                if let Some(a) = &mut self.active {
                    if state == State::Stopping {
                        a.metadata["radio_stop_reason"] = format!("{reason:?}").into();
                    }
                    if run != 0 {
                        a.rf = run;
                        a.metadata["rf_run_token"] = run.into();
                        a.metadata["schedule_generation"] = schedule.into();
                    }
                    if state == State::Measuring && !a.started {
                        a.started = true;
                        if let Some(reply) = self.pending.take() {
                            let _=reply.send(Ok(json!({"session_id":a.session,"rf_run_token":run,"schedule_generation":schedule,"requested_per_tx_hz":self.rate,"requested_aggregate_hz":u32::from(self.rate)*self.selected.len() as u32})));
                        }
                    }
                }
            }
            Body::CommandResult {
                command: Kind::Start,
                result,
                ..
            } if result != ResultCode::Ok => {
                if let Some(reply) = self.pending.take() {
                    let _ = reply.send(Err(fail(format!(
                        "START rejected: {result:?}; rollback pending"
                    ))));
                }
                if self
                    .active
                    .as_ref()
                    .is_some_and(|a| a.rf == 0 || a.rf == u64::MAX)
                {
                    self.finish(None, "start_rejected").await;
                }
            }
            Body::DrainComplete { .. } => {
                self.finish(Some(p), "drained").await;
            }
            Body::GrantOutcome {
                tx,
                result: ResultCode::Ok,
                ..
            } => {
                if let Some(k) = self.known.iter_mut().find(|k| k.info.identity == tx) {
                    k.observed = true;
                    k.seen = Some(Instant::now());
                }
            }
            _ => {}
        }
    }
    async fn run(mut self) {
        let mut bytes = [0; 4096];
        let mut deframer = stream::Deframer::new();
        let mut ticker = interval(Duration::from_millis(50));
        let mut heartbeat = Instant::now() - Duration::from_secs(1);
        let mut time_probe = Instant::now();
        loop {
            tokio::select! {
                request=self.requests.recv(),if !self.shutdown=>{if let Some(r)=request{self.command(r).await}else{self.shutdown=true;self.stop("controller_closed").await;}},
                result=self.io.read(&mut bytes)=>{
                    match result {
                        Ok(n) if n>0=>{let mut input=&bytes[..n];while let Some(raw)=deframer.push(&mut input){let stamp=self.stamp();let(payload,kind)=match raw{Ok(raw)=>(raw.to_vec(),if raw.starts_with(wire::MAGIC){RecordKind::StarControl}else{RecordKind::Frame}),Err(_)=>(vec![2],RecordKind::StreamError)};
                            let packet=if kind==RecordKind::StarControl{Packet::decode(&payload).ok()}else{None};
                            // A different RX boot terminates this attachment; it never resumes RF.
                            if let Some(p)=packet&&matches!(p.body,Body::RxReady{..})&&p.sender!=self.rx {self.shutdown=true;self.finish(None,"receiver_reboot").await;break}
                            self.records+=1;self.queue(Record{mono_ns:stamp.mono_ns,wall_ns:stamp.wall_ns,source:0,kind,payload});
                            if let Some(p)=packet{self.emit(json!({"event":"device","mono_ns":stamp.mono_ns,"packet":packet_json(&p)}));self.packet(p).await;}
                        }},
                        _=>{self.shutdown=true;self.finish(None,"serial_disconnected").await;}
                    }
                },
                prepared=self.prepare_rx.recv(),if self.starting=>{
                    self.starting=false;
                    match prepared.unwrap_or_else(||Err(fail("preparation worker closed"))) {
                        Ok(p)=>{self.active=Some(Active{sink:p.sink,health:p.health,file:p.metadata["file_mode"]==true,socket:p.metadata["stream_mode"]==true,session:p.session,metadata:p.metadata,rf:0,started:false,preparing:Instant::now(),origin:Instant::now(),received_hello:false,stopping:None});if self.shutdown||!self.stoppers.is_empty(){self.finish(None,"cancelled_before_start").await;}else{self.emit(json!({"event":"outputs_ready","connect_primary_now":self.active.as_ref().is_some_and(|a|a.socket)}));}},
                        Err(e)=>{if let Some(r)=self.pending.take(){let _=r.send(Err(e));}for r in self.stoppers.drain(..){let _=r.send(Ok(json!({"stopped":true})));}}
                    }
                },
                finished=self.finish_rx.recv(),if self.active.is_some()||self.finishing=>{
                    if let Some(result)=finished {self.finishing=false;if self.active.is_some(){self.stop("sink_failure").await;self.last=json!({"error":result.err().map(|e|e.to_string())});}else{match result{Ok(v)=>{self.last=v.clone();for r in self.stoppers.drain(..){let _=r.send(Ok(v.clone()));}self.emit(json!({"event":"run_finished","status":v}));},Err(e)=>{self.last=json!({"error":e.to_string(),"complete":false});for r in self.stoppers.drain(..){let _=r.send(Err(fail(e.to_string())));}}}if let Some(r)=self.pending.take(){let _=r.send(Err(fail("run ended before successful START")));}}}
                },
                _=ticker.tick()=>{
                    if self.clock.elapsed()>Duration::from_secs(4)&&!self.attached{self.emit(json!({"event":"fault","error":"attachment handshake timed out"}));self.shutdown=true;}
                    if heartbeat.elapsed()>=Duration::from_millis(500){if self.send(Body::HostHeartbeat{token:self.token}).await.is_err(){self.shutdown=true;}heartbeat=Instant::now();}
                    if time_probe.elapsed()>=Duration::from_secs(1){let h=csi_protocol::Header{node:self.rx.mac,epoch:0,seq:self.seq};let msg=Message::TimeProbe(csi_protocol::TimeProbe{probe_id:self.seq,host_time:self.stamp().mono_ns});if let Ok(bytes)=stream::Encoder::new().encode(&h,&msg){let _=timeout(Duration::from_millis(250),self.io.write_all(bytes)).await;}time_probe=Instant::now();}
                    if self.drops>0&&self.active.as_ref().is_some_and(|a|a.stopping.is_none()){self.stop("sink_input_overflow").await;}
                    if let Some(a)=&self.active {
                        if a.stopping.is_some_and(|t|t.elapsed()>Duration::from_secs(4)){self.finish(None,"drain_timeout; TX lease fallback, unconfirmed stop").await;}
                        else if !a.file&&a.health.primary_failed.load(Ordering::Acquire){self.stop("primary_consumer_failed").await;}
                        else if !a.started&&a.rf!=0&&a.stopping.is_none()&&a.preparing.elapsed()>Duration::from_secs(4){self.stop("start_response_timeout").await;}
                        else if !a.started&&a.rf==0&&a.stopping.is_none(){
                            if (!a.socket||a.health.primary_ready.load(Ordering::Acquire))&&a.received_hello{let mut nodes=[Identity::default();8];nodes[..self.selected.len()].copy_from_slice(&self.selected);let start=Start{attachment:self.token,run:0,schedule:0,rate:self.rate,duration_ms:self.duration,lease_ms:3000,count:self.selected.len() as u8,nodes};if let Some(a)=&mut self.active{a.rf=u64::MAX;}
                            if let Err(e)=self.send(Body::Start(start)).await{self.stop(&e.to_string()).await;}}
                            else if a.preparing.elapsed()>Duration::from_secs(10){self.finish(None,"primary_schema_timeout; RF never started").await;}
                        }
                    }
                }
            }
            if self.shutdown && self.active.is_none() && !self.starting && !self.finishing {
                break;
            }
        }
    }
}

fn prepare(
    options: StartOptions,
    mut metadata: Value,
    rx: NodeInfo,
    finished: mpsc::Sender<Result<Value, Error>>,
) -> Result<Prepared, Error> {
    let geometry = options
        .geometry
        .as_deref()
        .map(Geometry::load)
        .transpose()?
        .map(|(g, _)| g);
    let publisher = options.socket.as_deref().map(Publisher::bind).transpose()?;
    metadata["file_mode"] = options.out.is_some().into();
    metadata["stream_mode"] = options.socket.is_some().into();
    let sources = vec![SourceInfo {
        name: "star-rx".into(),
        kind: "serial".into(),
        details: json!({"receiver":node_json(rx),"star":metadata}),
    }];
    let mut bundle = None;
    let mut writer = if let Some(dir) = &options.out {
        if let Some(plan_path) = &options.plan {
            let plan = trial::Plan::load(plan_path)?;
            if plan.schema != "csi-trial-plan/2" {
                return Err(fail(
                    "star labels require csi-trial-plan/2 (one physical source)",
                ));
            }
            let declared: Vec<_> = plan.nodes.iter().map(|n| n.id.clone()).collect();
            let mut expected = vec![rx.identity.mac.to_string()];
            expected.extend(
                metadata["frozen_roster"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n["id"].as_str().unwrap().to_owned()),
            );
            if declared.len() != expected.len()
                || expected.iter().any(|id| !declared.contains(id))
                || plan.nodes.iter().any(|n| {
                    n.channel != rx.channel
                        || (n.id == rx.identity.mac.to_string() && n.mode != trial::Mode::Rx)
                        || (n.id != rx.identity.mac.to_string()
                            && (n.mode != trial::Mode::Tx
                                || n.requested_tx_hz != f64::from(options.rate)))
                })
            {
                return Err(fail(
                    "plan nodes/roles/channel/rates must match the selected live star kit",
                ));
            }
            let config = crate::live::config::HubConfig {
                sources: vec![crate::live::config::SourceConfig {
                    name: "star-rx".into(),
                    transport: crate::live::config::Transport::Serial {
                        path: PathBuf::from("star-controller-owned"),
                        baud: 921600,
                    },
                }],
                ..Default::default()
            };
            let bytes = toml::to_string(&config).map_err(io_error)?.into_bytes();
            bundle = Some(Bundle::prepare_config(
                plan_path,
                &config,
                bytes,
                options.geometry.as_deref().unwrap(),
                dir,
            )?);
        } else {
            if dir.exists() {
                return Err(Error::Exists(dir.clone()));
            }
            fs::create_dir_all(dir).map_err(io_error)?;
        }
        Some(SessionWriter::create(
            &dir.join("raw"),
            sources.clone(),
            WriterOptions {
                geometry: options.geometry.clone(),
                ..Default::default()
            },
        )?)
    } else {
        None
    };
    let manifest = writer
        .as_ref()
        .map(|w| w.manifest().clone())
        .unwrap_or_else(|| Manifest {
            format: manifest::FORMAT.into(),
            session_id: Uuid::now_v7(),
            created_wall_ns: wall_now_ns(),
            closed_wall_ns: None,
            complete: false,
            recorder: "csi-star".into(),
            protocol_version: csi_protocol::VERSION,
            derived_from: None,
            geometry: None,
            rotation: Default::default(),
            sources,
            segments: vec![],
        });
    metadata["session_id"] = json!(manifest.session_id);
    let labels = bundle.as_mut().map(|b| b.bind(&manifest)).transpose()?;
    let health = Arc::new(DeliveryHealth::default());
    let health_worker = health.clone();
    let session = manifest.session_id;
    let (publish_tx, publish_rx) = mpsc::channel(256);
    let publish_drops = Arc::new(AtomicU64::new(0));
    let publisher = publisher
        .map(|p| {
            p.with_flush_interval_ms(20)
                .map(|p| {
                    p.with_primary_handshake(HashMap::from([
                        ("csi.primary_handshake".into(), "CSI-READY/1".into()),
                        ("csi.star".into(), metadata.to_string()),
                    ]))
                })
                .and_then(|p| {
                    p.spawn_optional(
                        publish_rx,
                        publish_drops.clone(),
                        manifest.clone(),
                        geometry,
                        options.out.as_ref().map(|p| p.join("raw")),
                        health.clone(),
                    )
                    .map_err(io_error)
                })
        })
        .transpose()?;
    let (sink, input) = mpsc::channel(256);
    let initial = metadata.clone();
    if let Some(dir) = &options.out {
        trial::store_json(&dir.join("star-run.json"), &metadata)?;
        if bundle.is_none() {
            trial::store_json(
                &dir.join("trial.json"),
                &json!({"schema":"csi-trial/2","complete":false,"plan":null,"raw_session":"raw","export_dir":"export","labels_file":"labels.jsonl","session_id":session,"label_timebase":"host_mono_ns","artifacts":[]}),
            )?;
        }
    }
    thread::Builder::new()
        .name("csi-star-sinks".into())
        .spawn(move || {
            let result = sinks(
                input,
                writer.take(),
                bundle,
                labels,
                publisher,
                publish_tx,
                publish_drops,
                health_worker,
                options.out,
                initial,
            );
            let _ = finished.blocking_send(result);
        })
        .map_err(io_error)?;
    Ok(Prepared {
        sink,
        health,
        session,
        metadata,
    })
}
#[allow(clippy::too_many_arguments)]
fn sinks(
    mut input: mpsc::Receiver<SinkInput>,
    mut writer: Option<SessionWriter>,
    mut bundle: Option<Bundle>,
    mut labels: Option<Labels>,
    publisher: Option<thread::JoinHandle<Result<(), Error>>>,
    publish: mpsc::Sender<Arc<Record>>,
    publish_drops: Arc<AtomicU64>,
    health: Arc<DeliveryHealth>,
    out: Option<PathBuf>,
    mut metadata: Value,
) -> Result<Value, Error> {
    let has_publisher = publisher.is_some();
    let mut journal = out
        .as_ref()
        .map(|d| File::create(d.join("star-events.jsonl")).map_err(io_error))
        .transpose()?;
    let mut final_stamp = None;
    while let Some(item) = input.blocking_recv() {
        match item {
            SinkInput::Record(record) => {
                if let Some(w) = &mut writer {
                    w.write(&record)?;
                }
                if record.kind == RecordKind::StarControl
                    && let Some(j) = &mut journal
                {
                    let value = json!({"mono_ns":record.mono_ns,"wall_ns":record.wall_ns,"packet":Packet::decode(&record.payload).ok().map(|p|packet_json(&p))});
                    serde_json::to_writer(&mut *j, &value).map_err(io_error)?;
                    j.write_all(b"\n").map_err(io_error)?;
                }
                if has_publisher && publish.try_send(record).is_err() {
                    publish_drops.fetch_add(1, Ordering::Relaxed);
                    if out.is_none() {
                        health.primary_failed.store(true, Ordering::Release);
                    }
                }
            }
            SinkInput::Label(command, stamp, reply) => {
                let result = if let Some(l) = &mut labels {
                    match command {
                        Command::Begin { activity, position } => {
                            l.begin(&activity, position.as_deref(), stamp)
                        }
                        Command::End => l.end(stamp, "operator", false),
                        Command::Note { text } => l.note(&text, stamp),
                        _ => unreachable!(),
                    }
                } else {
                    Err(fail(
                        "labels need a declared training --plan and surveyed --geometry",
                    ))
                };
                let _ = reply.send(
                    result
                        .map(|_| json!({"label_recorded":true,"time_basis":"host_arrival_proxy"})),
                );
            }
            SinkInput::Finish(value, stamp) => {
                metadata = value;
                final_stamp = Some(stamp);
                break;
            }
        }
    }
    drop(publish);
    if let Some(p) = publisher {
        let result = p
            .join()
            .map_err(|_| fail("publisher panicked"))
            .and_then(|r| r);
        if let Err(e) = result {
            if out.is_none() {
                return Err(e);
            }
            metadata["publisher_error"] = e.to_string().into();
        }
    }
    metadata["delivery"] = health.status.lock().unwrap().clone();
    metadata["complete"] = final_stamp.is_some().into();
    let Some(stamp) = final_stamp else {
        return Err(fail(
            "sink input closed without a drain result; output incomplete",
        ));
    };
    if let Some(l) = &mut labels {
        l.finish(stamp, "run_stopped")?;
    }
    if let Some(j) = journal {
        j.sync_all().map_err(io_error)?;
    }
    if let Some(w) = writer {
        let manifest = w.finish()?;
        if let Some(dir) = &out {
            trial::store_json(&dir.join("star-run.json"), &metadata)?;
            trial::store_json(
                &dir.join("raw/hub-status.json"),
                &json!({"sources":[{"name":"star-rx","node":metadata["receiver"]["id"]}]}),
            )?;
            let summary = crate::export::session(
                &dir.join("raw"),
                &dir.join("export"),
                crate::export::Format::Arrow,
            )?;
            if let Some(b) = &mut bundle {
                b.finish(stamp.mono_ns, "star_stop")?;
            } else {
                let label_path = dir.join("labels.jsonl");
                fs::write(&label_path,format!("{}\n",json!({"schema":"csi-labels/1","type":"header","session_id":manifest.session_id,"timebase":"host_mono_ns","uncertainty_ns":null}))).map_err(io_error)?;
                trial::store_json(
                    &dir.join("quality.json"),
                    &json!({"schema":"csi-trial-quality/1","session_id":manifest.session_id,"complete":true,"summary":summary,"star":metadata}),
                )?;
                let star_bytes = fs::read(dir.join("star-run.json")).map_err(io_error)?;
                trial::store_json(
                    &dir.join("trial.json"),
                    &json!({"schema":"csi-trial/2","session_id":manifest.session_id,"complete":true,"plan":null,"raw_session":"raw","export_dir":"export","labels_file":"labels.jsonl","label_timebase":"host_mono_ns","geometry":manifest.geometry,"labels_sha256":crate::segment::sha256_hex(&fs::read(&label_path).map_err(io_error)?),"artifacts":[{"file":"star-run.json","sha256":crate::segment::sha256_hex(&star_bytes)}],"purpose":"transport; no activity/participant ground truth"}),
                )?;
            }
        }
    }
    Ok(metadata)
}
