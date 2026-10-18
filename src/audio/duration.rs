use crate::bucket::FileResolver;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

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

    pub async fn get_duration(&self, filename: &str) -> Option<Duration> {
        {
            let cache = self.cache.read().await;
            if let Some(duration) = cache.get(filename) {
                tracing::trace!("Using cached duration for {}", filename);
                return *duration;
            }
        }

        tracing::debug!("Computing duration for {}", filename);
        let resolved = self
            .file_resolver
            .resolve(filename)
            .await
            .inspect_err(|e| {
                tracing::warn!("Failed to resolve {} for duration lookup: {}", filename, e)
            })
            .ok();

        let duration = resolved.and_then(|resolved| compute_audio_duration(&resolved));

        {
            let mut cache = self.cache.write().await;
            cache.insert(filename.to_string(), duration);
        }

        duration
    }
}

fn compute_audio_duration(filename: &str) -> Option<Duration> {
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let file = std::fs::File::open(filename).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = Path::new(filename).extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let format = symphonia::default::get_probe()
        .probe(
            &hint,
            mss,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .ok()?;

    let track = format.default_track(TrackType::Audio)?;
    let time = track.time_base?.calc_duration(track.duration?)?;

    Duration::try_from_secs_f64(time.as_secs_f64()).ok()
}
