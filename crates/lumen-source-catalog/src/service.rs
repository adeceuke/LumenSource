use crate::{
    CacheLoadError, Catalog, CatalogCache, CatalogFetcher, CatalogLocation, LoadError,
    RemoteCatalogError, SignatureVerifier,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogSource {
    Remote,
    Cache,
}

pub struct CatalogService<F, V> {
    fetcher: F,
    verifier: V,
    cache: CatalogCache,
}

impl<F, V> CatalogService<F, V>
where
    F: CatalogFetcher,
    V: SignatureVerifier,
{
    pub fn new(fetcher: F, verifier: V, cache: CatalogCache) -> Self {
        Self {
            fetcher,
            verifier,
            cache,
        }
    }

    pub async fn load(
        &self,
        location: &CatalogLocation,
    ) -> Result<(Catalog, CatalogSource), LoadError> {
        match self.load_remote(location).await {
            Ok(catalog) => Ok((catalog, CatalogSource::Remote)),
            Err(remote) => match self.load_cache().await {
                Ok(catalog) => Ok((catalog, CatalogSource::Cache)),
                Err(cache) => Err(LoadError::RemoteAndCache { remote, cache }),
            },
        }
    }

    async fn load_remote(&self, location: &CatalogLocation) -> Result<Catalog, RemoteCatalogError> {
        let fetched = self.fetcher.fetch(location).await?;
        self.verifier
            .verify(&fetched.catalog_bytes, &fetched.detached_signature)?;
        let catalog = Catalog::from_slice(&fetched.catalog_bytes)?;
        self.cache.store(&fetched.catalog_bytes).await?;
        Ok(catalog)
    }

    async fn load_cache(&self) -> Result<Catalog, CacheLoadError> {
        let bytes = self.cache.load().await?;
        Ok(Catalog::from_slice(&bytes)?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::{
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use async_trait::async_trait;
    use tokio::fs;

    use crate::{FetchError, FetchedCatalog, VerificationError};

    use super::*;

    const VALID: &[u8] = include_bytes!("../../../catalog/fixtures/catalog.v1.valid.json");

    struct FakeFetcher {
        result: Result<FetchedCatalog, FetchError>,
    }

    #[async_trait]
    impl CatalogFetcher for FakeFetcher {
        async fn fetch(&self, _location: &CatalogLocation) -> Result<FetchedCatalog, FetchError> {
            match &self.result {
                Ok(fetched) => Ok(fetched.clone()),
                Err(FetchError::InsecureUrl(url)) => Err(FetchError::InsecureUrl(url.clone())),
                Err(_) => Err(FetchError::InsecureUrl("fake fetch failure".to_owned())),
            }
        }
    }

    struct AcceptingVerifier;

    impl SignatureVerifier for AcceptingVerifier {
        fn verify(
            &self,
            _message: &[u8],
            _detached_signature: &[u8],
        ) -> Result<(), VerificationError> {
            Ok(())
        }
    }

    fn cache_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lumen-source-service-{test_name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn location() -> CatalogLocation {
        CatalogLocation::new(
            "https://catalog.example/catalog.json",
            "https://catalog.example/catalog.json.sig",
        )
    }

    #[tokio::test]
    async fn caches_a_verified_remote_catalog() {
        let path = cache_path("remote");
        let cache = CatalogCache::new(&path);
        let service = CatalogService::new(
            FakeFetcher {
                result: Ok(FetchedCatalog {
                    catalog_bytes: VALID.to_vec(),
                    detached_signature: b"signature".to_vec(),
                }),
            },
            AcceptingVerifier,
            cache.clone(),
        );

        let (catalog, source) = service.load(&location()).await.unwrap();

        assert_eq!(source, CatalogSource::Remote);
        assert_eq!(catalog.schema_version, "1");
        assert_eq!(cache.load().await.unwrap(), VALID);
        fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn falls_back_to_last_known_good_when_fetch_fails() {
        let path = cache_path("fallback");
        let cache = CatalogCache::new(&path);
        cache.store(VALID).await.unwrap();
        let service = CatalogService::new(
            FakeFetcher {
                result: Err(FetchError::InsecureUrl("simulated failure".to_owned())),
            },
            AcceptingVerifier,
            cache,
        );

        let (catalog, source) = service.load(&location()).await.unwrap();

        assert_eq!(source, CatalogSource::Cache);
        assert_eq!(catalog.catalog_version, "2026.07.20.1");
        fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn returns_both_errors_when_remote_and_cache_fail() {
        let service = CatalogService::new(
            FakeFetcher {
                result: Err(FetchError::InsecureUrl("simulated failure".to_owned())),
            },
            AcceptingVerifier,
            CatalogCache::new(cache_path("both-fail")),
        );

        assert!(matches!(
            service.load(&location()).await,
            Err(LoadError::RemoteAndCache {
                remote: RemoteCatalogError::Fetch(_),
                cache: CacheLoadError::Cache(_)
            })
        ));
    }
}
