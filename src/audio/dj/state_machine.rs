use crate::audio::dj::config::DJConfig;
use crate::audio::dj::manager::{DJStateType, HexMessageRequest};
use crate::audio::dj::scheduler::{DJStateEntry, WeightedScheduler};
use crate::audio::dj::segments::{HexSegment, Segment, SignalProfilePlayback, TrackSegment};
use crate::audio::dj::segments::{NoiseSegment, SegmentCtx};
use crate::audio::tracks::TrackManager;
use crate::state::{BotState, Data};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, RwLock, RwLockReadGuard};
use tracing::info;

#[derive(Debug, Serialize, Deserialize)]
pub struct DJState {
    pub segment: Box<dyn Segment>,
}

impl<S: Segment + 'static> From<S> for DJState {
    fn from(segment: S) -> Self {
        DJState {
            segment: Box::new(segment),
        }
    }
}

impl DJState {
    pub fn idle() -> Self {
        NoiseSegment::idle(Duration::from_secs(1)).into()
    }

    pub async fn is_complete(&self) -> bool {
        self.segment.is_complete().await
    }

    pub fn forced_profile(&self) -> SignalProfilePlayback {
        self.segment.signal_profile()
    }

    pub fn is_hex_message(&self) -> bool {
        self.segment.state_name() == HexSegment::STATE_NAME
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

    pub async fn is_complete(&self) -> bool {
        self.current_state().await.is_complete().await
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

        if let Err(e) = bot_state
            .log_dj_activity(
                self.guild_id.get(),
                "stopped".to_string(),
                serde_json::json!({}),
            )
            .await
        {
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
        info!(
            "DJ force transitioning to hex message with custom text: {}",
            request.message
        );
        let defaults = &self.scheduler.config().hex_message_defaults;
        let loops = request
            .loops
            .unwrap_or_else(|| rand::rng().random_range(defaults.loop_min..=defaults.loop_max))
            as usize;
        let next_state = HexSegment::new(
            request.message,
            request.volume.unwrap_or(1.0),
            loops,
            self.pick_random_announcement(),
            defaults.signal_profile.clone(),
        )
        .into();
        self.enter_state(next_state, track_manager, bot_state).await
    }

    async fn transition_to_state(
        &self,
        next_state_type: DJStateEntry,
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("DJ transitioning to state: {next_state_type}");
        let next_state = match next_state_type {
            DJStateEntry::Track(entry) => {
                TrackSegment::from_entry(entry, &bot_state.duration_cache)
                    .await
                    .into()
            }
            DJStateEntry::HexMessage(entry) => {
                HexSegment::from_entry(entry, &self.scheduler.config().hex_message_defaults, || {
                    self.pick_random_announcement()
                })
                .into()
            }
            DJStateEntry::Noise(entry) => NoiseSegment::from(entry).into(),
        };

        self.enter_state(next_state, track_manager, bot_state).await
    }

    async fn enter_state(
        &self,
        next_state: DJState,
        track_manager: &Arc<Mutex<TrackManager>>,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Record the previous state before transitioning
        let from_state = self.get_state_name(&*self.current_state().await);

        self.cleanup_current_state(track_manager, bot_state).await?;

        let segment = &next_state.segment;
        segment.enter(&self.segment_ctx(track_manager)).await?;

        if let Some(channel) = self.announcement_channel
            && let Some(announcement) = segment.announcement()
            && let Err(e) = channel.say(&self.http, announcement).await
        {
            tracing::warn!("Failed to send DJ announcement: {}", e);
        }

        if let Some(status) = segment.channel_status() {
            bot_state
                .voice_status_manager
                .push_status(self.guild_id, &status, &self.http)
                .await;
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
        let segment = &current.segment;
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

        Ok(())
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

    async fn log_state_transition(
        &self,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let state = self.state.read().await;
        let event_type = self.get_state_name(&state);
        let details = state.segment.loggable_properties();
        // Drop the read guard before awaiting the log write below so it isn't held
        // across unrelated file I/O.
        drop(state);

        bot_state
            .log_dj_activity(self.guild_id.get(), event_type, details)
            .await
    }

    fn get_state_name(&self, state: &DJState) -> String {
        state.segment.state_name().to_string()
    }
}
