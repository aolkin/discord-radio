use crate::bucket::FileResolver;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Caches audio durations, keyed by the raw track identifier (a content-relative
/// path or an `s3://` key) rather than a resolved filesystem path. Keying on the
/// raw identifier lets a cache hit return without resolving anything.
#[derive(Clone)]
pub struct DurationCache {
    cache: Arc<RwLock<HashMap<String, Option<Duration>>>>,
    file_resolver: FileResolver,
}

impl DurationCache {
    pub fn new(file_resolver: FileResolver) -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            file_resolver,
        }
    }

    /// Returns the duration for `filename`, a raw track identifier (a
    /// content-relative path or an `s3://` key). On a cache miss, resolves it
    /// to a local path before computing.
    ///
    /// A resolution failure is not cached, since it can be transient (e.g. a
    /// dropped R2 fetch), unlike a resolved file with no readable duration,
    /// which is cached as `None` the same way `compute_audio_duration`'s other
    /// failure cases are.
    pub async fn get_duration(&self, filename: &str) -> Option<Duration> {
        {
            let cache = self.cache.read().await;
            if let Some(duration) = cache.get(filename) {
                tracing::trace!("Using cached duration for {}", filename);
                return *duration;
            }
        }

        tracing::debug!("Computing duration for {}", filename);
        let resolved = match self.file_resolver.resolve(filename).await {
            Ok(resolved) => resolved,
            Err(e) => {
                tracing::warn!("Failed to resolve {} for duration lookup: {}", filename, e);
                return None;
            }
        };

        let duration = compute_audio_duration(&resolved);

        {
            let mut cache = self.cache.write().await;
            cache.insert(filename.to_string(), duration);
        }

        duration
    }
}

fn compute_audio_duration(filename: &str) -> Option<Duration> {
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let file = std::fs::File::open(filename).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(filename).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format_opts = FormatOptions::default();
    let metadata_opts = MetadataOptions::default();

    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &format_opts, &metadata_opts)
        .ok()?;

    let format = probed.format;
    let track = format.default_track()?;

    if let Some(time_base) = track.codec_params.time_base
        && let Some(n_frames) = track.codec_params.n_frames
    {
        let seconds = time_base.calc_time(n_frames).seconds;
        let frac = time_base.calc_time(n_frames).frac;

        let duration_secs = seconds as f64 + frac;
        return Some(Duration::from_secs_f64(duration_secs));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bucket::FileCache;

    #[tokio::test]
    async fn resolution_failure_is_not_cached_but_a_computed_none_is() {
        let content_dir = tempfile::tempdir().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let file_cache = Arc::new(
            FileCache::new(cache_dir.path().to_path_buf(), None, None)
                .await
                .unwrap(),
        );
        let file_resolver =
            FileResolver::new(content_dir.path().to_string_lossy().to_string(), file_cache);
        let duration_cache = DurationCache::new(file_resolver);

        // Local path: resolves fine, but the file isn't valid audio, so
        // duration computation fails. That `None` is a real answer and gets
        // cached.
        std::fs::write(content_dir.path().join("track.bin"), b"not audio").unwrap();
        assert_eq!(duration_cache.get_duration("track.bin").await, None);
        assert!(duration_cache.cache.read().await.contains_key("track.bin"));

        // s3:// key against an unconfigured file cache: resolution itself
        // fails, so nothing is cached and a later retry can still succeed.
        assert_eq!(
            duration_cache.get_duration("s3://tracks/song.ogg").await,
            None
        );
        assert!(
            !duration_cache
                .cache
                .read()
                .await
                .contains_key("s3://tracks/song.ogg")
        );
    }
}
