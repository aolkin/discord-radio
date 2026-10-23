use crate::audio::dj::config::{DJConfig, Pool, PoolEntry, SignalProfileEntry};
use crate::audio::dj::manager::{
    ADVANCE_RETRY_DELAY, DEFAULT_DJ_CONFIG, DJCommand, HexMessageRequest, IdleRequest,
    TrackRequest, load_dj_config,
};
use crate::audio::dj::segments::{
    Completed, HexMessageDefaults, HexSegment, Segment, SignalProfilePlayback, TrackSegment,
};
use crate::audio::dj::segments::{NoisePeriodEntry, NoiseSegment, SegmentCtx};
use crate::audio::dj::serde::{deser_instant, ser_instant};
use crate::audio::dj::weighted_choice::{Weighted, WeightedSelector};
use crate::state::Data;
use anyhow::Context;
use rand::RngExt;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};
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

    pub fn completed(&self) -> Completed {
        self.segment.completed(self.elapsed())
    }

    pub async fn restore(&self, ctx: &SegmentCtx<'_>) -> anyhow::Result<()> {
        self.segment.restore(ctx, self.elapsed()).await
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
        (Box::new(segment) as Box<dyn Segment>).into()
    }
}

impl From<Box<dyn Segment>> for SegmentContainer {
    fn from(segment: Box<dyn Segment>) -> Self {
        SegmentContainer {
            segment,
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
    pub fn shift_back(&mut self, by: Duration) {
        let shift = |instant: &mut Instant| {
            *instant = instant.checked_sub(by).unwrap_or_else(Instant::now);
        };
        shift(&mut self.segment.started_at);
        if let BackgroundProfile::Rotating { started_at, .. } = &mut self.background_profile {
            shift(started_at);
        }
    }

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
    guild_id: GuildId,
    announcement_channel: Option<ChannelId>,
    bot_state: Data,
    config_name: String,
    shared_config_name: Arc<std::sync::RwLock<String>>,
    config: Arc<DJConfig>,
    pool_selector: WeightedSelector<ActivePool, Box<[ActivePool]>>,
    signal_profile_selector: WeightedSelector<SignalProfileEntry>,
    applied_profile: Option<(SignalProfilePlayback, ProfileSource)>,
    /// An empty status is how Discord clears it.
    applied_channel_status: Option<String>,
}

impl DJStateMachine {
    pub async fn new(
        guild_id: GuildId,
        saved: Option<crate::persistence::DJState>,
        state: Arc<RwLock<DJState>>,
        shared_config_name: Arc<std::sync::RwLock<String>>,
        bot_state: Data,
    ) -> Self {
        let (config_name, announcement_channel, restored_state) = match saved {
            Some(saved) => {
                let mut state_machine = saved.state_machine;
                if !saved.stopped
                    && let Some(state) = &mut state_machine
                {
                    state.shift_back(saved.saved_at.elapsed().unwrap_or_default());
                }
                (
                    saved.config_name,
                    saved.announcement_channel_id.map(ChannelId::new),
                    state_machine,
                )
            }
            None => (None, None, None),
        };
        let config_name = config_name.unwrap_or_else(|| DEFAULT_DJ_CONFIG.to_string());
        let config = load_dj_config(&bot_state, &config_name).unwrap_or_else(|e| {
            tracing::warn!("Starting DJ with an empty config: {e:#}");
            Arc::default()
        });

        tracing::info!("Starting DJ task with config {:?}", config_name);
        config_name.clone_into(
            &mut shared_config_name
                .write()
                .unwrap_or_else(|e| e.into_inner()),
        );

        let initial_state = restored_state.unwrap_or_default();

        let _ = initial_state
            .segment
            .restore(&SegmentCtx::new(&guild_id, bot_state))
            .await
            .inspect_err(|e| tracing::warn!("Failed to restore segment: {e:?}"));

        *state.write().await = initial_state;

        Self {
            state,
            guild_id,
            announcement_channel,
            bot_state,
            config_name,
            pool_selector: pool_selector(&config),
            signal_profile_selector: (&config.signal_profiles).into(),
            shared_config_name,
            config,
            applied_profile: None,
            applied_channel_status: None,
        }
    }

    pub async fn run(mut self, mut command_rx: mpsc::Receiver<DJCommand>) {
        let bot_state = self.bot_state;

        'dj: loop {
            self.update_profile().await;
            self.update_channel_status().await;
            let rotation_due = self.current_state().await.background_profile.completed();
            self.persist_state(false).await;

            tokio::select! {
                command = command_rx.recv() => {
                    let command = command.unwrap_or(DJCommand::Stop);
                    if self.handle_command(command).await {
                        tracing::info!("DJ task stopped");
                        break 'dj;
                    }
                }
                _ = self.state_completed() => {
                    if let Err(e) = self.force_advance(&bot_state, None).await {
                        tracing::error!("Failed to advance to next state: {}", e);
                        tokio::time::sleep(ADVANCE_RETRY_DELAY).await;
                    }
                }
                _ = rotation_due => self.rotate_profile().await,
            }
        }

        tracing::info!("DJ task terminated");
    }

    async fn rotate_profile(&mut self) {
        let entry = self
            .signal_profile_selector
            .choose()
            .unwrap_or(&DEFAULT_SIGNAL_PROFILE)
            .to_owned();
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
            if let Some(new_profile) = self.bot_state.profile_manager.get(&target.name) {
                let mut processor = processor_arc.write().await;
                processor
                    .start_profile_transition((*new_profile).clone(), fade.as_secs_f32() * 1000.0);

                tracing::info!(
                    "DJ transitioning to profile '{}' over {:.1}s",
                    target.name,
                    fade.as_secs_f32(),
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
        let config_status = self.config.channel_status.as_deref();
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
                crate::http(),
                serenity::builder::EditChannel::new().status(&target),
            )
            .await
        {
            Ok(_) => {
                tracing::debug!("Set voice channel status to {target:?}");
                self.applied_channel_status = Some(target)
            }
            Err(e) => tracing::warn!("Failed to set voice channel status: {e:#}"),
        }
    }

    async fn persist_state(&self, stopped: bool) {
        let current_state = self.current_state().await;
        let persist_state = crate::persistence::DJStateSnapshot {
            config_name: &self.config_name,
            announcement_channel_id: self.announcement_channel.map(|id| id.get()),
            state_machine: &current_state,
            saved_at: SystemTime::now(),
            stopped,
        };
        if let Err(e) = self
            .bot_state
            .state_store
            .save_dj_state(self.guild_id, &persist_state)
            .await
        {
            tracing::warn!("Failed to persist DJ state: {}", e);
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
        pool_name: Option<&str>,
    ) -> anyhow::Result<()> {
        self.reload_config();
        let next_state = self.next_segment(pool_name, bot_state).await?;
        tracing::info!("DJ transitioning to state: {}", next_state.state_name());
        self.enter_state(next_state.into(), bot_state).await
    }

    /// Picks a segment from the pool named `pool_name`, or else from a pool
    /// chosen by weight.
    async fn next_segment(
        &mut self,
        pool_name: Option<&str>,
        bot_state: Data,
    ) -> anyhow::Result<Box<dyn Segment>> {
        let active = match pool_name {
            Some(name) => Some(
                self.pool_selector
                    .iter_mut()
                    .find(|active| active.pool.name == name)
                    .with_context(|| format!("no pool named {name:?}"))?,
            ),
            None => self.pool_selector.choose_mut(),
        };
        match active {
            Some(ActivePool { pool, selector }) => {
                let entry = selector.choose().with_context(|| {
                    format!("pool {:?} has no entry with weight above 0", pool.name)
                })?;
                Ok(entry.segment(pool.hex_defaults(), &bot_state).await)
            }
            None => Ok(Box::new(NoiseSegment::from(&*DEFAULT_NOISE_PERIOD))),
        }
    }

    /// Switches to the latest saved copy of the DJ's named config.
    fn reload_config(&mut self) {
        let Some(config) = self.bot_state.dj_config_manager.get(&self.config_name) else {
            return;
        };
        if !Arc::ptr_eq(&config, &self.config) {
            tracing::info!("DJ reloading config {:?}", self.config_name);
            self.set_config(config);
        }
    }

    /// Selection starts with an empty recent history.
    fn set_config(&mut self, config: Arc<DJConfig>) {
        self.pool_selector = pool_selector(&config);
        self.signal_profile_selector = (&config.signal_profiles).into();
        self.config = config;
    }

    pub async fn force_hex_message(
        &mut self,
        bot_state: &Data,
        request: HexMessageRequest,
    ) -> anyhow::Result<()> {
        self.reload_config();
        tracing::info!(
            "DJ force transitioning to hex message with custom text: {}",
            request.message
        );
        let built_in_defaults = HexMessageDefaults::default();
        let defaults = match &request.pool {
            Some(name) => self
                .config
                .pools
                .iter()
                .find(|pool| pool.name == *name)
                .with_context(|| format!("no pool named {name:?}"))?
                .hex_defaults(),
            None => &built_in_defaults,
        };
        let loops = request
            .loops
            .unwrap_or_else(|| rand::rng().random_range(defaults.loop_min..=defaults.loop_max))
            as usize;
        let next_state = HexSegment::new(
            request.message,
            request.volume.unwrap_or(1.0),
            loops,
            defaults.pick_random_announcement(),
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
            && let Err(e) = channel.say(crate::http(), announcement).await
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
            DJCommand::ForceAdvance(pool_name) => {
                tracing::info!("Processing force advance for DJ");
                self.force_advance(&bot_state, pool_name.as_deref()).await
            }
            DJCommand::ForceHexMessage(request) => {
                self.force_hex_message(&bot_state, request).await
            }
            DJCommand::ForceTrack(request) => self.force_track(&bot_state, request).await,
            DJCommand::ForceIdle(request) => self.force_idle(&bot_state, request).await,
            DJCommand::Stop => {
                tracing::info!("Processing stop command for DJ");
                self.persist_state(true).await;
                self.stop(&bot_state).await;
                return true;
            }
            DJCommand::SetConfig { name, config } => {
                tracing::info!("DJ switching to config {name:?}");
                self.set_config(config);
                name.clone_into(
                    &mut self
                        .shared_config_name
                        .write()
                        .unwrap_or_else(|e| e.into_inner()),
                );
                self.config_name = name;
                Ok(())
            }
            DJCommand::SetAnnouncementChannel(new_channel) => {
                tracing::info!("Setting DJ announcement channel to {:?}", new_channel);
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
            tracing::error!("DJ command {cmd_name} failed: {e:#}");
        }
        false
    }

    async fn log_state_transition(&self, bot_state: &Data) {
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

struct ActivePool {
    pool: Arc<Pool>,
    selector: WeightedSelector<PoolEntry>,
}

impl From<&Arc<Pool>> for ActivePool {
    fn from(pool: &Arc<Pool>) -> Self {
        Self {
            pool: pool.clone(),
            selector: pool.as_ref().into(),
        }
    }
}

impl Weighted for ActivePool {
    fn weight(&self) -> u32 {
        self.pool.weight
    }
}

fn pool_selector(config: &DJConfig) -> WeightedSelector<ActivePool, Box<[ActivePool]>> {
    WeightedSelector::new(
        config.pools.iter().map(ActivePool::from).collect(),
        config.history_penalty_multiplier,
    )
}

static DEFAULT_NOISE_PERIOD: LazyLock<NoisePeriodEntry> = LazyLock::new(|| NoisePeriodEntry {
    noise_profile: "default".to_string(),
    min_duration_seconds: 2.0,
    max_duration_seconds: 2.0,
    weight: 1,
});

static DEFAULT_SIGNAL_PROFILE: LazyLock<SignalProfileEntry> =
    LazyLock::new(|| SignalProfileEntry {
        profile_name: "clear".to_string(),
        weight: 1,
        fade_duration_seconds: 3.0,
        min_time_seconds: 60.0,
        max_time_seconds: 120.0,
    });
