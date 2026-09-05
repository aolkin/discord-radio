// `pre_cache` and `invalidate` have no callers outside this module and its
// tests yet.
#![allow(dead_code)]

use async_trait::async_trait;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Evicting an entry costs a redundant download, since this cache is the only
/// record of what is on disk.
const MAX_TRACKED_KEYS: u64 = 10_000;

const TEMP_SUFFIX: &str = ".tmp";

#[derive(Clone, Debug, thiserror::Error)]
pub enum CacheError {
    #[error("object storage is not configured")]
    NotConfigured,
    #[error("object not found")]
    NotFound,
    #[error("remote download failed: {0}")]
    Remote(Arc<dyn Error + Send + Sync>),
    #[error("local cache i/o failed: {0}")]
    Local(Arc<std::io::Error>),
}

/// Marks a [`ObjectDownloader::download`] failure as specifically meaning the
/// key doesn't exist in object storage, distinct from a network or auth
/// failure.
#[derive(Debug, thiserror::Error)]
#[error("object not found")]
struct ObjectNotFound;

/// Exists so tests can substitute a stub for a real bucket without real
/// credentials.
#[async_trait]
pub(crate) trait ObjectDownloader: Send + Sync {
    async fn download(&self, key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>>;

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, Box<dyn Error + Send + Sync>>;
}

#[async_trait]
impl ObjectDownloader for s3::Bucket {
    async fn download(&self, key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        let response = self.get_object(key).await?;

        let status = response.status_code();
        if status == 404 {
            return Err(Box::new(ObjectNotFound));
        }
        if !(200..300).contains(&status) {
            return Err(format!("unexpected status {status} fetching {key}").into());
        }

        Ok(response.bytes().to_vec())
    }

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        let pages = self.list(prefix.to_string(), None).await?;
        Ok(pages
            .into_iter()
            .flat_map(|page| page.contents.into_iter().map(|object| object.key))
            .collect())
    }
}

/// Caches objects on local disk, mirroring the object storage key structure
/// (`{cache_dir}/{key}`).
pub struct FileCache {
    cache_dir: PathBuf,
    downloader: Option<Arc<dyn ObjectDownloader>>,
    /// Seeded at construction from the files already in `cache_dir`.
    present: moka::future::Cache<String, ()>,
}

impl FileCache {
    /// Creates the cache, creating `cache_dir` on disk if it doesn't exist
    /// yet, and adopts the files already in it. With `bucket` set to `None`
    /// nothing can be downloaded, and `ensure_cached` fails with
    /// [`CacheError::NotConfigured`] for any key that wasn't adopted.
    ///
    /// A file whose last access is older than `entry_ttl` is deleted rather
    /// than adopted; `None` keeps every file forever.
    pub async fn new(
        cache_dir: PathBuf,
        bucket: Option<Arc<s3::Bucket>>,
        entry_ttl: Option<Duration>,
    ) -> Result<Self, CacheError> {
        let downloader = bucket.map(|bucket| bucket as Arc<dyn ObjectDownloader>);
        Self::build(cache_dir, downloader, entry_ttl).await
    }

    #[cfg(test)]
    pub(crate) async fn with_downloader(
        cache_dir: PathBuf,
        downloader: Arc<dyn ObjectDownloader>,
    ) -> Self {
        Self::build(cache_dir, Some(downloader), None)
            .await
            .expect("the test cache dir is usable")
    }

    async fn build(
        cache_dir: PathBuf,
        downloader: Option<Arc<dyn ObjectDownloader>>,
        entry_ttl: Option<Duration>,
    ) -> Result<Self, CacheError> {
        std::fs::create_dir_all(&cache_dir).map_err(|e| CacheError::Local(Arc::new(e)))?;

        let cache = Self {
            cache_dir,
            downloader,
            present: moka::future::Cache::new(MAX_TRACKED_KEYS),
        };

        let mut keys = Vec::new();
        collect_cached_keys(&cache.cache_dir, &cache.cache_dir, entry_ttl, &mut keys)
            .map_err(|e| CacheError::Local(Arc::new(e)))?;
        for key in keys {
            cache.present.insert(key, ()).await;
        }

        Ok(cache)
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

    /// Downloads `key` and writes it to `dest`, replacing anything already
    /// there.
    async fn fill(&self, key: &str, dest: &Path) -> Result<(), CacheError> {
        let downloader = self.downloader.as_ref().ok_or(CacheError::NotConfigured)?;
        let bytes = downloader.download(key).await.map_err(|e| {
            if e.is::<ObjectNotFound>() {
                CacheError::NotFound
            } else {
                CacheError::Remote(e.into())
            }
        })?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| CacheError::Local(Arc::new(e)))?;
        }

        // Write to a temporary path in the same directory as `dest` and
        // rename into place only once the write fully succeeds, so a
        // process kill mid-download can't leave a partial file at `dest`
        // that construction would adopt as a completed download.
        let temp_dest = temp_path(dest);

        if let Err(e) = tokio::fs::write(&temp_dest, bytes).await {
            if let Err(cleanup_err) = tokio::fs::remove_file(&temp_dest).await
                && cleanup_err.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!("Failed to clean up temp file {temp_dest:?}: {cleanup_err}");
            }
            return Err(CacheError::Local(Arc::new(e)));
        }

        tokio::fs::rename(&temp_dest, dest)
            .await
            .map_err(|e| CacheError::Local(Arc::new(e)))
    }

    /// Forgets `key`, so the next `ensure_cached` re-downloads it.
    ///
    /// The file stays on disk. `fill` overwrites unconditionally and nothing
    /// reads a file this cache has forgotten, so the stale bytes cost disk
    /// space until the next download replaces them; deleting here would
    /// instead race a concurrent `fill` and remove the file it just renamed
    /// into place while its entry claims the key is cached.
    pub async fn invalidate(&self, key: &str) {
        self.present.invalidate(key).await;
    }

    /// Empty when no bucket is configured or the listing request fails.
    pub async fn list_remote(&self, prefix: &str) -> Vec<String> {
        let Some(downloader) = &self.downloader else {
            return Vec::new();
        };

        downloader.list_keys(prefix).await.unwrap_or_else(|e| {
            tracing::warn!("Failed to list remote objects under {prefix:?}: {e}");
            Vec::new()
        })
    }
}

fn temp_path(dest: &Path) -> PathBuf {
    let mut temp = dest.as_os_str().to_owned();
    temp.push(TEMP_SUFFIX);
    PathBuf::from(temp)
}

/// Collects the key of every cached file under `dir`, recursing into
/// subdirectories. Deletes rather than collects a partial download and, when
/// `entry_ttl` is set, a file that hasn't been read within it.
fn collect_cached_keys(
    dir: &Path,
    cache_dir: &Path,
    entry_ttl: Option<Duration>,
    keys: &mut Vec<String>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            collect_cached_keys(&path, cache_dir, entry_ttl, keys)?;
            continue;
        }

        // Symlinks and device nodes are never something `fill` wrote.
        if !file_type.is_file() {
            continue;
        }

        let Some(key) = cache_key(cache_dir, &path) else {
            tracing::warn!("Ignoring cached file with a non-UTF-8 path: {path:?}");
            continue;
        };

        if key.ends_with(TEMP_SUFFIX) || is_expired(&path, entry_ttl) {
            remove_best_effort(&path);
            continue;
        }

        keys.push(key);
    }

    Ok(())
}

/// The object storage key a cached file came from: its path relative to
/// `cache_dir`, always `/`-separated. `None` for a path that isn't valid
/// UTF-8, which no key can be.
fn cache_key(cache_dir: &Path, path: &Path) -> Option<String> {
    let mut key = String::new();
    for component in path.strip_prefix(cache_dir).ok()?.components() {
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(component.as_os_str().to_str()?);
    }
    Some(key)
}

/// Whether `path` hasn't been read within `ttl`. Filesystems mounted
/// `noatime` or `relatime` report access times coarsely or not at all, so a
/// file can outlive its ttl. An unreadable access time keeps the file.
fn is_expired(path: &Path, ttl: Option<Duration>) -> bool {
    let Some(ttl) = ttl else {
        return false;
    };

    match std::fs::metadata(path).and_then(|metadata| metadata.accessed()) {
        Ok(accessed) => accessed.elapsed().is_ok_and(|age| age > ttl),
        Err(e) => {
            tracing::warn!("Keeping {path:?}, its access time is unreadable: {e}");
            false
        }
    }
}

fn remove_best_effort(path: &Path) {
    if let Err(e) = std::fs::remove_file(path) {
        tracing::warn!("Failed to remove {path:?} from the cache dir: {e}");
    }
}

/// Downloads `keys` into `cache` in the background, at most `max_concurrent`
/// at a time. Returns immediately; failures are logged rather than surfaced.
/// Await the returned handle to wait for every key to have been attempted.
pub fn pre_cache(
    cache: Arc<FileCache>,
    keys: Vec<String>,
    max_concurrent: usize,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let semaphore = Arc::new(Semaphore::new(max_concurrent));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct StubDownloader {
        calls: Arc<AtomicUsize>,
        payload: Vec<u8>,
        remote_keys: Vec<String>,
    }

    impl StubDownloader {
        fn new(calls: &Arc<AtomicUsize>) -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::clone(calls),
                payload: b"hello world".to_vec(),
                remote_keys: Vec::new(),
            })
        }

        fn with_remote_keys(keys: &[&str]) -> Arc<Self> {
            Arc::new(Self {
                calls: Arc::new(AtomicUsize::new(0)),
                payload: b"hello world".to_vec(),
                remote_keys: keys.iter().map(|k| k.to_string()).collect(),
            })
        }
    }

    #[async_trait]
    impl ObjectDownloader for StubDownloader {
        async fn download(&self, _key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.payload.clone())
        }

        async fn list_keys(
            &self,
            prefix: &str,
        ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
            Ok(self
                .remote_keys
                .iter()
                .filter(|key| key.starts_with(prefix))
                .cloned()
                .collect())
        }
    }

    struct FailingDownloader;

    #[async_trait]
    impl ObjectDownloader for FailingDownloader {
        async fn download(&self, key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
            Err(format!("no such object: {key}").into())
        }

        async fn list_keys(
            &self,
            _prefix: &str,
        ) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
            Err("listing failed".into())
        }
    }

    struct NotFoundDownloader;

    #[async_trait]
    impl ObjectDownloader for NotFoundDownloader {
        async fn download(&self, _key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
            Err(Box::new(ObjectNotFound))
        }
    }

    fn write_cached_file(path: &Path, contents: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[tokio::test]
    async fn new_creates_the_cache_dir() {
        let dir = tempfile::tempdir().unwrap();
        let cache_dir = dir.path().join("nested/cache");
        assert!(!cache_dir.exists());

        FileCache::new(cache_dir.clone(), None, None).await.unwrap();

        assert!(cache_dir.is_dir());
    }

    #[tokio::test]
    async fn ensure_cached_downloads_and_returns_the_local_path() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;

        let path = cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(path, dir.path().join("tracks/song.ogg"));
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"hello world");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ensure_cached_reuses_a_file_left_by_an_earlier_run() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        write_cached_file(&dir.path().join("tracks/song.ogg"), b"already cached");

        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;
        cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn new_deletes_a_partial_download_without_adopting_it() {
        let dir = tempfile::tempdir().unwrap();
        let partial = temp_path(&dir.path().join("tracks/song.ogg"));
        write_cached_file(&partial, b"half a s");

        let cache = FileCache::new(dir.path().to_path_buf(), None, None)
            .await
            .unwrap();

        assert!(!partial.exists());
        assert!(matches!(
            cache
                .ensure_cached("tracks/song.ogg.tmp")
                .await
                .unwrap_err(),
            CacheError::NotConfigured
        ));
    }

    #[tokio::test]
    async fn new_deletes_a_file_unread_for_longer_than_the_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let stale = dir.path().join("tracks/stale.ogg");
        let fresh = dir.path().join("tracks/fresh.ogg");
        write_cached_file(&stale, b"stale");
        write_cached_file(&fresh, b"fresh");
        let times = std::fs::FileTimes::new()
            .set_accessed(std::time::SystemTime::now() - Duration::from_secs(3_600));
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_times(times)
            .unwrap();

        let cache = FileCache::new(
            dir.path().to_path_buf(),
            None,
            Some(Duration::from_secs(60)),
        )
        .await
        .unwrap();

        assert!(!stale.exists());
        assert!(fresh.exists());
        // With no downloader configured, only an adopted key resolves.
        cache.ensure_cached("tracks/fresh.ogg").await.unwrap();
        assert!(matches!(
            cache.ensure_cached("tracks/stale.ogg").await.unwrap_err(),
            CacheError::NotConfigured
        ));
    }

    #[tokio::test]
    async fn ensure_cached_re_downloads_after_invalidate() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;

        cache.ensure_cached("tracks/song.ogg").await.unwrap();
        cache.invalidate("tracks/song.ogg").await;
        cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn ensure_cached_does_not_leave_a_temp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;

        cache.ensure_cached("tracks/song.ogg").await.unwrap();

        assert!(!temp_path(&cache.cached_path("tracks/song.ogg")).exists());
    }

    #[tokio::test]
    async fn fill_does_not_leave_a_partial_file_when_the_write_fails() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;
        let dest = cache.cached_path("tracks/song.ogg");
        tokio::fs::create_dir_all(dest.parent().unwrap())
            .await
            .unwrap();

        // Occupy the temp path with a directory so the write into it fails.
        tokio::fs::create_dir(temp_path(&dest)).await.unwrap();

        let err = cache.ensure_cached("tracks/song.ogg").await.unwrap_err();

        assert!(matches!(err, CacheError::Local(_)));
        assert!(!dest.exists());
    }

    #[tokio::test]
    async fn ensure_cached_surfaces_download_failures() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), Arc::new(FailingDownloader)).await;

        let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

        assert!(matches!(err, CacheError::Remote(_)));
    }

    #[tokio::test]
    async fn ensure_cached_distinguishes_a_missing_object_from_other_failures() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), Arc::new(NotFoundDownloader))
                .await;

        let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

        assert!(matches!(err, CacheError::NotFound));
    }

    #[tokio::test]
    async fn pre_cache_downloads_every_key() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let cache = Arc::new(
            FileCache::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await,
        );

        pre_cache(
            Arc::clone(&cache),
            vec![
                "tracks/a.ogg".to_string(),
                "tracks/b.ogg".to_string(),
                "tracks/c.ogg".to_string(),
            ],
            2,
        )
        .await
        .unwrap();

        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(cache.cached_path("tracks/a.ogg").exists());
        assert!(cache.cached_path("tracks/b.ogg").exists());
        assert!(cache.cached_path("tracks/c.ogg").exists());
    }

    #[tokio::test]
    async fn list_remote_is_empty_without_a_bucket() {
        let dir = tempfile::tempdir().unwrap();
        let cache = FileCache::new(dir.path().to_path_buf(), None, None)
            .await
            .unwrap();

        assert!(cache.list_remote("tracks/").await.is_empty());
    }

    #[tokio::test]
    async fn list_remote_filters_by_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let downloader =
            StubDownloader::with_remote_keys(&["tracks/a.ogg", "tracks/b.ogg", "other/c.ogg"]);
        let cache = FileCache::with_downloader(dir.path().to_path_buf(), downloader).await;

        let mut keys = cache.list_remote("tracks/").await;
        keys.sort();

        assert_eq!(keys, ["tracks/a.ogg", "tracks/b.ogg"]);
    }

    #[tokio::test]
    async fn list_remote_is_empty_when_listing_fails() {
        let dir = tempfile::tempdir().unwrap();
        let cache =
            FileCache::with_downloader(dir.path().to_path_buf(), Arc::new(FailingDownloader)).await;

        assert!(cache.list_remote("tracks/").await.is_empty());
    }
}
