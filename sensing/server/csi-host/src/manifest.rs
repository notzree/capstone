//! `manifest.json`: what a session contains and how to trust it. Rewritten atomically whenever a
//! segment is sealed and when the session closes.

use std::fs::{self, File};
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::Error;
use crate::error::IoContext;

pub const FORMAT: &str = "csi-session/1";
pub const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub session_id: Uuid,
    pub created_wall_ns: u64,
    pub closed_wall_ns: Option<u64>,
    /// False until the recorder closes cleanly. An incomplete session may have an unsealed final
    /// segment that is not listed in `segments`.
    pub complete: bool,
    pub recorder: String,
    pub protocol_version: u8,
    #[serde(default)]
    pub derived_from: Option<DerivedFrom>,
    /// The installation geometry copied into the session, if one was given.
    #[serde(default)]
    pub geometry: Option<GeometryRef>,
    pub rotation: Rotation,
    pub sources: Vec<SourceInfo>,
    /// Sealed segments, in order.
    pub segments: Vec<SegmentInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DerivedFrom {
    pub session_id: Uuid,
    pub how: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeometryRef {
    /// A file name inside the session directory.
    pub file: String,
    pub room: String,
    pub revision: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rotation {
    pub max_bytes: u64,
    pub max_duration_ns: u64,
}

impl Default for Rotation {
    fn default() -> Self {
        Self {
            max_bytes: 256 << 20,
            max_duration_ns: 600_000_000_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub details: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentInfo {
    pub file: String,
    pub records: u64,
    pub bytes: u64,
    pub sha256: String,
    pub first_mono_ns: Option<u64>,
    pub last_mono_ns: Option<u64>,
}

impl Manifest {
    pub fn load(dir: &Path) -> Result<Self, Error> {
        let path = dir.join(MANIFEST_FILE);
        let text = fs::read(&path).at(&path)?;
        let manifest: Self = serde_json::from_slice(&text).map_err(|source| Error::Manifest {
            path: path.clone(),
            source,
        })?;
        if manifest.format != FORMAT {
            return Err(Error::Format {
                path,
                reason: format!("unsupported session format {:?}", manifest.format),
            });
        }
        Ok(manifest)
    }

    pub(crate) fn store(&self, dir: &Path) -> Result<(), Error> {
        let path = dir.join(MANIFEST_FILE);
        let tmp = dir.join(format!("{MANIFEST_FILE}.tmp"));
        let mut text = serde_json::to_vec_pretty(self).map_err(|source| Error::Manifest {
            path: path.clone(),
            source,
        })?;
        text.push(b'\n');
        let mut file = File::create(&tmp).at(&tmp)?;
        file.write_all(&text).at(&tmp)?;
        file.sync_all().at(&tmp)?;
        fs::rename(&tmp, &path).at(&path)?;
        File::open(dir).and_then(|d| d.sync_all()).at(dir)
    }
}
