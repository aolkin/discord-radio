use super::*;
use crate::bucket::ObjectStore;
use std::sync::Arc;

async fn resolver(content_path: &std::path::Path) -> crate::bucket::FileResolver {
    let file_cache = Arc::new(
        ObjectStore::new(content_path.to_path_buf(), None, None)
            .await
            .unwrap(),
    );
    crate::bucket::FileResolver::new(content_path.to_string_lossy().to_string(), file_cache)
}

#[tokio::test]
async fn validate_content_uri_checks_the_file_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("pool.json"), "[]").unwrap();
    let resolver = resolver(dir.path()).await;

    assert!(validate_content_uri(&resolver, "pool.json").await.is_ok());
    assert!(
        validate_content_uri(&resolver, "missing.json")
            .await
            .is_err()
    );
}
