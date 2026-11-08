use crate::audio::dj::config::{
    ChannelStatusEntry, ConfiguredPool, DurationRange, EntryPlaybackConfig, LoadedConfig,
    PoolEntry, SegmentDefaults, SignalProfileEntry,
};
use crate::audio::dj::manager::{
    ADVANCE_RETRY_DELAY, DJCommand, HexMessageRequest, IdleRequest, TrackRequest,
};
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, NoiseEntry, NoiseSegment, SegmentCtx,
};
use crate::audio::dj::segments::{
    Completed, HexSegment, Segment, SignalProfilePlayback, TrackSegment,
};
use crate::audio::dj::serde::{ShiftBack, deser_instant, ser_instant};
use crate::audio::dj::tiered::{
    ConfigurableTier, TieredChange, TieredStateMachine, TieredValue, TimedValue,
};
use crate::audio::dj::weighted_choice::WeightedSelector;
use crate::state::Data;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{RwLock, RwLockReadGuard, mpsc, watch};

pub const DEFAULT_PROFILE_FADE: Duration = Duration::from_secs(2);

pub fn default_profile_fade(tier: ConfigurableTier) -> Option<Duration> {
    match tier {
        ConfigurableTier::Manual => None,
        ConfigurableTier::Override => Some(DEFAULT_PROFILE_FADE),
    }
}

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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelStatus {
    Unmanaged,
    Fixed(String),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DJState {
    pub segment: SegmentContainer,
    pub signal_profile: TieredStateMachine<SignalProfileEntry, SignalProfilePlayback>,
    pub channel_status: TieredStateMachine<ChannelStatusEntry, ChannelStatus>,
}

impl Default for DJState {
    fn default() -> Self {
        Self {
            segment: NoiseSegment::quiet(Duration::from_secs(1)).into(),
            signal_profile: TieredStateMachine::new(&SignalProfileEntry::default()),
            channel_status: TieredStateMachine::new(&ChannelStatusEntry::default()),
        }
    }
}

impl DJState {
    pub fn shift_back(&mut self, by: Duration) {
        self.segment.started_at.shift_back(by);
        self.signal_profile.shift_back(by);
        self.channel_status.shift_back(by);
    }

    pub fn signal_profile(&self) -> TieredValue<SignalProfilePlayback> {
        self.signal_profile.value()
    }

    pub fn channel_status(&self) -> TieredValue<ChannelStatus> {
        self.channel_status.value()
    }
}

pub struct DJStateMachine {
    state: Arc<RwLock<DJState>>,
    guild_id: GuildId,
    announcement_channel: Option<ChannelId>,
    bot_state: Data,
    config: watch::Receiver<Arc<LoadedConfig>>,
    pool_selector: WeightedSelector<ConfiguredPool>,
    entry_selectors: HashMap<String, WeightedSelector<PoolEntry>>,
    /// An empty status is how Discord clears it.
    applied_channel_status: Option<String>,
}

impl DJStateMachine {
    pub async fn new(
        guild_id: GuildId,
        saved: Option<crate::persistence::DJState>,
        state: Arc<RwLock<DJState>>,
        mut config: watch::Receiver<Arc<LoadedConfig>>,
        bot_state: Data,
    ) -> Self {
        let (announcement_channel, restored_state) = match saved {
            Some(saved) => {
                let mut state_machine = saved.state_machine;
                if !saved.stopped
                    && let Some(state) = &mut state_machine
                {
                    state.shift_back(saved.saved_at.elapsed().unwrap_or_default());
                }
                (
                    saved.announcement_channel_id.map(ChannelId::new),
                    state_machine,
                )
            }
            None => (None, None),
        };

        let initial_state = restored_state.unwrap_or_default();

        let _ = initial_state
            .segment
            .restore(&SegmentCtx::new(&guild_id, bot_state))
            .await
            .inspect_err(|e| tracing::warn!("Failed to restore segment: {e:?}"));

        *state.write().await = initial_state;

        let loaded = config.borrow_and_update().clone();
        tracing::info!("Starting DJ task with config {:?}", loaded.config.name);
        let mut state_machine = Self {
            state,
            guild_id,
            announcement_channel,
            bot_state,
            config,
            pool_selector: WeightedSelector::default(),
            entry_selectors: HashMap::new(),
            applied_channel_status: None,
        };
        state_machine.set_config(&loaded).await;
        state_machine
    }

    pub async fn run(mut self, mut command_rx: mpsc::Receiver<DJCommand>) {
        let bot_state = self.bot_state;

        'dj: loop {
            self.update_profile().await;
            self.update_channel_status().await;
            let (profile_due, channel_status_due) = {
                let state = self.current_state().await;
                (
                    state.signal_profile.completed(),
                    state.channel_status.completed(),
                )
            };
            self.persist_state(false).await;
            let mut config = self.config.clone();

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
                _ = profile_due => self.state.write().await.signal_profile.advance(),
                _ = channel_status_due => self.state.write().await.channel_status.advance(),
                Ok(()) = config.changed() => {
                    let loaded = self.config.borrow_and_update().clone();
                    self.set_config(&loaded).await;
                }
            }
        }

        tracing::info!("DJ task terminated");
    }

    async fn update_profile(&mut self) {
        let Some(TieredChange {
            previous,
            current: target,
        }) = self.state.write().await.signal_profile.changed()
        else {
            return;
        };
        let fade = match previous {
            None => Duration::ZERO,
            Some(previous) if target.tier <= previous.tier => target.value.fade_in(),
            Some(previous) => previous.value.fade_out(),
        };
        let processor_arc = self.bot_state.audio_processor(self.guild_id).await;
        if let Some(new_profile) = self.bot_state.profile_manager.get(&target.value.name) {
            let mut processor = processor_arc.write().await;
            processor.start_profile_transition((*new_profile).clone(), fade.as_secs_f32() * 1000.0);

            tracing::info!(
                "DJ transitioning to profile '{}' over {:.1}s",
                target.value.name,
                fade.as_secs_f32(),
            );
        }
    }

    async fn update_channel_status(&mut self) {
        let target = self.state.read().await.channel_status().value;
        match target {
            ChannelStatus::Unmanaged => self.applied_channel_status = None,
            ChannelStatus::Fixed(status) => self.apply_channel_status(status).await,
        }
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
        let config = self.config.borrow().clone();
        let persist_state = crate::persistence::DJStateSnapshot {
            config_name: &config.config.name,
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
        let target = self.state.read().await.channel_status().value;
        if let ChannelStatus::Fixed(_) = target {
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
        let pool = match pool_name {
            Some(name) => Some(self.pool_named(name)?),
            None => self.pool_selector.choose().cloned(),
        };
        let Some(pool) = pool else {
            return Ok(Box::new(NoiseSegment::from_entry(
                &DEFAULT_NOISE_PERIOD,
                &SegmentDefaults::default(),
            )));
        };
        let entry = self
            .entry_selectors
            .entry(pool.file.name.clone())
            .or_insert_with(|| pool.file.as_ref().into())
            .choose()
            .with_context(|| {
                format!("pool {:?} has no entry with weight above 0", pool.file.name)
            })?;
        Ok(entry.segment(&pool.segment_defaults, &bot_state).await)
    }

    fn pool_named(&self, name: &str) -> anyhow::Result<ConfiguredPool> {
        if let Some(pool) = self
            .pool_selector
            .items()
            .iter()
            .find(|pool| pool.file.name == name)
        {
            return Ok(pool.clone());
        }
        let file = self.bot_state.dj_config_manager.pools.require(name)?;
        Ok(ConfiguredPool {
            weight: 0,
            segment_defaults: file.segment_defaults.clone(),
            file,
        })
    }

    async fn set_config(&mut self, loaded @ LoadedConfig { config, pools }: &LoadedConfig) {
        self.entry_selectors
            .retain(|name, _| pools.iter().any(|pool| pool.file.name == *name));
        for pool in pools.iter() {
            match self.entry_selectors.entry(pool.file.name.clone()) {
                Entry::Occupied(mut selector) => selector.get_mut().set_pool(pool.file.as_ref()),
                Entry::Vacant(slot) => {
                    slot.insert(pool.file.as_ref().into());
                }
            }
        }
        self.pool_selector.set_pool(loaded);
        let mut state = self.state.write().await;
        state.signal_profile.set_pool(&config.signal_profiles);
        state.channel_status.set_pool(&config.channel_statuses);
    }

    pub async fn force_hex_message(
        &mut self,
        bot_state: &Data,
        request: HexMessageRequest,
    ) -> anyhow::Result<()> {
        tracing::info!(
            "DJ force transitioning to hex message with custom text: {}",
            request.message
        );
        let segment_defaults = request
            .pool
            .as_deref()
            .map(|name| self.pool_named(name))
            .transpose()?
            .map(|pool| pool.segment_defaults)
            .unwrap_or_default();
        let defaults = segment_defaults.hex_config();
        let loops = request
            .loops
            .unwrap_or_else(|| defaults.pick_loops(None, None)) as usize;
        let next_state = HexSegment::new(
            request.message,
            request.volume.unwrap_or(1.0),
            loops,
            EntryPlaybackConfig::default(),
            &segment_defaults,
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
            CommonSegmentPlaybackConfig::default(),
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
            (Some(noise_profile), duration) => NoiseSegment::new(
                duration,
                None,
                CommonSegmentPlaybackConfig {
                    signal_profile: Some(noise_profile.into()),
                    ..Default::default()
                },
            ),
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
        let mut state = self.state.write().await;
        state
            .signal_profile
            .update_forced(segment.signal_profile().map(TimedValue::permanent));
        state.channel_status.update_forced(
            segment
                .channel_status()
                .map(|status| TimedValue::permanent(ChannelStatus::Fixed(status))),
        );
        state.segment = segment;
        drop(state);
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
            DJCommand::SetConfig(mut config) => {
                let loaded = config.borrow_and_update().clone();
                tracing::info!("DJ switching to config {:?}", loaded.config.name);
                self.config = config;
                self.set_config(&loaded).await;
                Ok(())
            }
            DJCommand::SetAnnouncementChannel(new_channel) => {
                tracing::info!("Setting DJ announcement channel to {:?}", new_channel);
                self.set_announcement_channel(new_channel);
                Ok(())
            }
            DJCommand::SetSignalProfile(tier, profile) => {
                self.state
                    .write()
                    .await
                    .signal_profile
                    .update(tier, profile);
                Ok(())
            }
            DJCommand::SetChannelStatus(tier, status) => {
                self.state.write().await.channel_status.update(tier, status);
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

static DEFAULT_NOISE_PERIOD: LazyLock<NoiseEntry> = LazyLock::new(|| NoiseEntry {
    duration: DurationRange {
        min_duration_seconds: 2.0,
        max_duration_seconds: 2.0,
    },
    weight: 1,
    fade_duration_seconds: None,
    playback: EntryPlaybackConfig {
        signal_profile: Some("default".to_string()),
        ..Default::default()
    },
});
