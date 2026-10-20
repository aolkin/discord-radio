use crate::activity::ActivityManager;
use crate::audio::dj::config::DJConfig;
use crate::audio::dj::manager::{DJCommand, DJManager};
use crate::audio::dj::state_machine::DJState;
use crate::audio::duration::DurationCache;
use crate::audio::processing::processor::{AudioProcessor, ProcessorRef};
use crate::audio::profiles::SignalProfile;
use crate::audio::tracks::TrackManager;
use crate::bucket::FileResolver;
use crate::bucket::registry::JsonRegistry;
use crate::logging::JsonLogger;
use crate::persistence::{DjSettingsStore, StateStore};
use serde::Serialize;
use serenity::all::{ChannelId, UserId};
use serenity::model::id::GuildId;
use songbird::Songbird;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::RwLock;
use tokio::sync::broadcast::Receiver;

pub type Data = &'static BotState;

#[derive(Clone)]
pub struct GuildRadio {
    pub audio_processor: ProcessorRef,
    pub track_manager: Arc<TrackManager>,
    dj_manager: Arc<DJManager>,
}

impl GuildRadio {
    pub fn dj_state(&self) -> Arc<RwLock<DJState>> {
        self.dj_manager.state()
    }
}

pub struct BotState {
    pub songbird: Arc<Songbird>,
    pub state_store: Arc<dyn StateStore>,
    pub dj_settings: DjSettingsStore,
    pub duration_cache: DurationCache,
    guilds: StdMutex<HashMap<GuildId, GuildRadio>>,
    pub profile_manager: JsonRegistry<SignalProfile>,
    pub dj_config_manager: JsonRegistry<DJConfig>,
    pub activity_manager: ActivityManager,
    pub file_resolver: FileResolver,
    pub bot_user_id: UserId,
}

impl BotState {
    pub fn new(
        file_resolver: FileResolver,
        state_store: Arc<dyn StateStore>,
        dj_settings: DjSettingsStore,
        profile_manager: JsonRegistry<SignalProfile>,
        dj_config_manager: JsonRegistry<DJConfig>,
        bot_user_id: UserId,
        songbird: Arc<Songbird>,
    ) -> Self {
        Self {
            songbird,
            duration_cache: DurationCache::new(file_resolver.clone()),
            state_store: state_store.clone(),
            dj_settings,
            guilds: StdMutex::new(HashMap::new()),
            profile_manager,
            dj_config_manager,
            activity_manager: ActivityManager::new(state_store),
            file_resolver,
            bot_user_id,
        }
    }

    fn guild_radio(&'static self, guild_id: GuildId) -> GuildRadio {
        self.guilds
            .lock()
            .unwrap()
            .entry(guild_id)
            .or_insert_with(|| {
                let audio_processor = Arc::new(RwLock::new(AudioProcessor::new(
                    self.profile_manager.default_profile(),
                )));
                let track_manager =
                    Arc::new(TrackManager::new(guild_id, self, audio_processor.clone()));
                GuildRadio {
                    audio_processor,
                    track_manager,
                    dj_manager: Arc::new(DJManager::new(guild_id, self)),
                }
            })
            .clone()
    }

    fn existing_guild_radio(&self, guild_id: GuildId) -> Option<GuildRadio> {
        self.guilds.lock().unwrap().get(&guild_id).cloned()
    }

    /// Every guild that has radio state, ordered by guild id.
    pub fn guild_radios(&self) -> Vec<(GuildId, GuildRadio)> {
        let mut guilds: Vec<_> = self
            .guilds
            .lock()
            .unwrap()
            .iter()
            .map(|(id, radio)| (*id, radio.clone()))
            .collect();
        guilds.sort_by_key(|(id, _)| *id);
        guilds
    }

    pub fn dj_manager(&'static self, guild_id: GuildId) -> Arc<DJManager> {
        self.guild_radio(guild_id).dj_manager
    }

    pub fn existing_dj_manager(&self, guild_id: GuildId) -> Option<Arc<DJManager>> {
        Some(self.existing_guild_radio(guild_id)?.dj_manager)
    }

    pub async fn audio_processor(&'static self, guild_id: GuildId) -> ProcessorRef {
        self.guild_radio(guild_id).audio_processor.clone()
    }

    pub async fn track_manager(&'static self, guild_id: GuildId) -> Arc<TrackManager> {
        self.guild_radio(guild_id).track_manager.clone()
    }

    pub async fn opus_stream(&self, guild_id: GuildId) -> Option<Receiver<Vec<u8>>> {
        let audio_processor = self.existing_guild_radio(guild_id)?.audio_processor;
        let receiver = audio_processor.read().await.stream_opus();
        Some(receiver)
    }

    pub fn send_dj_command(&self, guild_id: GuildId, command: DJCommand) -> anyhow::Result<bool> {
        let Some(manager) = self.existing_dj_manager(guild_id) else {
            return Ok(false);
        };
        manager.send(command)
    }

    /// Get the current voice channel ID for a guild
    pub async fn get_voice_channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        let call_lock = self.songbird.get(guild_id)?;
        let call = call_lock.lock().await;
        call.current_channel().map(|id| ChannelId::new(id.0.get()))
    }

    /// Count the guilds with a Call that is in a voice channel
    pub async fn voice_connection_count(&self) -> usize {
        let mut count = 0;
        for (_, call_lock) in self.songbird.iter() {
            if call_lock.lock().await.current_channel().is_some() {
                count += 1;
            }
        }
        count
    }

    /// Log a member activity event
    pub async fn log_member_activity(
        &self,
        guild_id: u64,
        user_id: u64,
        username: &str,
        nickname: Option<&str>,
        action: &str,
        channel_id: Option<u64>,
    ) {
        #[derive(Serialize)]
        struct MemberActivityEntry {
            timestamp: String,
            user_id: u64,
            username: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            nickname: Option<String>,
            action: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            channel_id: Option<u64>,
        }

        let log_path = self
            .state_store
            .guild_logs_dir(guild_id)
            .join("members.jsonl");
        let logger = JsonLogger::new(log_path);

        let entry = MemberActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            user_id,
            username: username.to_string(),
            nickname: nickname.map(|s| s.to_string()),
            action: action.to_string(),
            channel_id,
        };

        if let Err(e) = logger.log(&entry).await {
            tracing::error!("{e:#}");
        }
    }

    /// Log a DJ state transition event
    pub async fn log_dj_activity(
        &self,
        guild_id: u64,
        event_type: String,
        details: serde_json::Value,
    ) {
        #[derive(Serialize)]
        struct DJActivityEntry {
            timestamp: String,
            event_type: String,
            details: serde_json::Value,
        }

        let log_path = self.state_store.guild_logs_dir(guild_id).join("dj.jsonl");
        let logger = JsonLogger::new(log_path);

        let entry = DJActivityEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type,
            details,
        };

        if let Err(e) = logger.log(&entry).await {
            tracing::error!("{e:#}");
        }
    }
}

impl Debug for BotState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotState")
            .field("content_path", &self.file_resolver.content_path())
            .field("state_store", &"<dyn StateStore>")
            .finish_non_exhaustive()
    }
}
