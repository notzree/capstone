//! A session is a directory holding `manifest.json` and append-only `segment-NNNNNN.bin` files.
//! Labels, events and split assignments belong in separate files beside them.

use std::collections::{BTreeSet, VecDeque};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use uuid::Uuid;

use crate::error::IoContext;
use crate::geometry::{GEOMETRY_FILE, Geometry};
use crate::manifest::{
    DerivedFrom, FORMAT, GeometryRef, Manifest, Rotation, SegmentInfo, SourceInfo,
};
use crate::segment::{self, SegmentEnd, SegmentReader, SegmentWriter, sha256_hex};
use crate::{Error, Record};

pub fn wall_now_ns() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64)
}

#[derive(Debug, Clone, Default)]
pub struct WriterOptions {
    pub rotation: Rotation,
    pub derived_from: Option<DerivedFrom>,
    /// A geometry file to validate and copy, byte for byte, into the session.
    pub geometry: Option<PathBuf>,
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Error> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .at(path)?;
    file.write_all(bytes).at(path)?;
    file.sync_all().at(path)
}

/// Records a session. Dropping it without [`finish`](Self::finish) leaves the session marked
/// incomplete with its last segment unsealed, the same state a crash leaves.
pub struct SessionWriter {
    dir: PathBuf,
    manifest: Manifest,
    segment: Option<SegmentWriter>,
}

impl SessionWriter {
    /// Creates `dir`, which must not exist.
    pub fn create(
        dir: &Path,
        sources: Vec<SourceInfo>,
        options: WriterOptions,
    ) -> Result<Self, Error> {
        if sources.len() > usize::from(u16::MAX) + 1 {
            return Err(Error::Config("more than 65536 sources".into()));
        }
        if dir.exists() {
            return Err(Error::Exists(dir.to_owned()));
        }
        let geometry = options
            .geometry
            .as_deref()
            .map(Geometry::load)
            .transpose()?;
        if let Some(parent) = dir.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).at(parent)?;
        }
        fs::create_dir(dir).at(dir)?;
        let geometry = match geometry {
            Some((geometry, bytes)) => {
                write_new(&dir.join(GEOMETRY_FILE), &bytes)?;
                Some(GeometryRef {
                    file: GEOMETRY_FILE.into(),
                    room: geometry.room,
                    revision: geometry.revision,
                    sha256: sha256_hex(&bytes),
                })
            }
            None => None,
        };
        let manifest = Manifest {
            format: FORMAT.into(),
            session_id: Uuid::now_v7(),
            created_wall_ns: wall_now_ns(),
            closed_wall_ns: None,
            complete: false,
            recorder: concat!("csi-host ", env!("CARGO_PKG_VERSION")).into(),
            protocol_version: csi_protocol::VERSION,
            derived_from: options.derived_from,
            geometry,
            rotation: options.rotation,
            sources,
            segments: Vec::new(),
        };
        manifest.store(dir)?;
        Ok(Self {
            dir: dir.to_owned(),
            manifest,
            segment: None,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn write(&mut self, record: &Record) -> Result<(), Error> {
        if usize::from(record.source) >= self.manifest.sources.len() {
            return Err(Error::Config(format!(
                "record source {} is not in the manifest",
                record.source
            )));
        }
        let rotation = self.manifest.rotation;
        let rotate = self.segment.as_ref().is_some_and(|s| {
            let first = s.info.first_mono_ns.unwrap_or(record.mono_ns);
            s.info.records > 0
                && (s.info.bytes + segment::record_len(record) > rotation.max_bytes
                    || record.mono_ns.saturating_sub(first) >= rotation.max_duration_ns)
        });
        if rotate {
            self.seal_segment()?;
        }
        let segment = match &mut self.segment {
            Some(segment) => segment,
            None => self.segment.insert(SegmentWriter::create(
                &self.dir,
                segment::file_name(self.manifest.segments.len()),
            )?),
        };
        segment.write(record)
    }

    pub fn flush(&mut self) -> Result<(), Error> {
        self.segment.as_mut().map_or(Ok(()), SegmentWriter::flush)
    }

    fn seal_segment(&mut self) -> Result<(), Error> {
        if let Some(segment) = self.segment.take() {
            self.manifest.segments.push(segment.seal()?);
            self.manifest.store(&self.dir)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<Manifest, Error> {
        self.seal_segment()?;
        self.manifest.complete = true;
        self.manifest.closed_wall_ns = Some(wall_now_ns());
        self.manifest.store(&self.dir)?;
        Ok(self.manifest)
    }
}

/// Result of reading one segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SegmentCheck {
    pub file: String,
    /// Listed in the manifest with a hash.
    pub sealed: bool,
    pub records: u64,
    pub bytes: u64,
    pub integrity: Integrity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Integrity {
    /// Sealed, and the hash and record count match the manifest.
    Verified,
    /// Not sealed (the recorder did not close it) but every record is whole.
    Unsealed,
    /// Sealed, but the contents differ from the manifest.
    Mismatch,
    Missing,
    Torn {
        offset: u64,
        bytes: u64,
    },
    Corrupt {
        offset: u64,
        reason: String,
    },
}

/// Reads a session's records in order: sealed segments first, then any unsealed segment files a
/// crash left behind.
pub struct SessionReader {
    dir: PathBuf,
    manifest: Manifest,
    pending: VecDeque<(String, Option<SegmentInfo>)>,
    current: Option<(SegmentReader, String, Option<SegmentInfo>)>,
    checks: Vec<SegmentCheck>,
    failed: bool,
}

impl SessionReader {
    pub fn open(dir: &Path) -> Result<Self, Error> {
        let manifest = Manifest::load(dir)?;
        let listed: BTreeSet<&str> = manifest.segments.iter().map(|s| s.file.as_str()).collect();
        let mut unlisted = Vec::new();
        for entry in fs::read_dir(dir).at(dir)? {
            let name = entry.at(dir)?.file_name().to_string_lossy().into_owned();
            if name.starts_with("segment-")
                && name.ends_with(".bin")
                && !listed.contains(name.as_str())
            {
                unlisted.push(name);
            }
        }
        unlisted.sort();
        let pending = manifest
            .segments
            .iter()
            .map(|s| (s.file.clone(), Some(s.clone())))
            .chain(unlisted.into_iter().map(|file| (file, None)))
            .collect();
        Ok(Self {
            dir: dir.to_owned(),
            manifest,
            pending,
            current: None,
            checks: Vec::new(),
            failed: false,
        })
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The session's installation geometry, checked against the manifest's hash.
    pub fn geometry(&self) -> Result<Option<Geometry>, Error> {
        let Some(reference) = &self.manifest.geometry else {
            return Ok(None);
        };
        let path = self.dir.join(&reference.file);
        let format_error = |reason: String| Error::Format {
            path: path.clone(),
            reason,
        };
        if Path::new(&reference.file).file_name() != Some(reference.file.as_ref()) {
            return Err(format_error(
                "geometry file must be inside the session".into(),
            ));
        }
        let bytes = fs::read(&path).at(&path)?;
        if sha256_hex(&bytes) != reference.sha256 {
            return Err(format_error(
                "does not match the manifest's geometry hash".into(),
            ));
        }
        Geometry::parse(&bytes)
            .map(Some)
            .map_err(|reason| format_error(format!("invalid geometry: {reason}")))
    }

    /// One entry per segment read so far; complete once iteration ends.
    pub fn checks(&self) -> &[SegmentCheck] {
        &self.checks
    }

    fn check(
        file: String,
        expected: Option<SegmentInfo>,
        summary: segment::SegmentSummary,
    ) -> SegmentCheck {
        let integrity = match (&summary.end, &expected) {
            (SegmentEnd::Torn { offset, bytes }, _) => Integrity::Torn {
                offset: *offset,
                bytes: *bytes,
            },
            (SegmentEnd::Corrupt { offset, reason }, _) => Integrity::Corrupt {
                offset: *offset,
                reason: reason.clone(),
            },
            (SegmentEnd::Clean, None) => Integrity::Unsealed,
            (SegmentEnd::Clean, Some(info)) => {
                if info.sha256 == summary.sha256 && info.records == summary.records {
                    Integrity::Verified
                } else {
                    Integrity::Mismatch
                }
            }
        };
        SegmentCheck {
            file,
            sealed: expected.is_some(),
            records: summary.records,
            bytes: summary.bytes,
            integrity,
        }
    }
}

impl Iterator for SessionReader {
    type Item = Result<Record, Error>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.failed {
            let Some((reader, ..)) = &mut self.current else {
                let (file, expected) = self.pending.pop_front()?;
                let path = self.dir.join(&file);
                if !path.exists() {
                    self.checks.push(SegmentCheck {
                        file,
                        sealed: expected.is_some(),
                        records: 0,
                        bytes: 0,
                        integrity: Integrity::Missing,
                    });
                    continue;
                }
                match SegmentReader::open(&path) {
                    Ok(reader) => self.current = Some((reader, file, expected)),
                    Err(Error::Format { reason, .. }) => self.checks.push(SegmentCheck {
                        file,
                        sealed: expected.is_some(),
                        records: 0,
                        bytes: 0,
                        integrity: Integrity::Corrupt { offset: 0, reason },
                    }),
                    Err(e) => {
                        self.failed = true;
                        return Some(Err(e));
                    }
                }
                continue;
            };
            match reader.next() {
                Ok(Some(record)) => return Some(Ok(record)),
                Ok(None) => {
                    let (reader, file, expected) = self.current.take()?;
                    self.checks
                        .push(Self::check(file, expected, reader.finish()));
                }
                Err(e) => {
                    self.failed = true;
                    return Some(Err(e));
                }
            }
        }
        None
    }
}
