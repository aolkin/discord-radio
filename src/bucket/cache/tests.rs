use super::*;
use std::fs::FileTimes;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

struct StubDownloader {
    calls: Arc<AtomicUsize>,
    list_calls: Arc<AtomicUsize>,
    payload: Vec<u8>,
    remote_keys: Vec<String>,
}

impl StubDownloader {
    fn new(calls: &Arc<AtomicUsize>) -> Arc<Self> {
        Arc::new(Self {
            calls: Arc::clone(calls),
            list_calls: Arc::new(AtomicUsize::new(0)),
            payload: b"hello world".to_vec(),
            remote_keys: Vec::new(),
        })
    }

    fn with_remote_keys(keys: &[&str]) -> Arc<Self> {
        Self::with_remote_keys_and_counter(keys, &Arc::new(AtomicUsize::new(0)))
    }

    fn with_remote_keys_and_counter(keys: &[&str], list_calls: &Arc<AtomicUsize>) -> Arc<Self> {
        Arc::new(Self {
            calls: Arc::new(AtomicUsize::new(0)),
            list_calls: Arc::clone(list_calls),
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

    async fn list_keys(&self, prefix: &str) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
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

    async fn list_keys(&self, _prefix: &str) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Err("listing failed".into())
    }
}

struct NotFoundDownloader;

#[async_trait]
impl ObjectDownloader for NotFoundDownloader {
    async fn download(&self, _key: &str) -> Result<Vec<u8>, Box<dyn Error + Send + Sync>> {
        Err(Box::new(ObjectNotFound))
    }

    async fn list_keys(&self, _prefix: &str) -> Result<Vec<String>, Box<dyn Error + Send + Sync>> {
        Ok(Vec::new())
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

    ObjectStore::new(cache_dir.clone(), None, None)
        .await
        .unwrap();

    assert!(cache_dir.is_dir());
}

#[tokio::test]
async fn ensure_cached_downloads_and_returns_the_local_path() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache =
        ObjectStore::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;

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
        ObjectStore::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;
    cache.ensure_cached("tracks/song.ogg").await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn new_deletes_a_partial_download_without_adopting_it() {
    let dir = tempfile::tempdir().unwrap();
    let partial = temp_path(&dir.path().join("tracks/song.ogg"));
    write_cached_file(&partial, b"half a s");

    let cache = ObjectStore::new(dir.path().to_path_buf(), None, None)
        .await
        .unwrap();

    assert!(!partial.exists());
    assert!(matches!(
        cache
            .ensure_cached("tracks/song.ogg.tmp")
            .await
            .unwrap_err(),
        ObjectStoreError::NotConfigured
    ));
}

#[tokio::test]
async fn new_deletes_a_file_unread_for_longer_than_the_ttl() {
    let dir = tempfile::tempdir().unwrap();
    let stale = dir.path().join("tracks/stale.ogg");
    let fresh = dir.path().join("tracks/fresh.ogg");
    write_cached_file(&stale, b"stale");
    write_cached_file(&fresh, b"fresh");
    let times = FileTimes::new().set_accessed(SystemTime::now() - Duration::from_secs(3_600));
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_times(times)
        .unwrap();

    let cache = ObjectStore::new(
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
        ObjectStoreError::NotConfigured
    ));
}

#[tokio::test]
async fn ensure_cached_re_downloads_after_invalidate() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache =
        ObjectStore::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;

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
        ObjectStore::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;

    cache.ensure_cached("tracks/song.ogg").await.unwrap();

    assert!(!temp_path(&cache.cached_path("tracks/song.ogg")).exists());
}

#[tokio::test]
async fn fill_does_not_leave_a_partial_file_when_the_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache =
        ObjectStore::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await;
    let dest = cache.cached_path("tracks/song.ogg");
    tokio::fs::create_dir_all(dest.parent().unwrap())
        .await
        .unwrap();

    // Occupy the temp path with a directory so the write into it fails.
    tokio::fs::create_dir(temp_path(&dest)).await.unwrap();

    let err = cache.ensure_cached("tracks/song.ogg").await.unwrap_err();

    assert!(matches!(err, ObjectStoreError::Local(_)));
    assert!(!dest.exists());
}

#[tokio::test]
async fn ensure_cached_surfaces_download_failures() {
    let dir = tempfile::tempdir().unwrap();
    let cache =
        ObjectStore::with_downloader(dir.path().to_path_buf(), Arc::new(FailingDownloader)).await;

    let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

    assert!(matches!(err, ObjectStoreError::Remote(_)));
}

#[tokio::test]
async fn ensure_cached_distinguishes_a_missing_object_from_other_failures() {
    let dir = tempfile::tempdir().unwrap();
    let cache =
        ObjectStore::with_downloader(dir.path().to_path_buf(), Arc::new(NotFoundDownloader)).await;

    let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

    assert!(matches!(err, ObjectStoreError::NotFound));
}

#[tokio::test]
async fn pre_cache_downloads_every_key() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache = Arc::new(
        ObjectStore::with_downloader(dir.path().to_path_buf(), StubDownloader::new(&calls)).await,
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
    let cache = ObjectStore::new(dir.path().to_path_buf(), None, None)
        .await
        .unwrap();

    assert!(cache.list_remote("tracks/").await.is_empty());
}

#[tokio::test]
async fn list_remote_filters_by_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let downloader =
        StubDownloader::with_remote_keys(&["tracks/a.ogg", "tracks/b.ogg", "other/c.ogg"]);
    let cache = ObjectStore::with_downloader(dir.path().to_path_buf(), downloader).await;

    let mut keys = cache.list_remote("tracks/").await;
    keys.sort();

    assert_eq!(keys, ["tracks/a.ogg", "tracks/b.ogg"]);
}

#[tokio::test]
async fn list_remote_reuses_a_cached_result_for_the_same_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let list_calls = Arc::new(AtomicUsize::new(0));
    let downloader = StubDownloader::with_remote_keys_and_counter(
        &["tracks/a.ogg", "tracks/b.ogg"],
        &list_calls,
    );
    let cache = ObjectStore::with_downloader(dir.path().to_path_buf(), downloader).await;

    cache.list_remote("tracks/").await;
    cache.list_remote("tracks/").await;

    assert_eq!(list_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn list_remote_is_empty_when_listing_fails() {
    let dir = tempfile::tempdir().unwrap();
    let cache =
        ObjectStore::with_downloader(dir.path().to_path_buf(), Arc::new(FailingDownloader)).await;

    assert!(cache.list_remote("tracks/").await.is_empty());
}
