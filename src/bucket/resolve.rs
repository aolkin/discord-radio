use crate::bucket::FileCache;
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

    pub async fn resolve(
        &self,
        filename: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
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
        let resolver = FileResolver::new("content".to_string(), file_cache);

        // s3://-prefixed: dispatched into the (unconfigured) cache, which fails.
        assert!(resolver.resolve("s3://tracks/song.ogg").await.is_err());

        // Everything else: dispatched to the local-path branch, never touching the cache.
        let resolved = resolver.resolve("audio/song.ogg").await.unwrap();
        assert_eq!(resolved, "content/audio/song.ogg");
    }
}
