use crate::state::Data;
use poise::serenity_prelude as serenity;

pub struct Handler {
    pub data: Data,
}

#[serenity::async_trait]
impl serenity::EventHandler for Handler {
    async fn ready(&self, ctx: serenity::Context, ready: serenity::Ready) {
        tracing::info!("{} is connected!", ready.user.name);

        // Set the context for the activity manager
        self.data.activity_manager.set_context(ctx.clone()).await;

        // Restore activity state from disk
        if let Err(e) = self.data.activity_manager.restore_from_disk().await {
            tracing::error!("Failed to restore activity state: {e:#}");
        }

        if let Err(e) = crate::startup::restore_voice_channels(&ctx, self.data).await {
            tracing::error!("Failed to restore voice channels: {e:#}");
        }
    }

    #[tracing::instrument(skip_all, fields(guild_id = new.guild_id.map(|id| id.get())))]
    async fn voice_state_update(
        &self,
        ctx: serenity::Context,
        old: Option<serenity::VoiceState>,
        new: serenity::VoiceState,
    ) {
        // Only track voice state changes for channels we're connected to
        let guild_id = match new.guild_id {
            Some(id) => id,
            None => return,
        };

        if self.data.get_voice_channel_id(guild_id).await.is_none() {
            return;
        }

        let user_id = new.user_id;
        if user_id == ctx.cache.current_user().id {
            return;
        }

        // Get user information
        let member = new.member.as_ref();
        let username = member.map(|m| m.user.name.clone()).unwrap_or_default();
        let nickname = member.and_then(|m| m.nick.clone());

        match (old.and_then(|o| o.channel_id), new.channel_id) {
            (None, Some(channel_id)) => {
                tracing::info!("User {} joined voice channel {}", user_id, channel_id);

                // Log the join event
                self.data
                    .log_member_activity(
                        guild_id.get(),
                        user_id.get(),
                        &username,
                        nickname.as_deref(),
                        "joined",
                        Some(channel_id.get()),
                    )
                    .await;
            }
            (Some(old_channel), None) => {
                tracing::info!("User {} left voice channel {}", user_id, old_channel);

                // Log the leave event
                self.data
                    .log_member_activity(
                        guild_id.get(),
                        user_id.get(),
                        &username,
                        nickname.as_deref(),
                        "left",
                        Some(old_channel.get()),
                    )
                    .await;
            }
            (Some(old_channel), Some(new_channel)) if old_channel != new_channel => {
                tracing::info!(
                    "User {} moved from voice channel {} to {}",
                    user_id,
                    old_channel,
                    new_channel
                );

                // Log the move event
                self.data
                    .log_member_activity(
                        guild_id.get(),
                        user_id.get(),
                        &username,
                        nickname.as_deref(),
                        "moved",
                        Some(new_channel.get()),
                    )
                    .await;
            }
            _ => {}
        }
    }
}
