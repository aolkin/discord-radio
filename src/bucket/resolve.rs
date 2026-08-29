use crate::bucket::FileCache;

/// Resolves a playlist entry's `filename` to a local filesystem path.
///
/// A `s3://`-prefixed filename is an R2 object key: the prefix is stripped
/// and the remainder is fetched into `file_cache` if not already cached.
/// Anything else is joined with `content_path` directly.
pub async fn resolve_track_path(
    filename: &str,
    content_path: &str,
    file_cache: &FileCache,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(key) = filename.strip_prefix("s3://") {
        Ok(file_cache
            .ensure_cached(key)
            .await?
            .to_string_lossy()
            .to_string())
    } else {
        Ok(format!("{content_path}/{filename}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatches_on_the_s3_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let file_cache = FileCache::new(dir.path().to_path_buf(), None, None)
            .await
            .unwrap();

        // s3://-prefixed: dispatched into the (unconfigured) cache, which fails.
        assert!(
            resolve_track_path("s3://tracks/song.ogg", "content", &file_cache)
                .await
                .is_err()
        );

        // Everything else: dispatched to the local-path branch, never touching the cache.
        let resolved = resolve_track_path("audio/song.ogg", "content", &file_cache)
            .await
            .unwrap();
        assert_eq!(resolved, "content/audio/song.ogg");
    }
}
