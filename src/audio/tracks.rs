use crate::audio::codec::decoder::SymphoniaSource;
use crate::audio::processing::mixer::{
    AudioSource, StopReason, TrackCallback, TrackCommand, TrackHandle, TrackId,
};
use crate::audio::processing::processor::ProcessorRef;
use crate::state::Data;
use anyhow::{Context, bail};
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug)]
pub struct TrackSnapshot {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub loops: bool,
    pub start_time: SystemTime,
    pub duration: Option<Duration>,
}

pub struct TrackInfo {
    pub name: String,
    /// The raw filename or `s3://` key, not a resolved path — persisting the
    /// resolved form would break restoration after an object storage cache eviction.
    pub filename: String,
    pub mixer_handle: TrackHandle,
    pub volume: f32,
    pub loops: bool,
    pub start_time: SystemTime,
}

#[derive(Clone, Debug)]
pub struct StartTrackArgs {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    pub fade_time: f32,
    pub loops: bool,
    pub start_position: Option<Duration>,
}

pub struct TrackManager {
    tracks: HashMap<String, TrackInfo>,
    guild_id: GuildId,
    bot_state: Data,
    audio_processor: ProcessorRef,
}

impl TrackManager {
    pub fn new(guild_id: GuildId, bot_state: Data, processor: ProcessorRef) -> Self {
        Self {
            tracks: HashMap::new(),
            guild_id,
            bot_state,
            audio_processor: processor,
        }
    }

    pub async fn start_track(&mut self, args: StartTrackArgs) -> anyhow::Result<()> {
        let resolved_filename = self.bot_state.file_resolver.resolve(&args.filename).await?;
        let source = open_track_source(&resolved_filename, args.start_position)?;
        if self.tracks.contains_key(&args.name) {
            self.stop_track(&args.name, args.fade_time).await?;
        }
        let duration = self
            .bot_state
            .duration_cache
            .get_duration(&args.filename)
            .await;

        let position_info = if let Some(pos) = args.start_position {
            format!(", starting at: {:.2}s", pos.as_secs_f64())
        } else {
            String::new()
        };

        tracing::info!(
            "Starting track '{}' (file: {}, duration: {:.2}s, volume: {}, fade: {}s, loops: {}{}) in guild {}",
            args.name,
            resolved_filename,
            duration.map(|d| d.as_secs_f64()).unwrap_or(-1.0),
            args.volume,
            args.fade_time,
            args.loops,
            position_info,
            self.guild_id
        );

        let (ended_tx, ended) = TrackCallback::oneshot();
        let mixer_handle = self.audio_processor.write().await.add_track(
            source,
            0.0,
            vec![
                TrackCommand::SetLoops(args.loops),
                TrackCommand::SetVolume {
                    volume: args.volume,
                    fade: Duration::from_secs_f32(args.fade_time.max(0.0)),
                    at: None,
                },
                TrackCommand::AddStopTrigger(ended_tx),
            ],
        );

        let guild_id = self.guild_id;
        let bot_state = self.bot_state;
        let name = args.name.clone();
        let ended_track = mixer_handle.clone();
        tokio::spawn(async move {
            let reason = ended.await.unwrap_or_else(|e| {
                tracing::error!("Track '{name}' lost its end signal: {e}");
                StopReason::Lost
            });
            tracing::debug!("Track '{name}' left the mixer in guild {guild_id}: {reason:?}");
            if let Some(guild) = bot_state.guild(guild_id) {
                guild
                    .track_manager
                    .lock()
                    .await
                    .remove_handle(ended_track.id)
                    .await;
            }
        });

        // If we're starting from a position other than the beginning, adjust start_time
        // backwards so that elapsed time calculations include the seek position
        let start_time = if let Some(pos) = args.start_position {
            SystemTime::now() - pos
        } else {
            SystemTime::now()
        };

        let track = TrackInfo {
            name: args.name.clone(),
            filename: args.filename.clone(),
            mixer_handle,
            volume: args.volume,
            loops: args.loops,
            start_time,
        };

        self.tracks.insert(args.name, track);

        self.persist_state().await;

        Ok(())
    }

    pub fn has_track(&self, name: &str) -> bool {
        self.tracks.contains_key(name)
    }

    pub async fn update_track_volume(
        &mut self,
        name: &str,
        volume: f32,
        fade_time: f32,
    ) -> anyhow::Result<()> {
        let track_info = self
            .tracks
            .get_mut(name)
            .with_context(|| format!("Track '{name}' not found"))?;

        tracing::info!(
            "Updating track '{}' volume from {} to {} (fade: {}s) in guild {}",
            name,
            track_info.volume,
            volume,
            fade_time,
            self.guild_id
        );

        track_info.volume = volume;

        let set_volume = TrackCommand::SetVolume {
            volume,
            fade: Duration::from_secs_f32(fade_time.max(0.0)),
            at: None,
        };
        if !track_info
            .mixer_handle
            .send(set_volume)
            .with_context(|| format!("Track '{name}'"))?
        {
            bail!("Track '{name}' has ended");
        }

        self.persist_state().await;

        Ok(())
    }

    pub async fn update_track_loops(&mut self, name: &str, loops: bool) -> anyhow::Result<()> {
        let track_info = self
            .tracks
            .get_mut(name)
            .with_context(|| format!("Track '{name}' not found"))?;

        if track_info.loops == loops {
            return Ok(());
        }

        tracing::info!(
            "Updating track '{}' loops from {} to {} in guild {}",
            name,
            track_info.loops,
            loops,
            self.guild_id
        );

        track_info.loops = loops;

        let set_loops = TrackCommand::SetLoops(loops);
        if !track_info
            .mixer_handle
            .send(set_loops)
            .with_context(|| format!("Track '{name}'"))?
        {
            bail!("Track '{name}' has ended");
        }

        self.persist_state().await;

        Ok(())
    }

    async fn remove_handle(&mut self, handle: TrackId) {
        let info = self
            .tracks
            .extract_if(|_k, info| info.mixer_handle.id == handle)
            .map(|(_, info)| info)
            .next();
        if let Some(info) = info {
            tracing::debug!("Removed track '{}' from guild {}", info.name, self.guild_id);
            self.persist_state().await;
        }
    }

    pub async fn stop_track(&mut self, name: &str, fade_time: f32) -> anyhow::Result<()> {
        let track_info = self
            .tracks
            .get(name)
            .with_context(|| format!("Track '{name}' not found"))?;

        tracing::info!(
            "Stopping track '{}' (fade: {}s) in guild {}",
            name,
            fade_time,
            self.guild_id
        );

        let stop = TrackCommand::Stop {
            fade: Duration::from_secs_f32(fade_time.max(0.0)),
            at: None,
        };
        if !track_info
            .mixer_handle
            .send(stop)
            .with_context(|| format!("Track '{name}'"))?
        {
            bail!("Track '{name}' has ended");
        }

        self.tracks.remove(name);
        self.persist_state().await;

        Ok(())
    }

    pub async fn get_all_tracks(&self) -> Vec<TrackSnapshot> {
        let mut snapshots = Vec::new();
        for info in self.tracks.values() {
            let duration = self
                .bot_state
                .duration_cache
                .get_duration(&info.filename)
                .await;
            snapshots.push(TrackSnapshot {
                name: info.name.clone(),
                filename: info.filename.clone(),
                volume: info.volume,
                loops: info.loops,
                start_time: info.start_time,
                duration,
            });
        }
        snapshots
    }

    async fn persist_state(&self) {
        let state = crate::persistence::MultiTrackPlaybackState {
            tracks: self
                .tracks
                .values()
                .map(|info| crate::persistence::TrackState {
                    name: info.name.clone(),
                    filename: info.filename.clone(),
                    volume: info.volume,
                    loops: info.loops,
                    start_time: Some(info.start_time),
                })
                .collect(),
        };

        if let Err(e) = self
            .bot_state
            .state_store
            .save_multitrack_playback(self.guild_id, &state)
            .await
        {
            tracing::warn!("Failed to persist multitrack state: {}", e);
        }
    }
}

pub fn open_track_source(
    path: &str,
    start_position: Option<Duration>,
) -> anyhow::Result<Box<dyn AudioSource>> {
    let mut source = SymphoniaSource::from_file(path, 48000)?;
    if let Some(pos) = start_position {
        source.seek(pos)?;
    }
    Ok(Box::new(source))
}

/// Get or create a TrackManager for a guild, ensuring audio processor exists
pub async fn get_or_create_track_manager(
    bot_state: &Data,
    guild_id: GuildId,
) -> Arc<tokio::sync::Mutex<TrackManager>> {
    bot_state.guild_or_create(guild_id).track_manager.clone()
}
