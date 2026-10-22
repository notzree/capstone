//! Segment files: an 8-byte header (`CSISEG` and a `u16` format version), then records.
//!
//! ```text
//! record
//! offset  size  field
//! 0       4     payload_len
//! 4       8     mono_ns
//! 12      8     wall_ns
//! 20      2     source
//! 22      1     kind (1 frame, 2 stream error)
//! 23      1     reserved, 0
//! 24      n     payload
//! ```
//!
//! All integers are little-endian. A crash can leave a torn final record; readers stop there and
//! report it rather than failing the session.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::error::IoContext;
use crate::manifest::SegmentInfo;
use crate::{Error, Record, RecordKind};

pub const MAGIC: [u8; 6] = *b"CSISEG";
pub const VERSION: u16 = 1;
pub const HEADER_LEN: usize = 8;
pub const RECORD_HEADER_LEN: usize = 24;
pub const MAX_PAYLOAD_LEN: usize = 1 << 20;

pub(crate) fn file_name(index: usize) -> String {
    format!("segment-{index:06}.bin")
}

pub(crate) fn record_len(record: &Record) -> u64 {
    (RECORD_HEADER_LEN + record.payload.len()) as u64
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub(crate) struct SegmentWriter {
    path: PathBuf,
    out: BufWriter<File>,
    hasher: Sha256,
    pub(crate) info: SegmentInfo,
}

impl SegmentWriter {
    pub(crate) fn create(dir: &Path, file: String) -> Result<Self, Error> {
        let path = dir.join(&file);
        let handle = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .at(&path)?;
        let mut writer = Self {
            path,
            out: BufWriter::new(handle),
            hasher: Sha256::new(),
            info: SegmentInfo {
                file,
                records: 0,
                bytes: 0,
                sha256: String::new(),
                first_mono_ns: None,
                last_mono_ns: None,
            },
        };
        let mut header = [0; HEADER_LEN];
        header[..6].copy_from_slice(&MAGIC);
        header[6..].copy_from_slice(&VERSION.to_le_bytes());
        writer.put(&header)?;
        Ok(writer)
    }

    fn put(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.out.write_all(bytes).at(&self.path)?;
        self.hasher.update(bytes);
        self.info.bytes += bytes.len() as u64;
        Ok(())
    }

    pub(crate) fn write(&mut self, record: &Record) -> Result<(), Error> {
        let len = u32::try_from(record.payload.len())
            .ok()
            .filter(|&n| n as usize <= MAX_PAYLOAD_LEN)
            .ok_or_else(|| Error::Format {
                path: self.path.clone(),
                reason: format!(
                    "record payload of {} bytes is too large",
                    record.payload.len()
                ),
            })?;
        let mut header = [0; RECORD_HEADER_LEN];
        header[0..4].copy_from_slice(&len.to_le_bytes());
        header[4..12].copy_from_slice(&record.mono_ns.to_le_bytes());
        header[12..20].copy_from_slice(&record.wall_ns.to_le_bytes());
        header[20..22].copy_from_slice(&record.source.to_le_bytes());
        header[22] = record.kind.code();
        self.put(&header)?;
        self.put(&record.payload)?;
        self.info.records += 1;
        self.info.first_mono_ns.get_or_insert(record.mono_ns);
        self.info.last_mono_ns = Some(record.mono_ns);
        Ok(())
    }

    pub(crate) fn flush(&mut self) -> Result<(), Error> {
        self.out.flush().at(&self.path)
    }

    pub(crate) fn seal(mut self) -> Result<SegmentInfo, Error> {
        self.flush()?;
        self.out.get_ref().sync_all().at(&self.path)?;
        self.info.sha256 = hex(&self.hasher.finalize());
        Ok(self.info)
    }
}

/// How a segment's records ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "end", rename_all = "snake_case")]
pub enum SegmentEnd {
    Clean,
    /// The file ends partway through a record.
    Torn {
        offset: u64,
        bytes: u64,
    },
    /// A record header is invalid; nothing after it was read.
    Corrupt {
        offset: u64,
        reason: String,
    },
}

pub(crate) struct SegmentReader {
    path: PathBuf,
    input: BufReader<File>,
    hasher: Sha256,
    offset: u64,
    records: u64,
    end: Option<SegmentEnd>,
}

pub(crate) struct SegmentSummary {
    pub(crate) records: u64,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
    pub(crate) end: SegmentEnd,
}

impl SegmentReader {
    pub(crate) fn open(path: &Path) -> Result<Self, Error> {
        let input = BufReader::new(File::open(path).at(path)?);
        let mut reader = Self {
            path: path.to_owned(),
            input,
            hasher: Sha256::new(),
            offset: 0,
            records: 0,
            end: None,
        };
        let mut header = [0; HEADER_LEN];
        let n = reader.fill(&mut header)?;
        let format_error = |reason: &str| Error::Format {
            path: path.to_owned(),
            reason: reason.to_owned(),
        };
        if n < HEADER_LEN || header[..6] != MAGIC {
            return Err(format_error("not a segment file"));
        }
        let version = u16::from_le_bytes([header[6], header[7]]);
        if version != VERSION {
            return Err(format_error(&format!(
                "unsupported segment version {version}"
            )));
        }
        Ok(reader)
    }

    /// Reads until `buf` is full or the file ends; returns the bytes read.
    fn fill(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        let mut filled = 0;
        while filled < buf.len() {
            match self.input.read(&mut buf[filled..]) {
                Ok(0) => break,
                Ok(n) => filled += n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e).at(&self.path),
            }
        }
        self.hasher.update(&buf[..filled]);
        self.offset += filled as u64;
        Ok(filled)
    }

    pub(crate) fn next(&mut self) -> Result<Option<Record>, Error> {
        if self.end.is_some() {
            return Ok(None);
        }
        let start = self.offset;
        let mut header = [0; RECORD_HEADER_LEN];
        let n = self.fill(&mut header)?;
        if n == 0 {
            self.end = Some(SegmentEnd::Clean);
            return Ok(None);
        }
        if n < RECORD_HEADER_LEN {
            self.end = Some(SegmentEnd::Torn {
                offset: start,
                bytes: n as u64,
            });
            return Ok(None);
        }
        let field = |range: std::ops::Range<usize>| &header[range];
        let len = u32::from_le_bytes(field(0..4).try_into().expect("4 bytes")) as usize;
        if len > MAX_PAYLOAD_LEN || header[23] != 0 {
            self.end = Some(SegmentEnd::Corrupt {
                offset: start,
                reason: format!("invalid record header {header:02x?}"),
            });
            return Ok(None);
        }
        let mut payload = vec![0; len];
        let got = self.fill(&mut payload)?;
        if got < len {
            self.end = Some(SegmentEnd::Torn {
                offset: start,
                bytes: (n + got) as u64,
            });
            return Ok(None);
        }
        self.records += 1;
        Ok(Some(Record {
            mono_ns: u64::from_le_bytes(field(4..12).try_into().expect("8 bytes")),
            wall_ns: u64::from_le_bytes(field(12..20).try_into().expect("8 bytes")),
            source: u16::from_le_bytes(field(20..22).try_into().expect("2 bytes")),
            kind: RecordKind::from_code(header[22]),
            payload,
        }))
    }

    pub(crate) fn finish(self) -> SegmentSummary {
        SegmentSummary {
            records: self.records,
            bytes: self.offset,
            sha256: hex(&self.hasher.finalize()),
            end: self.end.unwrap_or(SegmentEnd::Clean),
        }
    }
}
