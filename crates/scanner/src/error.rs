use std::path::PathBuf;

/// Why a metadata reader could not read a file. Readers fall back to
/// lower-confidence sources where they can, so this is usually per source.
#[derive(Debug, thiserror::Error)]
pub enum ReadError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: not a readable PE file: {message}")]
    Pe { path: PathBuf, message: String },
    #[error("{path}: no version resource")]
    NoVersionInfo { path: PathBuf },
    #[error("{path}: invalid Info.plist: {message}")]
    Plist { path: PathBuf, message: String },
    #[error("{path}: invalid moduleinfo.json: {message}")]
    ModuleInfo { path: PathBuf, message: String },
}
