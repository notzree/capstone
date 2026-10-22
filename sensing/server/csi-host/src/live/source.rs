use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use csi_protocol::{Header, Message, NodeId, TimeProbe, frame, stream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UdpSocket;
use tokio::sync::{mpsc, watch};
use tokio::time::{MissedTickBehavior, interval, sleep, timeout};
use tokio_serial::SerialPortBuilderExt;

use super::config::{HubConfig, SourceConfig, Transport};
use super::{ArrivalClock, Input};
use crate::{Record, RecordKind, StreamError};

pub(super) struct Source {
    pub config: SourceConfig,
    pub socket: Option<UdpSocket>,
}

pub(super) async fn prepare(config: SourceConfig) -> io::Result<Source> {
    let socket = match config.transport {
        Transport::Udp { bind, .. } => Some(UdpSocket::bind(bind).await?),
        Transport::Serial { .. } => None,
    };
    Ok(Source { config, socket })
}

struct Probes {
    seq: u32,
    pending: VecDeque<(u32, u64)>,
}

impl Probes {
    fn new() -> Self {
        Self {
            seq: 0,
            pending: VecDeque::new(),
        }
    }

    fn request(&mut self, clock: &ArrivalClock, serial: bool) -> (u32, u64, Vec<u8>) {
        let id = self.seq;
        self.seq = self.seq.wrapping_add(1);
        let sent = clock.mono_ns();
        let header = Header {
            node: NodeId([0; 6]),
            epoch: 0,
            seq: id,
        };
        let message = Message::TimeProbe(TimeProbe {
            probe_id: id,
            host_time: sent,
        });
        let bytes = if serial {
            stream::Encoder::new()
                .encode(&header, &message)
                .expect("fixed-size time probe")
                .to_vec()
        } else {
            let mut buf = vec![0; csi_protocol::MAX_FRAME_LEN];
            let n = frame::encode(&header, &message, &mut buf).expect("fixed-size time probe");
            buf.truncate(n);
            buf
        };
        (id, sent, bytes)
    }

    fn sent(&mut self, id: u32, stamp: u64) {
        self.pending.push_back((id, stamp));
        while self.pending.len() > 128 {
            self.pending.pop_front();
        }
    }

    fn correlate(&mut self, payload: &[u8], now: u64) -> bool {
        let Ok((_, Message::TimeEcho(echo))) = frame::decode(payload) else {
            return false;
        };
        self.pending
            .retain(|&(_, sent)| now.saturating_sub(sent) <= 2_000_000_000);
        let Some(i) = self
            .pending
            .iter()
            .position(|&(id, sent)| id == echo.probe_id && sent == echo.host_time)
        else {
            return false;
        };
        self.pending.remove(i);
        true
    }
}

async fn emit(
    tx: &mpsc::Sender<Input>,
    index: u16,
    clock: &ArrivalClock,
    payload: Vec<u8>,
    kind: RecordKind,
    probes: &mut Probes,
) -> io::Result<()> {
    let mono_ns = clock.mono_ns();
    let record = Record {
        mono_ns,
        wall_ns: crate::session::wall_now_ns(),
        source: index,
        kind,
        payload,
    };
    let correlated_echo = probes.correlate(&record.payload, mono_ns);
    tx.send(Input::Record {
        record,
        correlated_echo,
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "recorder stopped"))
}

async fn state(
    tx: &mpsc::Sender<Input>,
    source: u16,
    clock: &ArrivalClock,
    connected: bool,
    error: Option<String>,
) -> io::Result<()> {
    tx.send(Input::Connection {
        source,
        mono_ns: clock.mono_ns(),
        connected,
        error,
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "recorder stopped"))
}

pub(super) async fn run(
    mut source: Source,
    index: u16,
    config: Arc<HubConfig>,
    clock: ArrivalClock,
    tx: mpsc::Sender<Input>,
    mut stop: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut probes = Probes::new();
    let mut ticker = interval(Duration::from_millis(config.echo_interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    if let Some(socket) = source.socket.take() {
        let Transport::Udp { peer, .. } = source.config.transport else {
            unreachable!()
        };
        state(&tx, index, &clock, true, None).await?;
        // An extra byte ensures an oversized datagram cannot be truncated to a valid frame.
        let mut buf = vec![0; csi_protocol::MAX_FRAME_LEN + 1];
        loop {
            tokio::select! {
                _ = stop.changed() => return Ok(()),
                result = socket.recv_from(&mut buf) => {
                    let (n, from) = result?;
                    if from != peer {
                        tx.send(Input::Foreign(index)).await.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?;
                    } else if n > csi_protocol::MAX_FRAME_LEN {
                        emit(&tx, index, &clock, vec![StreamError::Oversized as u8], RecordKind::StreamError, &mut probes).await?;
                    } else {
                        emit(&tx, index, &clock, buf[..n].to_vec(), RecordKind::Frame, &mut probes).await?;
                    }
                }
                _ = ticker.tick() => {
                    let (id, sent, bytes) = probes.request(&clock, false);
                    match timeout(Duration::from_millis(500), socket.send_to(&bytes, peer)).await {
                        Ok(Ok(_)) => probes.sent(id, sent),
                        _ => { tx.send(Input::ProbeWriteError(index)).await.map_err(|_| io::Error::from(io::ErrorKind::BrokenPipe))?; }
                    }
                }
            }
        }
    }
    let Transport::Serial { path, baud } = source.config.transport else {
        unreachable!()
    };
    loop {
        if *stop.borrow() {
            return Ok(());
        }
        let opened = tokio_serial::new(path.to_string_lossy(), baud)
            .preserve_dtr_on_open()
            .data_bits(tokio_serial::DataBits::Eight)
            .parity(tokio_serial::Parity::None)
            .stop_bits(tokio_serial::StopBits::One)
            .flow_control(tokio_serial::FlowControl::None)
            .open_native_async();
        let failure = match opened {
            Err(error) => error.to_string(),
            Ok(mut serial) => {
                state(&tx, index, &clock, true, None).await?;
                probes.pending.clear();
                let mut deframer = stream::Deframer::new();
                let mut bytes = [0; 8192];
                loop {
                    tokio::select! {
                                _ = stop.changed() => return Ok(()),
                        result = serial.read(&mut bytes) => {
                            match result {
                                Ok(0) => break "serial EOF".to_string(),
                                Err(error) => break error.to_string(),
                                Ok(n) => {
                                    let mut input = &bytes[..n];
                                    while let Some(result) = deframer.push(&mut input) {
                                        let (payload, kind) = match result {
                                            Ok(frame) => (frame.to_vec(), RecordKind::Frame),
                                            Err(error) => (vec![StreamError::from_decode_error(error).unwrap_or(StreamError::BadCobs) as u8], RecordKind::StreamError),
                                        };
                                        emit(&tx, index, &clock, payload, kind, &mut probes).await?;
                                    }
                                }
                            }
                        }
                        _ = ticker.tick() => {
                            let (id, sent, bytes) = probes.request(&clock, true);
                            match timeout(Duration::from_millis(500), serial.write_all(&bytes)).await {
                                Ok(Ok(())) => probes.sent(id, sent),
                                Ok(Err(error)) => break error.to_string(),
                                Err(_) => break "serial time-probe write timed out".to_string(),
                            }
                        }
                    }
                }
            }
        };
        tracing::warn!(source = %source.config.name, error = %failure, "serial disconnected; retrying");
        state(&tx, index, &clock, false, Some(failure)).await?;
        tokio::select! {
            _ = stop.changed() => return Ok(()),
            _ = sleep(Duration::from_millis(config.reconnect_ms)) => {}
        }
    }
}
