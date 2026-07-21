use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("catalog JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported catalog schema version `{found}`; expected `{expected}`")]
    UnsupportedSchemaVersion {
        expected: &'static str,
        found: String,
    },
}

#[derive(Debug, Error)]
pub enum FetchError {
    #[error("catalog URL must use HTTPS: {0}")]
    InsecureUrl(String),
    #[error("invalid catalog URL `{url}`: {reason}")]
    InvalidUrl { url: String, reason: String },
    #[error("request for `{url}` failed: {source}")]
    Request {
        url: String,
        #[source]
        source: reqwest::Error,
    },
}

#[derive(Debug, Error)]
pub enum VerificationError {
    #[error("catalog signature is not UTF-8 base64 text")]
    InvalidSignatureEncoding,
    #[error("catalog signature is not valid base64: {0}")]
    InvalidEncoding(#[from] base64::DecodeError),
    #[error("catalog signature has an invalid length: {0}")]
    InvalidSignatureLength(#[from] ed25519_dalek::SignatureError),
    #[error("catalog signature verification failed")]
    InvalidSignature,
    #[error("Ed25519 public key is invalid: {0}")]
    InvalidPublicKey(ed25519_dalek::SignatureError),
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache has not been populated at `{0}`")]
    NotFound(PathBuf),
    #[error("failed to read catalog cache `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to create catalog cache directory `{path}`: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to write temporary catalog cache `{path}`: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to atomically replace catalog cache `{path}`: {source}")]
    Replace {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("catalog cache path has no parent: `{0}`")]
    MissingParent(PathBuf),
    #[error("system clock is before the Unix epoch: {0}")]
    Clock(#[from] std::time::SystemTimeError),
}

#[derive(Debug, Error)]
pub enum RemoteCatalogError {
    #[error(transparent)]
    Fetch(#[from] FetchError),
    #[error(transparent)]
    Verification(#[from] VerificationError),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Cache(#[from] CacheError),
}

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("remote catalog failed ({remote}); cached catalog also failed ({cache})")]
    RemoteAndCache {
        remote: RemoteCatalogError,
        cache: CacheLoadError,
    },
}

#[derive(Debug, Error)]
pub enum CacheLoadError {
    #[error(transparent)]
    Cache(#[from] CacheError),
    #[error(transparent)]
    Catalog(#[from] CatalogError),
}
