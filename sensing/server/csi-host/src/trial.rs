//! Trial provenance and operator labels live outside the immutable raw session.
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use csi_protocol::NodeId;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::Error;
use crate::error::IoContext;
use crate::geometry::Geometry;
use crate::live::config::HubConfig;
use crate::manifest::Manifest;
use crate::segment::sha256_hex;

pub const SCHEMA: &str = "csi-trial/1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub trial_id: String,
    pub day_id: String,
    pub participant_id: Option<String>,
    pub split_group: String,
    pub room: String,
    pub geometry_revision: String,
    pub label_uncertainty_ms: u32,
    pub activities: Vec<String>,
    #[serde(default)]
    pub positions: Vec<Position>,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Position {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub azimuth_deg: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Node {
    pub id: String,
    pub source: String,
    pub mode: Mode,
    pub requested_tx_hz: f64,
    pub channel: u8,
    /// Relative to the plan file. The operator declares this is the flashed image.
    pub image: PathBuf,
    pub build_manifest: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Tx,
    Rx,
    Independent,
    Pair,
}

impl Mode {
    fn transmits(self) -> bool {
        self != Self::Rx
    }
    fn receives(self) -> bool {
        self != Self::Tx
    }
}

fn config_error(reason: impl Into<String>) -> Error {
    Error::Config(reason.into())
}

impl Plan {
    pub fn load(path: &Path) -> Result<Self, Error> {
        serde_json::from_slice(&fs::read(path).at(path)?).map_err(|e| config_error(e.to_string()))
    }

    pub fn validate(&self, config: &HubConfig, geometry: &Geometry) -> Result<(), Error> {
        let star = self.schema == "csi-trial-plan/2";
        if !star && self.schema != "csi-trial-plan/1" {
            return Err(config_error("expected csi-trial-plan/1"));
        }
        for value in [
            &self.trial_id,
            &self.day_id,
            &self.split_group,
            &self.room,
            &self.geometry_revision,
        ] {
            if value.trim().is_empty() || value.len() > 128 {
                return Err(config_error("trial identifiers must be 1..=128 bytes"));
            }
        }
        if self
            .participant_id
            .as_ref()
            .is_some_and(|p| p.trim().is_empty() || p.len() > 128)
        {
            return Err(config_error(
                "participant_id must be null or a nonempty pseudonym",
            ));
        }
        if self.room != geometry.room || self.geometry_revision != geometry.revision {
            return Err(config_error(
                "trial room/revision does not match the geometry survey",
            ));
        }
        if !(1..=60_000).contains(&self.label_uncertainty_ms)
            || self.activities.is_empty()
            || self.activities.len() > 128
        {
            return Err(config_error(
                "declare activities and label_uncertainty_ms in 1..=60000",
            ));
        }
        let token = |s: &str| {
            !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        };
        let activities: BTreeSet<_> = self.activities.iter().collect();
        if activities.len() != self.activities.len() || !self.activities.iter().all(|s| token(s)) {
            return Err(config_error(
                "activities must be unique single-word identifiers",
            ));
        }
        let mut positions = BTreeSet::new();
        for p in &self.positions {
            if !token(&p.id)
                || !positions.insert(&p.id)
                || ![p.x, p.y, p.z, p.azimuth_deg.unwrap_or(0.0)]
                    .into_iter()
                    .all(f64::is_finite)
            {
                return Err(config_error(
                    "positions need unique identifiers and finite coordinates/angles",
                ));
            }
        }
        let geometry_ids = geometry.node_ids();
        if self.nodes.iter().any(|n| n.mode == Mode::Pair)
            && (self.nodes.len() != 2 || self.nodes.iter().any(|n| n.mode != Mode::Pair))
        {
            return Err(config_error(
                "packet-driven pair plans require exactly two pair nodes",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut sources = BTreeSet::new();
        for node in &self.nodes {
            let id: NodeId = node
                .id
                .parse()
                .map_err(|_| config_error("invalid node MAC"))?;
            if node.id != id.to_string()
                || !ids.insert(id)
                || !geometry_ids.contains(&id)
                || (!star && !sources.insert(node.source.as_str()))
            {
                return Err(config_error(
                    "nodes need unique canonical MACs, sources and geometry placements",
                ));
            }
            if !(1..=14).contains(&node.channel)
                || !node.requested_tx_hz.is_finite()
                || (node.mode.transmits()
                    && !(0.0 < node.requested_tx_hz && node.requested_tx_hz <= 10_000.0))
                || (!node.mode.transmits() && node.requested_tx_hz != 0.0)
            {
                return Err(config_error("invalid node channel/requested TX rate"));
            }
        }
        if star {
            sources.extend(self.nodes.iter().map(|n| n.source.as_str()));
            if config.sources.len() != 1
                || self.nodes.iter().filter(|n| n.mode == Mode::Rx).count() != 1
                || self
                    .nodes
                    .iter()
                    .any(|n| !matches!(n.mode, Mode::Rx | Mode::Tx))
            {
                return Err(config_error(
                    "star plan requires one physical RX source and TX radio participants",
                ));
            }
        }
        let configured: BTreeSet<_> = config.sources.iter().map(|s| s.name.as_str()).collect();
        if configured != sources {
            return Err(config_error(
                "declare exactly one expected node for every configured source",
            ));
        }
        Ok(())
    }
}

pub fn store_json(path: &Path, document: &impl Serialize) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(document).map_err(|e| config_error(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    let mut file = File::create(&tmp).at(&tmp)?;
    file.write_all(&bytes).at(&tmp)?;
    file.sync_all().at(&tmp)?;
    fs::rename(&tmp, path).at(path)
}

pub struct Bundle {
    pub plan: Plan,
    pub dir: PathBuf,
    metadata: Value,
}

impl Bundle {
    /// Validate all inputs before creating a new bundle; no raw session exists yet.
    pub fn prepare(
        plan_path: &Path,
        config_path: &Path,
        geometry_path: &Path,
        dir: &Path,
    ) -> Result<Self, Error> {
        let config_bytes = fs::read(config_path).at(config_path)?;
        let config: HubConfig = toml::from_str(
            std::str::from_utf8(&config_bytes).map_err(|e| config_error(e.to_string()))?,
        )
        .map_err(|e| config_error(e.to_string()))?;
        Self::prepare_config(plan_path, &config, config_bytes, geometry_path, dir)
    }

    pub fn prepare_config(
        plan_path: &Path,
        config: &HubConfig,
        config_bytes: Vec<u8>,
        geometry_path: &Path,
        dir: &Path,
    ) -> Result<Self, Error> {
        if dir.exists() {
            return Err(Error::Exists(dir.to_owned()));
        }
        let plan_bytes = fs::read(plan_path).at(plan_path)?;
        let plan: Plan =
            serde_json::from_slice(&plan_bytes).map_err(|e| config_error(e.to_string()))?;
        config.validate()?;
        let (geometry, geometry_bytes) = Geometry::load(geometry_path)?;
        plan.validate(config, &geometry)?;
        let base = plan_path.parent().unwrap_or(Path::new("."));
        let mut snapshots = vec![
            ("plan.json".to_owned(), plan_bytes),
            ("hub-config.toml".to_owned(), config_bytes),
            ("geometry.json".to_owned(), geometry_bytes),
            (
                "plan-normalized.json".to_owned(),
                serde_json::to_vec(&plan).map_err(|e| config_error(e.to_string()))?,
            ),
        ];
        for (i, node) in plan.nodes.iter().enumerate() {
            for (kind, relative) in [
                ("image.bin", &node.image),
                ("build.json", &node.build_manifest),
            ] {
                let path = base.join(relative);
                snapshots.push((format!("node-{i}-{kind}"), fs::read(&path).at(&path)?));
            }
        }
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).at(parent)?;
        }
        fs::create_dir(dir).at(dir)?;
        let provenance = dir.join("provenance");
        fs::create_dir(&provenance).at(&provenance)?;
        let mut artifacts = Vec::new();
        for (name, bytes) in snapshots {
            let path = provenance.join(&name);
            fs::write(&path, &bytes).at(&path)?;
            artifacts.push(json!({"file":format!("provenance/{name}"),"sha256":sha256_hex(&bytes),"bytes":bytes.len()}));
        }
        let metadata = json!({"schema":if plan.schema == "csi-trial-plan/2" {"csi-trial/2"} else {SCHEMA},"plan":plan,"session_id":null,"raw_session":"raw",
            "labels_file":"labels.jsonl","export_dir":"export","artifacts":artifacts,
            "firmware_provenance":"operator_declared_image_and_build; not an on-device flash verification",
            "label_timebase":"host_mono_ns","complete":false});
        store_json(&dir.join("trial.json"), &metadata)?;
        Ok(Self {
            plan,
            dir: dir.to_owned(),
            metadata,
        })
    }

    pub fn bind(&mut self, manifest: &Manifest) -> Result<Labels, Error> {
        self.metadata["session_id"] = json!(manifest.session_id);
        self.metadata["geometry"] = json!(manifest.geometry);
        store_json(&self.dir.join("trial.json"), &self.metadata)?;
        store_json(
            &self.dir.join("split.json"),
            &json!({"schema":"csi-split/1","session_id":manifest.session_id,
            "group":self.plan.split_group,"assignment":"unassigned"}),
        )?;
        Labels::create(
            &self.dir.join("labels.jsonl"),
            manifest.session_id,
            &self.plan,
        )
    }

    pub fn finish(&mut self, stopped_mono_ns: u64, reason: &str) -> Result<(), Error> {
        let export_path = self.dir.join("export/export.json");
        let export: Value = serde_json::from_slice(&fs::read(&export_path).at(&export_path)?)
            .map_err(|e| config_error(e.to_string()))?;
        if export["manifest"]["session_id"] != self.metadata["session_id"]
            || export["complete"] != true
        {
            return Err(config_error(
                "trial export does not match the recorded session",
            ));
        }
        let mut expected = Vec::new();
        let mut missing = Vec::new();
        let links = export["summary"]["links"].as_object();
        for rx in self.plan.nodes.iter().filter(|n| n.mode.receives()) {
            for tx in self
                .plan
                .nodes
                .iter()
                .filter(|n| n.mode.transmits() && n.id != rx.id && n.channel == rx.channel)
            {
                let link = json!({"receiver":rx.id,"transmitter":tx.id});
                expected.push(link.clone());
                let observed = links.is_some_and(|links| {
                    links
                        .values()
                        .any(|l| l["receiver"] == rx.id && l["transmitter"] == tx.id)
                });
                if !observed {
                    missing.push(link);
                }
            }
        }
        let status_path = self.dir.join("raw/hub-status.json");
        let status: Value = serde_json::from_slice(&fs::read(&status_path).at(&status_path)?)
            .map_err(|e| config_error(e.to_string()))?;
        let source_checks: Vec<_> = self.plan.nodes.iter().filter(|n| self.plan.schema != "csi-trial-plan/2" || n.mode == Mode::Rx).map(|n| {
            let observed = status["sources"].as_array().and_then(|s| s.iter().find(|s| s["name"] == n.source)).map(|s| s["node"].clone()).unwrap_or(Value::Null);
            json!({"source":n.source,"expected_node":n.id,"observed_node":observed,"matches":observed == n.id})
        }).collect();
        let configuration_checks: Vec<_> = self.plan.nodes.iter().map(|n| {
            let observed: Vec<_> = export["health"]["nodes"].as_array().into_iter().flatten()
                .filter(|node| node["node"] == n.id).flat_map(|node| node["epochs"].as_array().into_iter().flatten())
                .filter_map(|epoch| epoch.get("hello").filter(|h| !h.is_null()).cloned()).collect();
            let matches = (!observed.is_empty()).then(|| observed.iter().all(|h| h["channel"] == n.channel
                && h["tx"] == n.mode.transmits() && h["rx"] == n.mode.receives()));
            json!({"node":n.id,"declared_mode":n.mode,"declared_channel":n.channel,"observed":observed,"roles_and_channel_match":matches,
                "requested_tx_hz":n.requested_tx_hz,"mode_and_rate_provenance":"declared; collector does not reconfigure firmware"})
        }).collect();
        let star = fs::read(self.dir.join("star-run.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok());
        let quality = json!({"schema":"csi-trial-quality/1","session_id":self.metadata["session_id"],
            "summary":export["summary"],"health":export["health"],"star":star,"expected_links":expected,
            "missing_links":missing,"source_identity_checks":source_checks,"node_configuration_checks":configuration_checks,
            "timing":"labels use host monotonic; capture-to-host mapping is not applied; arrival matching is a proxy",
            "counter_scope":"summary.counter_windows deltas cover their first/last observed counter instants; health counters remain cumulative since boot",
            "complete":true});
        store_json(&self.dir.join("quality.json"), &quality)?;
        self.metadata["labels_sha256"] = json!(sha256_hex(
            &fs::read(self.dir.join("labels.jsonl")).at(&self.dir)?
        ));
        self.metadata["stopped_mono_ns"] = stopped_mono_ns.into();
        self.metadata["stop_reason"] = reason.into();
        self.metadata["complete"] = true.into();
        store_json(&self.dir.join("trial.json"), &self.metadata)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Stamp {
    pub mono_ns: u64,
    pub wall_ns: u64,
}

pub struct Labels {
    file: File,
    path: PathBuf,
    plan: Plan,
    active: Option<u64>,
    next_id: u64,
    last_ns: u64,
    finished: bool,
}

impl Labels {
    pub fn create(path: &Path, session: Uuid, plan: &Plan) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .at(path)?;
        let mut labels = Self {
            file,
            path: path.to_owned(),
            plan: plan.clone(),
            active: None,
            next_id: 0,
            last_ns: 0,
            finished: false,
        };
        labels.append(json!({"schema":"csi-labels/1","type":"header","session_id":session,
            "timebase":"host_mono_ns","uncertainty_ns":u64::from(plan.label_uncertainty_ms)*1_000_000}))?;
        Ok(labels)
    }

    fn append(&mut self, value: Value) -> Result<(), Error> {
        serde_json::to_writer(&mut self.file, &value).map_err(|e| config_error(e.to_string()))?;
        self.file.write_all(b"\n").at(&self.path)?;
        self.file.sync_data().at(&self.path)
    }

    fn stamp(&mut self, stamp: Stamp) -> Result<(), Error> {
        if self.finished {
            return Err(config_error("labels are already finished"));
        }
        if stamp.mono_ns < self.last_ns {
            return Err(config_error("label time moved backwards"));
        }
        self.last_ns = stamp.mono_ns;
        Ok(())
    }

    pub fn begin(
        &mut self,
        activity: &str,
        position: Option<&str>,
        stamp: Stamp,
    ) -> Result<(), Error> {
        if !self.plan.activities.iter().any(|a| a == activity) {
            return Err(config_error("activity is not declared in the trial plan"));
        }
        let position = position
            .map(|id| {
                self.plan
                    .positions
                    .iter()
                    .find(|p| p.id == id)
                    .cloned()
                    .ok_or_else(|| config_error("position is not declared in the trial plan"))
            })
            .transpose()?;
        self.stamp(stamp)?;
        if self.active.is_some() {
            self.end(stamp, "switch", false)?;
        }
        let id = self.next_id;
        self.append(
            json!({"type":"begin","id":id,"activity":activity,"position":position,
            "mono_ns":stamp.mono_ns,"wall_ns":stamp.wall_ns}),
        )?;
        self.active = Some(id);
        self.next_id += 1;
        Ok(())
    }

    pub fn end(&mut self, stamp: Stamp, reason: &str, truncated: bool) -> Result<(), Error> {
        let id = self
            .active
            .ok_or_else(|| config_error("no activity is open"))?;
        self.stamp(stamp)?;
        self.append(
            json!({"type":"end","id":id,"mono_ns":stamp.mono_ns,"wall_ns":stamp.wall_ns,
            "reason":reason,"truncated":truncated}),
        )?;
        self.active = None;
        Ok(())
    }

    pub fn note(&mut self, text: &str, stamp: Stamp) -> Result<(), Error> {
        if text.trim().is_empty() || text.len() > 4096 {
            return Err(config_error("note must be 1..=4096 bytes"));
        }
        self.stamp(stamp)?;
        self.append(
            json!({"type":"note","text":text,"mono_ns":stamp.mono_ns,"wall_ns":stamp.wall_ns}),
        )
    }

    pub fn finish(&mut self, stamp: Stamp, reason: &str) -> Result<(), Error> {
        self.stamp(stamp)?;
        if self.active.is_some() {
            self.end(stamp, reason, true)?;
        }
        self.append(
            json!({"type":"stop","mono_ns":stamp.mono_ns,"wall_ns":stamp.wall_ns,"reason":reason}),
        )?;
        self.finished = true;
        Ok(())
    }
}
