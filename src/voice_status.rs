use anyhow::Context;
use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use tokio::sync::RwLock;

/// A generic stack for managing status messages.
/// Components can push messages onto the stack and remove them when done.
/// The top of the stack is always the active status displayed.
#[derive(Clone, Debug)]
pub struct StatusStack<T> {
    stack: Vec<T>,
}

impl<T> StatusStack<T> {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Push a new item onto the stack
    pub fn push(&mut self, item: T) {
        self.stack.push(item);
    }

    /// Clear the entire stack
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    /// Get the current active item (top of stack)
    pub fn current(&self) -> Option<&T> {
        self.stack.last()
    }
}

impl<T: PartialEq> StatusStack<T> {
    /// Remove a specific item from the stack (removes the last occurrence)
    /// Returns true if the item was found and removed
    #[allow(dead_code)]
    pub fn remove(&mut self, item: &T) -> bool {
        if let Some(pos) = self.stack.iter().rposition(|m| m == item) {
            self.stack.remove(pos);
            true
        } else {
            false
        }
    }
}

/// Activity type enum that supports equality comparison
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityType {
    Listening,
    Playing,
    Streaming,
    Custom,
}

impl ActivityType {
    pub fn from_str(s: &str) -> Self {
        match s {
            "listening" => ActivityType::Listening,
            "playing" => ActivityType::Playing,
            "streaming" => ActivityType::Streaming,
            _ => ActivityType::Custom,
        }
    }
}

impl Display for ActivityType {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ActivityType::Listening => write!(f, "listening"),
            ActivityType::Playing => write!(f, "playing"),
            ActivityType::Streaming => write!(f, "streaming"),
            ActivityType::Custom => write!(f, "custom"),
        }
    }
}

/// Activity entry stored on the stack (type + status text)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityEntry {
    pub activity_type: ActivityType,
    pub status: String,
}

impl ActivityEntry {
    pub fn new(activity_type: ActivityType, status: String) -> Self {
        Self {
            activity_type,
            status,
        }
    }
}

impl From<&ActivityEntry> for serenity::all::ActivityData {
    fn from(entry: &ActivityEntry) -> Self {
        match entry.activity_type {
            ActivityType::Listening => serenity::all::ActivityData::listening(entry.status.clone()),
            ActivityType::Playing => serenity::all::ActivityData::playing(entry.status.clone()),
            ActivityType::Streaming => {
                serenity::all::ActivityData::streaming(entry.status.clone(), "https://example.com/")
                    .unwrap_or_else(|_| serenity::all::ActivityData::custom(entry.status.clone()))
            }
            ActivityType::Custom => serenity::all::ActivityData::custom(entry.status.clone()),
        }
    }
}

/// Type alias for activity stack
pub type ActivityStack = StatusStack<ActivityEntry>;

pub struct VoiceChannelStatusManager {
    voice_connections: Arc<RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<songbird::Call>>>>>,
}

impl VoiceChannelStatusManager {
    pub fn new(
        voice_connections: Arc<RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<songbird::Call>>>>>,
    ) -> Self {
        Self { voice_connections }
    }

    /// Get the current voice channel ID for a guild
    async fn get_voice_channel_id(&self, guild_id: GuildId) -> Option<ChannelId> {
        let voice_connections = self.voice_connections.read().await;
        if let Some(call_lock) = voice_connections.get(&guild_id) {
            let call = call_lock.lock().await;
            call.current_channel().map(|id| ChannelId::new(id.0.get()))
        } else {
            None
        }
    }

    /// Set or clear the status of the guild's current voice channel. Returns
    /// false without calling Discord when the bot is not in a voice channel.
    pub async fn set_status(
        &self,
        guild_id: GuildId,
        status: Option<&str>,
        http: &Http,
    ) -> anyhow::Result<bool> {
        let Some(channel_id) = self.get_voice_channel_id(guild_id).await else {
            return Ok(false);
        };
        channel_id
            .edit(
                http,
                serenity::builder::EditChannel::new().status(status.unwrap_or_default()),
            )
            .await
            .with_context(|| format!("Failed to edit status of voice channel {channel_id}"))?;
        tracing::debug!("Set voice channel status to {status:?} in guild {guild_id}");
        Ok(true)
    }
}

impl Default for VoiceChannelStatusManager {
    fn default() -> Self {
        Self::new(Arc::new(RwLock::new(HashMap::new())))
    }
}

/// Manager for bot activity stack (global, not per-guild)
/// The activity is global for the entire bot.
pub struct ActivityManager {
    stack: Arc<RwLock<ActivityStack>>,
    ctx: Arc<RwLock<Option<serenity::all::Context>>>,
    state_store: Arc<dyn crate::persistence::StateStore>,
}

impl ActivityManager {
    pub fn new(state_store: Arc<dyn crate::persistence::StateStore>) -> Self {
        Self {
            stack: Arc::new(RwLock::new(ActivityStack::new())),
            ctx: Arc::new(RwLock::new(None)),
            state_store,
        }
    }

    /// Set the serenity context for this manager
    pub async fn set_context(&self, ctx: serenity::all::Context) {
        let mut context = self.ctx.write().await;
        *context = Some(ctx);
    }

    /// Update the bot's activity to the current top of the stack
    async fn update_activity(&self) {
        let stack = self.stack.read().await;
        let current = stack.current().cloned();
        drop(stack);

        if let Some(ctx) = self.ctx.read().await.as_ref() {
            let activity_data = current.as_ref().map(|entry| entry.into());
            ctx.set_activity(activity_data);
            tracing::debug!("Updated bot activity");
        }
    }

    /// Clear the stack and push a new activity
    pub async fn set_activity(&self, activity_type: ActivityType, status: String) {
        let entry = ActivityEntry::new(activity_type, status);
        let mut stack = self.stack.write().await;
        stack.clear();
        stack.push(entry);
        drop(stack);

        self.update_activity().await;
        self.persist_current_activity().await;
        tracing::debug!("Set bot activity and cleared stack");
    }

    /// Push a new activity onto the stack and update the bot's activity
    #[allow(dead_code)]
    pub async fn push_activity(&self, activity_type: ActivityType, status: String) {
        let entry = ActivityEntry::new(activity_type, status);
        let mut stack = self.stack.write().await;
        stack.push(entry);
        drop(stack);

        self.update_activity().await;
        self.persist_current_activity().await;
        tracing::debug!("Pushed new bot activity");
    }

    /// Remove a specific activity from the stack and update the bot's activity
    /// Returns true if the activity was found and removed
    #[allow(dead_code)]
    pub async fn remove_activity(&self, activity_type: ActivityType, status: &str) -> bool {
        let entry = ActivityEntry::new(activity_type, status.to_string());
        let mut stack = self.stack.write().await;
        let was_removed = stack.remove(&entry);
        drop(stack);

        if was_removed {
            self.update_activity().await;
            self.persist_current_activity().await;
            tracing::debug!("Removed bot activity from stack");
        }
        was_removed
    }

    /// Persist the current top of the stack to disk (or that there is none)
    async fn persist_current_activity(&self) {
        let stack = self.stack.read().await;
        let current = stack.current().cloned();
        drop(stack);

        let activity_state = current.map(|entry| crate::persistence::ActivityState {
            activity_type: entry.activity_type.to_string(),
            status: entry.status,
        });

        if let Err(e) = self
            .state_store
            .save_activity_state(activity_state.as_ref())
            .await
        {
            tracing::error!("Failed to persist activity state: {}", e);
        }
    }

    /// Get the current active activity (top of stack)
    #[allow(dead_code)]
    pub async fn current(&self) -> Option<ActivityEntry> {
        let stack = self.stack.read().await;
        stack.current().cloned()
    }

    /// Get the serenity context
    pub async fn get_context(&self) -> Option<serenity::all::Context> {
        self.ctx.read().await.clone()
    }

    /// Restore activity state from disk
    pub async fn restore_from_disk(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(saved_state) = self.state_store.load_activity_state().await? {
            let activity_type = ActivityType::from_str(&saved_state.activity_type);
            let entry = ActivityEntry::new(activity_type, saved_state.status);

            let mut stack = self.stack.write().await;
            stack.clear();
            stack.push(entry);
            drop(stack);

            self.update_activity().await;
            tracing::info!("Restored activity state from disk");
        } else {
            tracing::info!("No saved activity state found");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
