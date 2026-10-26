//! ESP-WROOM-32 bench adapter: independently enabled probe TX and queued CSI RX.
//! Native CSI is copied unchanged; optional known-pair turn-taking, no DSP.
#![no_std]
#![no_main]

#[cfg(not(any(feature = "tx", feature = "rx")))]
compile_error!("enable at least one capability: tx, rx, or pair");

extern crate alloc;
#[cfg(any(feature = "star-rx", feature = "star-tx"))]
mod star;
include!(concat!(env!("OUT_DIR"), "/build_id.rs"));
#[cfg(feature = "wireless")]
mod wireless;
use csi_protocol::NodeId;
#[cfg(not(feature = "star-tx"))]
use csi_protocol::{
    Chip, Counters, Csi, CsiConfig, Header, Hello, Message, Probe, Roles, RxMeta, TimeEcho, frame,
    stream,
};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, channel::Channel};
#[cfg(any(all(feature = "tx", not(feature = "star-tx")), feature = "wireless"))]
use embassy_time::Duration;
use embassy_time::Timer;
#[cfg(not(any(feature = "wireless", feature = "star-tx")))]
use esp_hal::uart::Uart;
#[cfg(not(feature = "star-tx"))]
use esp_hal::{
    Async,
    uart::{UartRx, UartTx},
};
use esp_hal::{clock::CpuClock, time::Instant, timer::timg::TimerGroup};
use esp_radio::wifi::{
    Config, PowerSaveMode, SecondaryChannel, WifiController, sta::StationConfig,
};
#[cfg(not(feature = "star-tx"))]
use portable_atomic::{AtomicU8, AtomicU32};
use portable_atomic::{AtomicU64, Ordering};
use {esp_backtrace as _, esp_println as _};

esp_bootloader_esp_idf::esp_app_desc!();
#[cfg(not(any(feature = "star-rx", feature = "star-tx")))]
const CHANNEL: u8 = 6;
#[cfg(any(feature = "star-rx", feature = "star-tx"))]
const CHANNEL: u8 = star::channel();
#[cfg(not(feature = "star-tx"))]
static RADIO_CHANNEL: AtomicU8 = AtomicU8::new(CHANNEL);
#[cfg(not(any(feature = "wireless", feature = "star-tx")))]
const BAUD: u32 = 921600;
#[cfg(not(feature = "star-tx"))]
const POOL: usize = 32;
#[cfg(not(feature = "star-tx"))]
const TX_ROLE: bool = cfg!(feature = "tx");
#[cfg(not(feature = "star-tx"))]
const RX_ROLE: bool = cfg!(feature = "rx");
#[cfg(feature = "pair")]
const SCHEDULE: u32 = 1;
#[cfg(feature = "pair")]
const GUARD_US: u64 = 0;
#[cfg(feature = "pair")]
const MAX_RESPONSE_AGE_US: u64 = 30_000;
#[cfg(feature = "pair")]
static PEER_PROBES: embassy_sync::signal::Signal<
    CriticalSectionRawMutex,
    (Probe, csi_node::TurnTag, u64),
> = embassy_sync::signal::Signal::new();
#[cfg(feature = "pair")]
static PEER_MAC: AtomicU64 = AtomicU64::new(0);
static OWN_MAC: AtomicU64 = AtomicU64::new(0);
fn mac_key(mac: [u8; 6]) -> u64 {
    u64::from_le_bytes([mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], 0, 0])
}
#[cfg(not(feature = "star-tx"))]
const CSI_CONFIG: CsiConfig = CsiConfig {
    lltf: true,
    htltf: true,
    stbc_htltf2: false,
    ltf_merge: false,
    channel_filter: false,
    manu_scale: false,
    shift: 0,
};

#[cfg(not(feature = "star-tx"))]
struct Capture {
    seq: u32,
    node_us: u64,
    radio_us: u32,
    mac: [u8; 6],
    probe: Probe,
    first_invalid: bool,
    rx: RxMeta,
    len: usize,
    data: [u8; csi_protocol::MAX_CSI_LEN],
    #[cfg(feature = "star-rx")]
    tag: csi_protocol::star::ProbeTag,
}
#[cfg(not(feature = "star-tx"))]
static CAPTURES: Channel<CriticalSectionRawMutex, Capture, POOL> = Channel::new();
#[cfg(not(feature = "star-tx"))]
enum Control {
    Hello,
    Counters,
    Echo(TimeEcho),
    #[cfg(feature = "star-rx")]
    Star(csi_protocol::star::Packet),
    #[cfg(feature = "star-rx")]
    Drain(csi_protocol::star::Packet),
}
#[cfg(not(feature = "star-tx"))]
static CONTROL: Channel<CriticalSectionRawMutex, Control, 32> = Channel::new();
#[cfg(not(feature = "star-tx"))]
static CALLBACKS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(feature = "star-tx"))]
static FILTERED: AtomicU64 = AtomicU64::new(0);
#[cfg(not(feature = "star-tx"))]
static CAPTURED: AtomicU64 = AtomicU64::new(0);
#[cfg(not(feature = "star-tx"))]
static POOL_DROPS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(feature = "star-tx"))]
static EXPORT_DROPS: AtomicU64 = AtomicU64::new(0);
#[cfg(not(feature = "star-tx"))]
static EXPORT_BYTES: AtomicU64 = AtomicU64::new(0);
#[cfg(not(feature = "star-tx"))]
static HIGH_WATER: AtomicU32 = AtomicU32::new(0);
static TX_REQUESTED: AtomicU64 = AtomicU64::new(0);
static TX_ACCEPTED: AtomicU64 = AtomicU64::new(0);
fn now_us() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}

#[cfg(not(feature = "star-tx"))]
fn firmware_version() -> [u8; 3] {
    if cfg!(any(feature = "star-rx", feature = "star-tx")) {
        [0, 4, 0]
    } else if cfg!(feature = "wireless") {
        [0, 3, 1]
    } else {
        [0, 2, 2]
    }
}

#[cfg(feature = "rx")]
fn capture(info: esp_radio::wifi::csi::WifiCsiInfo<'_>) {
    CALLBACKS.fetch_add(1, Ordering::Relaxed);
    let node_us = now_us();
    // Direct ESP-NOW action frames only; exclude AP-forwarded IP traffic.
    if info.header().first().map(|b| b & 0xfc) != Some(0xd0)
        || info.header().get(1).map(|b| b & 3) != Some(0)
    {
        FILTERED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    #[cfg(feature = "star-rx")]
    let Some((probe, tag)) = csi_protocol::star::ProbeTag::sensing(info.payload()) else {
        FILTERED.fetch_add(1, Ordering::Relaxed);
        return;
    };
    #[cfg(not(feature = "star-rx"))]
    let Some(probe) = csi_protocol::wireless::sensing_probe(info.payload()) else {
        FILTERED.fetch_add(1, Ordering::Relaxed);
        return;
    };
    if mac_key(probe.tx.0) == OWN_MAC.load(Ordering::Relaxed)
        || probe.tx.0 != *info.mac()
        || info.channel() != RADIO_CHANNEL.load(Ordering::Relaxed)
        || info.buf().len() > csi_protocol::MAX_CSI_LEN
    {
        FILTERED.fetch_add(1, Ordering::Relaxed);
        return;
    }
    #[cfg(feature = "pair")]
    if mac_key(probe.tx.0) == PEER_MAC.load(Ordering::Relaxed)
        && probe.schedule == SCHEDULE
        && let Some(turn) = csi_node::TurnTag::find(info.payload())
    {
        PEER_PROBES.signal((probe, turn, node_us));
    }
    #[cfg(not(feature = "star-rx"))]
    let seq = CAPTURED.fetch_add(1, Ordering::Relaxed) as u32;
    #[cfg(feature = "star-rx")]
    let seq = 0;
    let mut record = Capture {
        seq,
        #[cfg(feature = "star-rx")]
        tag,
        node_us,
        radio_us: info.timestamp().duration_since_epoch().as_micros() as u32,
        mac: *info.mac(),
        probe,
        first_invalid: info.first_word_invalid(),
        len: info.buf().len(),
        rx: RxMeta {
            rssi: info.rssi(),
            noise_floor: info.noise_floor(),
            rate: info.rate(),
            sig_mode: info.packet_mode(),
            mcs: info.modulation_coding_scheme(),
            cwb: u8::from(info.cwb()),
            stbc: info.space_time_block_code(),
            channel: info.channel(),
            secondary_channel: match info.secondary_channel() {
                SecondaryChannel::None => 0,
                SecondaryChannel::Above => 1,
                SecondaryChannel::Below => 2,
            },
            smoothing: info.smoothing(),
            not_sounding: info.not_sounding(),
            aggregation: info.aggregation(),
            fec_coding: info.forward_error_correction_coding(),
            sgi: info.short_guide_interval(),
            ampdu_cnt: info.ampdu_count(),
            ant: info.antenna(),
            rx_state: info.rx_state(),
            sig_len: info.signal_length(),
            gain: None,
        },
        data: [0; csi_protocol::MAX_CSI_LEN],
    };
    for (to, &from) in record.data.iter_mut().zip(info.buf()) {
        *to = from as u8;
    }
    if CAPTURES.try_send(record).is_err() {
        POOL_DROPS.fetch_add(1, Ordering::Relaxed);
    } else {
        HIGH_WATER.fetch_max(CAPTURES.len() as u32, Ordering::Relaxed);
    }
}

#[cfg(not(feature = "star-tx"))]
fn counters() -> Counters {
    Counters {
        node_time_us: now_us(),
        tx_requested: TX_REQUESTED.load(Ordering::Relaxed),
        tx_accepted: TX_ACCEPTED.load(Ordering::Relaxed),
        csi_callbacks: CALLBACKS.load(Ordering::Relaxed),
        csi_filtered: FILTERED.load(Ordering::Relaxed),
        csi_captured: CAPTURED.load(Ordering::Relaxed),
        pool_drops: POOL_DROPS.load(Ordering::Relaxed),
        export_drops: EXPORT_DROPS.load(Ordering::Relaxed),
        export_bytes: EXPORT_BYTES.load(Ordering::Relaxed),
        pool_high_water: HIGH_WATER.load(Ordering::Relaxed) as u16,
    }
}

#[cfg(not(feature = "star-tx"))]
#[embassy_executor::task]
async fn export(mut uart: UartTx<'static, Async>, node: NodeId, epoch: u32) {
    let mut encoder = stream::Encoder::new();
    let mut seq = 0u32;
    #[cfg(feature = "star-rx")]
    let mut drain: Option<csi_protocol::star::Packet> = None;
    #[cfg(feature = "star-rx")]
    let mut exported = 0u64;
    // Terminate ROM/bootloader console output so the host can resynchronize.
    uart.write_async(&[0]).await.ok();
    loop {
        #[cfg(not(feature = "star-rx"))]
        let item = select(CONTROL.receive(), CAPTURES.receive()).await;
        #[cfg(feature = "star-rx")]
        let item = if let Some(mut p) = drain.take() {
            match star::OUTPUT.try_receive() {
                Ok(c) => {
                    drain = Some(p);
                    Either::Second(c)
                }
                Err(_) => {
                    if let csi_protocol::star::Body::DrainComplete {
                        export,
                        exported: total,
                        ..
                    } = &mut p.body
                    {
                        *export = seq.wrapping_sub(1);
                        *total = exported;
                    }
                    let mut bytes = [0; 254];
                    let n = p.encode_stream(&mut bytes).unwrap();
                    uart_write(&mut uart, &bytes[..n]).await;
                    star::DRAINED.signal(());
                    continue;
                }
            }
        } else {
            select(CONTROL.receive(), star::OUTPUT.receive()).await
        };
        #[cfg(feature = "star-rx")]
        match &item {
            Either::First(Control::Star(p)) => {
                if matches!(
                    p.body,
                    csi_protocol::star::Body::RunState {
                        state: csi_protocol::star::State::Starting,
                        ..
                    }
                ) {
                    exported = 0;
                }
                let mut bytes = [0; 254];
                let n = p.encode_stream(&mut bytes).unwrap();
                uart_write(&mut uart, &bytes[..n]).await;
                continue;
            }
            Either::First(Control::Drain(p)) => {
                drain = Some(*p);
                continue;
            }
            _ => {}
        }
        let message = match &item {
            Either::First(Control::Hello) => Message::Hello(Hello {
                chip: Chip::Esp32,
                roles: Roles {
                    tx: TX_ROLE,
                    rx: RX_ROLE,
                },
                firmware_version: firmware_version(),
                firmware_commit: BUILD_ID,
                channel: RADIO_CHANNEL.load(Ordering::Relaxed),
                secondary_channel: 0,
                csi_config: CSI_CONFIG,
                pool_capacity: POOL as u16,
            }),
            Either::First(Control::Counters) => Message::Counters(counters()),
            Either::First(Control::Echo(echo)) => Message::TimeEcho(TimeEcho {
                node_tx_us: now_us(),
                ..*echo
            }),
            #[cfg(feature = "star-rx")]
            Either::First(Control::Star(_) | Control::Drain(_)) => unreachable!(),
            Either::Second(c) => Message::Csi(Csi {
                capture_seq: c.seq,
                node_time_us: c.node_us,
                rx_timestamp_us: c.radio_us,
                source_mac: c.mac,
                probe: Some(c.probe),
                first_word_invalid: c.first_invalid,
                rx: c.rx,
                data: &c.data[..c.len],
            }),
        };
        let bytes = encoder
            .encode(&Header { node, epoch, seq }, &message)
            .expect("bounded protocol frame");
        seq = seq.wrapping_add(1);
        let success = uart_write(&mut uart, bytes).await;
        #[cfg(feature = "star-rx")]
        if success && matches!(item, Either::Second(_)) {
            exported += 1;
        }
        #[cfg(not(feature = "star-rx"))]
        let _ = success;
    }
}

#[cfg(not(feature = "star-tx"))]
async fn uart_write(uart: &mut UartTx<'static, Async>, bytes: &[u8]) -> bool {
    let mut sent = 0;
    while sent < bytes.len() {
        match embassy_time::with_timeout(
            embassy_time::Duration::from_millis(250),
            uart.write_async(&bytes[sent..]),
        )
        .await
        {
            Ok(Ok(n)) if n > 0 => {
                sent += n;
                EXPORT_BYTES.fetch_add(n as u64, Ordering::Relaxed);
            }
            _ => {
                EXPORT_DROPS.fetch_add(1, Ordering::Relaxed);
                return false;
            }
        }
    }
    true
}

#[cfg(not(feature = "star-tx"))]
#[embassy_executor::task]
async fn requests(mut uart: UartRx<'static, Async>) {
    let mut deframer = stream::Deframer::new();
    let mut bytes = [0; 256];
    loop {
        match uart.read_async(&mut bytes).await {
            Ok(n) => {
                let mut input = &bytes[..n];
                while let Some(result) = deframer.push(&mut input) {
                    #[cfg(feature = "star-rx")]
                    if let Ok(raw) = result
                        && raw.starts_with(csi_protocol::star::MAGIC)
                    {
                        if let Ok(p) = csi_protocol::star::Packet::decode(raw) {
                            star::HOST.try_send(p).ok();
                        }
                        continue;
                    }
                    if let Ok(raw) = result
                        && let Ok((_, Message::TimeProbe(probe))) = frame::decode(raw)
                    {
                        let echo = TimeEcho {
                            probe_id: probe.probe_id,
                            host_time: probe.host_time,
                            node_rx_us: now_us(),
                            node_tx_us: 0,
                        };
                        // Bounded control queue: a host cannot allocate unbounded node memory.
                        CONTROL.try_send(Control::Echo(echo)).ok();
                    }
                }
            }
            Err(_) => {
                deframer.reset();
                Timer::after_millis(10).await;
            }
        }
    }
}

#[cfg(not(feature = "star-tx"))]
#[embassy_executor::task]
async fn announcements() {
    loop {
        CONTROL.send(Control::Hello).await;
        CONTROL.send(Control::Counters).await;
        Timer::after_secs(1).await;
    }
}

#[cfg(any(feature = "rx", feature = "star-tx"))]
#[embassy_executor::task]
async fn keep_sniffer(sniffer: esp_radio::wifi::sniffer::Sniffer) {
    loop {
        Timer::after_secs(60).await;
        core::hint::black_box(&sniffer);
    }
}

#[cfg(any(all(feature = "tx", not(feature = "star-tx")), feature = "wireless"))]
async fn wait_periodic(next_us: &mut u64, period_us: u64) {
    let now = now_us();
    if *next_us > now {
        Timer::after(Duration::from_micros(*next_us - now)).await;
    }
    // Rebase after a delay, rather than replaying a backlog of missed turns.
    *next_us = now_us().saturating_add(period_us);
}

#[cfg(all(feature = "tx", not(any(feature = "wireless", feature = "star-tx"))))]
#[embassy_executor::task]
async fn drain_esp_now(mut receiver: esp_radio::esp_now::EspNowReceiver) {
    // CSI is handled independently in capture(). Drain the driver's additional
    // ESP-NOW body queue so it does not retain discarded application packets.
    loop {
        let _ = receiver.receive_async().await;
    }
}

#[cfg(any(feature = "tx", feature = "wireless", feature = "star-rx"))]
async fn send_probe(sender: &mut esp_radio::esp_now::EspNowSender, body: &[u8]) -> bool {
    TX_REQUESTED.fetch_add(1, Ordering::Relaxed);
    // Keep the pending future alive to completion, as required by this driver.
    let mut sending = core::pin::pin!(sender.send_async(&[0xff; 6], body));
    let first =
        core::future::poll_fn(|cx| core::task::Poll::Ready(sending.as_mut().poll(cx))).await;
    match first {
        core::task::Poll::Ready(Err(_)) => false,
        core::task::Poll::Ready(Ok(())) => {
            TX_ACCEPTED.fetch_add(1, Ordering::Relaxed);
            true
        }
        core::task::Poll::Pending => {
            TX_ACCEPTED.fetch_add(1, Ordering::Relaxed);
            // Broadcast completion does not establish peer delivery.
            match select(
                sending.as_mut(),
                Timer::after_millis(if cfg!(any(feature = "star-rx", feature = "star-tx")) {
                    250
                } else {
                    2000
                }),
            )
            .await
            {
                Either::First(result) => result.is_ok(),
                // Keep the pending future alive until the whole radio is reset;
                // a missing completion callback must not freeze this node forever.
                Either::Second(_) => esp_hal::system::software_reset(),
            }
        }
    }
}

#[cfg(feature = "pair")]
async fn wait_response(
    peer: NodeId,
    epoch: u32,
    round: u32,
    sent_us: u64,
    deadline_us: u64,
) -> u64 {
    loop {
        let (probe, tag, received_us) = PEER_PROBES.wait().await;
        if probe.tx == peer
            && probe.schedule == SCHEDULE
            && tag.is_response_to(epoch, round)
            && received_us >= sent_us
            && received_us <= deadline_us
        {
            return received_us - sent_us;
        }
    }
}

#[cfg(all(feature = "tx", not(any(feature = "wireless", feature = "star-tx"))))]
#[embassy_executor::task]
async fn transmit(mut sender: esp_radio::esp_now::EspNowSender, node: NodeId, epoch: u32) {
    #[cfg(feature = "pair")]
    let peer = csi_node::parse_mac(env!("CSI_PEER_MAC")).expect("build-validated peer MAC");
    #[cfg(feature = "pair")]
    let mut responder = csi_node::PairResponder::new(peer, SCHEDULE, GUARD_US, MAX_RESPONSE_AGE_US);
    #[cfg(feature = "pair")]
    let mut timeout = csi_node::AdaptiveTimeout::new();
    #[cfg(feature = "pair")]
    let leader = node.0 < peer.0;
    let period_us = option_env!("CSI_PROBE_INTERVAL_US")
        .map(|value| value.parse::<u64>().expect("build-validated interval"))
        .unwrap_or(if cfg!(feature = "pair") {
            100_000
        } else {
            10_000
        });
    let mut seq = 0u32;
    let mut next_periodic_us = now_us().saturating_add(period_us);
    let mut body = [0u8; 64];
    loop {
        #[cfg(feature = "pair")]
        let tag = if !leader {
            let (probe, tag, received_us) = PEER_PROBES.wait().await;
            if !tag.is_request_for(&probe)
                || responder.observe(probe, received_us, now_us()).is_none()
            {
                continue;
            }
            // Packet receipt grants the turn; there is no fixed guard delay.
            csi_node::TurnTag::response(tag.leader_epoch, tag.round)
        } else {
            wait_periodic(&mut next_periodic_us, period_us).await;
            csi_node::TurnTag::request(epoch, seq)
        };
        #[cfg(not(feature = "pair"))]
        wait_periodic(&mut next_periodic_us, period_us).await;
        let probe = Probe {
            tx: node,
            epoch,
            #[cfg(feature = "pair")]
            schedule: SCHEDULE,
            #[cfg(not(feature = "pair"))]
            schedule: 0,
            seq,
        };
        seq = seq.wrapping_add(1);
        probe.encode(&mut body).expect("probe buffer");
        #[cfg(feature = "pair")]
        tag.encode(&mut body[csi_protocol::probe::PROBE_LEN..])
            .expect("turn buffer");
        #[cfg(feature = "pair")]
        let sent_us = now_us();
        send_probe(&mut sender, &body).await;
        #[cfg(feature = "pair")]
        if leader {
            // Only the response to this leader boot/round releases the next turn.
            // The timer covers the whole round, including our driver completion.
            let elapsed_us = now_us().saturating_sub(sent_us);
            let budget_us = timeout.timeout_us().saturating_sub(elapsed_us);
            match embassy_time::with_timeout(
                Duration::from_micros(budget_us),
                wait_response(
                    peer,
                    epoch,
                    probe.seq,
                    sent_us,
                    sent_us.saturating_add(timeout.timeout_us()),
                ),
            )
            .await
            {
                Ok(rtt_us) => timeout.observe_rtt(rtt_us),
                Err(_) => timeout.on_timeout(),
            }
        }
    }
}

#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);
    #[cfg(feature = "gateway")]
    esp_alloc::heap_allocator!(size: 32768);
    let timg = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timg.timer0, p.FROM_CPU_INTR0);
    #[cfg(not(any(feature = "wireless", feature = "star-tx")))]
    let uart = Uart::new(
        p.UART0,
        esp_hal::uart::Config::default()
            .with_baudrate(BAUD)
            .with_rx(esp_hal::uart::RxConfig::default().with_fifo_full_threshold(32)),
    )
    .expect("UART0")
    .with_tx(p.GPIO1)
    .with_rx(p.GPIO3)
    .into_async();
    #[cfg(not(any(feature = "wireless", feature = "star-tx")))]
    let (uart_rx, uart_tx) = uart.split();
    let mut controller = WifiController::new(p.WIFI, Default::default()).expect("Wi-Fi init");
    controller
        .set_config(&Config::Station(StationConfig::default()))
        .expect("STA start");
    controller
        .set_power_saving(PowerSaveMode::None)
        .expect("disable modem sleep");
    #[cfg(not(feature = "wireless"))]
    controller
        .set_channel(CHANNEL, SecondaryChannel::None)
        .expect("fixed channel");
    let node = NodeId(
        esp_hal::efuse::base_mac_address()
            .as_bytes()
            .try_into()
            .unwrap(),
    );
    OWN_MAC.store(mac_key(node.0), Ordering::Relaxed);
    #[cfg(feature = "pair")]
    {
        let peer = csi_node::parse_mac(env!("CSI_PEER_MAC")).unwrap();
        assert_ne!(node, peer, "peer must be another node");
        PEER_MAC.store(mac_key(peer.0), Ordering::Relaxed);
    }
    let epoch = esp_hal::rng::Rng::new().random().max(1);
    #[cfg(not(any(feature = "wireless", feature = "star-tx")))]
    {
        spawner.spawn(export(uart_tx, node, epoch).unwrap());
        spawner.spawn(requests(uart_rx).unwrap());
    }
    #[cfg(feature = "wireless")]
    {
        wireless::configure(&mut controller);
        spawner.spawn(wireless::batch(node, epoch).unwrap());
    }
    #[cfg(not(feature = "star-tx"))]
    spawner.spawn(announcements().unwrap());
    #[cfg(any(feature = "rx", feature = "star-tx"))]
    {
        #[allow(unused_mut)]
        let mut sniffer = controller.sniffer();
        #[cfg(any(feature = "star-rx", feature = "star-tx"))]
        sniffer.set_receive_cb(star::control_callback);
        sniffer.set_promiscuous_mode(true).expect("promiscuous RX");
        #[cfg(feature = "rx")]
        controller
            .set_csi(
                esp_radio::wifi::csi::CsiConfig {
                    lltf_en: true,
                    htltf_en: true,
                    stbc_htltf2_en: false,
                    ltf_merge_en: false,
                    channel_filter_en: false,
                    manu_scale: false,
                    shift: 0,
                    dump_ack_en: false,
                },
                capture,
            )
            .expect("CSI callback");
        // Move the guard into a keeper task so TX can run with CSI still enabled.
        spawner.spawn(keep_sniffer(sniffer).unwrap());
    }
    #[cfg(any(feature = "tx", feature = "wireless", feature = "star-rx"))]
    {
        use esp_radio::esp_now::{EspNowWifiInterface, PeerInfo, PhyMode, RateConfig, WifiPhyRate};
        let radio = controller.esp_now();
        if !radio.peer_exists(&[0xff; 6]) {
            radio
                .add_peer(PeerInfo {
                    peer_address: [0xff; 6],
                    channel: if cfg!(feature = "wireless") {
                        None
                    } else {
                        Some(CHANNEL)
                    },
                    lmk: None,
                    encrypt: false,
                    interface: EspNowWifiInterface::Station,
                })
                .expect("broadcast peer");
        }
        radio
            .set_peer_rate(
                &[0xff; 6],
                RateConfig {
                    phy_mode: PhyMode::Ht20,
                    rate: WifiPhyRate::RateMcs0Lgi,
                    ersu: false,
                    dcm: false,
                },
            )
            .expect("force HT20 MCS0");
        let (manager, sender, receiver) = radio.split();
        #[cfg(not(any(feature = "wireless", feature = "star-rx", feature = "star-tx")))]
        {
            spawner.spawn(drain_esp_now(receiver).unwrap());
            spawner.spawn(transmit(sender, node, epoch).unwrap());
        }
        #[cfg(any(feature = "star-rx", feature = "star-tx"))]
        {
            spawner.spawn(star::receive(receiver).unwrap());
            spawner.spawn(star::radio(sender, node, epoch).unwrap());
        }
        #[cfg(feature = "wireless")]
        {
            spawner.spawn(wireless::receive(receiver, node).unwrap());
            spawner.spawn(wireless::radio(sender, node, epoch).unwrap());
            #[cfg(feature = "gateway")]
            wireless::start_network(spawner, node);
            #[cfg(feature = "gateway")]
            spawner.spawn(wireless::connection(controller).unwrap());
        }
        // Retain the manager guard while the sender/receiver tasks run.
        loop {
            Timer::after_secs(60).await;
            core::hint::black_box(&manager);
            #[cfg(all(feature = "wireless", not(feature = "gateway")))]
            core::hint::black_box(&controller);
        }
    }
    #[cfg(not(any(feature = "tx", feature = "wireless", feature = "star-rx")))]
    loop {
        Timer::after_secs(60).await;
        core::hint::black_box(&controller);
    }
}
