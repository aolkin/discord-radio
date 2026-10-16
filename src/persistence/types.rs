use crate::audio::dj::config::{HexMessageEntry, StateWeights};
use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serenity::model::id::{ChannelId, GuildId};
use std::time::SystemTime;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TrackState {
    pub name: String,
    pub filename: String,
    pub volume: f32,
    #[serde(default = "default_loops")]
    pub loops: bool,
    #[serde(default)]
    pub start_time: Option<SystemTime>,
}

fn default_loops() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MultiTrackPlaybackState {
    pub tracks: Vec<TrackState>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DjSettings {
    #[serde(default)]
    pub tracks: Option<String>,
    #[serde(default)]
    pub hex_messages: Option<String>,
    #[serde(default)]
    pub config: Option<String>,
    #[serde(default)]
    pub hex_message_overrides: DJConfigOverrideCategory<HexMessageEntry>,
    #[serde(default)]
    pub hex_message_announcement_overrides: DJConfigOverrideCategory<String>,
    #[serde(default)]
    pub state_weight_overrides: DJConfigOverrideSingle<StateWeights>,
    #[serde(default)]
    pub channel_status_disabled: bool,
}

#[derive(Deserialize, Debug)]
pub struct DJState {
    pub config_name: Option<String>,
    #[serde(default)]
    pub announcement_channel_id: Option<u64>,
    #[serde(default)]
    pub state_machine: Option<crate::audio::dj::state_machine::DJState>,
}

#[derive(Serialize, Debug)]
pub struct DJStateSnapshot<'a> {
    pub config_name: Option<&'a str>,
    pub announcement_channel_id: Option<u64>,
    pub state_machine: &'a crate::audio::dj::state_machine::DJState,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DJConfigOverrideCategory<T> {
    pub enabled: bool,
    pub items: Vec<T>,
}

impl<T> DJConfigOverrideCategory<T> {
    /// Replace the item at `index`, or append when `index` is `None`.
    pub fn set(&mut self, index: Option<usize>, item: T) -> anyhow::Result<()> {
        match index {
            Some(index) => {
                let len = self.items.len();
                let slot = self
                    .items
                    .get_mut(index)
                    .with_context(|| format!("index {index} out of bounds for {len} items"))?;
                *slot = item;
            }
            None => self.items.push(item),
        }
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> anyhow::Result<()> {
        let len = self.items.len();
        if index >= len {
            bail!("index {index} out of bounds for {len} items");
        }
        self.items.remove(index);
        Ok(())
    }
}

impl<T> Default for DJConfigOverrideCategory<T> {
    fn default() -> Self {
        Self {
            enabled: false,
            items: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DJConfigOverrideSingle<T> {
    pub enabled: bool,
    pub value: Option<T>,
}

impl<T> Default for DJConfigOverrideSingle<T> {
    fn default() -> Self {
        Self {
            enabled: false,
            value: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RegisteredChannel {
    pub channel_id: ChannelId,
    pub guild_id: GuildId,
    pub name: String,
    pub channel_type: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivityState {
    pub activity_type: String,
    pub status: String,
}
