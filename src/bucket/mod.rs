pub mod cache;
pub mod resolve;

pub use cache::FileCache;
pub use resolve::FileResolver;

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

pub fn init_from_env() -> Option<Box<Bucket>> {
    let endpoint = std::env::var("BUCKET_ENDPOINT").ok()?;
    let access_key_id = std::env::var("BUCKET_ACCESS_KEY_ID").ok()?;
    let secret_access_key = std::env::var("BUCKET_SECRET_ACCESS_KEY").ok()?;
    let bucket_name = std::env::var("BUCKET_NAME").ok()?;

    let region = Region::Custom {
        region: "auto".to_string(),
        endpoint,
    };

    let credentials = Credentials::new(
        Some(&access_key_id),
        Some(&secret_access_key),
        None,
        None,
        None,
    )
    .map_err(|e| tracing::warn!("Failed to create bucket credentials: {e}"))
    .ok()?;

    Bucket::new(&bucket_name, region, credentials)
        .map(|b| b.with_path_style())
        .map_err(|e| tracing::warn!("Failed to create bucket: {e}"))
        .ok()
}
