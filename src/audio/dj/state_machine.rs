use crate::audio::dj::config::{DJConfig, HexMessageEntry, NoisePeriodEntry, TrackEntry};
use crate::audio::dj::manager::DJStateType;
use crate::audio::dj::scheduler::{DJStateEntry, WeightedScheduler};
use crate::audio::dj::segments::SegmentCtx;
use crate::audio::dj::segments::{NoiseSegment, Segment, SignalProfilePlayback};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::tracks::{StartTrackArgs, TrackManager};
use crate::state::{BotState, Data};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, RwLockReadGuard};
use tracing::info;

pub fn format_dj_track_name(filename: &str) -> String {
    format!("dj_track_{}", filename)
}

#[derive(Debug, Serialize, Deserialize)]
pub enum DJState {
    PlayingTrack {
        filename: String,
        volume: f32,
        #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
        started_at: std::time::Instant,
        duration: Duration,
        forced_profile: Option<String>,
        status_message: Option<String>, // The channel status pushed to status stack
    },
    PlayingHexMessage {
        message: String,
        #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
        started_at: std::time::Instant,
        target_loops: usize,
        forced_profile: Option<String>,
        status_message: Option<String>, // The obfuscated message pushed to status stack
    },
    Segment(Box<dyn Segment>),
    Stopped,
}

impl<S: Segment + 'static> From<S> for DJState {
    fn from(segment: S) -> Self {
        DJState::Segment(Box::new(segment))
    }
}

impl DJState {
    pub fn idle() -> Self {
        NoiseSegment::idle(Duration::from_secs(1)).into()
    }

    pub async fn is_complete(&self, ctx: &SegmentCtx<'_>) -> bool {
        match self {
            DJState::PlayingTrack {
                started_at,
                duration,
                ..
            } => started_at.elapsed() >= *duration,
            DJState::PlayingHexMessage { .. } => false,
            DJState::Segment(segment) => segment.is_complete(ctx).await,
            DJState::Stopped => true,
        }
    }

    pub fn forced_profile(&self) -> SignalProfilePlayback {
        match self {
            DJState::PlayingTrack { forced_profile, .. }
            | DJState::PlayingHexMessage { forced_profile, .. } => forced_profile
                .as_ref()
                .map(|profile| profile.into())
                .unwrap_or_default(),
            DJState::Segment(segment) => segment.signal_profile(),
            DJState::Stopped => Default::default(),
        }
    }
}

pub struct DJStateMachine {
    state: Arc<RwLock<DJState>>,
    scheduler: WeightedScheduler,
    guild_id: GuildId,
    announcement_channel: Option<ChannelId>,
    http: Arc<Http>,
    hex_message_announcements: Vec<String>,
    bot_state: Arc<BotState>,
}

impl DJStateMachine {
    /// Constructs the state machine around the guild's shared state cell.
    pub fn new(
        config: DJConfig,
        guild_id: GuildId,
        announcement_channel: Option<ChannelId>,
        http: Arc<Http>,
        state: Arc<RwLock<DJState>>,
        bot_state: Arc<BotState>,
    ) -> Self {
        let hex_message_announcements =
            config.hex_message_announcements.clone().unwrap_or_default();

        Self {
            state,
            scheduler: WeightedScheduler::new(config),
            guild_id,
            announcement_channel,
            http,
            hex_message_announcements,
            bot_state,
        }
    }

    fn segment_ctx(&self) -> SegmentCtx<'_> {
        SegmentCtx {
            guild_id: &self.guild_id,
            bot_state: &self.bot_state,
            http: &self.http,
        }
    }

    pub async fn current_state(&self) -> RwLockReadGuard<'_, DJState> {
        self.state.read().await
    }

    pub fn update_config(&mut self, config: DJConfig) {
        self.hex_message_announcements =
            config.hex_message_announcements.clone().unwrap_or_default();
        self.scheduler.update_config(config);
    }

    pub fn set_announcement_channel(&mut self, channel: Option<ChannelId>) {
        self.announcement_channel = channel;
    }

    pub async fn stop(&mut self, track_manager: &mut TrackManager, bot_state: &Data) {
        // Clean up current state before stopping
        if let Err(e) = self.cleanup_current_state(track_manager, bot_state).await {
            tracing::error!("Error cleaning up DJ state during stop: {}", e);
        }
        *self.state.write().await = DJState::Stopped;

        // Log the stop event
        if let Err(e) = self.log_state_transition(bot_state).await {
            tracing::error!("Failed to log DJ state transition: {}", e);
        }
    }

    pub async fn force_advance(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
        state_type_filter: Option<DJStateType>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Self { scheduler, .. } = self;
        let next_state_type = scheduler.next_state(state_type_filter);
        self.transition_to_state(next_state_type, track_manager, bot_state)
            .await
    }

    pub async fn force_hex_message(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
        message: String,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.cleanup_current_state(track_manager, bot_state).await?;

        info!(
            "DJ force transitioning to hex message with custom text: {}",
            message
        );
        let next_state = self
            .start_custom_hex_message_state(message, bot_state)
            .await?;
        *self.state.write().await = next_state;

        // Log the forced hex message
        if let Err(e) = self.log_state_transition(bot_state).await {
            tracing::error!("Failed to log DJ state transition: {}", e);
        }

        Ok(())
    }

    pub async fn advance(
        &mut self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ctx = self.segment_ctx();
        if !self.state.read().await.is_complete(&ctx).await {
            return Ok(());
        }

        self.force_advance(track_manager, bot_state, None).await
    }

    async fn transition_to_state(
        &self,
        next_state_type: DJStateEntry,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Record the previous state before transitioning
        let from_state = self.get_state_name(&*self.current_state().await);

        self.cleanup_current_state(track_manager, bot_state).await?;

        info!("DJ transitioning to state: {next_state_type}");
        let next_state = match next_state_type {
            DJStateEntry::Track(entry) => {
                self.start_track_state(entry, track_manager, bot_state)
                    .await
            }
            DJStateEntry::HexMessage(entry) => self.start_hex_message_state(entry, bot_state).await,
            DJStateEntry::Noise(entry) => self.start_noise_state(entry, bot_state).await,
        }?;

        // Record state transition metric
        let to_state = self.get_state_name(&next_state);
        *self.state.write().await = next_state;
        if let Some(metrics) = bot_state.metrics.read().await.as_ref() {
            metrics.record_dj_state_transition(self.guild_id.get(), &from_state, &to_state);
        }

        // Log the DJ state transition
        if let Err(e) = self.log_state_transition(bot_state).await {
            tracing::error!("Failed to log DJ state transition: {}", e);
        }

        Ok(())
    }

    async fn cleanup_current_state(
        &self,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current = self.state.read().await;
        match &*current {
            DJState::PlayingTrack {
                filename,
                status_message,
                started_at,
                ..
            } => {
                let track_name = format_dj_track_name(filename);
                // Record duration metric
                let duration_secs = started_at.elapsed().as_secs_f64();
                if let Some(metrics) = bot_state.metrics.read().await.as_ref() {
                    metrics.record_dj_state_duration(
                        self.guild_id.get(),
                        "playing_track",
                        duration_secs,
                    );
                }

                if track_manager.has_track(&track_name) {
                    track_manager.stop_track(&track_name, 1.0, false).await?;
                }

                // Remove the track status from the stack if present
                if let Some(status_msg) = status_message {
                    bot_state
                        .voice_status_manager
                        .remove_status(self.guild_id, status_msg, &self.http)
                        .await;
                }
            }
            DJState::PlayingHexMessage {
                status_message,
                started_at,
                target_loops,
                ..
            } => {
                // Record duration metric
                let duration_secs = started_at.elapsed().as_secs_f64();
                if let Some(metrics) = bot_state.metrics.read().await.as_ref() {
                    metrics.record_dj_state_duration(
                        self.guild_id.get(),
                        "playing_hex_message",
                        duration_secs,
                    );
                    metrics.record_hex_message_completed(self.guild_id.get(), *target_loops as u64);
                }

                // Reset the hex playback state
                let hex_playback_states = bot_state.hex_playback_states.read().await;
                if let Some(state_arc) = hex_playback_states.get(&self.guild_id) {
                    let mut state = state_arc.write().await;
                    *state = crate::state::HexPlaybackState::stopped();
                }
                drop(hex_playback_states);

                // Remove the hex message status from the stack
                if let Some(status_msg) = status_message {
                    bot_state
                        .voice_status_manager
                        .remove_status(self.guild_id, status_msg, &self.http)
                        .await;
                }

                // Remove persisted message playback state
                if let Err(e) = bot_state
                    .state_store
                    .remove_message_playback(self.guild_id)
                    .await
                {
                    tracing::warn!("Failed to remove message playback state: {}", e);
                }
            }
            DJState::Segment(segment) => {
                segment.exit(&self.segment_ctx()).await?;
            }
            DJState::Stopped => {}
        }

        Ok(())
    }

    async fn start_track_state(
        &self,
        track_entry: TrackEntry,
        track_manager: &mut TrackManager,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let track_name = format_dj_track_name(&track_entry.filename);

        let duration = bot_state
            .duration_cache
            .get_duration(&track_entry.filename)
            .await;
        let (start_position, play_duration) = if track_entry.allow_subsection.unwrap_or(false) {
            if let Some(total_duration) = duration {
                if total_duration.as_secs_f32() > 240.0 {
                    let mut rng = rand::rng();
                    let subsection_duration =
                        Duration::from_secs_f32(rng.random_range(120.0..240.0));
                    let latest_start =
                        total_duration.as_secs_f32() - subsection_duration.as_secs_f32();
                    let start_secs = rng.random_range(0.0..latest_start.max(0.0));
                    (
                        Some(Duration::from_secs_f32(start_secs)),
                        subsection_duration,
                    )
                } else {
                    (None, duration.unwrap_or(Duration::from_secs(60)))
                }
            } else {
                (None, Duration::from_secs(60))
            }
        } else {
            let max_dur = track_entry
                .max_duration_seconds
                .map(Duration::from_secs_f32)
                .or(duration)
                .unwrap_or(Duration::from_secs(60));
            (None, max_dur)
        };

        let volume = track_entry.volume.unwrap_or(1.0);

        track_manager
            .start_track(StartTrackArgs {
                name: track_name.clone(),
                filename: track_entry.filename.clone(),
                volume,
                fade_time: 1.0,
                loops: false,
                start_position,
                persist: false,
            })
            .await?;

        tracing::info!(
            "DJ starting track '{}' (duration: {:.1}s) in guild {}",
            track_entry.filename,
            play_duration.as_secs_f32(),
            self.guild_id
        );

        // Push track channel status onto the voice channel status stack if configured
        let status_message = if let Some(ref status) = track_entry.channel_status {
            let starts_with_emoji = status.chars().next().is_some_and(|c| {
                matches!(c as u32,
                    0x1F300..=0x1F9FF | // Misc Symbols and Pictographs, Emoticons, Transport and Map, Supplemental Symbols
                    0x2600..=0x26FF |   // Misc symbols
                    0x2700..=0x27BF |   // Dingbats
                    0xFE00..=0xFE0F |   // Variation Selectors
                    0x1F000..=0x1F02F | // Mahjong Tiles, Domino Tiles
                    0x1F0A0..=0x1F0FF | // Playing Cards
                    0x1F100..=0x1F64F   // Enclosed characters, Emoticons
                )
            });
            let status_with_emoji = if starts_with_emoji {
                status.clone()
            } else {
                format!("🔊 {}", status)
            };
            bot_state
                .voice_status_manager
                .push_status(self.guild_id, &status_with_emoji, &self.http)
                .await;
            Some(status_with_emoji)
        } else {
            None
        };

        Ok(DJState::PlayingTrack {
            filename: track_entry.filename.clone(),
            volume,
            started_at: std::time::Instant::now(),
            duration: play_duration,
            forced_profile: track_entry.signal_profile.clone(),
            status_message,
        })
    }

    async fn start_hex_message_state(
        &self,
        hex_entry: HexMessageEntry,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let loop_min = hex_entry
            .loop_min
            .unwrap_or(self.scheduler.config().hex_message_defaults.loop_min);
        let loop_max = hex_entry
            .loop_max
            .unwrap_or(self.scheduler.config().hex_message_defaults.loop_max);

        let forced_profile = hex_entry.signal_profile.clone().or(self
            .scheduler
            .config()
            .hex_message_defaults
            .signal_profile
            .clone());

        // Use custom announcement if present, otherwise choose randomly from defaults
        let announcement = if let Some(custom_announcement) = &hex_entry.announcement {
            Some(custom_announcement.clone())
        } else {
            self.pick_random_announcement()
        };

        self.play_hex_message(
            hex_entry.text.clone(),
            loop_min,
            loop_max,
            forced_profile,
            announcement,
            bot_state,
        )
        .await
    }

    async fn start_custom_hex_message_state(
        &mut self,
        message: String,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        // Use defaults from the DJ config
        let loop_min = self.scheduler.config().hex_message_defaults.loop_min;
        let loop_max = self.scheduler.config().hex_message_defaults.loop_max;
        let forced_profile = self
            .scheduler
            .config()
            .hex_message_defaults
            .signal_profile
            .clone();

        // Use random default announcement if available
        let announcement = self.pick_random_announcement();

        self.play_hex_message(
            message,
            loop_min,
            loop_max,
            forced_profile,
            announcement,
            bot_state,
        )
        .await
    }

    fn pick_random_announcement(&self) -> Option<String> {
        if !self.hex_message_announcements.is_empty() {
            let mut rng = rand::rng();
            let announcement_idx = rng.random_range(0..self.hex_message_announcements.len());
            Some(self.hex_message_announcements[announcement_idx].clone())
        } else {
            None
        }
    }

    async fn play_hex_message(
        &self,
        message: String,
        loop_min: u32,
        loop_max: u32,
        forced_profile: Option<String>,
        announcement: Option<String>,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let target_loops = {
            let mut rng = rand::rng();
            if loop_min == loop_max {
                loop_min as usize
            } else {
                rng.random_range(loop_min..=loop_max) as usize
            }
        };

        let track_managers = bot_state.track_managers.read().await;
        let manager_arc = track_managers
            .get(&self.guild_id)
            .ok_or("Track manager not found for guild")?
            .clone();
        drop(track_managers);

        let hex_playback_state = bot_state
            .hex_playback_states
            .write()
            .await
            .entry(self.guild_id)
            .or_insert_with(|| {
                Arc::new(tokio::sync::RwLock::new(
                    crate::state::HexPlaybackState::stopped(),
                ))
            })
            .clone();

        self.ensure_hex_playback_task(bot_state, manager_arc.clone(), hex_playback_state.clone())
            .await;

        tracing::info!(
            "DJ playing hex message '{}' ({} loops) in guild {}",
            message,
            target_loops,
            self.guild_id
        );

        // Record hex message started metric
        if let Some(metrics) = bot_state.metrics.read().await.as_ref() {
            metrics.record_hex_message_started(self.guild_id.get());
        }

        // Push hex message status onto the voice channel status stack
        let obfuscated = crate::commands::voice::obfuscate_message(&message);
        bot_state
            .voice_status_manager
            .push_status(self.guild_id, &obfuscated, &self.http)
            .await;

        {
            let mut state = hex_playback_state.write().await;
            *state = crate::state::HexPlaybackState::playing(
                message.clone(),
                0,
                1.0,
                Some(target_loops),
                Some(obfuscated.clone()),
            );
        }

        // Send announcement to text channel if configured
        if let Some(channel_id) = self.announcement_channel
            && let Some(announcement_text) = announcement
        {
            let http_clone = self.http.clone();
            tokio::spawn(async move {
                if let Err(e) = channel_id.say(&http_clone, &announcement_text).await {
                    tracing::warn!("Failed to send DJ hex message announcement: {}", e);
                }
            });
        }

        Ok(DJState::PlayingHexMessage {
            message,
            started_at: std::time::Instant::now(),
            target_loops,
            forced_profile,
            status_message: Some(obfuscated),
        })
    }

    async fn ensure_hex_playback_task(
        &self,
        bot_state: &Data,
        manager_arc: Arc<tokio::sync::Mutex<TrackManager>>,
        playback_state: Arc<tokio::sync::RwLock<crate::state::HexPlaybackState>>,
    ) {
        let mut tasks = bot_state.hex_playback_tasks.write().await;
        if tasks.contains_key(&self.guild_id) {
            return;
        }

        let guild_id_copy = self.guild_id;
        let manager_copy = manager_arc.clone();
        let playback_state_copy = playback_state.clone();
        let bot_state_copy = bot_state.clone();

        let handle = tokio::spawn(async move {
            crate::audio::manager::hex_playback_task(
                guild_id_copy,
                manager_copy,
                playback_state_copy,
                bot_state_copy,
            )
            .await;
        });

        tasks.insert(self.guild_id, handle);
    }

    async fn start_noise_state(
        &self,
        noise_entry: NoisePeriodEntry,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let duration_secs = if noise_entry.min_duration_seconds >= noise_entry.max_duration_seconds
        {
            noise_entry.min_duration_seconds
        } else {
            let mut rng = rand::rng();
            rng.random_range(noise_entry.min_duration_seconds..noise_entry.max_duration_seconds)
        };
        let duration = Duration::from_secs_f32(duration_secs);

        let noise_profile = noise_entry.noise_profile.clone();

        tracing::info!(
            "DJ playing noise with profile '{}' for {:.1}s in guild {}",
            noise_profile,
            duration_secs,
            self.guild_id
        );

        // Record noise state change metric
        if let Some(metrics) = bot_state.metrics.read().await.as_ref() {
            metrics.record_noise_state_change(self.guild_id.get(), &noise_profile);
        }

        Ok(NoiseSegment::new(noise_profile, duration).into())
    }

    async fn log_state_transition(
        &self,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = self.state.read().await;
        let (event_type, details): (String, serde_json::Value) = match &*state {
            DJState::PlayingTrack {
                filename,
                duration,
                forced_profile,
                ..
            } => (
                "track_started".into(),
                serde_json::json!({
                    "track_name": format_dj_track_name(filename),
                    "filename": filename,
                    "duration_secs": duration.as_secs_f32(),
                    "forced_profile": forced_profile,
                }),
            ),
            DJState::PlayingHexMessage {
                message,
                target_loops,
                forced_profile,
                ..
            } => (
                "hex_message_started".into(),
                serde_json::json!({
                    "message": message,
                    "target_loops": target_loops,
                    "forced_profile": forced_profile,
                }),
            ),
            DJState::Segment(segment) => ("noise_started".into(), segment.loggable_properties()),
            DJState::Stopped => ("stopped".into(), serde_json::json!({})),
        };
        // Drop the read guard before awaiting the log write below so it isn't held
        // across unrelated file I/O.
        drop(state);

        bot_state
            .log_dj_activity(self.guild_id.get(), event_type, details)
            .await
    }

    fn get_state_name(&self, state: &DJState) -> String {
        match state {
            DJState::PlayingTrack { .. } => "playing_track".to_string(),
            DJState::PlayingHexMessage { .. } => "playing_hex_message".to_string(),
            DJState::Segment(segment) => segment.to_string(),
            DJState::Stopped => "stopped".to_string(),
        }
    }
}
