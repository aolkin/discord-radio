// Nothing outside this module and its tests calls into FileCache yet; the
// resolver that will (OLK-114) lands in a later PR.
#![allow(dead_code)]

use async_trait::async_trait;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore, broadcast};

/// How many objects `pre_cache` will download at once.
const MAX_CONCURRENT_PRE_CACHE_DOWNLOADS: usize = 4;

#[derive(Clone, Debug)]
pub enum CacheError {
    /// No object storage client is configured, so nothing can be downloaded.
    NotConfigured,
    /// The remote request failed or returned a non-2xx status.
    Remote(String),
    /// Writing the downloaded object to the local cache failed.
    Io(String),
}

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheError::NotConfigured => write!(f, "object storage is not configured"),
            CacheError::Remote(msg) => write!(f, "remote download failed: {msg}"),
            CacheError::Io(msg) => write!(f, "local cache i/o failed: {msg}"),
        }
    }
}

impl std::error::Error for CacheError {}

/// Downloads a single object's bytes from remote storage. Exists so tests can
/// substitute a stub for the real R2 bucket without real credentials.
#[async_trait]
trait ObjectDownloader: Send + Sync {
    async fn download(&self, r2_key: &str) -> Result<Vec<u8>, CacheError>;
}

#[async_trait]
impl ObjectDownloader for s3::Bucket {
    async fn download(&self, r2_key: &str) -> Result<Vec<u8>, CacheError> {
        let response = self
            .get_object(r2_key)
            .await
            .map_err(|e| CacheError::Remote(e.to_string()))?;

        let status = response.status_code();
        if !(200..300).contains(&status) {
            return Err(CacheError::Remote(format!(
                "unexpected status {status} fetching {r2_key}"
            )));
        }

        Ok(response.bytes().to_vec())
    }
}

/// Caches R2 objects on local disk, mirroring the R2 key structure
/// (`{cache_dir}/{r2_key}`).
///
/// Concurrent requests for the same key are deduplicated: only the first
/// caller downloads, and the rest wait on its result.
pub struct FileCache {
    cache_dir: PathBuf,
    downloader: Option<Arc<dyn ObjectDownloader>>,
    in_flight: Mutex<HashMap<String, broadcast::Sender<Result<(), CacheError>>>>,
}

impl FileCache {
    /// Creates the cache, creating `cache_dir` on disk if it doesn't exist
    /// yet. `bucket` is `None` when object storage isn't configured; in that
    /// case `ensure_cached` and `pre_cache` fail/no-op instead of panicking.
    pub fn new(cache_dir: PathBuf, bucket: Option<Arc<s3::Bucket>>) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create file cache directory {cache_dir:?}: {e}");
        }

        Self {
            cache_dir,
            downloader: bucket.map(|bucket| bucket as Arc<dyn ObjectDownloader>),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    #[cfg(test)]
    fn with_downloader(cache_dir: PathBuf, downloader: Arc<dyn ObjectDownloader>) -> Self {
        Self {
            cache_dir,
            downloader: Some(downloader),
            in_flight: Mutex::new(HashMap::new()),
        }
    }

    /// The deterministic local path for an R2 key, regardless of whether
    /// it's actually cached yet.
    pub fn cached_path(&self, r2_key: &str) -> PathBuf {
        self.cache_dir.join(r2_key)
    }

    /// Returns the local path for `r2_key`, downloading it from R2 first if
    /// it isn't already cached. Concurrent calls for the same key share a
    /// single download.
    pub async fn ensure_cached(&self, r2_key: &str) -> Result<PathBuf, CacheError> {
        let dest = self.cached_path(r2_key);
        if tokio::fs::try_exists(&dest).await.unwrap_or(false) {
            return Ok(dest);
        }

        let mut follower = {
            let mut in_flight = self.in_flight.lock().await;
            if let Some(sender) = in_flight.get(r2_key) {
                Some(sender.subscribe())
            } else {
                let (sender, _receiver) = broadcast::channel(1);
                in_flight.insert(r2_key.to_string(), sender);
                None
            }
        };

        if let Some(receiver) = follower.as_mut() {
            return match receiver.recv().await {
                Ok(Ok(())) => Ok(dest),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(CacheError::Remote(
                    "in-flight download ended without a result".to_string(),
                )),
            };
        }

        let result = self.download(r2_key, &dest).await;
        {
            let mut in_flight = self.in_flight.lock().await;
            if let Some(sender) = in_flight.remove(r2_key) {
                let _ = sender.send(result.clone());
            }
        }
        result.map(|()| dest)
    }

    async fn download(&self, r2_key: &str, dest: &Path) -> Result<(), CacheError> {
        let downloader = self.downloader.as_ref().ok_or(CacheError::NotConfigured)?;
        let bytes = downloader.download(r2_key).await?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CacheError::Io(e.to_string()))?;
        }

        tokio::fs::write(dest, bytes)
            .await
            .map_err(|e| CacheError::Io(e.to_string()))
    }

    /// Removes the local copy of `r2_key`, if any, so the next
    /// `ensure_cached` re-downloads it. Used for playlist re-fetch
    /// scenarios where the remote object may have changed.
    pub async fn invalidate(&self, r2_key: &str) -> Result<(), CacheError> {
        match tokio::fs::remove_file(self.cached_path(r2_key)).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(CacheError::Io(e.to_string())),
        }
    }

    /// Downloads `keys` in the background, bounded to
    /// `MAX_CONCURRENT_PRE_CACHE_DOWNLOADS` concurrent downloads. Returns
    /// immediately; failures are logged rather than surfaced, since callers
    /// don't wait on the result.
    pub fn pre_cache(self: &Arc<Self>, keys: &[String]) {
        if keys.is_empty() {
            return;
        }

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
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct StubDownloader {
        calls: Arc<AtomicUsize>,
        payload: Vec<u8>,
        delay: Duration,
    }

    #[async_trait]
    impl ObjectDownloader for StubDownloader {
        async fn download(&self, _r2_key: &str) -> Result<Vec<u8>, CacheError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if !self.delay.is_zero() {
                tokio::time::sleep(self.delay).await;
            }
            Ok(self.payload.clone())
        }
    }

    struct FailingDownloader;

    #[async_trait]
    impl ObjectDownloader for FailingDownloader {
        async fn download(&self, r2_key: &str) -> Result<Vec<u8>, CacheError> {
            Err(CacheError::Remote(format!("no such object: {r2_key}")))
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
    fn cached_path_is_deterministic_and_mirrors_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);

        let path = cache.cached_path("tracks/music/60s/song.ogg");

        assert_eq!(path, dir.path().join("tracks/music/60s/song.ogg"));
        assert_eq!(path, cache.cached_path("tracks/music/60s/song.ogg"));
    }

    #[tokio::test]
    async fn invalidate_removes_a_cached_file() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);
        let path = cache.cached_path("tracks/song.ogg");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, b"cached bytes").await.unwrap();
        assert!(path.exists());

        cache.invalidate("tracks/song.ogg").await.unwrap();

        assert!(!path.exists());
    }

    #[tokio::test]
    async fn invalidate_is_a_no_op_when_nothing_is_cached() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);

        cache.invalidate("tracks/never-downloaded.ogg").await.unwrap();
    }

    #[tokio::test]
    async fn ensure_cached_downloads_and_returns_the_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = Arc::new(StubDownloader {
            calls: calls.clone(),
            payload: b"hello world".to_vec(),
            delay: Duration::ZERO,
        });
        let cache = FileCache::with_downloader(dir.path().to_path_buf(), downloader);

        let path = cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(path, dir.path().join("tracks/song.ogg"));
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"hello world");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ensure_cached_reuses_an_existing_local_file_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = Arc::new(StubDownloader {
            calls: calls.clone(),
            payload: b"hello world".to_vec(),
            delay: Duration::ZERO,
        });
        let cache = FileCache::with_downloader(dir.path().to_path_buf(), downloader);
        let path = cache.cached_path("tracks/song.ogg");
        tokio::fs::create_dir_all(path.parent().unwrap())
            .await
            .unwrap();
        tokio::fs::write(&path, b"already cached").await.unwrap();

        cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ensure_cached_without_a_configured_bucket_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None);

        let err = cache.ensure_cached("tracks/song.ogg").await.unwrap_err();

        assert!(matches!(err, CacheError::NotConfigured));
    }

    #[tokio::test]
    async fn ensure_cached_propagates_download_failures_to_all_waiters() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Arc::new(FileCache::with_downloader(
            dir.path().to_path_buf(),
            Arc::new(FailingDownloader),
        ));

        let (a, b) = tokio::join!(
            cache.ensure_cached("tracks/missing.ogg"),
            cache.ensure_cached("tracks/missing.ogg"),
        );

        assert!(a.is_err());
        assert!(b.is_err());
    }

    #[tokio::test]
    async fn ensure_cached_dedups_concurrent_downloads_for_the_same_key() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = Arc::new(StubDownloader {
            calls: calls.clone(),
            payload: b"hello world".to_vec(),
            delay: Duration::from_millis(50),
        });
        let cache = Arc::new(FileCache::with_downloader(
            dir.path().to_path_buf(),
            downloader,
        ));

        let cache_a = cache.clone();
        let cache_b = cache.clone();
        let (a, b) = tokio::join!(
            tokio::spawn(async move { cache_a.ensure_cached("tracks/song.ogg").await }),
            tokio::spawn(async move { cache_b.ensure_cached("tracks/song.ogg").await }),
        );

        let path_a = a.unwrap().unwrap();
        let path_b = b.unwrap().unwrap();
        assert_eq!(path_a, path_b);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(tokio::fs::read(&path_a).await.unwrap(), b"hello world");
    }

    #[tokio::test]
    async fn pre_cache_downloads_every_key_in_the_background() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let downloader = Arc::new(StubDownloader {
            calls: calls.clone(),
            payload: b"hi".to_vec(),
            delay: Duration::ZERO,
        });
        let cache = Arc::new(FileCache::with_downloader(
            dir.path().to_path_buf(),
            downloader,
        ));

        cache.pre_cache(&[
            "tracks/a.ogg".to_string(),
            "tracks/b.ogg".to_string(),
            "tracks/c.ogg".to_string(),
        ]);

        // pre_cache is fire-and-forget; give the spawned task time to finish.
        for _ in 0..50 {
            if calls.load(Ordering::SeqCst) == 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(cache.cached_path("tracks/a.ogg").exists());
        assert!(cache.cached_path("tracks/b.ogg").exists());
        assert!(cache.cached_path("tracks/c.ogg").exists());
    }
}
