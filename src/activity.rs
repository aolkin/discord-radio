use poise::ChoiceParameter;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use strum::Display;
use tokio::sync::RwLock;

/// Activity type enum that supports equality comparison
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, ChoiceParameter)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum ActivityType {
    #[name = "listening"]
    Listening,
    #[name = "playing"]
    Playing,
    #[name = "streaming"]
    Streaming,
    #[name = "custom"]
    Custom,
}

/// Activity type and status text
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// The bot's current activity (global, not per-guild)
pub struct ActivityManager {
    current: RwLock<Option<ActivityEntry>>,
    ctx: OnceLock<serenity::all::Context>,
    state_store: Arc<dyn crate::persistence::StateStore>,
}

impl ActivityManager {
    pub fn new(state_store: Arc<dyn crate::persistence::StateStore>) -> Self {
        Self {
            current: RwLock::new(None),
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

    fn update_activity(&self, current: Option<&ActivityEntry>) {
        if let Some(ctx) = self.ctx.get() {
            ctx.set_activity(current.map(Into::into));
            tracing::debug!("Updated bot activity");
        }
    }

    /// Replace the current activity and persist it
    pub async fn set_activity(&self, activity_type: ActivityType, status: String) {
        let entry = ActivityEntry::new(activity_type, status);
        *self.current.write().await = Some(entry.clone());

        self.update_activity(Some(&entry));
        let state = crate::persistence::ActivityState {
            activity_type: entry.activity_type,
            status: entry.status,
        };
        if let Err(e) = self.state_store.save_activity_state(Some(&state)).await {
            tracing::error!("Failed to persist activity state: {e:#}");
        }
    }

    pub async fn current(&self) -> Option<ActivityEntry> {
        self.current.read().await.clone()
    }

    /// Load the saved activity, replacing the current one if a saved one exists
    pub async fn restore_from_disk(&self) -> anyhow::Result<()> {
        let Some(saved_state) = self.state_store.load_activity_state().await? else {
            tracing::info!("No saved activity state found");
            return Ok(());
        };
        let entry = ActivityEntry::new(saved_state.activity_type, saved_state.status);
        *self.current.write().await = Some(entry.clone());
        self.update_activity(Some(&entry));
        tracing::info!("Restored activity state from disk");
        Ok(())
    }
}
