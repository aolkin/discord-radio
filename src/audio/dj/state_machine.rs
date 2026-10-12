use crate::audio::dj::config::{DJConfig, HexMessageEntry, NoisePeriodEntry, TrackEntry};
use crate::audio::dj::manager::{DJStateType, HexMessageRequest};
use crate::audio::dj::scheduler::{DJStateEntry, WeightedScheduler};
use crate::audio::dj::segments::SegmentCtx;
use crate::audio::dj::segments::{
    HexSegment, NoiseSegment, Segment, SignalProfilePlayback, TrackSegment,
};
use crate::audio::tracks::TrackManager;
use crate::state::{BotState, Data};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::ops::RangeInclusive;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, RwLockReadGuard};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub enum DJState {
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
            DJState::Segment(segment) => segment.is_complete(ctx).await,
            DJState::Stopped => true,
        }
    }

    pub fn forced_profile(&self) -> SignalProfilePlayback {
        match self {
            DJState::Segment(segment) => segment.signal_profile(),
            DJState::Stopped => Default::default(),
        }
    }

    pub fn is_hex_message(&self) -> bool {
        matches!(self, DJState::Segment(segment) if segment.state_name() == HexSegment::STATE_NAME)
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

    fn segment_ctx<'a>(&'a self, track_manager: &'a Arc<Mutex<TrackManager>>) -> SegmentCtx<'a> {
        SegmentCtx {
            guild_id: &self.guild_id,
            bot_state: &self.bot_state,
            http: &self.http,
            track_manager,
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

    pub async fn stop(&mut self, track_manager: &Arc<Mutex<TrackManager>>, bot_state: &Data) {
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
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
        state_type_filter: Option<DJStateType>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Self { scheduler, .. } = self;
        let next_state_type = scheduler.next_state(state_type_filter);
        self.transition_to_state(next_state_type, track_manager, bot_state)
            .await
    }

    pub async fn force_hex_message(
        &self,
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
        request: HexMessageRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from_state = self.get_state_name(&*self.current_state().await);
        self.cleanup_current_state(track_manager, bot_state).await?;

        info!(
            "DJ force transitioning to hex message with custom text: {}",
            request.message
        );
        let next_state = self.start_custom_hex_message_state(request).await?;
        self.enter_state(from_state, next_state, track_manager, bot_state)
            .await
    }

    pub async fn advance(
        &mut self,
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let ctx = self.segment_ctx(track_manager);
        if !self.state.read().await.is_complete(&ctx).await {
            return Ok(());
        }

        self.force_advance(track_manager, bot_state, None).await
    }

    async fn transition_to_state(
        &self,
        next_state_type: DJStateEntry,
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let from_state = self.get_state_name(&*self.current_state().await);

        self.cleanup_current_state(track_manager, bot_state).await?;

        info!("DJ transitioning to state: {next_state_type}");
        let next_state = match next_state_type {
            DJStateEntry::Track(entry) => self.start_track_state(entry, bot_state).await?,
            DJStateEntry::HexMessage(entry) => self.start_hex_message_state(entry).await?,
            DJStateEntry::Noise(entry) => self.start_noise_state(entry).await?,
        };

        self.enter_state(from_state, next_state, track_manager, bot_state)
            .await
    }

    async fn enter_state(
        &self,
        from_state: String,
        next_state: DJState,
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let DJState::Segment(segment) = &next_state {
            segment.enter(&self.segment_ctx(track_manager)).await?;
            if let Some(status) = segment.channel_status() {
                bot_state
                    .voice_status_manager
                    .push_status(self.guild_id, &status, &self.http)
                    .await;
            }
        }

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
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let current = self.state.read().await;
        match &*current {
            DJState::Segment(segment) => {
                if let Some(metrics) = bot_state.metrics.read().await.as_ref() {
                    metrics.record_dj_state_duration(
                        self.guild_id.get(),
                        segment.state_name(),
                        segment.signal_profile().name.as_deref(),
                        segment.elapsed().as_secs_f64(),
                    );
                }
                segment.exit(&self.segment_ctx(track_manager)).await?;
                if let Some(status) = segment.channel_status() {
                    bot_state
                        .voice_status_manager
                        .remove_status(self.guild_id, &status, &self.http)
                        .await;
                }
            }
            DJState::Stopped => {}
        }

        Ok(())
    }

    async fn start_track_state(
        &self,
        track_entry: TrackEntry,
        bot_state: &Data,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
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

        let segment = TrackSegment::new(
            track_entry.filename,
            volume,
            start_position,
            play_duration,
            track_entry.signal_profile,
            track_entry.channel_status.as_deref(),
        );
        Ok(segment.into())
    }

    async fn start_hex_message_state(
        &self,
        hex_entry: HexMessageEntry,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let defaults = &self.scheduler.config().hex_message_defaults;
        let loop_min = hex_entry.loop_min.unwrap_or(defaults.loop_min);
        let loop_max = hex_entry.loop_max.unwrap_or(defaults.loop_max);
        let signal_profile = hex_entry
            .signal_profile
            .or_else(|| defaults.signal_profile.clone());
        let announcement = hex_entry
            .announcement
            .or_else(|| self.pick_random_announcement());

        Ok(self.play_hex_message(
            hex_entry.text,
            1.0,
            loop_min..=loop_max,
            signal_profile,
            announcement,
        ))
    }

    async fn start_custom_hex_message_state(
        &self,
        request: HexMessageRequest,
    ) -> Result<DJState, Box<dyn std::error::Error + Send + Sync>> {
        let defaults = &self.scheduler.config().hex_message_defaults;
        let loops = match request.loops {
            Some(loops) => loops..=loops,
            None => defaults.loop_min..=defaults.loop_max,
        };

        Ok(self.play_hex_message(
            request.message,
            request.volume.unwrap_or(1.0),
            loops,
            defaults.signal_profile.clone(),
            self.pick_random_announcement(),
        ))
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

    fn play_hex_message(
        &self,
        message: String,
        volume: f32,
        loops: RangeInclusive<u32>,
        signal_profile: Option<String>,
        announcement: Option<String>,
    ) -> DJState {
        let target_loops = rand::rng().random_range(loops) as usize;

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

        HexSegment::new(message, volume, target_loops, signal_profile).into()
    }

    async fn start_noise_state(
        &self,
        noise_entry: NoisePeriodEntry,
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

        Ok(NoiseSegment::new(noise_profile, duration).into())
    }

    async fn log_state_transition(
        &self,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = self.state.read().await;
        let event_type = self.get_state_name(&state);
        let details = match &*state {
            DJState::Segment(segment) => segment.loggable_properties(),
            DJState::Stopped => serde_json::json!({}),
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
            DJState::Segment(segment) => segment.state_name().to_string(),
            DJState::Stopped => "stopped".to_string(),
        }
    }
}
