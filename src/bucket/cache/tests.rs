use super::*;
use std::fs::FileTimes;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::SystemTime;

struct StubBackend {
    calls: Arc<AtomicUsize>,
    list_calls: Arc<AtomicUsize>,
    payload: Vec<u8>,
    remote_keys: Vec<String>,
}

impl StubBackend {
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
impl ObjectBackend for StubBackend {
    async fn download(&self, _key: &str) -> anyhow::Result<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.payload.clone())
    }

    async fn list_dir(&self, _prefix: &str) -> anyhow::Result<DirListing> {
        Ok(DirListing::default())
    }

    async fn list_keys(&self, prefix: &str) -> anyhow::Result<Vec<String>> {
        self.list_calls.fetch_add(1, Ordering::SeqCst);
        Ok(self
            .remote_keys
            .iter()
            .filter(|key| key.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn upload(&self, _key: &str, _bytes: &[u8], _content_type: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

struct FailingBackend;

#[async_trait]
impl ObjectBackend for FailingBackend {
    async fn download(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        Err(anyhow::anyhow!("no such object: {key}"))
    }

    async fn list_dir(&self, _prefix: &str) -> anyhow::Result<DirListing> {
        Err(anyhow::anyhow!("listing failed"))
    }

    async fn list_keys(&self, _prefix: &str) -> anyhow::Result<Vec<String>> {
        Err(anyhow::anyhow!("listing failed"))
    }

    async fn upload(&self, key: &str, _bytes: &[u8], _content_type: &str) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("upload failed: {key}"))
    }
}

struct NotFoundBackend;

#[async_trait]
impl ObjectBackend for NotFoundBackend {
    async fn download(&self, _key: &str) -> anyhow::Result<Vec<u8>> {
        Err(NotFound.into())
    }

    async fn list_dir(&self, _prefix: &str) -> anyhow::Result<DirListing> {
        Ok(DirListing::default())
    }

    async fn list_keys(&self, _prefix: &str) -> anyhow::Result<Vec<String>> {
        Ok(Vec::new())
    }

    async fn upload(&self, _key: &str, _bytes: &[u8], _content_type: &str) -> anyhow::Result<()> {
        Ok(())
    }
}

fn write_cached_file(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

async fn bucket(cache_dir: PathBuf, backend: Arc<dyn ObjectBackend>) -> Bucket {
    Bucket::build(cache_dir, backend, None).await.unwrap()
}

#[tokio::test]
async fn new_creates_the_cache_dir() {
    let dir = tempfile::tempdir().unwrap();
    let cache_dir = dir.path().join("nested/cache");
    assert!(!cache_dir.exists());

    bucket(cache_dir.clone(), Arc::new(NotFoundBackend)).await;

    assert!(cache_dir.is_dir());
}

#[tokio::test]
async fn ensure_cached_downloads_and_returns_the_local_path() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache = bucket(dir.path().to_path_buf(), StubBackend::new(&calls)).await;

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

    let cache = bucket(dir.path().to_path_buf(), StubBackend::new(&calls)).await;
    cache.ensure_cached("tracks/song.ogg").await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn new_deletes_a_partial_download_without_adopting_it() {
    let dir = tempfile::tempdir().unwrap();
    let partial = temp_path(&dir.path().join("tracks/song.ogg"));
    write_cached_file(&partial, b"half a s");

    let cache = bucket(dir.path().to_path_buf(), Arc::new(NotFoundBackend)).await;

    assert!(!partial.exists());
    assert!(
        cache
            .ensure_cached("tracks/song.ogg.tmp")
            .await
            .unwrap_err()
            .is::<NotFound>()
    );
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

    let cache = Bucket::build(
        dir.path().to_path_buf(),
        Arc::new(NotFoundBackend),
        Some(Duration::from_secs(60)),
    )
    .await
    .unwrap();

    assert!(!stale.exists());
    assert!(fresh.exists());
    cache.ensure_cached("tracks/fresh.ogg").await.unwrap();
    assert!(
        cache
            .ensure_cached("tracks/stale.ogg")
            .await
            .unwrap_err()
            .is::<NotFound>()
    );
}

#[tokio::test]
async fn ensure_cached_re_downloads_after_upload() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache = bucket(dir.path().to_path_buf(), StubBackend::new(&calls)).await;

    cache.ensure_cached("tracks/song.ogg").await.unwrap();
    cache
        .upload("tracks/song.ogg", b"new", "audio/ogg")
        .await
        .unwrap();
    cache.ensure_cached("tracks/song.ogg").await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn ensure_cached_does_not_leave_a_temp_file_behind() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache = bucket(dir.path().to_path_buf(), StubBackend::new(&calls)).await;

    cache.ensure_cached("tracks/song.ogg").await.unwrap();

    assert!(!temp_path(&cache.cached_path("tracks/song.ogg")).exists());
}

#[tokio::test]
async fn fill_does_not_leave_a_partial_file_when_the_write_fails() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let cache = bucket(dir.path().to_path_buf(), StubBackend::new(&calls)).await;
    let dest = cache.cached_path("tracks/song.ogg");
    tokio::fs::create_dir_all(dest.parent().unwrap())
        .await
        .unwrap();

    // Occupy the temp path with a directory so the write into it fails.
    tokio::fs::create_dir(temp_path(&dest)).await.unwrap();

    let err = cache.ensure_cached("tracks/song.ogg").await.unwrap_err();

    assert!(!err.is::<NotFound>());
    assert!(!dest.exists());
}

#[tokio::test]
async fn ensure_cached_surfaces_download_failures() {
    let dir = tempfile::tempdir().unwrap();
    let cache = bucket(dir.path().to_path_buf(), Arc::new(FailingBackend)).await;

    let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

    assert!(!err.is::<NotFound>());
}

#[tokio::test]
async fn ensure_cached_distinguishes_a_missing_object_from_other_failures() {
    let dir = tempfile::tempdir().unwrap();
    let cache = bucket(dir.path().to_path_buf(), Arc::new(NotFoundBackend)).await;

    let err = cache.ensure_cached("tracks/missing.ogg").await.unwrap_err();

    assert!(err.is::<NotFound>());
}

#[tokio::test]
async fn list_remote_filters_by_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let backend = StubBackend::with_remote_keys(&["tracks/a.ogg", "tracks/b.ogg", "other/c.ogg"]);
    let cache = bucket(dir.path().to_path_buf(), backend).await;

    let mut keys = cache.list_remote("tracks/").await;
    keys.sort();

    assert_eq!(keys, ["tracks/a.ogg", "tracks/b.ogg"]);
}

#[tokio::test]
async fn list_remote_reuses_a_cached_result_for_the_same_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let list_calls = Arc::new(AtomicUsize::new(0));
    let backend =
        StubBackend::with_remote_keys_and_counter(&["tracks/a.ogg", "tracks/b.ogg"], &list_calls);
    let cache = bucket(dir.path().to_path_buf(), backend).await;

    cache.list_remote("tracks/").await;
    cache.list_remote("tracks/").await;

    assert_eq!(list_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn list_remote_is_empty_when_listing_fails() {
    let dir = tempfile::tempdir().unwrap();
    let cache = bucket(dir.path().to_path_buf(), Arc::new(FailingBackend)).await;

    assert!(cache.list_remote("tracks/").await.is_empty());
}
