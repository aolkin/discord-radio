use crate::bucket::Bucket;
use anyhow::Context;

/// Marks a failure as caused by no object storage bucket being configured.
#[derive(Debug, thiserror::Error)]
#[error("object storage is not configured")]
pub struct NoBucket;

/// Resolves a playlist entry's `filename` to a local filesystem path.
///
/// A `s3://`-prefixed filename is an object storage key: the prefix is stripped
/// and the remainder is fetched into the file cache if not already cached.
/// Anything else is joined with the content path directly.
#[derive(Clone)]
pub struct FileResolver {
    content_path: String,
    bucket: Option<Bucket>,
}

impl FileResolver {
    pub fn new(content_path: String, bucket: Option<Bucket>) -> Self {
        Self {
            content_path,
            bucket,
        }
    }

    pub fn content_path(&self) -> &str {
        &self.content_path
    }

    pub fn bucket(&self) -> anyhow::Result<&Bucket> {
        self.bucket.as_ref().ok_or_else(|| NoBucket.into())
    }

    pub async fn resolve(&self, filename: &str) -> anyhow::Result<String> {
        if let Some(key) = filename.strip_prefix("s3://") {
            Ok(self
                .bucket()?
                .ensure_cached(key)
                .await?
                .to_string_lossy()
                .to_string())
        } else {
            Ok(format!("{}/{filename}", self.content_path))
        }
    }

    pub async fn resolve_contents(&self, filename: &str) -> anyhow::Result<Vec<u8>> {
        if let Some(key) = filename.strip_prefix("s3://") {
            self.bucket()?.fetch_bytes(key).await
        } else {
            let path = format!("{}/{filename}", self.content_path);
            tokio::fs::read(&path)
                .await
                .with_context(|| format!("reading {path}"))
        }
    }
}

#[cfg(test)]
mod tests;
