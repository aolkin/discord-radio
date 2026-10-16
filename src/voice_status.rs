use std::fmt::{Display, Formatter};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;

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

/// Activity entry stored on the stack (type and status text)
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
pub type ActivityStack = Vec<ActivityEntry>;

/// Manager for bot activity stack (global, not per-guild)
/// The activity is global for the entire bot.
pub struct ActivityManager {
    stack: RwLock<ActivityStack>,
    ctx: OnceLock<serenity::all::Context>,
    state_store: Arc<dyn crate::persistence::StateStore>,
}

impl ActivityManager {
    pub fn new(state_store: Arc<dyn crate::persistence::StateStore>) -> Self {
        Self {
            stack: RwLock::new(ActivityStack::new()),
            ctx: OnceLock::new(),
            state_store,
        }
    }

    /// Set the serenity context on the first call and apply the current activity to it.
    /// Later calls are ignored.
    pub async fn set_context(&self, ctx: serenity::all::Context) {
        if self.ctx.set(ctx).is_err() {
            return;
        }
        let current = self.current().await;
        self.update_activity(current.as_ref());
    }

    /// Update the bot's activity to the current top of the stack
    fn update_activity(&self, current: Option<&ActivityEntry>) {
        if let Some(ctx) = self.ctx.get() {
            let activity_data = current.map(|entry| entry.into());
            ctx.set_activity(activity_data);
            tracing::debug!("Updated bot activity");
        }
    }

    /// Clear the stack and push a new activity
    pub async fn set_activity(&self, activity_type: ActivityType, status: String) {
        let entry = ActivityEntry::new(activity_type, status);
        let mut stack = self.stack.write().await;
        stack.clear();
        stack.push(entry.clone());
        drop(stack);

        self.update_activity(Some(&entry));
        self.persist_current_activity().await;
        tracing::debug!("Set bot activity and cleared stack");
    }

    /// Push a new activity onto the stack and update the bot's activity
    pub async fn push_activity(&self, activity_type: ActivityType, status: String) {
        let entry = ActivityEntry::new(activity_type, status);
        let mut stack = self.stack.write().await;
        stack.push(entry.clone());
        drop(stack);

        self.update_activity(Some(&entry));
        self.persist_current_activity().await;
        tracing::debug!("Pushed new bot activity");
    }

    /// Remove a specific activity from the stack and update the bot's activity
    /// Returns true if the activity was found and removed
    pub async fn remove_activity(&self, activity_type: ActivityType, status: &str) -> bool {
        let entry = ActivityEntry::new(activity_type, status.to_string());
        let mut stack = self.stack.write().await;
        if let Some(pos) = stack.iter().rposition(|m| m == &entry) {
            stack.remove(pos);
            let last = stack.last().cloned();
            drop(stack);

            self.update_activity(last.as_ref());
            self.persist_current_activity().await;
            tracing::debug!("Removed bot activity from stack");
            true
        } else {
            false
        }
    }

    /// Persist the current top of the stack to disk (or that there is none)
    async fn persist_current_activity(&self) {
        let stack = self.stack.read().await;
        let current = stack.last().cloned();
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
    pub async fn current(&self) -> Option<ActivityEntry> {
        let stack = self.stack.read().await;
        stack.last().cloned()
    }

    /// Restore activity state from disk
    pub async fn restore_from_disk(&self) -> anyhow::Result<()> {
        if let Some(saved_state) = self.state_store.load_activity_state().await? {
            let activity_type = ActivityType::from_str(&saved_state.activity_type);
            let entry = ActivityEntry::new(activity_type, saved_state.status);

            let mut stack = self.stack.write().await;
            stack.clear();
            stack.push(entry.clone());
            drop(stack);

            self.update_activity(Some(&entry));
            tracing::info!("Restored activity state from disk");
        } else {
            tracing::info!("No saved activity state found");
        }
        Ok(())
    }
}
