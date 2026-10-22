//! Installation geometry: where each sensing node is mounted. A revision never changes once
//! surveyed; moving a node means a re-survey and a new file with a new revision.
//!
//! ```json
//! {
//!   "schema": "csi-geometry/1",
//!   "room": "bedroom-a",
//!   "revision": "2026-10-20a",
//!   "units": "m",
//!   "origin": "south-west floor corner; x east, y north, z up",
//!   "nodes": [
//!     { "id": "a0:b7:65:dd:e9:3c", "x": 0.0, "y": 0.4, "z": 0.3, "tier": "floor",
//!       "antenna": { "azimuth_deg": 0.0, "elevation_deg": 0.0 }, "notes": "behind bed" }
//!   ]
//! }
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use csi_protocol::NodeId;
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::error::IoContext;

pub const SCHEMA: &str = "csi-geometry/1";
/// File name inside a session directory.
pub const GEOMETRY_FILE: &str = "geometry.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Geometry {
    pub schema: String,
    pub room: String,
    pub revision: String,
    /// Always `"m"`.
    pub units: String,
    /// Where (0, 0, 0) is and which way the axes point.
    pub origin: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    pub nodes: Vec<Placement>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    /// The node's factory MAC, as `csi_protocol::NodeId` displays it.
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub tier: Tier,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub antenna: Option<Antenna>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Floor,
    Upper,
}

/// The direction the antenna points: azimuth counter-clockwise from +x in the x–y plane,
/// elevation above that plane.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Antenna {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
}

impl Geometry {
    /// Parses and validates a geometry file's bytes.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let geometry: Self = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        geometry.validate()?;
        Ok(geometry)
    }

    pub fn load(path: &Path) -> Result<(Self, Vec<u8>), Error> {
        let bytes = fs::read(path).at(path)?;
        let geometry = Self::parse(&bytes).map_err(|reason| Error::Format {
            path: path.to_owned(),
            reason: format!("invalid geometry: {reason}"),
        })?;
        Ok((geometry, bytes))
    }

    pub fn node_ids(&self) -> BTreeSet<NodeId> {
        self.nodes
            .iter()
            .filter_map(|n| n.id.parse().ok())
            .collect()
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA {
            return Err(format!("schema must be {SCHEMA:?}, not {:?}", self.schema));
        }
        if self.units != "m" {
            return Err(format!("units must be \"m\", not {:?}", self.units));
        }
        for (field, value) in [
            ("room", &self.room),
            ("revision", &self.revision),
            ("origin", &self.origin),
        ] {
            if value.trim().is_empty() {
                return Err(format!("{field} must not be empty"));
            }
        }
        if self.nodes.is_empty() {
            return Err("no nodes".into());
        }
        let mut seen = BTreeSet::new();
        for node in &self.nodes {
            let id: NodeId = node
                .id
                .parse()
                .map_err(|e| format!("node id {:?}: {e}", node.id))?;
            if !seen.insert(id) {
                return Err(format!("node {id} appears twice"));
            }
            let angles = node
                .antenna
                .map_or([0.0; 2], |a| [a.azimuth_deg, a.elevation_deg]);
            if ![node.x, node.y, node.z]
                .iter()
                .chain(&angles)
                .all(|v| v.is_finite())
            {
                return Err(format!("node {id} has a non-finite coordinate or angle"));
            }
        }
        Ok(())
    }
}
