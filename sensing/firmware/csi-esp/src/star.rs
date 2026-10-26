//! Star-only adapter: ESP-NOW control is distinct from CSI; all sends are serialized.
use super::*;
#[cfg(feature = "star-rx")]
use csi_node::star::{Action, Observation, Receiver};
#[cfg(feature = "star-tx")]
use csi_node::star::{Transmitter, response_delay};
use csi_protocol::star::{self as wire, Body, Identity, Packet};

pub const fn channel() -> u8 {
    let b = env!("CSI_STAR_CHANNEL").as_bytes();
    let mut i = 0;
    let mut n = 0;
    while i < b.len() {
        n = n * 10 + (b[i] - b'0');
        i += 1;
    }
    n
}
fn kit() -> u32 {
    env!("CSI_KIT_ID").parse().unwrap()
}
struct Received {
    bytes: [u8; wire::MAX_LEN],
    len: usize,
    source: [u8; 6],
    received_us: u64,
}
static AIR: Channel<CriticalSectionRawMutex, Received, 16> = Channel::new();
#[cfg(feature = "star-rx")]
pub static HOST: Channel<CriticalSectionRawMutex, Packet, 8> = Channel::new();
#[cfg(feature = "star-rx")]
pub static OUTPUT: Channel<CriticalSectionRawMutex, Capture, 32> = Channel::new();
#[cfg(feature = "star-rx")]
pub static DRAINED: embassy_sync::signal::Signal<CriticalSectionRawMutex, ()> =
    embassy_sync::signal::Signal::new();
static CONTROL_DROPS: AtomicU64 = AtomicU64::new(0);
/// Timestamp at the actual radio callback, before any executor/driver-queue delay.
/// Copy only; decoding/CRC/policy work is deferred to the radio task.
pub fn control_callback(frame: esp_radio::wifi::sniffer::PromiscuousPkt<'_>) {
    let received_us = now_us();
    let data = frame.data;
    if data.first().map(|b| b & 0xfc) != Some(0xd0) || data.get(1).map(|b| b & 3) != Some(0) {
        return;
    }
    let Some(source) = data.get(10..16) else {
        return;
    };
    let mut mac = [0; 6];
    mac.copy_from_slice(source);
    let Some(offset) = data.windows(4).position(|b| b == wire::MAGIC) else {
        return;
    };
    let Some(length) = data.get(offset + 6..offset + 8) else {
        return;
    };
    let n = usize::from(u16::from_le_bytes([length[0], length[1]])) + wire::HEADER_LEN + 4;
    if n > wire::MAX_LEN {
        return;
    }
    let Some(body) = data.get(offset..offset + n) else {
        return;
    };
    let mut bytes = [0; wire::MAX_LEN];
    bytes[..n].copy_from_slice(body);
    if AIR
        .try_send(Received {
            bytes,
            len: n,
            source: mac,
            received_us,
        })
        .is_err()
    {
        CONTROL_DROPS.fetch_add(1, Ordering::Relaxed);
    }
}
#[embassy_executor::task]
pub async fn receive(mut receiver: esp_radio::esp_now::EspNowReceiver) {
    loop {
        let _ = receiver.receive_async().await;
    }
}
async fn send(sender: &mut esp_radio::esp_now::EspNowSender, p: Packet) {
    let mut bytes = [0; wire::MAX_LEN];
    let n = p.encode(&mut bytes).unwrap();
    send_probe(sender, &bytes[..n]).await;
}

#[cfg(feature = "star-tx")]
#[embassy_executor::task]
pub async fn radio(mut sender: esp_radio::esp_now::EspNowSender, node: NodeId, epoch: u32) {
    let identity = Identity { mac: node, epoch };
    let mut policy = Transmitter::new(identity, kit(), channel(), BUILD_ID);
    let mut pending: Option<(u64, Packet)> = None;
    loop {
        let now = now_us() / 1000;
        policy.tick(now);
        if pending.is_some_and(|(due, _)| now >= due) {
            send(&mut sender, pending.take().unwrap().1).await;
        }
        match select(AIR.receive(), Timer::after_millis(1)).await {
            Either::First(received) => {
                let Ok(p) = Packet::decode(&received.bytes[..received.len]) else {
                    continue;
                };
                let now = now_us();
                if let Some((probe, tag)) =
                    policy.grant(p, received.source, received.received_us, now)
                {
                    let mut bytes = [0; 64];
                    let n = probe.encode(&mut bytes).unwrap();
                    tag.encode(&mut bytes[n..]).unwrap();
                    send_probe(&mut sender, &bytes).await;
                } else if let Some(body) = policy.command(p, received.source, now / 1000) {
                    let round = match p.body {
                        Body::Discover { round, .. } => round,
                        _ => 0,
                    };
                    let nonce = match p.body {
                        Body::Discover { nonce, .. } => nonce,
                        _ => p.correlation,
                    };
                    let delay =
                        response_delay(identity, nonce, round, esp_hal::rng::Rng::new().random());
                    // At most one pending response: no unbounded reply queue.
                    pending = Some((
                        now / 1000 + delay,
                        Packet {
                            kit: kit(),
                            sender: identity,
                            destination: p.sender,
                            correlation: p.correlation,
                            body,
                        },
                    ));
                }
            }
            Either::Second(_) => {}
        }
    }
}

#[cfg(feature = "star-rx")]
#[embassy_executor::task]
pub async fn radio(mut sender: esp_radio::esp_now::EspNowSender, node: NodeId, epoch: u32) {
    let mut policy = Receiver::new(
        Identity { mac: node, epoch },
        kit(),
        channel(),
        BUILD_ID,
        esp_hal::rng::Rng::new().random(),
        now_us() / 1000,
    );
    let mut draining = false;
    let mut announce = 0;
    let mut capture_base = 0;
    let mut drop_base = 0;
    let mut was_running = false;
    loop {
        // Host STOP and leases precede grants and capture work. Bound each queue drain.
        for _ in 0..8 {
            if let Ok(p) = HOST.try_receive() {
                policy.host(p, now_us() / 1000)
            } else {
                break;
            }
        }
        for _ in 0..16 {
            if let Ok(p) = AIR.try_receive() {
                if let Ok(packet) = Packet::decode(&p.bytes[..p.len]) {
                    policy.air_input(packet, p.source, p.received_us / 1000)
                }
            } else {
                break;
            }
        }
        for _ in 0..32 {
            let Ok(mut c) = CAPTURES.try_receive() else {
                break;
            };
            let seq = CAPTURED.load(Ordering::Relaxed) as u32;
            if policy.observe(c.probe, c.tag, c.mac, seq, c.node_us / 1000) == Observation::Current
            {
                c.seq = CAPTURED.fetch_add(1, Ordering::Relaxed) as u32;
                if OUTPUT.try_send(c).is_err() {
                    EXPORT_DROPS.fetch_add(1, Ordering::Relaxed);
                }
            } else {
                FILTERED.fetch_add(1, Ordering::Relaxed);
            }
        }
        policy.tick(now_us() / 1000);
        if policy.run.is_some() && !was_running {
            capture_base = CAPTURED.load(Ordering::Relaxed);
            drop_base = EXPORT_DROPS.load(Ordering::Relaxed)
                + POOL_DROPS.load(Ordering::Relaxed)
                + CONTROL_DROPS.load(Ordering::Relaxed)
                + policy.control_drops();
        }
        was_running = policy.run.is_some();
        if now_us() / 1000 >= announce {
            announce = now_us() / 1000 + 1000;
            CONTROL
                .try_send(Control::Star(Packet {
                    kit: kit(),
                    sender: policy.identity,
                    destination: Identity::BROADCAST,
                    correlation: 0,
                    body: Body::RxReady {
                        token: 0,
                        info: policy.info(),
                    },
                }))
                .ok();
        }
        while let Some(action) = policy.pop() {
            match action {
                Action::Air(p) => send(&mut sender, p).await,
                Action::Usb(p) => {
                    // USB export cannot block lease/STOP policy work. Overflow is explicit.
                    if CONTROL.try_send(Control::Star(p)).is_err() {
                        CONTROL_DROPS.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
        }
        if policy.needs_drain() && !draining {
            // Capture is disarmed in Stopping. OUTPUT now has a fixed boundary.
            let packet = Packet {
                kit: kit(),
                sender: policy.identity,
                destination: Identity::BROADCAST,
                correlation: 0,
                body: Body::DrainComplete {
                    run: policy.run.unwrap().run,
                    capture: CAPTURED.load(Ordering::Relaxed),
                    export: 0,
                    captured: CAPTURED.load(Ordering::Relaxed) - capture_base,
                    exported: 0,
                    dropped: (EXPORT_DROPS.load(Ordering::Relaxed)
                        + POOL_DROPS.load(Ordering::Relaxed)
                        + CONTROL_DROPS.load(Ordering::Relaxed)
                        + policy.control_drops())
                    .saturating_sub(drop_base),
                    selected: policy.selected,
                    confirmed: policy.acked,
                },
            };
            if CONTROL.try_send(Control::Drain(packet)).is_ok() {
                draining = true;
            }
        }
        if draining && DRAINED.try_take().is_some() {
            // Exporter already emitted the authoritative drain watermark. Suppress
            // the policy's placeholder watermark, retaining result/state updates.
            policy.drain_complete(0, 0, 0, 0, 0);
            while let Some(action) = policy.pop() {
                if let Action::Usb(p) = action {
                    if !matches!(p.body, Body::DrainComplete { .. }) {
                        CONTROL.try_send(Control::Star(p)).ok();
                    }
                }
            }
            draining = false;
        }
        Timer::after_millis(1).await;
    }
}
