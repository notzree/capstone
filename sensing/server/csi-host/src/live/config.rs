use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::clock::FitOptions;
use crate::error::IoContext;
use crate::{Error, manifest::SourceInfo};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HubConfig {
    pub queue_capacity: usize,
    pub clock_sample_capacity: usize,
    pub stale_ms: u64,
    pub reconnect_ms: u64,
    pub echo_interval_ms: u64,
    pub status_interval_ms: u64,
    pub clock_window_ms: u64,
    pub clock_step_ms: u64,
    pub sources: Vec<SourceConfig>,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            queue_capacity: 8192,
            clock_sample_capacity: 300_000,
            stale_ms: 2000,
            reconnect_ms: 500,
            echo_interval_ms: 1000,
            status_interval_ms: 1000,
            clock_window_ms: 60_000,
            clock_step_ms: 10_000,
            sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceConfig {
    pub name: String,
    #[serde(flatten)]
    pub transport: Transport,
}

/// One configured node/export endpoint per source. UDP peers are explicit so another sender
/// cannot silently replace an endpoint or become the destination of host requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Transport {
    Udp { bind: SocketAddr, peer: SocketAddr },
    Serial { path: PathBuf, baud: u32 },
}

impl HubConfig {
    pub fn load(path: &Path) -> Result<Self, Error> {
        let text = std::fs::read_to_string(path).at(path)?;
        let config: Self = toml::from_str(&text).map_err(|e| Error::Config(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), Error> {
        let invalid = |why: &str| Err(Error::Config(why.into()));
        if self.sources.is_empty() || self.sources.len() > 64 {
            return invalid("hub requires 1..=64 sources");
        }
        if !(1..=65_536).contains(&self.queue_capacity)
            || !(20..=2_000_000).contains(&self.clock_sample_capacity)
        {
            return invalid(
                "queue_capacity must be 1..=65536 and clock_sample_capacity 20..=2000000",
            );
        }
        for ms in [
            self.stale_ms,
            self.reconnect_ms,
            self.echo_interval_ms,
            self.status_interval_ms,
            self.clock_window_ms,
            self.clock_step_ms,
        ] {
            if !(10..=3_600_000).contains(&ms) {
                return invalid("hub intervals must be 10..=3600000 ms");
            }
        }
        if self.clock_step_ms > self.clock_window_ms {
            return invalid("clock step must be no longer than the window");
        }
        let mut names = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for source in &self.sources {
            if source.name.trim().is_empty()
                || source.name.len() > 128
                || !names.insert(&source.name)
            {
                return invalid("source names must be nonempty, unique and at most 128 bytes");
            }
            match &source.transport {
                Transport::Serial { path, .. } => {
                    if path.as_os_str().is_empty() || !paths.insert(path) {
                        return invalid("serial paths must be nonempty and unique");
                    }
                }
                Transport::Udp { bind, peer } => {
                    if bind.is_ipv4() != peer.is_ipv4()
                        || peer.port() == 0
                        || peer.ip().is_unspecified()
                        || peer.ip().is_multicast()
                    {
                        return invalid(
                            "UDP bind and unicast peer must use the same address family; peer port must be nonzero",
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub fn clock_options(&self) -> FitOptions {
        FitOptions {
            window_ns: self.clock_window_ms * 1_000_000,
            step_ns: self.clock_step_ms * 1_000_000,
            ..FitOptions::default()
        }
    }

    pub fn source_info(&self) -> Vec<SourceInfo> {
        self.sources
            .iter()
            .map(|s| SourceInfo {
                name: s.name.clone(),
                kind: match s.transport {
                    Transport::Udp { .. } => "udp",
                    Transport::Serial { .. } => "serial",
                }
                .into(),
                details: serde_json::to_value(&s.transport).expect("serializable source config"),
            })
            .collect()
    }
}
