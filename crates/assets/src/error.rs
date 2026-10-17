use std::path::{Path, PathBuf};

/// Errors from the asset readers. Every variant carries the file it came from
/// so a broken or unexpected install is easy to diagnose.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: XML: {source}")]
    Xml {
        path: PathBuf,
        #[source]
        source: roxmltree::Error,
    },
    #[error("{path}: PNG: {source}")]
    Png {
        path: PathBuf,
        #[source]
        source: png::DecodingError,
    },
    #[error("{path}: {msg}")]
    Format { path: PathBuf, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn io(path: &Path, source: std::io::Error) -> Self {
        Error::Io {
            path: path.to_owned(),
            source,
        }
    }

    pub fn format(path: &Path, msg: impl Into<String>) -> Self {
        Error::Format {
            path: path.to_owned(),
            msg: msg.into(),
        }
    }
}

/// Reads a whole file, mapping the error to [`Error::Io`].
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error::io(path, e))
}

/// Reads a text file (UTF-8, optional BOM stripped).
pub fn read_text(path: &Path) -> Result<String> {
    let bytes = read_file(path)?;
    let s = String::from_utf8(bytes).map_err(|_| Error::format(path, "not valid UTF-8"))?;
    Ok(match s.strip_prefix('\u{feff}') {
        Some(rest) => rest.to_owned(),
        None => s,
    })
}
