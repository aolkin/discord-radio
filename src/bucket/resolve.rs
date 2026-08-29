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
    use async_trait::async_trait;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubDownloader {
        calls: Arc<AtomicUsize>,
        payload: Vec<u8>,
    }

    #[async_trait]
    impl crate::bucket::cache::ObjectDownloader for StubDownloader {
        async fn download(
            &self,
            _key: &str,
        ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.payload.clone())
        }
    }

    #[tokio::test]
    async fn s3_prefixed_filenames_route_through_the_file_cache() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = Arc::new(StubDownloader {
            calls: calls.clone(),
            payload: b"hello world".to_vec(),
        });
        let file_cache = FileCache::with_downloader(dir.path().to_path_buf(), downloader).await;

        let resolved = resolve_track_path("s3://tracks/music/60s/song.ogg", "content", &file_cache)
            .await
            .unwrap();

        assert_eq!(
            resolved,
            dir.path()
                .join("tracks/music/60s/song.ogg")
                .to_string_lossy()
                .to_string()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn other_filenames_join_the_content_path_without_touching_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = Arc::new(StubDownloader {
            calls: calls.clone(),
            payload: b"unused".to_vec(),
        });
        let file_cache = FileCache::with_downloader(dir.path().to_path_buf(), downloader).await;

        let resolved = resolve_track_path("audio/radio-favs/song.ogg", "content", &file_cache)
            .await
            .unwrap();

        assert_eq!(resolved, "content/audio/radio-favs/song.ogg");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
