use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// A singleton state machine that manages voice channel status using a stack.
/// Components can push messages onto the stack and pop them off when done.
/// The top of the stack is always the active status displayed.
#[derive(Clone, Debug)]
pub struct VoiceChannelStatusStack {
    stack: Vec<String>,
}

impl VoiceChannelStatusStack {
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Push a new status message onto the stack
    pub fn push(&mut self, message: String) {
        self.stack.push(message);
    }

    /// Remove a specific message from the stack (removes the last occurrence)
    /// Returns true if the message was found and removed
    pub fn remove(&mut self, message: &str) -> bool {
        if let Some(pos) = self.stack.iter().rposition(|m| m == message) {
            self.stack.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get the current active status (top of stack)
    pub fn current(&self) -> Option<&str> {
        self.stack.last().map(|s| s.as_str())
    }
}

/// Manager for voice channel status stacks across all guilds
pub struct VoiceChannelStatusManager {
    stacks: Arc<RwLock<HashMap<GuildId, Arc<RwLock<VoiceChannelStatusStack>>>>>,
    voice_connections: Arc<RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<songbird::Call>>>>>,
}

impl VoiceChannelStatusManager {
    pub fn new(
        voice_connections: Arc<RwLock<HashMap<GuildId, Arc<tokio::sync::Mutex<songbird::Call>>>>>,
    ) -> Self {
        Self {
            stacks: Arc::new(RwLock::new(HashMap::new())),
            voice_connections,
        }
    }

    /// Get or create a status stack for a guild
    async fn get_or_create_stack(&self, guild_id: GuildId) -> Arc<RwLock<VoiceChannelStatusStack>> {
        let mut stacks = self.stacks.write().await;
        stacks
            .entry(guild_id)
            .or_insert_with(|| Arc::new(RwLock::new(VoiceChannelStatusStack::new())))
            .clone()
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

    /// Update the channel status to the given message
    /// Returns true if the channel was updated, false if not connected to a voice channel
    async fn update_channel_status(
        &self,
        guild_id: GuildId,
        status: &str,
        http: &Http,
    ) -> Result<bool, serenity::Error> {
        if let Some(channel_id) = self.get_voice_channel_id(guild_id).await {
            channel_id
                .edit(http, serenity::builder::EditChannel::new().status(status))
                .await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Push a status message for a guild and update the channel
    pub async fn push_status(&self, guild_id: GuildId, message: String, http: &Http) {
        let stack = self.get_or_create_stack(guild_id).await;
        let mut stack_guard = stack.write().await;
        stack_guard.push(message.clone());
        drop(stack_guard);

        // Update the channel status if connected to a voice channel
        match self.update_channel_status(guild_id, &message, http).await {
            Ok(true) => {
                tracing::debug!(
                    "Pushed voice channel status '{}' for guild {}",
                    message,
                    guild_id
                );
            }
            Ok(false) => {
                tracing::debug!(
                    "Pushed voice channel status '{}' for guild {} (not connected to voice)",
                    message,
                    guild_id
                );
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to update voice channel status for guild {}: {}",
                    guild_id,
                    e
                );
            }
        }
    }

    /// Remove a specific status message from the stack and update the channel
    /// This allows removing a message that may not be at the top of the stack
    pub async fn remove_status(&self, guild_id: GuildId, message: &str, http: &Http) {
        let stack = self.get_or_create_stack(guild_id).await;
        let mut stack_guard = stack.write().await;
        let was_removed = stack_guard.remove(message);
        let new_current = stack_guard.current().unwrap_or("").to_string();
        drop(stack_guard);

        if was_removed {
            // Update the channel status if connected to a voice channel
            match self
                .update_channel_status(guild_id, &new_current, http)
                .await
            {
                Ok(true) => {
                    tracing::debug!(
                        "Removed voice channel status '{}' for guild {}, new status: '{}'",
                        message,
                        guild_id,
                        new_current
                    );
                }
                Ok(false) => {
                    tracing::debug!(
                        "Removed voice channel status '{}' for guild {} (not connected to voice)",
                        message,
                        guild_id
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to update voice channel status for guild {}: {}",
                        guild_id,
                        e
                    );
                }
            }
        }
    }
}

impl Default for VoiceChannelStatusManager {
    fn default() -> Self {
        Self::new(Arc::new(RwLock::new(HashMap::new())))
    }
}
