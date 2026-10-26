//! Direct ESP-NOW sensing/piggybacking plus a Wi-Fi/UDP relay. USB carries no CSI.
use super::*;
use csi_protocol::wireless::{self as wire, Batch, Builder, Carrier};
#[cfg(feature = "gateway")]
use portable_atomic::AtomicBool;

include!(concat!(env!("OUT_DIR"), "/wifi_config.rs"));
#[cfg(feature = "gateway")]
static CONNECTED: AtomicBool = AtomicBool::new(false);
#[cfg(not(feature = "gateway"))]
static BATCHES: Channel<CriticalSectionRawMutex, Packet, 4> = Channel::new();
#[cfg(feature = "gateway")]
static UPLINK: Channel<CriticalSectionRawMutex, Packet, 8> = Channel::new();
#[cfg(feature = "gateway")]
static FORWARD_CONTROL: Channel<CriticalSectionRawMutex, Request, 4> = Channel::new();
static RELAY_DROPS: AtomicU64 = AtomicU64::new(0);

struct Packet {
    bytes: [u8; wire::MAX_LEN],
    len: usize,
    count: usize,
}
#[cfg(feature = "gateway")]
struct Request {
    bytes: [u8; csi_protocol::MAX_FRAME_LEN],
    len: usize,
}

#[cfg(feature = "gateway")]
pub fn configure(controller: &mut WifiController<'static>) {
    use esp_radio::wifi::AuthenticationMethodConfig;
    controller
        .set_config(&Config::Station(
            StationConfig::default()
                .with_ssid(SSID.try_into().expect("validated SSID"))
                .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
                    PASSWORD.try_into().expect("validated password"),
                )),
        ))
        .expect("station configuration");
}

#[cfg(not(feature = "gateway"))]
pub fn configure(controller: &mut WifiController<'static>) {
    controller
        .set_channel(REMOTE_CHANNEL, SecondaryChannel::None)
        .expect("remote ESP-NOW channel");
    RADIO_CHANNEL.store(REMOTE_CHANNEL, Ordering::Relaxed);
    esp_println::println!(
        "ESP-NOW remote ready on channel {}; no AP association",
        REMOTE_CHANNEL
    );
}

#[cfg(feature = "gateway")]
#[embassy_executor::task]
pub async fn connection(mut controller: WifiController<'static>) {
    #[cfg(feature = "gateway")]
    {
        use esp_radio::wifi::scan::{ScanConfig, ScanTypeConfig};
        let config = ScanConfig::default()
            .with_max(32)
            .with_show_hidden(true)
            .with_scan_type(ScanTypeConfig::Active {
                min: esp_hal::time::Duration::from_millis(100),
                max: esp_hal::time::Duration::from_millis(300),
            });
        if let Ok(aps) = controller.scan_async(&config).await {
            let mut matched = 0;
            for ap in &aps {
                if ap.ssid.as_str() == SSID {
                    matched += 1;
                    esp_println::println!(
                        "wireless target AP channel {} RSSI {} auth {:?}",
                        ap.channel,
                        ap.signal_strength,
                        ap.auth_method
                    );
                }
            }
            esp_println::println!(
                "wireless scan visible {} target matches {}",
                aps.len(),
                matched
            );
        }
    }
    loop {
        match controller.connect_async().await {
            Ok(info) => {
                RADIO_CHANNEL.store(info.channel, Ordering::Relaxed);
                CONNECTED.store(true, Ordering::Relaxed);
                esp_println::println!(
                    "wireless connected channel {} bssid {:02x?}",
                    info.channel,
                    info.bssid
                );
                CONTROL.try_send(Control::Hello).ok();
                controller.wait_for_disconnect_async().await.ok();
                CONNECTED.store(false, Ordering::Relaxed);
                esp_println::println!("wireless disconnected; reconnecting");
            }
            Err(esp_radio::wifi::ConnectionError::Failed(info)) => esp_println::println!(
                "wireless association retry: {:?}, RSSI {}",
                info.reason,
                info.rssi
            ),
            Err(error) => esp_println::println!("wireless association retry: {:?}", error),
        }
        Timer::after_millis(500).await;
    }
}

fn carrier(node: NodeId, epoch: u32, batch_seq: u32) -> Carrier {
    Carrier {
        probe: Probe {
            tx: node,
            epoch,
            schedule: 0,
            seq: 0,
        },
        sensing: false,
        sent_node_us: now_us(),
        batch_seq,
        hub_port: PORT,
        relay_drops: RELAY_DROPS.load(Ordering::Relaxed),
    }
}

fn queue(packet: Packet) {
    #[cfg(feature = "gateway")]
    let result = UPLINK.try_send(packet);
    #[cfg(not(feature = "gateway"))]
    let result = BATCHES.try_send(packet);
    if let Err(embassy_sync::channel::TrySendError::Full(packet)) = result {
        EXPORT_DROPS.fetch_add(packet.count as u64, Ordering::Relaxed);
    }
}

#[embassy_executor::task]
pub async fn batch(node: NodeId, epoch: u32) {
    #[cfg(feature = "gateway")]
    while !CONNECTED.load(Ordering::Relaxed) {
        Timer::after_millis(50).await;
    }
    let mut frame_bytes = [0u8; csi_protocol::MAX_FRAME_LEN];
    let mut frame_seq = 0u32;
    let mut batch_seq = 0u32;
    let mut carry: Option<usize> = None;
    loop {
        let mut bytes = [0u8; wire::MAX_LEN];
        let mut builder =
            Builder::new(&mut bytes, carrier(node, epoch, batch_seq), &[0; 14]).unwrap();
        batch_seq = batch_seq.wrapping_add(1);
        let mut deadline = None;
        if let Some(len) = carry.take() {
            builder.push(&frame_bytes[..len]).unwrap();
            deadline = Some(now_us() + BATCH_MS * 1000);
        }
        loop {
            let wait = deadline
                .map(|d: u64| d.saturating_sub(now_us()))
                .unwrap_or(60_000_000);
            let item = match embassy_time::with_timeout(
                Duration::from_micros(wait),
                select(CONTROL.receive(), CAPTURES.receive()),
            )
            .await
            {
                Ok(item) => item,
                Err(_) => {
                    if builder.count() > 0 {
                        break;
                    } else {
                        continue;
                    }
                }
            };
            let message = match &item {
                Either::First(Control::Hello) => Message::Hello(Hello {
                    chip: Chip::Esp32,
                    roles: Roles {
                        tx: TX_ROLE,
                        rx: RX_ROLE,
                    },
                    firmware_version: firmware_version(),
                    firmware_commit: [0; 8],
                    channel: RADIO_CHANNEL.load(Ordering::Relaxed),
                    secondary_channel: 0,
                    csi_config: CSI_CONFIG,
                    pool_capacity: POOL as u16,
                }),
                Either::First(Control::Counters) => Message::Counters(counters()),
                Either::First(Control::Echo(e)) => Message::TimeEcho(TimeEcho {
                    node_tx_us: now_us(),
                    ..*e
                }),
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
            let len = frame::encode(
                &Header {
                    node,
                    epoch,
                    seq: frame_seq,
                },
                &message,
                &mut frame_bytes,
            )
            .unwrap();
            frame_seq = frame_seq.wrapping_add(1);
            if builder.push(&frame_bytes[..len]).is_err() {
                if builder.count() == 0 {
                    EXPORT_DROPS.fetch_add(1, Ordering::Relaxed);
                } else {
                    carry = Some(len);
                    break;
                }
            }
            if deadline.is_none() {
                deadline = Some(now_us() + BATCH_MS * 1000);
            }
            if builder.count() == wire::MAX_RECORDS {
                break;
            }
        }
        let count = builder.count();
        let len = builder.finish();
        if count > 0 {
            queue(Packet { bytes, len, count });
        }
    }
}

#[embassy_executor::task]
pub async fn receive(mut receiver: esp_radio::esp_now::EspNowReceiver, node: NodeId) {
    loop {
        let packet = receiver.receive_async().await;
        #[cfg(feature = "pair")]
        if mac_key(packet.info.src_address) != PEER_MAC.load(Ordering::Relaxed) {
            continue;
        }
        if let Ok((header, Message::TimeProbe(probe))) = frame::decode(packet.data()) {
            if header.node == node {
                CONTROL
                    .try_send(Control::Echo(TimeEcho {
                        probe_id: probe.probe_id,
                        host_time: probe.host_time,
                        node_rx_us: now_us(),
                        node_tx_us: 0,
                    }))
                    .ok();
            }
            continue;
        }
        #[cfg(feature = "gateway")]
        if let Ok(batch) = Batch::decode(packet.data()) {
            if batch.carrier.probe.tx.0 != packet.info.src_address
                || batch.count == 0
                || !(9000..=9063).contains(&batch.carrier.hub_port)
            {
                continue;
            }
            let mut bytes = [0u8; wire::MAX_LEN];
            bytes[..packet.data().len()].copy_from_slice(packet.data());
            let outgoing = Packet {
                bytes,
                len: packet.data().len(),
                count: batch.count,
            };
            if let Err(embassy_sync::channel::TrySendError::Full(p)) = UPLINK.try_send(outgoing) {
                RELAY_DROPS.fetch_add(p.count as u64, Ordering::Relaxed);
            }
        }
    }
}

#[embassy_executor::task]
pub async fn radio(mut sender: esp_radio::esp_now::EspNowSender, node: NodeId, epoch: u32) {
    #[cfg(feature = "pair")]
    let peer = csi_node::parse_mac(env!("CSI_PEER_MAC")).unwrap();
    #[cfg(feature = "pair")]
    let leader = node.0 < peer.0;
    #[cfg(feature = "pair")]
    let mut responder = csi_node::PairResponder::new(peer, SCHEDULE, GUARD_US, MAX_RESPONSE_AGE_US);
    #[cfg(feature = "pair")]
    let mut timeout = csi_node::AdaptiveTimeout::new();
    let period = option_env!("CSI_PROBE_INTERVAL_US")
        .map(|v| v.parse().unwrap())
        .unwrap_or(20_000u64);
    let mut next = now_us() + period;
    let mut seq = 0u32;
    #[cfg(feature = "gateway")]
    while !CONNECTED.load(Ordering::Relaxed) {
        Timer::after_millis(50).await;
    }
    loop {
        #[cfg(feature = "gateway")]
        if let Ok(request) = FORWARD_CONTROL.try_receive() {
            send_probe(&mut sender, &request.bytes[..request.len]).await;
        }
        let mut sensing = TX_ROLE;
        let mut turn = [0u8; 14];
        #[cfg(feature = "pair")]
        if !leader {
            match embassy_time::with_timeout(Duration::from_millis(BATCH_MS), PEER_PROBES.wait())
                .await
            {
                Ok((probe, tag, received)) => {
                    if !tag.is_request_for(&probe)
                        || responder.observe(probe, received, now_us()).is_none()
                    {
                        continue;
                    }
                    csi_node::TurnTag::response(tag.leader_epoch, tag.round)
                        .encode(&mut turn)
                        .unwrap();
                }
                Err(_) => {
                    sensing = false;
                }
            }
        } else {
            wait_periodic(&mut next, period).await;
            csi_node::TurnTag::request(epoch, seq)
                .encode(&mut turn)
                .unwrap();
        }
        #[cfg(not(feature = "pair"))]
        wait_periodic(&mut next, if TX_ROLE { period } else { BATCH_MS * 1000 }).await;
        #[cfg(not(feature = "gateway"))]
        let pending = BATCHES.try_receive().ok();
        #[cfg(feature = "gateway")]
        let pending: Option<Packet> = None;
        if !sensing && pending.is_none() {
            continue;
        }
        let mut packet = if let Some(p) = pending {
            p
        } else {
            let mut bytes = [0; wire::MAX_LEN];
            let len = Builder::new(&mut bytes, carrier(node, epoch, 0), &turn)
                .unwrap()
                .finish();
            Packet {
                bytes,
                len,
                count: 0,
            }
        };
        let mut header = Batch::decode(&packet.bytes[..packet.len]).unwrap().carrier;
        header.probe = Probe {
            tx: node,
            epoch,
            schedule: if cfg!(feature = "pair") { 1 } else { 0 },
            seq,
        };
        header.sensing = sensing;
        header.sent_node_us = now_us();
        wire::retag(&mut packet.bytes[..packet.len], header, &turn).unwrap();
        let sent = now_us();
        let completed = send_probe(&mut sender, &packet.bytes[..packet.len]).await;
        if packet.count > 0 {
            if completed {
                EXPORT_BYTES.fetch_add(packet.len as u64, Ordering::Relaxed);
            } else {
                EXPORT_DROPS.fetch_add(packet.count as u64, Ordering::Relaxed);
            }
        }
        if sensing {
            seq = seq.wrapping_add(1);
        }
        #[cfg(feature = "pair")]
        if leader && sensing {
            let deadline = sent.saturating_add(timeout.timeout_us());
            match embassy_time::with_timeout(
                Duration::from_micros(deadline.saturating_sub(now_us())),
                wait_response(peer, epoch, header.probe.seq, sent, deadline),
            )
            .await
            {
                Ok(rtt) => timeout.observe_rtt(rtt),
                Err(_) => timeout.on_timeout(),
            }
        }
    }
}

#[cfg(feature = "gateway")]
#[embassy_executor::task]
async fn net_runner(mut runner: embassy_net::Runner<'static, esp_radio::wifi::Interface>) {
    runner.run().await;
}

#[cfg(feature = "gateway")]
pub fn start_network(spawner: Spawner, node: NodeId) {
    static RESOURCES: static_cell::StaticCell<embassy_net::StackResources<3>> =
        static_cell::StaticCell::new();
    let (stack, runner) = embassy_net::new(
        esp_radio::wifi::Interface::station(),
        embassy_net::Config::dhcpv4(Default::default()),
        RESOURCES.init(embassy_net::StackResources::new()),
        esp_hal::rng::Rng::new().random() as u64,
    );
    spawner.spawn(net_runner(runner).unwrap());
    spawner.spawn(uplink(stack, node).unwrap());
}

#[cfg(feature = "gateway")]
#[embassy_executor::task]
async fn uplink(stack: embassy_net::Stack<'static>, node: NodeId) {
    use embassy_net::{
        IpAddress,
        udp::{PacketMetadata, UdpSocket},
    };
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx = [0u8; 2400];
    let mut tx = [0u8; 4800];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    socket.bind(9000).unwrap();
    stack.wait_config_up().await;
    if let Some(c) = stack.config_v4() {
        esp_println::println!("wireless gateway IPv4 {}", c.address);
    }
    let mut input = [0u8; wire::MAX_LEN];
    loop {
        match select(UPLINK.receive(), socket.recv_from(&mut input)).await {
            Either::First(mut packet) => {
                let mut header = Batch::decode(&packet.bytes[..packet.len]).unwrap().carrier;
                if header.probe.tx == node {
                    header.sent_node_us = now_us();
                    header.relay_drops = RELAY_DROPS.load(Ordering::Relaxed);
                    wire::retag(&mut packet.bytes[..packet.len], header, &[0; 14]).unwrap();
                }
                let destination = (
                    IpAddress::v4(HUB[0], HUB[1], HUB[2], HUB[3]),
                    header.hub_port,
                );
                match embassy_time::with_timeout(
                    Duration::from_millis(100),
                    socket.send_to(&packet.bytes[..packet.len], destination),
                )
                .await
                {
                    Ok(Ok(())) => {
                        if header.probe.tx == node {
                            EXPORT_BYTES.fetch_add(packet.len as u64, Ordering::Relaxed);
                        }
                    }
                    _ => {
                        if header.probe.tx == node {
                            EXPORT_DROPS.fetch_add(packet.count as u64, Ordering::Relaxed);
                        } else {
                            RELAY_DROPS.fetch_add(packet.count as u64, Ordering::Relaxed);
                        }
                    }
                }
            }
            Either::Second(Ok((n, endpoint))) => {
                if endpoint.endpoint.addr != IpAddress::v4(HUB[0], HUB[1], HUB[2], HUB[3]) {
                    continue;
                }
                if let Ok((header, Message::TimeProbe(probe))) = frame::decode(&input[..n]) {
                    if header.node == node {
                        CONTROL
                            .try_send(Control::Echo(TimeEcho {
                                probe_id: probe.probe_id,
                                host_time: probe.host_time,
                                node_rx_us: now_us(),
                                node_tx_us: 0,
                            }))
                            .ok();
                    } else {
                        #[cfg(feature = "pair")]
                        if header.node != csi_node::parse_mac(env!("CSI_PEER_MAC")).unwrap() {
                            continue;
                        }
                        let mut bytes = [0; csi_protocol::MAX_FRAME_LEN];
                        bytes[..n].copy_from_slice(&input[..n]);
                        FORWARD_CONTROL.try_send(Request { bytes, len: n }).ok();
                    }
                }
            }
            _ => {}
        }
    }
}
