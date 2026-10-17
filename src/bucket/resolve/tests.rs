use super::*;

#[tokio::test]
async fn dispatches_on_the_s3_prefix() {
    let dir = tempfile::tempdir().unwrap();
    let resolver = FileResolver::new(dir.path().to_string_lossy().to_string(), None);

    assert!(resolver.resolve("s3://tracks/song.ogg").await.is_err());
    assert!(
        resolver
            .resolve_contents("s3://tracks/song.ogg")
            .await
            .is_err()
    );

    std::fs::create_dir_all(dir.path().join("audio")).unwrap();
    std::fs::write(dir.path().join("audio/song.ogg"), b"hello world").unwrap();

    let resolved = resolver.resolve("audio/song.ogg").await.unwrap();
    assert_eq!(resolved, format!("{}/audio/song.ogg", dir.path().display()));

    let contents = resolver.resolve_contents("audio/song.ogg").await.unwrap();
    assert_eq!(contents, b"hello world");
}
