fn main() {
    let star_rx = std::env::var_os("CARGO_FEATURE_STAR_RX").is_some();
    let star_tx = std::env::var_os("CARGO_FEATURE_STAR_TX").is_some();
    if star_rx || star_tx {
        assert!(
            !(star_rx && star_tx),
            "star-rx and star-tx are mutually exclusive"
        );
        for key in [
            "CARGO_FEATURE_PAIR",
            "CARGO_FEATURE_GATEWAY",
            "CARGO_FEATURE_WIRELESS",
        ] {
            assert!(
                std::env::var_os(key).is_none(),
                "star roles cannot use legacy pair/wireless features"
            );
        }
        assert!(
            !(star_rx && std::env::var_os("CARGO_FEATURE_TX").is_some()),
            "star RX cannot sense as TX"
        );
        assert!(
            !(star_tx && std::env::var_os("CARGO_FEATURE_RX").is_some()),
            "star TX cannot capture CSI"
        );
        for key in ["CSI_KIT_ID", "CSI_STAR_CHANNEL"] {
            println!("cargo:rerun-if-env-changed={key}");
        }
        let kit = std::env::var("CSI_KIT_ID")
            .unwrap_or_else(|_| "1".into())
            .parse::<u32>()
            .expect("numeric kit ID");
        assert_ne!(kit, 0, "kit ID must be nonzero");
        let channel = std::env::var("CSI_STAR_CHANNEL")
            .unwrap_or_else(|_| "6".into())
            .parse::<u8>()
            .expect("numeric channel");
        assert!((1..=11).contains(&channel));
        println!("cargo:rustc-env=CSI_KIT_ID={kit}");
        println!("cargo:rustc-env=CSI_STAR_CHANNEL={channel}");
    }
    // Fingerprint the exact firmware/policy/protocol sources and build settings.
    // This identifies a prepared image, including uncommitted source, without a fake commit.
    let mut fingerprint = 0xcbf29ce484222325u64;
    fn hash_tree(path: &std::path::Path, hash: &mut u64) {
        let mut entries: Vec<_> = std::fs::read_dir(path)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                hash_tree(&p, hash);
            } else {
                println!("cargo:rerun-if-changed={}", p.display());
                for b in p
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .bytes()
                    .chain(std::fs::read(&p).unwrap())
                {
                    *hash = (*hash ^ u64::from(b)).wrapping_mul(0x100000001b3);
                }
            }
        }
    }
    for path in [
        "src",
        "../../crates/csi-node/src",
        "../../crates/csi-protocol/src",
    ] {
        hash_tree(std::path::Path::new(path), &mut fingerprint);
    }
    for name in ["build.rs", "Cargo.toml", "../Cargo.lock"] {
        println!("cargo:rerun-if-changed={name}");
        for b in name.bytes().chain([0]).chain(std::fs::read(name).unwrap()) {
            fingerprint = (fingerprint ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
    }
    for key in [
        "CSI_KIT_ID",
        "CSI_STAR_CHANNEL",
        "CSI_PEER_MAC",
        "CSI_PROBE_INTERVAL_US",
        "CARGO_FEATURE_STAR_RX",
        "CARGO_FEATURE_STAR_TX",
    ] {
        println!("cargo:rerun-if-env-changed={key}");
        let value = std::env::var(key).unwrap_or_default();
        for b in key.bytes().chain([b'=']).chain(value.bytes()).chain([0]) {
            fingerprint = (fingerprint ^ u64::from(b)).wrapping_mul(0x100000001b3);
        }
    }
    std::fs::write(
        std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("build_id.rs"),
        format!("pub const BUILD_ID:[u8;8]={:?};", fingerprint.to_le_bytes()),
    )
    .unwrap();
    if std::env::var_os("CARGO_FEATURE_WIRELESS").is_some() {
        let gateway = std::env::var_os("CARGO_FEATURE_GATEWAY").is_some();
        // Remote nodes never read the credentials file, including when it exists locally.
        let local = if gateway {
            println!("cargo:rerun-if-changed=../.wifi.env");
            std::fs::read_to_string("../.wifi.env").unwrap_or_default()
        } else {
            String::new()
        };
        let generated = wireless_config(gateway, |key| {
            println!("cargo:rerun-if-env-changed={key}");
            std::env::var(key).ok().or_else(|| {
                local
                    .lines()
                    .filter_map(|line| line.split_once('='))
                    .find(|(k, _)| k.trim() == key)
                    .map(|(_, value)| value.trim().trim_matches('"').to_owned())
            })
        });
        std::fs::write(
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("wifi_config.rs"),
            generated,
        )
        .unwrap();
    }
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

// Also host-testable with: rustc +stable --test build.rs -o /tmp/csi-build-tests.
fn wireless_config(gateway: bool, mut get: impl FnMut(&str) -> Option<String>) -> String {
    let port = get("CSI_HUB_PORT")
        .unwrap_or_else(|| "9001".into())
        .parse::<u16>()
        .expect("invalid hub port");
    assert!(
        (9000..=9063).contains(&port),
        "hub ports must be 9000..9063"
    );
    let batch = get("CSI_BATCH_MS")
        .unwrap_or_else(|| "50".into())
        .parse::<u64>()
        .expect("invalid batch age");
    assert!((5..=500).contains(&batch), "batch age must be 5..500 ms");
    let mut generated = format!("pub const PORT:u16={port};\npub const BATCH_MS:u64={batch};\n");
    if gateway {
        let ssid =
            get("CSI_WIFI_SSID").expect("gateway requires CSI_WIFI_SSID in ignored .wifi.env");
        let password = get("CSI_WIFI_PASSWORD").expect("gateway requires CSI_WIFI_PASSWORD");
        assert!(
            !ssid.is_empty() && ssid.len() <= 32,
            "SSID length must be 1..32 bytes"
        );
        assert!(
            (8..=63).contains(&password.len()),
            "WPA2 password length must be 8..63 bytes"
        );
        let hub = get("CSI_HUB_ADDR")
            .expect("gateway requires CSI_HUB_ADDR")
            .parse::<std::net::Ipv4Addr>()
            .expect("CSI_HUB_ADDR must be IPv4");
        assert!(
            !hub.is_unspecified(),
            "gateway requires a nonzero CSI_HUB_ADDR"
        );
        generated.push_str(&format!(
            "pub const SSID:&str={ssid:?};\npub const PASSWORD:&str={password:?};\npub const HUB:[u8;4]={:?};\n", hub.octets()));
    } else {
        let channel = get("CSI_RADIO_CHANNEL")
            .expect("remote requires CSI_RADIO_CHANNEL matching the gateway AP channel")
            .parse::<u8>()
            .expect("CSI_RADIO_CHANNEL must be an integer");
        assert!((1..=11).contains(&channel), "remote channel must be 1..11");
        generated.push_str(&format!("pub const REMOTE_CHANNEL:u8={channel};\n"));
    }
    generated
}

#[cfg(test)]
mod tests {
    use super::wireless_config;

    #[test]
    fn remote_config_never_requests_network_credentials_or_host_address() {
        let source = wireless_config(false, |key| match key {
            "CSI_RADIO_CHANNEL" => Some("6".into()),
            "CSI_HUB_PORT" => Some("9002".into()),
            "CSI_BATCH_MS" => None,
            _ => panic!("remote requested network setting {key}"),
        });
        assert!(source.contains("REMOTE_CHANNEL:u8=6"));
        assert!(source.contains("PORT:u16=9002"));
        assert!(!source.contains("SSID"));
        assert!(!source.contains("PASSWORD"));
        assert!(!source.contains("HUB:"));
    }

    #[test]
    #[should_panic(expected = "remote requires CSI_RADIO_CHANNEL")]
    fn remote_channel_must_be_explicit() {
        wireless_config(false, |_| None);
    }

    #[test]
    fn remote_rejects_channels_outside_supported_range() {
        for channel in ["0", "12", "255", "invalid"] {
            assert!(
                std::panic::catch_unwind(|| wireless_config(false, |key| (key
                    == "CSI_RADIO_CHANNEL")
                    .then(|| channel.into())))
                .is_err()
            );
        }
    }

    #[test]
    fn gateway_config_contains_network_settings_but_no_remote_channel() {
        let source = wireless_config(true, |key| match key {
            "CSI_WIFI_SSID" => Some("test-network".into()),
            "CSI_WIFI_PASSWORD" => Some("test-password".into()),
            "CSI_HUB_ADDR" => Some("192.0.2.1".into()),
            "CSI_HUB_PORT" | "CSI_BATCH_MS" => None,
            _ => panic!("unexpected gateway setting {key}"),
        });
        assert!(source.contains("test-network"));
        assert!(source.contains("test-password"));
        assert!(source.contains("[192, 0, 2, 1]"));
        assert!(!source.contains("REMOTE_CHANNEL"));
    }

    #[test]
    #[should_panic(expected = "gateway requires CSI_WIFI_SSID")]
    fn gateway_still_requires_network_credentials() {
        wireless_config(true, |_| None);
    }
}
