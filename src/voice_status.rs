use serenity::all::Http;
use serenity::model::id::{ChannelId, GuildId};
use std::collections::HashMap;
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

impl StatusStack<String> {
    /// Remove a specific message from the stack (removes the last occurrence)
    /// Returns true if the message was found and removed
    pub fn remove_str(&mut self, message: &str) -> bool {
        if let Some(pos) = self.stack.iter().rposition(|m| m == message) {
            self.stack.remove(pos);
            true
        } else {
            false
        }
    }

    /// Get the current active status (top of stack) as a string slice
    pub fn current_str(&self) -> Option<&str> {
        self.stack.last().map(|s| s.as_str())
    }
}

/// Type alias for voice channel status stack
pub type VoiceChannelStatusStack = StatusStack<String>;

/// Activity type enum that supports equality comparison
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActivityType {
    Listening,
    Playing,
    Streaming,
    Custom,
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
        let was_removed = stack_guard.remove_str(message);
        let new_current = stack_guard.current_str().unwrap_or("").to_string();
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

/// Manager for bot activity stack (global, not per-guild)
/// The activity is global for the entire bot.
pub struct ActivityManager {
    stack: Arc<RwLock<ActivityStack>>,
    ctx: Arc<RwLock<Option<serenity::all::Context>>>,
}

impl ActivityManager {
    pub fn new() -> Self {
        Self {
            stack: Arc::new(RwLock::new(ActivityStack::new())),
            ctx: Arc::new(RwLock::new(None)),
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
            tracing::debug!("Removed bot activity from stack");
        }
        was_removed
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
}

impl Default for ActivityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_stack_basic_operations() {
        let mut stack = StatusStack::<String>::new();

        // Test empty stack
        assert!(stack.current().is_none());

        // Test push
        stack.push("First".to_string());
        assert_eq!(stack.current(), Some(&"First".to_string()));

        stack.push("Second".to_string());
        assert_eq!(stack.current(), Some(&"Second".to_string()));

        stack.push("Third".to_string());
        assert_eq!(stack.current(), Some(&"Third".to_string()));
    }

    #[test]
    fn test_status_stack_remove_str() {
        let mut stack = StatusStack::<String>::new();

        stack.push("First".to_string());
        stack.push("Second".to_string());
        stack.push("Third".to_string());

        // Remove from middle
        assert!(stack.remove_str("Second"));
        assert_eq!(stack.current(), Some(&"Third".to_string()));

        // Remove non-existent
        assert!(!stack.remove_str("NonExistent"));
        assert_eq!(stack.current(), Some(&"Third".to_string()));

        // Remove top
        assert!(stack.remove_str("Third"));
        assert_eq!(stack.current(), Some(&"First".to_string()));
    }

    #[test]
    fn test_status_stack_clear() {
        let mut stack = StatusStack::<String>::new();

        stack.push("First".to_string());
        stack.push("Second".to_string());
        stack.push("Third".to_string());

        assert_eq!(stack.current(), Some(&"Third".to_string()));

        stack.clear();
        assert!(stack.current().is_none());
    }

    #[test]
    fn test_status_stack_current_str() {
        let mut stack = StatusStack::<String>::new();

        assert!(stack.current_str().is_none());

        stack.push("Test Status".to_string());
        assert_eq!(stack.current_str(), Some("Test Status"));
    }

    #[test]
    fn test_status_stack_generic_with_integers() {
        let mut stack = StatusStack::<i32>::new();

        assert!(stack.current().is_none());

        stack.push(1);
        stack.push(2);
        stack.push(3);

        assert_eq!(stack.current(), Some(&3));

        stack.clear();
        assert!(stack.current().is_none());
    }

    #[test]
    fn test_activity_entry_creation() {
        let entry = ActivityEntry::new(ActivityType::Playing, "Test Game".to_string());
        assert_eq!(entry.activity_type, ActivityType::Playing);
        assert_eq!(entry.status, "Test Game");
    }

    #[test]
    fn test_activity_entry_equality() {
        let entry1 = ActivityEntry::new(ActivityType::Listening, "Music".to_string());
        let entry2 = ActivityEntry::new(ActivityType::Listening, "Music".to_string());
        let entry3 = ActivityEntry::new(ActivityType::Playing, "Music".to_string());
        let entry4 = ActivityEntry::new(ActivityType::Listening, "Podcast".to_string());

        assert_eq!(entry1, entry2);
        assert_ne!(entry1, entry3);
        assert_ne!(entry1, entry4);
    }

    #[test]
    fn test_activity_stack_with_entries() {
        let mut stack = StatusStack::<ActivityEntry>::new();

        let entry1 = ActivityEntry::new(ActivityType::Playing, "Game 1".to_string());
        let entry2 = ActivityEntry::new(ActivityType::Listening, "Music".to_string());

        stack.push(entry1.clone());
        stack.push(entry2.clone());

        assert_eq!(stack.current(), Some(&entry2));

        // Test remove
        assert!(stack.remove(&entry2));
        assert_eq!(stack.current(), Some(&entry1));
    }
}
