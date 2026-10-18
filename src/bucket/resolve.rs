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

    /// Filenames starting with `prefix`, from the content path and from object
    /// storage when configured. Local directories end in `/`; object storage
    /// keys carry the `s3://` prefix and cover every key under `prefix`.
    pub async fn list(&self, prefix: &str) -> Vec<String> {
        let (dir_part, name_part) = match prefix.rfind('/') {
            Some(pos) => prefix.split_at(pos + 1),
            None => ("", prefix),
        };

        let mut filenames = Vec::new();
        if let Ok(entries) = std::fs::read_dir(format!("{}/{dir_part}", self.content_path)) {
            for entry in entries.flatten() {
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !name.starts_with(name_part) {
                    continue;
                }
                if entry.path().is_dir() {
                    filenames.push(format!("{dir_part}{name}/"));
                } else {
                    filenames.push(format!("{dir_part}{name}"));
                }
            }
        }

        if let Ok(bucket) = self.bucket() {
            let key_prefix = prefix.strip_prefix("s3://").unwrap_or(prefix);
            filenames.extend(
                bucket
                    .list_remote(key_prefix)
                    .await
                    .into_iter()
                    .map(|key| format!("s3://{key}")),
            );
        }
        filenames
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
