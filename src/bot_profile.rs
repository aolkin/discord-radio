use crate::bucket::FileResolver;
use crate::persistence::{BotProfile, GuildMapStore};
use anyhow::{Context, anyhow, bail};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde_json::{Map, Value};
use serenity::all::Http;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::mem;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};

// Discord rate-limits modify-current-member, so edits arriving within this
// window are sent as one request.
const EDIT_DEBOUNCE: Duration = Duration::from_secs(2);

pub type ApplyResult = Result<(), Arc<anyhow::Error>>;

struct PendingEdit {
    desired: BotProfile,
    generation: u64,
    waiters: Vec<oneshot::Sender<ApplyResult>>,
}

/// Keeps the last profile Discord accepted for each guild and sends edits to
/// Discord's modify-current-member route.
pub struct BotProfileManager {
    applied: GuildMapStore<BotProfile>,
    pending: Mutex<HashMap<GuildId, PendingEdit>>,
    next_generation: AtomicU64,
    apply_lock: Mutex<()>,
    file_resolver: FileResolver,
}

impl BotProfileManager {
    pub fn new(path: PathBuf, file_resolver: FileResolver) -> Self {
        Self {
            applied: GuildMapStore::new(path),
            pending: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
            apply_lock: Mutex::new(()),
            file_resolver,
        }
    }

    pub async fn applied(&self, guild_id: GuildId) -> BotProfile {
        self.applied.get(guild_id).await
    }

    /// Applies `edit` on top of any not-yet-sent edits for the guild, then waits
    /// for the debounced request to Discord and returns its result.
    pub async fn edit(
        &'static self,
        http: Arc<Http>,
        guild_id: GuildId,
        edit: impl FnOnce(&mut BotProfile) + Send,
    ) -> ApplyResult {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        let entry = match pending.entry(guild_id) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => entry.insert(PendingEdit {
                desired: self.applied.get(guild_id).await,
                generation,
                waiters: Vec::new(),
            }),
        };
        edit(&mut entry.desired);
        entry.generation = generation;
        entry.waiters.push(tx);
        drop(pending);

        tokio::spawn(async move {
            tokio::time::sleep(EDIT_DEBOUNCE).await;
            self.flush(&http, guild_id, generation).await;
        });

        rx.await.unwrap_or_else(|_| {
            Err(Arc::new(anyhow!(
                "profile edit task ended without a result"
            )))
        })
    }

    async fn flush(&self, http: &Http, guild_id: GuildId, generation: u64) {
        let _apply = self.apply_lock.lock().await;
        let (desired, waiters) = {
            let mut pending = self.pending.lock().await;
            let Some(entry) = pending.get_mut(&guild_id) else {
                return;
            };
            if entry.generation != generation {
                return;
            }
            (entry.desired.clone(), mem::take(&mut entry.waiters))
        };

        let result = self.apply(http, guild_id, desired).await.map_err(Arc::new);
        if let Err(e) = &result {
            tracing::warn!("Failed to apply bot profile in guild {guild_id}: {e:#}");
        }

        let mut pending = self.pending.lock().await;
        if pending
            .get(&guild_id)
            .is_some_and(|entry| entry.generation == generation)
        {
            pending.remove(&guild_id);
        }
        drop(pending);

        for waiter in waiters {
            let _ = waiter.send(result.clone());
        }
    }

    async fn apply(
        &self,
        http: &Http,
        guild_id: GuildId,
        desired: BotProfile,
    ) -> anyhow::Result<()> {
        let applied = self.applied.get(guild_id).await;
        let mut body = Map::new();
        if desired.nick != applied.nick {
            body.insert("nick".into(), desired.nick.clone().into());
        }
        if desired.avatar != applied.avatar {
            let avatar = self.image_data(desired.avatar.as_deref()).await?;
            body.insert("avatar".into(), avatar);
        }
        if desired.banner != applied.banner {
            let banner = self.image_data(desired.banner.as_deref()).await?;
            body.insert("banner".into(), banner);
        }
        if body.is_empty() {
            return Ok(());
        }

        http.edit_member_me(guild_id, &body, None)
            .await
            .context("Failed to send the profile edit to Discord")?;
        self.applied
            .update(guild_id, |profile| {
                *profile = desired;
                Ok(())
            })
            .await
            .map_err(|e| anyhow!(e).context("Failed to save the applied profile"))
    }

    async fn image_data(&self, file: Option<&str>) -> anyhow::Result<Value> {
        let Some(file) = file else {
            return Ok(Value::Null);
        };
        let bytes = self
            .file_resolver
            .resolve_contents(file)
            .await
            .with_context(|| format!("Failed to load image {file}"))?;
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF8") {
            "image/gif"
        } else {
            bail!("Image {file} is not a PNG, JPEG, or GIF");
        };
        Ok(format!("data:{mime};base64,{}", BASE64_STANDARD.encode(&bytes)).into())
    }
}
