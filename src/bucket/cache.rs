use anyhow::{Context, anyhow, bail};
use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

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

const SWEEP_INTERVAL: Duration = Duration::from_secs(10 * 60);

const RECENT_USE_GRACE: Duration = Duration::from_secs(5 * 60);

/// Marks a download failure as specifically meaning the key doesn't exist in
/// object storage, distinct from a network or auth failure.
#[derive(Debug, thiserror::Error)]
#[error("object not found")]
pub struct NotFound;

#[derive(Clone, Debug, Default, Serialize)]
pub struct DirListing {
    pub folders: Vec<String>,
    pub files: Vec<ObjectInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ObjectInfo {
    pub key: String,
    pub size: u64,
    pub last_modified: String,
}

/// Exists so tests can substitute a stub for a real bucket without real
/// credentials.
#[async_trait]
pub(crate) trait ObjectBackend: Send + Sync {
    async fn download(&self, key: &str) -> anyhow::Result<Vec<u8>>;

    async fn list_keys(&self, prefix: &str) -> anyhow::Result<Vec<String>>;

    /// Lists the single level directly under `prefix`.
    async fn list_dir(&self, prefix: &str) -> anyhow::Result<DirListing>;

    async fn upload(&self, key: &str, bytes: &[u8], content_type: &str) -> anyhow::Result<()>;
}

#[async_trait]
impl ObjectBackend for s3::Bucket {
    async fn download(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let response = self
            .get_object(key)
            .await
            .with_context(|| format!("fetching {key}"))?;

        let status = response.status_code();
        if status == 404 {
            return Err(NotFound.into());
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

    async fn list_dir(&self, prefix: &str) -> anyhow::Result<DirListing> {
        let mut listing = DirListing::default();
        let mut continuation_token = None;

        loop {
            let (page, _) = self
                .list_page(
                    prefix.to_string(),
                    Some("/".to_string()),
                    continuation_token,
                    None,
                    None,
                )
                .await
                .with_context(|| format!("listing {prefix}"))?;

            listing.folders.extend(
                page.common_prefixes
                    .into_iter()
                    .flatten()
                    .map(|common| common.prefix),
            );
            listing.files.extend(
                page.contents
                    .into_iter()
                    .filter(|object| object.key != prefix)
                    .map(|object| ObjectInfo {
                        key: object.key,
                        size: object.size,
                        last_modified: object.last_modified,
                    }),
            );

            continuation_token = page.next_continuation_token;
            if continuation_token.is_none() {
                break;
            }
        }

        Ok(listing)
    }

    async fn upload(&self, key: &str, bytes: &[u8], content_type: &str) -> anyhow::Result<()> {
        let response = self
            .put_object_with_content_type(key, bytes, content_type)
            .await
            .with_context(|| format!("uploading {key}"))?;

        let status = response.status_code();
        if !(200..300).contains(&status) {
            bail!("unexpected status {status} uploading {key}");
        }
        Ok(())
    }
}

/// Caches objects on local disk, mirroring the object storage key structure
/// (`{cache_dir}/{key}`).
#[derive(Clone)]
pub struct Bucket {
    cache_dir: PathBuf,
    backend: Arc<dyn ObjectBackend>,
    /// Seeded at construction from the files already in `cache_dir`.
    present: moka::future::Cache<String, ()>,
    locks: KeyLocks,
    /// `list_remote` results, keyed by prefix.
    remote_listings: moka::future::Cache<String, Vec<String>>,
    _sweeper: Option<Arc<SweepTask>>,
}

#[derive(Clone, Copy, Debug)]
pub struct EvictionPolicy {
    pub entry_ttl: Option<Duration>,
    pub max_bytes: Option<u64>,
}

struct SweepTask(tokio::task::JoinHandle<()>);

impl Drop for SweepTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Bucket {
    /// Creates the cache, creating `cache_dir` on disk if it doesn't exist
    /// yet, and adopts the files already in it.
    ///
    /// Files the `policy` evicts are deleted rather than adopted, and the same
    /// eviction repeats every [`SWEEP_INTERVAL`] while the cache lives.
    pub async fn new(
        cache_dir: PathBuf,
        bucket: Arc<s3::Bucket>,
        policy: EvictionPolicy,
    ) -> anyhow::Result<Self> {
        Self::build(cache_dir, bucket, policy).await
    }

    async fn build(
        cache_dir: PathBuf,
        backend: Arc<dyn ObjectBackend>,
        policy: EvictionPolicy,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(&cache_dir)
            .with_context(|| format!("creating cache dir {cache_dir:?}"))?;

        let present = moka::future::Cache::new(MAX_TRACKED_KEYS);

        let locks = KeyLocks::default();
        let sweeper = Sweeper {
            cache_dir: cache_dir.clone(),
            policy,
            locks: locks.clone(),
            present: present.clone(),
        };

        let outcome = sweeper.run(true).await.context("adopting cached files")?;
        for key in outcome.kept {
            present.insert(key, ()).await;
        }

        let sweep_task = (policy.entry_ttl.is_some() || policy.max_bytes.is_some())
            .then(|| Arc::new(SweepTask(tokio::spawn(sweeper.run_periodically()))));

        Ok(Self {
            cache_dir,
            backend,
            present,
            locks,
            remote_listings: moka::future::Cache::builder()
                .max_capacity(MAX_TRACKED_LISTINGS)
                .time_to_live(REMOTE_LISTING_TTL)
                .build(),
            _sweeper: sweep_task,
        })
    }

    /// The local path for a key, regardless of whether it's actually cached
    /// yet.
    fn cached_path(&self, key: &str) -> PathBuf {
        self.cache_dir.join(key)
    }

    /// Returns the local path for `key`, downloading the object first if it
    /// isn't already cached. Concurrent calls for the same key share a single
    /// download.
    pub async fn ensure_cached(&self, key: &str) -> anyhow::Result<PathBuf> {
        let dest = self.cached_path(key);
        let _guard = self.locks.lock(key).await;

        self.present
            .try_get_with(key.to_string(), self.fill(key, &dest))
            .await
            .map_err(|e| {
                if e.is::<NotFound>() {
                    NotFound.into()
                } else {
                    anyhow!("{e:#}")
                }
            })?;

        mark_accessed(&dest).await;

        Ok(dest)
    }

    /// Downloads `key` and writes it to `dest`, replacing anything already
    /// there.
    async fn fill(&self, key: &str, dest: &Path) -> anyhow::Result<()> {
        let bytes = self.backend.download(key).await?;

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .with_context(|| format!("creating {parent:?}"))?;
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
            return Err(e).with_context(|| format!("writing {temp_dest:?}"));
        }

        tokio::fs::rename(&temp_dest, dest)
            .await
            .with_context(|| format!("renaming {temp_dest:?} into place"))
    }

    /// Downloads `key` and returns its bytes directly, without writing them
    /// to local disk.
    pub async fn fetch_bytes(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        self.backend.download(key).await
    }

    /// Writes `bytes` to `key` in object storage, replacing any object already
    /// there, and forgets any cached copy of it.
    pub async fn upload(&self, key: &str, bytes: &[u8], content_type: &str) -> anyhow::Result<()> {
        self.backend.upload(key, bytes, content_type).await?;
        // The stale file stays on disk until the next download overwrites it;
        // deleting it here would race a concurrent `fill` that just renamed a
        // fresh copy into place.
        self.present.invalidate(key).await;
        self.remote_listings.invalidate_all();
        Ok(())
    }

    /// The folders and files directly under `prefix`, listed live rather than
    /// from the cache. A zero-byte folder marker object at `prefix` itself is
    /// left out.
    pub async fn list_dir(&self, prefix: &str) -> anyhow::Result<DirListing> {
        self.backend.list_dir(prefix).await
    }

    /// Empty when the listing request fails. Cached per prefix for
    /// [`REMOTE_LISTING_TTL`].
    pub async fn list_remote(&self, prefix: &str) -> Vec<String> {
        self.remote_listings
            .try_get_with(prefix.to_string(), self.backend.list_keys(prefix))
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

struct CachedFile {
    path: PathBuf,
    key: String,
    size: u64,
    accessed: Option<SystemTime>,
    in_use: bool,
}

#[derive(Default)]
struct SweepOutcome {
    kept: Vec<String>,
    removed: Vec<String>,
}

#[derive(Clone)]
struct Sweeper {
    cache_dir: PathBuf,
    policy: EvictionPolicy,
    locks: KeyLocks,
    present: moka::future::Cache<String, ()>,
}

#[derive(Clone, Default)]
struct KeyLocks(Arc<std::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>);

struct KeyGuard {
    locks: KeyLocks,
    key: String,
    mutex: Option<Arc<tokio::sync::Mutex<()>>>,
    guard: Option<tokio::sync::OwnedMutexGuard<()>>,
}

impl KeyLocks {
    fn mutex_for(&self, key: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        Arc::clone(map.entry(key.to_string()).or_default())
    }

    async fn lock(&self, key: &str) -> KeyGuard {
        let mutex = self.mutex_for(key);
        let guard = Arc::clone(&mutex).lock_owned().await;
        KeyGuard {
            locks: self.clone(),
            key: key.to_string(),
            mutex: Some(mutex),
            guard: Some(guard),
        }
    }

    fn try_lock(&self, key: &str) -> Option<KeyGuard> {
        let mutex = self.mutex_for(key);
        match Arc::clone(&mutex).try_lock_owned() {
            Ok(guard) => Some(KeyGuard {
                locks: self.clone(),
                key: key.to_string(),
                mutex: Some(mutex),
                guard: Some(guard),
            }),
            Err(_) => {
                self.release(key, mutex);
                None
            }
        }
    }

    fn release(&self, key: &str, mutex: Arc<tokio::sync::Mutex<()>>) {
        let mut map = self.0.lock().unwrap_or_else(|e| e.into_inner());
        drop(mutex);
        if map
            .get(key)
            .is_some_and(|entry| Arc::strong_count(entry) == 1)
        {
            map.remove(key);
        }
    }
}

impl Drop for KeyGuard {
    fn drop(&mut self) {
        self.guard.take();
        if let Some(mutex) = self.mutex.take() {
            self.locks.release(&self.key, mutex);
        }
    }
}

impl Sweeper {
    async fn run(&self, remove_partial_downloads: bool) -> anyhow::Result<SweepOutcome> {
        let cache_dir = self.cache_dir.clone();
        let files = tokio::task::spawn_blocking(move || {
            let mut files = Vec::new();
            list_cached_files(&cache_dir, &cache_dir, remove_partial_downloads, &mut files)
                .map(|()| files)
        })
        .await
        .context("joining the cache sweep")?
        .context("sweeping the cache dir")?;

        let mut outcome = SweepOutcome::default();
        let mut live = Vec::new();
        for file in files {
            if !file.in_use && is_expired(&file, self.policy.entry_ttl) && self.evict(&file).await {
                outcome.removed.push(file.key);
            } else {
                live.push(file);
            }
        }

        let Some(max_bytes) = self.policy.max_bytes else {
            outcome.kept = live.into_iter().map(|file| file.key).collect();
            return Ok(outcome);
        };

        let mut total: u64 = live.iter().map(|file| file.size).sum();
        live.sort_by_key(|file| (file.accessed.is_none(), file.accessed));
        for file in live {
            if total > max_bytes && !file.in_use && self.evict(&file).await {
                total -= file.size;
                outcome.removed.push(file.key);
            } else {
                outcome.kept.push(file.key);
            }
        }

        Ok(outcome)
    }

    async fn evict(&self, file: &CachedFile) -> bool {
        let Some(_guard) = self.locks.try_lock(&file.key) else {
            return false;
        };
        match std::fs::metadata(&file.path) {
            Ok(metadata) if !is_in_use(&file.path, &metadata) => {}
            Ok(_) => return false,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::NotFound {
                    self.present.invalidate(&file.key).await;
                }
                return false;
            }
        }
        if !remove_best_effort(&file.path) {
            return false;
        }
        self.present.invalidate(&file.key).await;
        true
    }

    async fn run_periodically(self) {
        let mut interval =
            tokio::time::interval_at(tokio::time::Instant::now() + SWEEP_INTERVAL, SWEEP_INTERVAL);
        loop {
            interval.tick().await;
            match self.run(false).await {
                Ok(outcome) if !outcome.removed.is_empty() => {
                    tracing::info!(
                        "Evicted {} files from the file cache",
                        outcome.removed.len()
                    );
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("File cache sweep failed: {e:#}"),
            }
        }
    }
}

async fn mark_accessed(path: &Path) {
    let path = path.to_path_buf();
    let result = tokio::task::spawn_blocking(move || {
        std::fs::File::open(&path)?
            .set_times(std::fs::FileTimes::new().set_accessed(SystemTime::now()))
    })
    .await;
    if let Ok(Err(e)) = result {
        tracing::warn!("Failed to record an access to a cached file: {e}");
    }
}

fn is_in_use(path: &Path, metadata: &std::fs::Metadata) -> bool {
    let used_recently = [metadata.accessed().ok(), metadata.modified().ok()]
        .into_iter()
        .flatten()
        .any(|time| time.elapsed().is_ok_and(|age| age < RECENT_USE_GRACE));
    used_recently || temp_path(path).exists()
}

/// Collects every cached file under `dir`, recursing into subdirectories.
/// Partial downloads are deleted or skipped, never collected.
fn list_cached_files(
    dir: &Path,
    cache_dir: &Path,
    remove_partial_downloads: bool,
    files: &mut Vec<CachedFile>,
) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        if file_type.is_dir() {
            list_cached_files(&path, cache_dir, remove_partial_downloads, files)?;
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

        if key.ends_with(TEMP_SUFFIX) {
            if remove_partial_downloads {
                remove_best_effort(&path);
            }
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                tracing::warn!("Keeping {path:?}, its metadata is unreadable: {e}");
                continue;
            }
        };
        let accessed = match metadata.accessed() {
            Ok(accessed) => Some(accessed),
            Err(e) => {
                tracing::warn!("Keeping {path:?}, its access time is unreadable: {e}");
                None
            }
        };

        files.push(CachedFile {
            in_use: is_in_use(&path, &metadata),
            size: metadata.len(),
            accessed,
            key,
            path,
        });
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

/// Whether `file` hasn't been read within `ttl`. Filesystems mounted
/// `noatime` or `relatime` report access times coarsely or not at all, so a
/// file can outlive its ttl. An unreadable access time keeps the file.
fn is_expired(file: &CachedFile, ttl: Option<Duration>) -> bool {
    let Some(ttl) = ttl else {
        return false;
    };

    file.accessed
        .is_some_and(|accessed| accessed.elapsed().is_ok_and(|age| age > ttl))
}

fn remove_best_effort(path: &Path) -> bool {
    match std::fs::remove_file(path) {
        Ok(()) => true,
        Err(e) => {
            tracing::warn!("Failed to remove {path:?} from the cache dir: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests;
