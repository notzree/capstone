//! Host-side acquisition: ingest from sensing nodes and local receivers, session recording and
//! replay, per-link health, and batch delivery to the Python pipeline.

pub mod clock;
mod error;
pub mod geometry;
pub mod histogram;
pub mod inspect;
pub mod live;
pub mod manifest;
pub mod pace;
mod record;
pub mod segment;
pub mod seq;
pub mod session;
pub mod synthetic;

pub use error::Error;
pub use record::{Record, RecordKind, StreamError};
