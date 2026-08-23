// Nothing outside this module and its tests calls into FileCache yet; the
// resolver that will (OLK-114) lands in a later PR.
#![allow(dead_code)]

use async_trait::async_trait;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;

const MAX_CONCURRENT_PRE_CACHE_DOWNLOADS: usize = 4;

/// Evicting an entry costs at most one redundant filesystem check.
const MAX_TRACKED_KEYS: u64 = 10_000;

#[derive(Clone, Debug, thiserror::Error)]
pub enum CacheError {
    #[error("object storage is not configured")]
    NotConfigured,
    #[error("remote download failed: {0}")]
    Remote(Arc<dyn Error + Send + Sync>),
    #[error("local cache i/o failed: {0}")]
    Local(Arc<std::io::Error>),
}

/// Downloads a single object's bytes from remote storage. Exists so tests can
/// substitute a stub for a real bucket without real credentials.
#[async_trait]
trait ObjectDownloader: Send + Sync {
    async fn download(&self, key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;
}

#[async_trait]
impl ObjectDownloader for s3::Bucket {
    async fn download(&self, key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let response = self.get_object(key).await?;

        let status = response.status_code();
        if !(200..300).contains(&status) {
            return Err(format!("unexpected status {status} fetching {key}").into());
        }

        Ok(response.bytes().to_vec())
    }
}

/// Caches objects on local disk, mirroring the object storage key structure
/// (`{cache_dir}/{key}`).
pub struct FileCache {
    cache_dir: PathBuf,
    downloader: Option<Arc<dyn ObjectDownloader>>,
    /// Keys this process has confirmed are on disk.
    present: moka::future::Cache<String, ()>,
}

impl FileCache {
    /// Creates the cache, creating `cache_dir` on disk if it doesn't exist
    /// yet. With `bucket` set to `None` nothing can be downloaded, and
    /// `ensure_cached` fails with [`CacheError::NotConfigured`] for any key
    /// that isn't already on disk.
    pub fn new(cache_dir: PathBuf, bucket: Option<Arc<s3::Bucket>>) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create file cache directory {cache_dir:?}: {e}");
        }

        Self {
            cache_dir,
            downloader: bucket.map(|bucket| bucket as Arc<dyn ObjectDownloader>),
            present: moka::future::Cache::new(MAX_TRACKED_KEYS),
        }
    }

    #[cfg(test)]
    fn with_downloader(cache_dir: PathBuf, downloader: Arc<dyn ObjectDownloader>) -> Self {
        Self {
            cache_dir,
            downloader: Some(downloader),
            present: moka::future::Cache::new(MAX_TRACKED_KEYS),
        }
    }

    /// The local path for a key, regardless of whether it's actually cached
    /// yet.
    pub fn cached_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(key)
    }

    /// Returns the local path for `key`, downloading the object first if it
    /// isn't already cached. Concurrent calls for the same key share a single
    /// download.
    pub async fn ensure_cached(&self, key: &str) -> Result<PathBuf, CacheError> {
        let dest = self.cached_path(key);

        self.present
            .try_get_with(key.to_string(), self.fill(key, &dest))
            .await
            .map_err(|e| (*e).clone())?;

        Ok(dest)
    }

    /// Downloads `key` unless the file is already on disk, which it can be
    /// from an earlier run of the process.
    async fn fill(&self, key: &str, dest: &Path) -> Result<(), CacheError> {
        if tokio::fs::try_exists(dest)
            .await
            .map_err(|e| CacheError::Local(Arc::new(e)))?
        {
            return Ok(());
        }

        let downloader = self.downloader.as_ref().ok_or(CacheError::NotConfigured)?;
        let bytes = downloader
            .download(key)
            .await
            .map_err(|e| CacheError::Remote(e.into()))?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CacheError::Local(Arc::new(e)))?;
        }

        tokio::fs::write(dest, bytes)
            .await
            .map_err(|e| CacheError::Local(Arc::new(e)))
    }

    /// Removes the local copy of `key`, if any, so the next `ensure_cached`
    /// re-downloads it.
    pub async fn invalidate(&self, key: &str) -> Result<(), CacheError> {
        self.present.invalidate(key).await;
        match tokio::fs::remove_file(self.cached_path(key)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CacheError::Local(Arc::new(e))),
        }
    }

    /// Downloads `keys` in the background, bounded to
    /// `MAX_CONCURRENT_PRE_CACHE_DOWNLOADS` concurrent downloads. Returns
    /// immediately; failures are logged rather than surfaced. Await the
    /// returned handle to wait for every key to have been attempted.
    pub fn pre_cache(self: &Arc<Self>, keys: &[String]) -> tokio::task::JoinHandle<()> {
        let keys = keys.to_vec();
        let cache = Arc::clone(self);
        tokio::spawn(async move {
            let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_PRE_CACHE_DOWNLOADS));
            let mut tasks = tokio::task::JoinSet::new();
            for key in keys {
                let semaphore = Arc::clone(&semaphore);
                let cache = Arc::clone(&cache);
                tasks.spawn(async move {
                    let _permit = semaphore
                        .acquire_owned()
                        .await
                        .expect("semaphore is never closed");
                    if let Err(e) = cache.ensure_cached(&key).await {
                        tracing::warn!("Failed to pre-cache {key}: {e}");
                    }
                });
            }
            while tasks.join_next().await.is_some() {}
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubDownloader {
        calls: Arc<AtomicUsize>,
        payload: Vec<u8>,
    }

    impl StubDownloader {
        fn new(calls: &Arc<AtomicUsize>) -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::clone(calls),
                payload: b"hello world".to_vec(),
            })
        }
    }

    #[async_trait]
    impl ObjectDownloader for StubDownloader {
        async fn download(&self, _key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.payload.clone())
        }
    }

    struct FailingDownloader;

    #[async_trait]
    impl ObjectDownloader for FailingDownloader {
        async fn download(&self, key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
            Err(format!("no such object: {key}").into())
        }
    }

    #[test]
    fn new_creates_the_cache_dir() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("nested/cache");
        assert!(!cache_dir.exists());

        FileCache::new(cache_dir.clone(), None);

        assert!(cache_dir.is_dir());
    }

    #[test]
    fn cached_path_mirrors_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);

        let path = cache.cached_path("tracks/music/60s/song.ogg");

        assert_eq!(path, dir.path().join("tracks/music/60s/song.ogg"));
    }

    #[tokio::test]
    async fn invalidate_is_a_no_op_when_nothing_is_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);

        cache
            .invalidate("tracks/never-downloaded.ogg")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ensure_cached_downloads_and_returns_the_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls));

        let path = cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(path, dir.path().join("tracks/song.ogg"));
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"hello world");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ensure_cached_reuses_an_existing_local_file_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls));
        let path = cache.cached_path("tracks/song.ogg");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, b"already cached").await.unwrap();

        cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ensure_cached_re_downloads_after_invalidate() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls));

        let path = cache.ensure_cached("tracks/song.ogg").await.unwrap();
        cache.invalidate("tracks/song.ogg").await.unwrap();
        assert!(!path.exists());
        cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ensure_cached_without_a_configured_bucket_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);

        let err = cache.ensure_cached("tracks/song.ogg").await.unwrap_err();

        assert!(matches!(err, CacheError::NotConfigured));
    }

    #[tokio::test]
    async fn ensure_cached_surfaces_download_failures() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), Arc::new(FailingDownloader));

        let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

        assert!(matches!(err, CacheError::Remote(_)));
    }

    #[tokio::test]
    async fn pre_cache_downloads_every_key() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache = Arc::new(FileCache::with_downloader(
            dir.path().to_path_buf(),
            StubDownloader::new(&calls),
        ));

        cache
            .pre_cache(&[
                "tracks/a.ogg".to_string(),
                "tracks/b.ogg".to_string(),
                "tracks/c.ogg".to_string(),
            ])
            .await
            .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(cache.cached_path("tracks/a.ogg").exists());
        assert!(cache.cached_path("tracks/b.ogg").exists());
        assert!(cache.cached_path("tracks/c.ogg").exists());
    }
}
