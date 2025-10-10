use crate::state::Data;
use serenity::model::id::GuildId;
use songbird::events::{Event, EventContext, EventHandler};

pub struct TrackEndHandler {
    bot_state: Data,
    guild_id: GuildId,
    track_name: String,
}

impl TrackEndHandler {
    pub fn new(bot_state: Data, guild_id: GuildId, track_name: String) -> Self {
        Self {
            bot_state,
            guild_id,
            track_name,
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for TrackEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        tracing::info!(
            "Track '{}' finished playing in guild {}, cleaning up",
            self.track_name,
            self.guild_id
        );

        let track_managers = self.bot_state.track_managers.read().await;
        if let Some(manager_arc) = track_managers.get(&self.guild_id) {
            let mut manager = manager_arc.lock().await;
            manager.remove_track(&self.track_name).await;
        }

        None
    }
}

pub struct TrackLoopHandler {
    bot_state: Data,
    guild_id: GuildId,
    track_name: String,
}

impl TrackLoopHandler {
    pub fn new(bot_state: Data, guild_id: GuildId, track_name: String) -> Self {
        Self {
            bot_state,
            guild_id,
            track_name,
        }
    }
}

#[async_trait::async_trait]
impl EventHandler for TrackLoopHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        tracing::debug!(
            "Track '{}' looped in guild {}, updating start time",
            self.track_name,
            self.guild_id
        );

        let track_managers = self.bot_state.track_managers.read().await;
        if let Some(manager_arc) = track_managers.get(&self.guild_id) {
            let mut manager = manager_arc.lock().await;
            manager.update_track_start_time(&self.track_name).await;
        }

        None
    }
}

pub struct HexCharacterEndHandler {
    signal: std::sync::Arc<tokio::sync::Notify>,
}

impl HexCharacterEndHandler {
    pub fn new(signal: std::sync::Arc<tokio::sync::Notify>) -> Self {
        Self { signal }
    }
}

#[async_trait::async_trait]
impl EventHandler for HexCharacterEndHandler {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        self.signal.notify_one();
        None
    }
}
