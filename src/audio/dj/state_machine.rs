use crate::audio::dj::announcement::Announcement;
use crate::audio::dj::config::signal_profiles::SignalProfileEntry;
use crate::audio::dj::config::{
    AnnouncementEntry, ChannelStatusEntry, ConfiguredPool, DurationRange, EntryPlaybackConfig,
    LoadedConfig, Order, Playback, PoolEntry, SegmentDefaults,
};
use crate::audio::dj::manager::{
    ADVANCE_RETRY_DELAY, DJCommand, HexMessageRequest, IdleRequest, TrackRequest,
};
use crate::audio::dj::segments::{
    CommonSegmentPlaybackConfig, NoiseEntry, NoiseSegment, SegmentCtx,
};
use crate::audio::dj::segments::{Completed, HexSegment, Segment, TrackSegment};
use crate::audio::dj::serde::{ShiftBack, deser_instant, ser_instant};
use crate::audio::dj::signal_profile::SignalProfilePlayback;
use crate::audio::dj::tiered::{TieredChange, TieredStateMachine, TieredValue, TimedValue};
use crate::audio::dj::weighted_choice::WeightedSelector;
use crate::state::Data;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::{RwLock, RwLockReadGuard, mpsc, watch};

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
    pub announcement: TieredStateMachine<AnnouncementEntry, Option<Announcement>>,
    #[serde(default)]
    pub announcement_channels: BTreeSet<ChannelId>,
    pub pool_selector: WeightedSelector,
    pub entry_selectors: BTreeMap<String, WeightedSelector>,
    pub playing_pool: Option<PlayingPool>,
    pub config_name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlayingPool {
    pub pool: String,
    pub selector: WeightedSelector,
}

impl Default for DJState {
    fn default() -> Self {
        Self {
            segment: NoiseSegment::quiet(Duration::from_secs(1)).into(),
            signal_profile: TieredStateMachine::new(&SignalProfileEntry::default()),
            channel_status: TieredStateMachine::new(&ChannelStatusEntry::default()),
            announcement: TieredStateMachine::new(&AnnouncementEntry::default()),
            announcement_channels: BTreeSet::new(),
            pool_selector: WeightedSelector::default(),
            entry_selectors: BTreeMap::new(),
            playing_pool: None,
            config_name: None,
        }
    }
}

impl DJState {
    fn continuing_pool<'a>(&self, pools: &'a [ConfiguredPool]) -> Option<&'a ConfiguredPool> {
        let playing = self.playing_pool.as_ref()?;
        pools
            .iter()
            .find(|pool| pool.file.name == playing.pool && pool.playback == Playback::All)
    }

    fn pick_entry<'a>(&mut self, pool: &'a ConfiguredPool) -> Option<&'a PoolEntry> {
        match pool.playback {
            Playback::Single => {
                self.playing_pool = None;
                self.entry_selectors
                    .entry(pool.file.name.clone())
                    .or_default()
                    .choose(pool.order, &pool.file.entries)
            }
            Playback::All => {
                let playing = self
                    .playing_pool
                    .take_if(|playing| playing.pool == pool.file.name)
                    .unwrap_or_else(|| PlayingPool {
                        pool: pool.file.name.clone(),
                        selector: WeightedSelector::default(),
                    });
                self.playing_pool
                    .insert(playing)
                    .selector
                    .choose_unplayed(pool.order, &pool.file.entries)
            }
        }
    }

    pub fn shift_back(&mut self, by: Duration) {
        self.segment.started_at.shift_back(by);
        self.signal_profile.shift_back(by);
        self.channel_status.shift_back(by);
    }

    fn apply_segment_playback(&mut self) {
        self.signal_profile
            .update_forced(self.segment.signal_profile().map(TimedValue::permanent));
        self.channel_status.update_forced(
            self.segment
                .channel_status()
                .map(|status| TimedValue::permanent(ChannelStatus::Fixed(status))),
        );
        self.announcement.update_forced(
            self.segment
                .announcement()
                .map(|announcement| TimedValue::permanent(Some(announcement))),
        );
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
    bot_state: Data,
    config: watch::Receiver<Arc<LoadedConfig>>,
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
        let restored_state = match saved {
            Some(saved) => {
                let mut state_machine = saved.state_machine;
                if !saved.stopped
                    && let Some(state) = &mut state_machine
                {
                    state.shift_back(saved.saved_at.elapsed().unwrap_or_default());
                }
                state_machine
            }
            None => None,
        };

        let mut initial_state = restored_state.unwrap_or_default();
        initial_state.announcement.changed();

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
            bot_state,
            config,
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
            self.update_announcement().await;
            let (segment_changed, profile_due, channel_status_due) = {
                let state = self.current_state().await;
                (
                    state.segment.changed(),
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
                _ = segment_changed => self.state.write().await.apply_segment_playback(),
                _ = profile_due =>self.state.write().await.signal_profile.advance(),
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

    async fn update_announcement(&mut self) {
        let (announcement, channels) = {
            let mut state = self.state.write().await;
            let Some(TieredChange {
                current:
                    TieredValue {
                        value: Some(announcement),
                        ..
                    },
                ..
            }) = state.announcement.changed()
            else {
                return;
            };
            (announcement, state.announcement_channels.clone())
        };
        for channel in channels {
            if let Err(e) = channel
                .say(crate::http(), announcement.message.clone())
                .await
            {
                tracing::warn!("Failed to send DJ announcement to channel {channel}: {e:#}");
            }
        }
    }

    async fn persist_state(&self, stopped: bool) {
        let current_state = self.current_state().await;
        let persist_state = crate::persistence::DJStateSnapshot {
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
        let requested = pool_name.map(|name| self.pool_named(name)).transpose()?;
        let loaded = self.config.borrow().clone();
        let mut state = self.state.write().await;
        let first = match &requested {
            Some(pool) => {
                state.playing_pool = None;
                Some(pool)
            }
            None => state.continuing_pool(&loaded.pools),
        };
        let first_pick = first.and_then(|pool| Some((pool, state.pick_entry(pool)?)));
        let (pool, entry) = match first_pick {
            Some(picked) => picked,
            None => {
                state.playing_pool = None;
                let Some(pool) = state.pool_selector.choose(Order::Weighted, &loaded.pools) else {
                    return Ok(Box::new(NoiseSegment::from_entry(
                        &DEFAULT_NOISE_PERIOD,
                        &SegmentDefaults::default(),
                    )));
                };
                let name = pool.file.name.clone();
                let entry = state
                    .pick_entry(pool)
                    .with_context(|| format!("pool {name:?} has no entry to play"))?;
                (pool, entry)
            }
        };
        drop(state);
        Ok(entry.segment(&pool.segment_defaults, &bot_state).await)
    }

    fn pool_named(&self, name: &str) -> anyhow::Result<ConfiguredPool> {
        if let Some(pool) = self
            .config
            .borrow()
            .pools
            .iter()
            .find(|pool| pool.file.name == name)
        {
            return Ok(pool.clone());
        }
        let file = self.bot_state.dj_config_manager.pools.require(name)?;
        Ok(ConfiguredPool {
            weight: 0,
            playback: Playback::default(),
            order: Order::default(),
            segment_defaults: file.segment_defaults.clone(),
            file,
        })
    }

    async fn set_config(&mut self, LoadedConfig { config, pools }: &LoadedConfig) {
        let mut state = self.state.write().await;
        state.config_name = Some(config.name.clone());
        state
            .entry_selectors
            .retain(|name, _| pools.iter().any(|pool| pool.file.name == *name));
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

        let to_state = segment.state_name().to_string();
        let mut state = self.state.write().await;
        state.segment = segment;
        state.apply_segment_playback();
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
            DJCommand::AddAnnouncementChannel(channel) => {
                tracing::info!("Adding DJ announcement channel {channel}");
                self.state
                    .write()
                    .await
                    .announcement_channels
                    .insert(channel);
                Ok(())
            }
            DJCommand::RemoveAnnouncementChannel(channel) => {
                tracing::info!("Removing DJ announcement channel {channel}");
                self.state
                    .write()
                    .await
                    .announcement_channels
                    .remove(&channel);
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
