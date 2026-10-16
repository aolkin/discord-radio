use crate::bucket::FileResolver;
use anyhow::{Context, anyhow, bail};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use serde::Deserialize;
use serde_json::{Map, Value};
use serenity::all::Http;
use serenity::model::id::GuildId;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::mem;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};

// Discord rate-limits modify-current-member, so edits arriving within this
// window are sent as one request.
const EDIT_DEBOUNCE: Duration = Duration::from_secs(2);

pub type ApplyResult = Result<(), Arc<anyhow::Error>>;

/// Fields left as `None` are not sent. `avatar` and `banner` are file references
/// resolved like track filenames.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct ProfileEdit {
    pub nick: Option<String>,
    pub avatar: Option<String>,
    pub banner: Option<String>,
}

impl ProfileEdit {
    pub fn is_empty(&self) -> bool {
        self.nick.is_none() && self.avatar.is_none() && self.banner.is_none()
    }

    fn merge(&mut self, later: ProfileEdit) {
        self.nick = later.nick.or(self.nick.take());
        self.avatar = later.avatar.or(self.avatar.take());
        self.banner = later.banner.or(self.banner.take());
    }
}

struct PendingEdit {
    edit: ProfileEdit,
    generation: u64,
    waiters: Vec<oneshot::Sender<ApplyResult>>,
}

/// Sends edits to Discord's modify-current-member route.
pub struct BotProfileManager {
    pending: Mutex<HashMap<GuildId, PendingEdit>>,
    next_generation: AtomicU64,
    apply_lock: Mutex<()>,
    file_resolver: FileResolver,
}

impl BotProfileManager {
    pub fn new(file_resolver: FileResolver) -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            next_generation: AtomicU64::new(0),
            apply_lock: Mutex::new(()),
            file_resolver,
        }
    }

    /// Merges `edit` into any not-yet-sent edit for the guild (later values win
    /// per field), then waits for the debounced request to Discord and returns
    /// its result.
    pub async fn edit(
        &'static self,
        http: Arc<Http>,
        guild_id: GuildId,
        edit: ProfileEdit,
    ) -> ApplyResult {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        match pending.entry(guild_id) {
            Entry::Occupied(entry) => {
                let entry = entry.into_mut();
                entry.edit.merge(edit);
                entry.generation = generation;
                entry.waiters.push(tx);
            }
            Entry::Vacant(entry) => {
                entry.insert(PendingEdit {
                    edit,
                    generation,
                    waiters: vec![tx],
                });
            }
        }
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
        let (edit, waiters) = {
            let mut pending = self.pending.lock().await;
            let Some(entry) = pending.get_mut(&guild_id) else {
                return;
            };
            if entry.generation != generation {
                return;
            }
            (entry.edit.clone(), mem::take(&mut entry.waiters))
        };

        let result = self.apply(http, guild_id, edit).await.map_err(Arc::new);
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

    async fn apply(&self, http: &Http, guild_id: GuildId, edit: ProfileEdit) -> anyhow::Result<()> {
        let mut body = Map::new();
        if let Some(nick) = edit.nick {
            body.insert("nick".into(), nick.into());
        }
        if let Some(avatar) = edit.avatar {
            body.insert("avatar".into(), self.image_data(&avatar).await?);
        }
        if let Some(banner) = edit.banner {
            body.insert("banner".into(), self.image_data(&banner).await?);
        }

        http.edit_member_me(guild_id, &body, None)
            .await
            .context("Failed to send the profile edit to Discord")?;
        Ok(())
    }

    async fn image_data(&self, file: &str) -> anyhow::Result<Value> {
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
