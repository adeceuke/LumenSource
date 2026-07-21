use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::{
    fs::{self, OpenOptions},
    io::AsyncWriteExt,
};

use crate::CacheError;

#[derive(Debug, Clone)]
pub struct CatalogCache {
    path: PathBuf,
}

impl CatalogCache {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn load(&self) -> Result<Vec<u8>, CacheError> {
        fs::read(&self.path).await.map_err(|source| {
            if source.kind() == ErrorKind::NotFound {
                CacheError::NotFound(self.path.clone())
            } else {
                CacheError::Read {
                    path: self.path.clone(),
                    source,
                }
            }
        })
    }

    pub async fn store(&self, catalog_bytes: &[u8]) -> Result<(), CacheError> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| CacheError::MissingParent(self.path.clone()))?;
        let parent = if parent.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent
        };
        fs::create_dir_all(parent)
            .await
            .map_err(|source| CacheError::CreateDirectory {
                path: parent.to_path_buf(),
                source,
            })?;

        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let temporary_path = self
            .path
            .with_extension(format!("tmp-{}-{nonce}", std::process::id()));
        let write_result = self.write_temporary(&temporary_path, catalog_bytes).await;
        if let Err(error) = write_result {
            let _ = fs::remove_file(&temporary_path).await;
            return Err(error);
        }

        if let Err(source) = fs::rename(&temporary_path, &self.path).await {
            let _ = fs::remove_file(&temporary_path).await;
            return Err(CacheError::Replace {
                path: self.path.clone(),
                source,
            });
        }
        Ok(())
    }

    async fn write_temporary(
        &self,
        temporary_path: &Path,
        catalog_bytes: &[u8],
    ) -> Result<(), CacheError> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(temporary_path)
            .await
            .map_err(|source| CacheError::Write {
                path: temporary_path.to_path_buf(),
                source,
            })?;
        file.write_all(catalog_bytes)
            .await
            .map_err(|source| CacheError::Write {
                path: temporary_path.to_path_buf(),
                source,
            })?;
        file.flush().await.map_err(|source| CacheError::Write {
            path: temporary_path.to_path_buf(),
            source,
        })?;
        file.sync_all().await.map_err(|source| CacheError::Write {
            path: temporary_path.to_path_buf(),
            source,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn cache_path(test_name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "lumen-source-catalog-{test_name}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn atomically_replaces_last_known_good_bytes() {
        let path = cache_path("replace");
        let cache = CatalogCache::new(&path);

        cache.store(b"first").await.unwrap();
        cache.store(b"second").await.unwrap();

        assert_eq!(cache.load().await.unwrap(), b"second");
        fs::remove_file(path).await.unwrap();
    }

    #[tokio::test]
    async fn reports_an_empty_cache_explicitly() {
        let cache = CatalogCache::new(cache_path("missing"));

        assert!(matches!(cache.load().await, Err(CacheError::NotFound(_))));
    }
}
