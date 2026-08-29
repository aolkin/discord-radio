use crate::bucket::FileCache;

/// Resolves a playlist entry's `filename` to a local filesystem path.
///
/// Filenames prefixed with `s3://` are R2 object keys: the prefix is
/// stripped and the remainder is downloaded into `file_cache` (if not
/// already cached), returning the local cache path. Any other filename is
/// treated as a path relative to `content_path`, unchanged from how
/// playlists have always resolved local files. This lets existing playlists
/// keep working while new ones route through R2 without a migration.
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
