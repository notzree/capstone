fn main() {
    println!("cargo:rerun-if-env-changed=CSI_PROBE_INTERVAL_US");
    if let Ok(value) = std::env::var("CSI_PROBE_INTERVAL_US") {
        let interval: u64 = value
            .parse()
            .expect("CSI_PROBE_INTERVAL_US must be an integer");
        assert!(
            (1_000..=1_000_000).contains(&interval),
            "probe interval must be 1000..1000000 us"
        );
    }
    println!("cargo:rerun-if-env-changed=CSI_PEER_MAC");
    if std::env::var_os("CARGO_FEATURE_PAIR").is_some() {
        let peer =
            std::env::var("CSI_PEER_MAC").expect("pair requires CSI_PEER_MAC=aa:bb:cc:dd:ee:ff");
        let parts: Vec<_> = peer.split(':').collect();
        assert!(
            parts.len() == 6
                && parts
                    .iter()
                    .all(|p| p.len() == 2 && u8::from_str_radix(p, 16).is_ok()),
            "CSI_PEER_MAC must contain six colon-separated hex octets"
        );
        let mac: Vec<_> = parts
            .iter()
            .map(|p| u8::from_str_radix(p, 16).unwrap())
            .collect();
        assert!(
            mac[0] & 1 == 0 && mac.iter().any(|b| *b != 0),
            "peer must be a nonzero unicast MAC"
        );
    }
}
