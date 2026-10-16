use crate::audio::dj::config::DJConfig;
use crate::audio::dj::manager::{
    ADVANCE_RETRY_DELAY, DJCommand, DJStateType, HexMessageRequest, IdleRequest, TrackRequest,
};
use crate::audio::dj::scheduler::{DJStateEntry, WeightedScheduler};
use crate::audio::dj::segments::{
    Completed, HexSegment, Segment, SignalProfilePlayback, TrackSegment,
};
use crate::audio::dj::segments::{NoiseSegment, SegmentCtx};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::state::Data;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::ops::Deref;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, RwLockReadGuard, mpsc};

pub const DEFAULT_PROFILE_FADE: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize, Deserialize)]
pub struct SegmentContainer {
    pub segment: Box<dyn Segment>,
    #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
    pub started_at: Instant,
}

impl SegmentContainer {
    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }
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
            started_at: Instant::now(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum BackgroundProfile {
    Manual(SignalProfilePlayback),
    Rotating {
        profile: SignalProfilePlayback,
        #[serde(serialize_with = "ser_instant", deserialize_with = "deser_instant")]
        started_at: Instant,
        duration: Duration,
    },
}

impl BackgroundProfile {
    pub fn completed(&self) -> Completed {
        match self {
            Self::Manual(_) => Completed::never(),
            Self::Rotating {
                started_at,
                duration,
                ..
            } => Completed::at(*started_at + *duration),
        }
    }
}

impl Default for BackgroundProfile {
    fn default() -> Self {
        Self::Rotating {
            profile: "clear".to_string().into(),
            started_at: Instant::now(),
            duration: Duration::ZERO,
        }
    }
}

/// Ordered from lowest to highest priority.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, strum::Display)]
#[strum(serialize_all = "lowercase")]
pub enum ProfileSource {
    Rotating,
    Manual,
    Forced,
    Override,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DJState {
    pub segment: SegmentContainer,
    pub background_profile: BackgroundProfile,
    pub override_profile: Option<SignalProfilePlayback>,
    pub override_channel_status: Option<String>,
}

impl Default for DJState {
    fn default() -> Self {
        Self {
            segment: NoiseSegment::quiet(Duration::from_secs(1)).into(),
            background_profile: BackgroundProfile::default(),
            override_profile: None,
            override_channel_status: None,
        }
    }
}

impl DJState {
    pub fn effective_channel_status(&self, config_status: Option<&str>) -> Option<String> {
        self.override_channel_status
            .clone()
            .or_else(|| self.segment.channel_status())
            .or_else(|| config_status.map(str::to_owned))
    }

    pub fn effective_profile(&self) -> SignalProfilePlayback {
        if let Some(profile) = &self.override_profile {
            return profile.clone();
        }
        if let Some(forced) = self.segment.signal_profile() {
            return forced;
        }
        match &self.background_profile {
            BackgroundProfile::Manual(profile) | BackgroundProfile::Rotating { profile, .. } => {
                profile.clone()
            }
        }
    }

    pub fn profile_source(&self) -> ProfileSource {
        if self.override_profile.is_some() {
            ProfileSource::Override
        } else if self.segment.signal_profile().is_some() {
            ProfileSource::Forced
        } else if let BackgroundProfile::Manual(_) = self.background_profile {
            ProfileSource::Manual
        } else {
            ProfileSource::Rotating
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
    bot_state: Data,
    config_name: Option<String>,
    applied_profile: Option<(SignalProfilePlayback, ProfileSource)>,
    /// An empty status is how Discord clears it.
    applied_channel_status: Option<String>,
}

impl DJStateMachine {
    pub async fn new(
        config: DJConfig,
        guild_id: GuildId,
        http: Arc<Http>,
        saved: Option<crate::persistence::DJState>,
        bot_state: Data,
    ) -> Self {
        let (config_name, announcement_channel, restored_state) = match saved {
            Some(saved) => (
                saved.config_name,
                saved.announcement_channel_id.map(ChannelId::new),
                saved.state_machine,
            ),
            None => (None, None, None),
        };

        tracing::info!(
            "Starting DJ task for guild {} with config {:?}",
            guild_id,
            config_name
        );

        let hex_message_announcements =
            config.hex_message_announcements.clone().unwrap_or_default();

        let initial_state = restored_state.unwrap_or_default();

        let _ = initial_state
            .segment
            .restore(&SegmentCtx::new(&guild_id, bot_state))
            .await
            .inspect_err(|e| tracing::warn!("Failed to restore segment: {e:?}"));

        let state = Arc::new(RwLock::new(initial_state));
        bot_state
            .dj_states
            .write()
            .await
            .insert(guild_id, state.clone());

        Self {
            state,
            scheduler: WeightedScheduler::new(config),
            guild_id,
            announcement_channel,
            http,
            hex_message_announcements,
            bot_state,
            config_name,
            applied_profile: None,
            applied_channel_status: None,
        }
    }

    pub async fn run(mut self, mut command_rx: mpsc::Receiver<DJCommand>) {
        let guild_id = self.guild_id;
        let bot_state = self.bot_state;

        'dj: loop {
            self.update_profile().await;
            self.update_channel_status().await;
            let rotation_due = self.current_state().await.background_profile.completed();
            self.persist_state().await;

            tokio::select! {
                command = command_rx.recv() => {
                    let command = command.unwrap_or(DJCommand::Stop);
                    if self.handle_command(command).await {
                        tracing::info!("DJ task stopped for guild {}", guild_id);
                        break 'dj;
                    }
                }
                _ = self.state_completed() => {
                    if let Err(e) = self.force_advance(&bot_state, None).await {
                        tracing::error!("Failed to advance to next state for guild {}: {}", guild_id, e);
                        tokio::time::sleep(ADVANCE_RETRY_DELAY).await;
                    }
                }
                _ = rotation_due => self.rotate_profile().await,
            }
        }

        // Clean up shared state
        {
            let mut dj_states = self.bot_state.dj_states.write().await;
            dj_states.remove(&guild_id);
        }

        tracing::info!("DJ task terminated for guild {}", guild_id);
    }

    async fn rotate_profile(&mut self) {
        let entry = self.scheduler.next_signal_profile();
        let fade = Duration::from_secs_f32(entry.fade_duration_seconds);
        let hold_secs = rand::rng().random_range(entry.min_time_seconds..entry.max_time_seconds);
        self.state.write().await.background_profile = BackgroundProfile::Rotating {
            profile: SignalProfilePlayback {
                fade_in_duration: Some(fade),
                ..entry.profile_name.into()
            },
            started_at: Instant::now(),
            duration: fade + Duration::from_secs_f32(hold_secs),
        };
    }

    async fn update_profile(&mut self) {
        let state = self.state.read().await;
        let source = state.profile_source();
        let target = state.effective_profile();
        drop(state);

        let previous = self.applied_profile.as_ref();
        if previous.map(|(p, _)| &p.name) != Some(&target.name) {
            let fade = match previous {
                None => Duration::ZERO,
                Some((_, previous_source)) if source >= *previous_source => target.fade_in(),
                Some((previous, _)) => previous.fade_out(),
            };
            let processor_arc = self.bot_state.audio_processor(self.guild_id).await;
            if let Some(new_profile) = self.bot_state.profile_manager.get_profile(&target.name) {
                let mut processor = processor_arc.write().await;
                processor
                    .start_profile_transition(new_profile.clone(), fade.as_secs_f32() * 1000.0);

                tracing::info!(
                    "DJ transitioning to profile '{}' over {:.1}s in guild {}",
                    target.name,
                    fade.as_secs_f32(),
                    self.guild_id
                );
            }
            self.applied_profile = Some((target, source));
        }
    }

    async fn channel_status_enabled(&self) -> bool {
        !self
            .bot_state
            .dj_settings
            .get(self.guild_id)
            .await
            .channel_status_disabled
    }

    async fn update_channel_status(&mut self) {
        if !self.channel_status_enabled().await {
            self.applied_channel_status = None;
            return;
        }
        let config_status = self.scheduler.config().channel_status.as_deref();
        let target = self
            .state
            .read()
            .await
            .effective_channel_status(config_status)
            .unwrap_or_default();
        self.apply_channel_status(target).await;
    }

    async fn apply_channel_status(&mut self, target: String) {
        if self.applied_channel_status.as_ref() == Some(&target) {
            return;
        }
        let Some(channel_id) = self.bot_state.get_voice_channel_id(self.guild_id).await else {
            return;
        };

        match channel_id
            .edit(
                &self.http,
                serenity::builder::EditChannel::new().status(&target),
            )
            .await
        {
            Ok(_) => {
                tracing::debug!(
                    "Set voice channel status to {target:?} in guild {}",
                    self.guild_id
                );
                self.applied_channel_status = Some(target)
            }
            Err(e) => tracing::warn!(
                "Failed to set voice channel status in guild {}: {e:#}",
                self.guild_id
            ),
        }
    }

    async fn persist_state(&self) {
        let current_state = self.current_state().await;
        let persist_state = crate::persistence::DJStateSnapshot {
            config_name: self.config_name.as_deref(),
            announcement_channel_id: self.announcement_channel.map(|id| id.get()),
            state_machine: &current_state,
        };
        if let Err(e) = self
            .bot_state
            .state_store
            .save_dj_state(self.guild_id, &persist_state)
            .await
        {
            tracing::warn!(
                "Failed to persist DJ state for guild {}: {}",
                self.guild_id,
                e
            );
        }
    }

    fn segment_ctx(&self) -> SegmentCtx<'_> {
        SegmentCtx::new(&self.guild_id, self.bot_state)
    }

    pub async fn current_state(&self) -> RwLockReadGuard<'_, DJState> {
        self.state.read().await
    }

    pub async fn state_completed(&self) {
        self.current_state().await.segment.completed().await
    }

    pub fn set_announcement_channel(&mut self, channel: Option<ChannelId>) {
        self.announcement_channel = channel;
    }

    pub async fn stop(&mut self, bot_state: &Data) {
        self.cleanup_current_state().await;
        if self.channel_status_enabled().await {
            self.apply_channel_status(String::new()).await;
        }

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
    ) -> anyhow::Result<()> {
        let Self { scheduler, .. } = self;
        let next_state_type = scheduler.next_state(state_type_filter);
        tracing::info!("DJ transitioning to state: {next_state_type}");
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
        self.enter_state(next_state, bot_state).await
    }

    pub async fn force_hex_message(
        &self,
        bot_state: &Data,
        request: HexMessageRequest,
    ) -> anyhow::Result<()> {
        tracing::info!(
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
        self.enter_state(next_state, bot_state).await
    }

    pub async fn force_track(&self, bot_state: &Data, request: TrackRequest) -> anyhow::Result<()> {
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
        let next_state = TrackSegment::new(
            request.filename,
            request.volume.unwrap_or(1.0),
            start_position,
            duration,
            None,
            None,
        )
        .into();
        self.enter_state(next_state, bot_state).await
    }

    pub async fn force_idle(&self, bot_state: &Data, request: IdleRequest) -> anyhow::Result<()> {
        tracing::info!(
            "DJ force transitioning to idle with noise profile {:?} for {:?}s",
            request.noise_profile,
            request.duration
        );
        let duration = request
            .duration
            .map(Duration::try_from_secs_f32)
            .transpose()?;
        let next_state = match (request.noise_profile, duration) {
            (Some(noise_profile), duration) => NoiseSegment::new(noise_profile, duration),
            (None, Some(duration)) => NoiseSegment::quiet(duration),
            (None, None) => NoiseSegment::idle(),
        };
        self.enter_state(next_state.into(), bot_state).await
    }

    async fn enter_state(&self, segment: SegmentContainer, bot_state: &Data) -> anyhow::Result<()> {
        // Record the previous state before transitioning
        let from_state = self.current_state().await.segment.state_name().to_string();

        self.cleanup_current_state().await;

        segment.enter(&self.segment_ctx()).await?;

        if let Some(channel) = self.announcement_channel
            && let Some(announcement) = segment.announcement()
            && let Err(e) = channel.say(&self.http, announcement).await
        {
            tracing::warn!("Failed to send DJ announcement: {}", e);
        }

        let to_state = segment.state_name().to_string();
        self.state.write().await.segment = segment;
        if let Some(metrics) = crate::metrics::get() {
            metrics.record_dj_state_transition(self.guild_id.get(), &from_state, &to_state);
        }

        self.log_state_transition(bot_state).await;

        Ok(())
    }

    async fn cleanup_current_state(&self) {
        let current = self.state.read().await;
        let segment = &current.segment;
        if let Some(metrics) = crate::metrics::get() {
            metrics.record_dj_state_duration(
                self.guild_id.get(),
                segment.state_name(),
                segment.signal_profile().as_ref().map(|p| p.name.as_str()),
                current.segment.started_at.elapsed().as_secs_f64(),
            );
        }
        segment.exit(&self.segment_ctx()).await;
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
            DJCommand::ForceIdle(request) => self.force_idle(&bot_state, request).await,
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
            DJCommand::SetBackgroundProfile(profile) => {
                let mut state = self.state.write().await;
                match profile {
                    Some(profile) => state.background_profile = BackgroundProfile::Manual(profile),
                    None if matches!(state.background_profile, BackgroundProfile::Manual(_)) => {
                        state.background_profile = BackgroundProfile::default();
                    }
                    None => {}
                }
                Ok(())
            }
            DJCommand::SetOverrideProfile(profile_override) => {
                self.state.write().await.override_profile = profile_override;
                Ok(())
            }
        };
        if let Err(e) = result {
            tracing::error!(
                "DJ command {cmd_name} failed in guild {}: {e:#}",
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
        let event_type = state.segment.state_name().to_string();
        let details = state.segment.loggable_properties();
        // Drop the read guard before awaiting the log write below so it isn't held
        // across unrelated file I/O.
        drop(state);

        bot_state
            .log_dj_activity(self.guild_id.get(), event_type, details)
            .await
    }
}
