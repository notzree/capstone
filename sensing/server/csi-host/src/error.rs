use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Arrow export: {0}")]
    Arrow(#[from] arrow_schema::ArrowError),
    #[error("Parquet export: {0}")]
    Parquet(#[from] parquet::errors::ParquetError),
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{}: invalid manifest: {source}", path.display())]
    Manifest {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{}: {reason}", path.display())]
    Format { path: PathBuf, reason: String },
    #[error("{} already exists", .0.display())]
    Exists(PathBuf),
    #[error("invalid configuration: {0}")]
    Config(String),
}

pub(crate) trait IoContext<T> {
    fn at(self, path: &Path) -> Result<T, Error>;
}

impl<T> IoContext<T> for io::Result<T> {
    fn at(self, path: &Path) -> Result<T, Error> {
        self.map_err(|source| Error::Io {
            path: path.to_owned(),
            source,
        })
    }
}
