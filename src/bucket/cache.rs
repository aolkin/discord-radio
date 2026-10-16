// `pre_cache` and `invalidate` have no callers outside this module and its
// tests yet.
#![allow(dead_code)]

use anyhow::{Context, bail};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Semaphore;

/// Evicting an entry costs a redundant download, since this cache is the only
/// record of what is on disk.
const MAX_TRACKED_KEYS: u64 = 10_000;

/// A dropdown only shows so many results anyway, so a cap this small still
/// covers autocomplete while bounding how much of the bucket a single
/// listing call can walk.
const MAX_LISTED_KEYS: usize = 50;

/// Long enough that repeated autocomplete calls for the same prefix don't
/// re-hit object storage on every keystroke, short enough that a newly
/// uploaded file shows up again soon after.
const REMOTE_LISTING_TTL: Duration = Duration::from_secs(30);

const MAX_TRACKED_LISTINGS: u64 = 1_000;

const TEMP_SUFFIX: &str = ".tmp";

#[derive(Clone, Debug, thiserror::Error)]
pub enum ObjectStoreError {
    #[error("object storage is not configured")]
    NotConfigured,
    #[error("object not found")]
    NotFound,
    #[error("remote download failed: {0:#}")]
    Remote(Arc<anyhow::Error>),
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
    async fn download(&self, key: &str) -> anyhow::Result<Vec<u8>>;

    async fn list_keys(&self, prefix: &str) -> anyhow::Result<Vec<String>>;
}

#[async_trait]
impl ObjectDownloader for s3::Bucket {
    async fn download(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let response = self
            .get_object(key)
            .await
            .with_context(|| format!("fetching {key}"))?;

        let status = response.status_code();
        if status == 404 {
            return Err(ObjectNotFound.into());
        }
        if !(200..300).contains(&status) {
            bail!("unexpected status {status} fetching {key}");
        }

        Ok(response.bytes().to_vec())
    }

    // TODO: this returns a flat list of every object key under `prefix`,
    // which is hard to use once a prefix holds more than a handful of
    // objects. A real implementation should list one directory level at a
    // time instead, the way S3's `delimiter` parameter (passed to
    // `list_page`) groups keys under a separator into "common prefixes"
    // (folders) distinct from the objects directly in that folder.
    async fn list_keys(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        let mut keys = Vec::new();
        let mut continuation_token = None;

        loop {
            let (page, _) = self
                .list_page(
                    prefix.to_string(),
                    None,
                    continuation_token,
                    None,
                    Some(MAX_LISTED_KEYS - keys.len()),
                )
                .await
                .with_context(|| format!("listing {prefix}"))?;

            keys.extend(page.contents.into_iter().map(|object| object.key));
            if keys.len() >= MAX_LISTED_KEYS {
                keys.truncate(MAX_LISTED_KEYS);
                break;
            }

            continuation_token = page.next_continuation_token;
            if continuation_token.is_none() {
                break;
            }
        }

        Ok(keys)
    }
}

/// Caches objects on local disk, mirroring the object storage key structure
/// (`{cache_dir}/{key}`).
pub struct ObjectStore {
    cache_dir: PathBuf,
    downloader: Option<Arc<dyn ObjectDownloader>>,
    /// Seeded at construction from the files already in `cache_dir`.
    present: moka::future::Cache<String, ()>,
    /// `list_remote` results, keyed by prefix.
    remote_listings: moka::future::Cache<String, Vec<String>>,
}

impl ObjectStore {
    /// Creates the cache, creating `cache_dir` on disk if it doesn't exist
    /// yet, and adopts the files already in it. With `bucket` set to `None`
    /// nothing can be downloaded, and `ensure_cached` fails with
    /// [`ObjectStoreError::NotConfigured`] for any key that wasn't adopted.
    ///
    /// A file whose last access is older than `entry_ttl` is deleted rather
    /// than adopted; `None` keeps every file forever.
    pub async fn new(
        cache_dir: PathBuf,
        bucket: Option<Arc<s3::Bucket>>,
        entry_ttl: Option<Duration>,
    ) -> Result<Self, ObjectStoreError> {
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
    ) -> Result<Self, ObjectStoreError> {
        std::fs::create_dir_all(&cache_dir).map_err(|e| ObjectStoreError::Local(Arc::new(e)))?;

        let cache = Self {
            cache_dir,
            downloader,
            present: moka::future::Cache::new(MAX_TRACKED_KEYS),
            remote_listings: moka::future::Cache::builder()
                .max_capacity(MAX_TRACKED_LISTINGS)
                .time_to_live(REMOTE_LISTING_TTL)
                .build(),
        };

        let mut keys = Vec::new();
        collect_cached_keys(&cache.cache_dir, &cache.cache_dir, entry_ttl, &mut keys)
            .map_err(|e| ObjectStoreError::Local(Arc::new(e)))?;
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
    pub async fn ensure_cached(&self, key: &str) -> Result<PathBuf, ObjectStoreError> {
        let dest = self.cached_path(key);

        self.present
            .try_get_with(key.to_string(), self.fill(key, &dest))
            .await
            .map_err(|e| (*e).clone())?;

        Ok(dest)
    }

    /// Downloads `key` and writes it to `dest`, replacing anything already
    /// there.
    async fn fill(&self, key: &str, dest: &Path) -> Result<(), ObjectStoreError> {
        let downloader = self
            .downloader
            .as_ref()
            .ok_or(ObjectStoreError::NotConfigured)?;
        let bytes = downloader.download(key).await.map_err(|e| {
            if e.is::<ObjectNotFound>() {
                ObjectStoreError::NotFound
            } else {
                ObjectStoreError::Remote(Arc::new(e))
            }
        })?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ObjectStoreError::Local(Arc::new(e)))?;
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
            return Err(ObjectStoreError::Local(Arc::new(e)));
        }

        tokio::fs::rename(&temp_dest, dest)
            .await
            .map_err(|e| ObjectStoreError::Local(Arc::new(e)))
    }

    /// Downloads `key` and returns its bytes directly, without writing them
    /// to local disk.
    pub async fn fetch_bytes(&self, key: &str) -> Result<Vec<u8>, ObjectStoreError> {
        let downloader = self
            .downloader
            .as_ref()
            .ok_or(ObjectStoreError::NotConfigured)?;
        downloader.download(key).await.map_err(|e| {
            if e.is::<ObjectNotFound>() {
                ObjectStoreError::NotFound
            } else {
                ObjectStoreError::Remote(Arc::new(e))
            }
        })
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
    /// Cached per prefix for [`REMOTE_LISTING_TTL`].
    pub async fn list_remote(&self, prefix: &str) -> Vec<String> {
        let Some(downloader) = &self.downloader else {
            return Vec::new();
        };

        self.remote_listings
            .try_get_with(prefix.to_string(), downloader.list_keys(prefix))
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to list remote objects under {prefix:?}: {e:#}");
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
    cache: Arc<ObjectStore>,
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
mod tests;
