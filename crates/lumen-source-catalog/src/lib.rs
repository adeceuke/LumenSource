//! Catalog schema, retrieval, authenticity verification, and last-known-good caching.

mod cache;
mod error;
mod fetch;
mod schema;
mod service;
mod verify;

pub use cache::CatalogCache;
pub use error::{
    CacheError, CacheLoadError, CatalogError, FetchError, LoadError, RemoteCatalogError,
    VerificationError,
};
pub use fetch::{CatalogFetcher, CatalogLocation, FetchedCatalog, ReqwestCatalogFetcher};
pub use schema::{
    Accelerator, Artifact, Catalog, Install, InstallStrategy, License, ModelEntry, ModelVariant,
    OperatingSystem, PerformanceHint, Platform, Requirements, RuntimeEntry, CURRENT_SCHEMA_VERSION,
};
pub use service::{CatalogService, CatalogSource};
pub use verify::{Ed25519Verifier, SignatureVerifier};
