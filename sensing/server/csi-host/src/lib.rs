//! Host-side acquisition: ingest from sensing nodes and local receivers, session recording and
//! replay, per-link health, and batch delivery to the Python pipeline.

pub mod clock;
#[cfg(unix)]
pub mod delivery;
mod error;
pub mod export;
pub mod geometry;
pub mod histogram;
pub mod inspect;
pub mod layout;
pub mod live;
pub mod manifest;
pub mod pace;
mod record;
pub mod segment;
pub mod seq;
pub mod session;
#[cfg(unix)]
pub mod star;
pub mod synthetic;
pub mod trial;

pub use error::Error;
pub use record::{Record, RecordKind, StreamError};
