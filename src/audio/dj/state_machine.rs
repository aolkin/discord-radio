use crate::audio::dj::config::DJConfig;
use crate::audio::dj::manager::{DJCommand, DJStateType, HexMessageRequest, TrackRequest};
use crate::audio::dj::scheduler::{DJStateEntry, WeightedScheduler};
use crate::audio::dj::segments::{HexSegment, Segment, SignalProfilePlayback, TrackSegment};
use crate::audio::dj::segments::{NoiseSegment, SegmentCtx};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::state::Data;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, RwLockReadGuard};

#[derive(Debug, Serialize, Deserialize)]
pub struct SegmentContainer {
    pub segment: Box<dyn Segment>,
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    pub started_at: std::time::Instant,
}

impl Deref for SegmentContainer {
    type Target = dyn Segment;
    fn deref(&self) -> &Self::Target {
        &*self.segment
    }
}

impl<S: Segment + 'static> From<S> for SegmentContainer {
    fn from(segment: S) -> Self {
        SegmentContainer {
            segment: Box::new(segment),
            started_at: std::time::Instant::now(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DJState {
    pub current: SegmentContainer,
}

impl DJState {
    pub fn idle() -> Self {
        Self {
            current: NoiseSegment::idle(Duration::from_secs(1)).into(),
        }
    }

    pub fn forced_profile(&self) -> SignalProfilePlayback {
        self.current.signal_profile()
    }
}

pub struct DJStateMachine {
    state: Arc<RwLock<DJState>>,
    scheduler: WeightedScheduler,
    guild_id: GuildId,
    announcement_channel: Option<ChannelId>,
    http: Arc<Http>,
    hex_message_announcements: Vec<String>,
    bot_state: Data,
}

impl DJStateMachine {
    /// Constructs the state machine around the guild's shared state cell.
    pub fn new(
        config: DJConfig,
        guild_id: GuildId,
        announcement_channel: Option<ChannelId>,
        http: Arc<Http>,
        state: Arc<RwLock<DJState>>,
        bot_state: Data,
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
        SegmentCtx::new(&self.guild_id, self.bot_state)
    }

    pub async fn current_state(&self) -> RwLockReadGuard<'_, DJState> {
        self.state.read().await
    }

    pub async fn state_completed(&self) {
        self.current_state().await.current.completed().await
    }

    pub fn announcement_channel(&self) -> Option<ChannelId> {
        self.announcement_channel
    }

    pub fn set_announcement_channel(&mut self, channel: Option<ChannelId>) {
        self.announcement_channel = channel;
    }

    pub async fn stop(&mut self, bot_state: &Data) {
        self.cleanup_current_state(bot_state).await;

        bot_state
            .log_dj_activity(
                self.guild_id.get(),
                "stopped".to_string(),
                serde_json::json!({}),
            )
            .await;
    }

    pub async fn force_advance(
        &mut self,
        bot_state: &Data,
        state_type_filter: Option<DJStateType>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Self { scheduler, .. } = self;
        let next_state_type = scheduler.next_state(state_type_filter);
        tracing::info!("DJ transitioning to state: {next_state_type}");
        let next: SegmentContainer = match next_state_type {
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
        self.enter_state(next, bot_state).await
    }

    pub async fn force_hex_message(
        &self,
        bot_state: &Data,
        request: HexMessageRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!(
            "DJ force transitioning to hex message with custom text: {}",
            request.message
        );
        let defaults = &self.scheduler.config().hex_message_defaults;
        let loops = request
            .loops
            .unwrap_or_else(|| rand::rng().random_range(defaults.loop_min..=defaults.loop_max))
            as usize;
        let next: SegmentContainer = HexSegment::new(
            request.message,
            request.volume.unwrap_or(1.0),
            loops,
            self.pick_random_announcement(),
            defaults.signal_profile.clone(),
        )
        .into();
        self.enter_state(next, bot_state).await
    }

    pub async fn force_track(
        &self,
        bot_state: &Data,
        request: TrackRequest,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::info!("DJ force transitioning to track: {}", request.filename);
        let start_position = request
            .start_position
            .map(Duration::try_from_secs_f32)
            .transpose()?;
        let duration = match request.duration {
            Some(seconds) => Duration::try_from_secs_f32(seconds)?,
            None => bot_state
                .duration_cache
                .get_duration(&request.filename)
                .await
                .unwrap_or(Duration::from_secs(60)),
        };
        let next: SegmentContainer = TrackSegment::new(
            request.filename,
            request.volume.unwrap_or(1.0),
            start_position,
            duration,
            None,
            None,
        )
        .into();
        self.enter_state(next, bot_state).await
    }

    async fn enter_state(
        &self,
        next: SegmentContainer,
        bot_state: &Data,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Record the previous state before transitioning
        let from_state = self.get_state_name(&*self.current_state().await);

        self.cleanup_current_state(bot_state).await;

        let segment = &*next;
        segment.enter(&self.segment_ctx()).await?;

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

        let to_state = next.state_name().to_string();
        self.state.write().await.current = next;
        if let Some(metrics) = crate::metrics::get() {
            metrics.record_dj_state_transition(self.guild_id.get(), &from_state, &to_state);
        }

        self.log_state_transition(bot_state).await;

        Ok(())
    }

    async fn cleanup_current_state(&self, bot_state: &Data) {
        let current = self.state.read().await;
        let segment = &*current.current;
        if let Some(metrics) = crate::metrics::get() {
            metrics.record_dj_state_duration(
                self.guild_id.get(),
                segment.state_name(),
                segment.signal_profile().name.as_deref(),
                current.current.started_at.elapsed().as_secs_f64(),
            );
        }
        segment.exit(&self.segment_ctx()).await;
        if let Some(status) = segment.channel_status() {
            bot_state
                .voice_status_manager
                .remove_status(self.guild_id, &status, &self.http)
                .await;
        }
    }

    /// Handle the command and return true if the DJ should be stopped
    pub async fn handle_command(&mut self, command: DJCommand) -> bool {
        let bot_state = self.bot_state;
        let cmd_name: &'static str = (&command).into();
        let result = match command {
            DJCommand::ForceAdvance(state_type_filter) => {
                tracing::info!("Processing force advance for DJ in guild {}", self.guild_id);
                self.force_advance(&bot_state, state_type_filter).await
            }
            DJCommand::ForceHexMessage(request) => {
                self.force_hex_message(&bot_state, request).await
            }
            DJCommand::ForceTrack(request) => self.force_track(&bot_state, request).await,
            DJCommand::Stop => {
                tracing::info!("Processing stop command for DJ in guild {}", self.guild_id);
                self.stop(&bot_state).await;
                return true;
            }
            DJCommand::SetAnnouncementChannel(new_channel) => {
                tracing::info!(
                    "Setting DJ announcement channel to {:?} in guild {}",
                    new_channel,
                    self.guild_id
                );
                self.set_announcement_channel(new_channel);
                Ok(())
            }
        };
        if let Err(e) = result {
            tracing::error!(
                "DJ command {cmd_name} failed in guild {}: {e}",
                self.guild_id
            );
        }
        false
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

    async fn log_state_transition(&self, bot_state: &Data) -> bool {
        let state = self.state.read().await;
        let event_type = self.get_state_name(&state);
        let details = state.current.loggable_properties();
        // Drop the read guard before awaiting the log write below so it isn't held
        // across unrelated file I/O.
        drop(state);

        bot_state
            .log_dj_activity(self.guild_id.get(), event_type, details)
            .await
    }

    fn get_state_name(&self, state: &DJState) -> String {
        state.current.state_name().to_string()
    }
}
