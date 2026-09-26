use crate::bucket::{CacheError, FileCache};
use std::sync::Arc;

/// Resolves a playlist entry's `filename` to a local filesystem path.
///
/// A `s3://`-prefixed filename is an R2 object key: the prefix is stripped
/// and the remainder is fetched into the file cache if not already cached.
/// Anything else is joined with the content path directly.
#[derive(Clone)]
pub struct FileResolver {
    content_path: String,
    file_cache: Arc<FileCache>,
}

impl FileResolver {
    pub fn new(content_path: String, file_cache: Arc<FileCache>) -> Self {
        Self {
            content_path,
            file_cache,
        }
    }

    pub async fn resolve(&self, filename: &str) -> Result<String, CacheError> {
        if let Some(key) = filename.strip_prefix("s3://") {
            Ok(self
                .file_cache
                .ensure_cached(key)
                .await?
                .to_string_lossy()
                .to_string())
        } else {
            Ok(format!("{}/{filename}", self.content_path))
        }
    }

    pub async fn resolve_contents(&self, filename: &str) -> Result<Vec<u8>, CacheError> {
        if let Some(key) = filename.strip_prefix("s3://") {
            self.file_cache.fetch_bytes(key).await
        } else {
            tokio::fs::read(format!("{}/{filename}", self.content_path))
                .await
                .map_err(|e| CacheError::Local(Arc::new(e)))
        }
    }

    /// Empty when no bucket is configured or the listing request fails.
    pub async fn list_remote(&self, prefix: &str) -> Vec<String> {
        self.file_cache.list_remote(prefix).await
    }

    /// Lists files (as s3:// URIs) whose key matches `partial` under `prefix`.
    pub async fn list_content(&self, prefix: &str, partial: &str) -> Vec<String> {
        let partial = partial.strip_prefix("s3://").unwrap_or(partial);
        let partial = partial.strip_prefix(prefix).unwrap_or(partial);
        self.list_remote(&format!("{prefix}{partial}"))
            .await
            .into_iter()
            .map(|key| format!("s3://{key}"))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatches_on_the_s3_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let file_cache = Arc::new(
            FileCache::new(dir.path().to_path_buf(), None, None)
                .await
                .unwrap(),
        );
        let resolver = FileResolver::new(dir.path().to_string_lossy().to_string(), file_cache);

        assert!(resolver.resolve("s3://tracks/song.ogg").await.is_err());
        assert!(
            resolver
                .resolve_contents("s3://tracks/song.ogg")
                .await
                .is_err()
        );

        std::fs::create_dir_all(dir.path().join("audio")).unwrap();
        std::fs::write(dir.path().join("audio/song.ogg"), b"hello world").unwrap();

        let resolved = resolver.resolve("audio/song.ogg").await.unwrap();
        assert_eq!(resolved, format!("{}/audio/song.ogg", dir.path().display()));

        let contents = resolver.resolve_contents("audio/song.ogg").await.unwrap();
        assert_eq!(contents, b"hello world");
    }
}
