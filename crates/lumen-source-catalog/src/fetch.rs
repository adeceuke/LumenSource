use async_trait::async_trait;

use crate::FetchError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogLocation {
    pub catalog_url: String,
    pub signature_url: String,
}

impl CatalogLocation {
    pub fn new(catalog_url: impl Into<String>, signature_url: impl Into<String>) -> Self {
        Self {
            catalog_url: catalog_url.into(),
            signature_url: signature_url.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedCatalog {
    pub catalog_bytes: Vec<u8>,
    pub detached_signature: Vec<u8>,
}

#[async_trait]
pub trait CatalogFetcher: Send + Sync {
    async fn fetch(&self, location: &CatalogLocation) -> Result<FetchedCatalog, FetchError>;
}

#[derive(Debug, Clone)]
pub struct ReqwestCatalogFetcher {
    client: reqwest::Client,
}

impl ReqwestCatalogFetcher {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    async fn get_bytes(&self, raw_url: &str) -> Result<Vec<u8>, FetchError> {
        let url = reqwest::Url::parse(raw_url).map_err(|source| FetchError::InvalidUrl {
            url: raw_url.to_owned(),
            reason: source.to_string(),
        })?;
        if url.scheme() != "https" {
            return Err(FetchError::InsecureUrl(raw_url.to_owned()));
        }

        let response = self
            .client
            .get(url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|source| FetchError::Request {
                url: raw_url.to_owned(),
                source,
            })?;
        let bytes = response
            .bytes()
            .await
            .map_err(|source| FetchError::Request {
                url: raw_url.to_owned(),
                source,
            })?;
        Ok(bytes.to_vec())
    }
}

impl Default for ReqwestCatalogFetcher {
    fn default() -> Self {
        Self::new(reqwest::Client::new())
    }
}

#[async_trait]
impl CatalogFetcher for ReqwestCatalogFetcher {
    async fn fetch(&self, location: &CatalogLocation) -> Result<FetchedCatalog, FetchError> {
        let (catalog, signature) = tokio::join!(
            self.get_bytes(&location.catalog_url),
            self.get_bytes(&location.signature_url)
        );
        Ok(FetchedCatalog {
            catalog_bytes: catalog?,
            detached_signature: signature?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejects_non_https_urls_before_requesting() {
        let fetcher = ReqwestCatalogFetcher::new(reqwest::Client::new());
        let result = fetcher
            .get_bytes("http://catalog.example/catalog.json")
            .await;

        assert!(matches!(result, Err(FetchError::InsecureUrl(_))));
    }
}
