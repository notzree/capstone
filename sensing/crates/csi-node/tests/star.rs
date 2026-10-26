use csi_node::star::{Action, Observation, Receiver, Transmitter};
use csi_protocol::{NodeId, star::*};
#[test]
fn five_node_grants_are_paced_round_robin_and_probe_sequences_stay_per_tx() {
    let rx = Identity {
        mac: NodeId([2, 0, 0, 0, 0, 1]),
        epoch: 1,
    };
    let mut fleet: [Transmitter; 5] = core::array::from_fn(|i| {
        Transmitter::new(
            Identity {
                mac: NodeId([2, 0, 0, 0, 0, 10 + i as u8]),
                epoch: 2 + i as u32,
            },
            1,
            6,
            [1; 8],
        )
    });
    let mut r = Receiver::new(rx, 1, 6, [2; 8], 7, 0);
    r.tick(0);
    while r.pop().is_some() {}
    let mut nodes = [Identity::default(); 8];
    for (i, t) in fleet.iter().enumerate() {
        nodes[i] = t.info.identity;
        r.air_input(
            Packet {
                kit: 1,
                sender: t.info.identity,
                destination: rx,
                correlation: 0,
                body: Body::DiscoverAck {
                    nonce: 7,
                    round: 0,
                    info: t.info,
                },
            },
            t.info.identity.mac.0,
            9999,
        );
    }
    r.tick(10000);
    while r.pop().is_some() {}
    r.host(
        Packet {
            kit: 1,
            sender: Identity::default(),
            destination: rx,
            correlation: 1,
            body: Body::Attach { token: 9 },
        },
        10000,
    );
    while r.pop().is_some() {}
    r.host(
        Packet {
            kit: 1,
            sender: Identity::default(),
            destination: rx,
            correlation: 2,
            body: Body::Start(Start {
                attachment: 9,
                run: 0,
                schedule: 0,
                rate: 10,
                duration_ms: 1000,
                lease_ms: 3000,
                count: 5,
                nodes,
            }),
        },
        10000,
    );
    let mut counts = [0; 5];
    let mut captures = 0;
    let mut order = vec![];
    for now in 10000..11000 {
        r.tick(now);
        while let Some(action) = r.pop() {
            if let Action::Air(p) = action {
                let mut sent = 0;
                for (i, t) in fleet.iter_mut().enumerate() {
                    if let Some((probe, tag)) = t.grant(p, rx.mac.0, now * 1000, now * 1000) {
                        assert_eq!(probe.seq, counts[i]);
                        assert_eq!(
                            r.observe(probe, tag, t.info.identity.mac.0, captures, now),
                            Observation::Current
                        );
                        counts[i] += 1;
                        captures += 1;
                        sent += 1;
                        order.push(i);
                    } else if let Some(body) = t.command(p, rx.mac.0, now) {
                        r.air_input(
                            Packet {
                                kit: 1,
                                sender: t.info.identity,
                                destination: rx,
                                correlation: p.correlation,
                                body,
                            },
                            t.info.identity.mac.0,
                            now,
                        );
                    }
                }
                assert!(sent <= 1);
            }
        }
    }
    assert_eq!(counts, [10; 5]);
    assert_eq!(order, (0..50).map(|i| i % 5).collect::<Vec<_>>());
    assert_eq!(r.grant_timeouts, 0);
}
