use crate::audio::connection::connect_voice_channel;
use crate::audio::dj::manager::DJCommand;
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::{ChannelStatus, default_profile_fade};
use crate::audio::dj::tiered::{ConfigurableTier, TimedValue};
use crate::audio::processing::processor::ProcessorRef;
use crate::audio::tracks::{ChangeTrackStateRequest, TrackState};
use crate::commands::utils;
use crate::commands::utils::{Context, require_guild};
use anyhow::{Context as _, anyhow, bail};
use serenity::all::{ChannelId, ChannelType, CreateAutocompleteResponse, GuildId};
use std::path::Path;
use std::time::Duration;

fn send_to_dj(
    bot_state: crate::state::Data,
    guild_id: GuildId,
    command: DJCommand,
) -> anyhow::Result<()> {
    if !bot_state.dj_manager(guild_id).send(command)? {
        bail!("Not in a voice channel");
    }
    Ok(())
}

/// Join a voice channel to broadcast audio
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn join_voice_channel(
    ctx: Context<'_>,
    #[description = "Voice channel to join and broadcast audio"] channel: ChannelId,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed join_voice_channel for channel {}",
        user_id,
        channel,
    );

    ctx.defer_ephemeral().await?;

    let channel_info = utils::get_channel_details(ctx, channel).await?;

    if channel_info
        .guild()
        .is_none_or(|guild_channel| guild_channel.kind != ChannelType::Voice)
    {
        tracing::warn!("Channel {} is not a voice channel", channel);
        ctx.say("The specified channel is not a voice channel!")
            .await?;
        return Ok(());
    }

    join_voice_channel_helper(ctx, guild_id, channel)
        .await
        .context("Failed to join the voice channel")?;

    let _ = ctx
        .data()
        .state_store
        .save_voice_channel(guild_id, channel)
        .await;

    ctx.say(format!("Joined voice channel <#{}>", channel))
        .await?;

    Ok(())
}

/// Leave the current voice channel
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn leave_voice_channel(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!("User {} executed leave_voice_channel", user_id,);

    ctx.defer_ephemeral().await?;

    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    if manager.get(guild_id).is_some() {
        if let Err(e) = manager.remove(guild_id).await {
            ctx.say(format!("Failed to leave voice channel: {}", e))
                .await?;
        } else {
            let _ = ctx.data().state_store.remove_voice_channel(guild_id).await;
            if let Some(dj_manager) = ctx.data().existing_dj_manager(guild_id) {
                dj_manager.stop().await;
            }
            ctx.say("Left voice channel").await?;
        }
    } else {
        ctx.say("Not in a voice channel").await?;
    }

    Ok(())
}

/// Have the DJ play a text message as hex audio
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn hex(
    ctx: Context<'_>,
    #[description = "Message to convert to hex and play"] message: String,
    #[description = "Volume 0.0-2.0 (0=silence, 1.0=unity, 2.0=+18dB boost, default 1.0)"]
    volume: Option<f32>,
    #[description = "Times to loop the message (default from the pool's hex config)"] loops: Option<
        u32,
    >,
    #[description = "Pool to take segment defaults from"]
    #[autocomplete = "autocomplete_pool"]
    pool: Option<String>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed dj hex with message '{}'",
        user_id,
        message,
    );

    if let Some(pool) = &pool {
        ctx.data()
            .dj_config_manager
            .pools
            .require(pool)
            .context("Failed to play message")?;
    }

    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content("Playing message...")
                .ephemeral(true),
        )
        .await?;

    let request = crate::audio::dj::manager::HexMessageRequest {
        message: message.clone(),
        volume,
        loops,
        pool,
    };
    send_to_dj(ctx.data(), guild_id, DJCommand::ForceHexMessage(request))
        .context("Failed to play message")?;
    let loops_text = match loops {
        Some(n) => format!("{n} loops"),
        None => "default loop count".to_string(),
    };
    let content = format!(
        "Playing message: \"{}\" ({}, volume {})",
        message,
        loops_text,
        volume.unwrap_or(1.0)
    );
    reply
        .edit(
            ctx,
            poise::CreateReply::default()
                .content(content)
                .ephemeral(true),
        )
        .await?;

    Ok(())
}

/// Have the DJ play an audio track now
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn track(
    ctx: Context<'_>,
    #[description = "Audio filename"]
    #[autocomplete = "autocomplete_audio_file"]
    filename: String,
    #[description = "Volume 0.0-2.0 (0=silence, 1.0=unity, 2.0=+18dB boost, default 1.0)"]
    volume: Option<f32>,
    #[description = "Start position in seconds (default 0)"] start_position: Option<f32>,
    #[description = "Seconds to play (default: track length)"] duration: Option<f32>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed dj track with file '{}'",
        user_id,
        filename,
    );

    ctx.defer_ephemeral().await?;

    let request = crate::audio::dj::manager::TrackRequest {
        filename: filename.clone(),
        volume,
        start_position,
        duration,
    };
    request.validate()?;

    ctx.data()
        .file_resolver
        .resolve(&filename)
        .await
        .context("Failed to resolve audio file")?;

    send_to_dj(ctx.data(), guild_id, DJCommand::ForceTrack(request))
        .context("Failed to use DJ for track playback")?;
    ctx.say(format!(
        "DJ is now playing track: {} (volume {})",
        filename,
        volume.unwrap_or(1.0)
    ))
    .await?;

    Ok(())
}

/// Have the DJ go idle now, optionally playing a noise profile
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn idle(
    ctx: Context<'_>,
    #[description = "Noise profile to play (default: none)"]
    #[autocomplete = "autocomplete_profile"]
    noise_profile: Option<String>,
    #[description = "Seconds to stay idle (default: until the DJ is advanced)"] duration: Option<
        f32,
    >,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!(
        "User {} executed dj idle with noise profile {:?}",
        ctx.author().id,
        noise_profile,
    );

    ctx.defer_ephemeral().await?;

    let request = crate::audio::dj::manager::IdleRequest {
        noise_profile: noise_profile.clone(),
        duration,
    };
    request.validate()?;

    if let Some(profile) = &noise_profile
        && ctx.data().profile_manager.get(profile).is_none()
    {
        bail!("Profile '{profile}' not found");
    }

    send_to_dj(ctx.data(), guild_id, DJCommand::ForceIdle(request))
        .context("Failed to make the DJ idle")?;
    let what = match &noise_profile {
        Some(profile) => format!("playing noise profile '{profile}'"),
        None => "idle".to_string(),
    };
    let until = match duration {
        Some(seconds) => format!("for {seconds}s"),
        None => "until advanced".to_string(),
    };
    ctx.say(format!("DJ is now {what} {until}")).await?;

    Ok(())
}

fn has_audio_extension(path: &Path) -> bool {
    path.extension().is_some_and(|ext| {
        matches!(
            ext.to_string_lossy().to_lowercase().as_str(),
            "mp3" | "ogg" | "wav" | "flac"
        )
    })
}

fn autocomplete_response(choices: Vec<String>) -> CreateAutocompleteResponse {
    choices
        .into_iter()
        .fold(CreateAutocompleteResponse::new(), |response, choice| {
            response.add_string_choice(choice.clone(), choice)
        })
}

async fn autocomplete_audio_file(ctx: Context<'_>, partial: &'_ str) -> CreateAutocompleteResponse {
    let mut results: Vec<String> = ctx
        .data()
        .file_resolver
        .list(partial)
        .await
        .into_iter()
        .filter(|filename| filename.ends_with('/') || has_audio_extension(Path::new(filename)))
        .collect();
    results.sort();
    results.truncate(25);
    autocomplete_response(results)
}

/// Start or stop an audio track with fade transition
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn change_track_state(
    ctx: Context<'_>,
    #[description = "Track name identifier"] name: String,
    #[description = "Track state: start or stop (omit to update existing)"] state: Option<
        TrackState,
    >,
    #[description = "Audio filename (required for start)"]
    #[autocomplete = "autocomplete_audio_file"]
    filename: Option<String>,
    #[description = "Volume 0.0-2.0 (0=silence, 1.0=unity, 2.0=+18dB boost)"] volume: Option<f32>,
    #[description = "Loop track"] loops: Option<bool>,
    #[description = "Fade time in seconds (default 1.0)"] fade_time: Option<f32>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    ctx.defer_ephemeral().await?;

    let message = ctx
        .data()
        .track_manager(guild_id)
        .await
        .change_state(ChangeTrackStateRequest {
            name,
            state,
            filename,
            volume,
            loops,
            fade_time,
        })
        .await?;
    ctx.say(message).await?;

    Ok(())
}

/// Display all currently playing audio tracks
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR"
)]
pub async fn get_current_tracks(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    let tracks = ctx
        .data()
        .track_manager(guild_id)
        .await
        .get_all_tracks()
        .await;

    ctx.defer_ephemeral().await?;

    if tracks.is_empty() {
        ctx.say("No tracks currently playing").await?;
        return Ok(());
    }

    let embed = serenity::all::CreateEmbed::default()
        .title("Currently Playing Tracks")
        .description(format!("{} track(s) active", tracks.len()))
        .fields(tracks.iter().map(|track| {
            (
                &track.name,
                format!(
                    "File: {}\nVolume: {:.2}\nLoops: {}",
                    track.filename, track.volume, track.loops
                ),
                false,
            )
        }))
        .color(0x00ff00);

    ctx.send(poise::CreateReply::default().embed(embed).ephemeral(true))
        .await?;

    Ok(())
}

async fn join_voice_channel_helper(
    ctx: Context<'_>,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> anyhow::Result<ProcessorRef> {
    let manager = songbird::get(ctx.serenity_context())
        .await
        .expect("Songbird Voice client placed in at initialisation.")
        .clone();

    let processor = connect_voice_channel(&manager, guild_id, channel_id, ctx.data()).await?;
    tracing::info!("Joined voice channel");
    ctx.data()
        .dj_manager(guild_id)
        .start()
        .await
        .map_err(|e| e.context("Failed to start DJ"))?;
    Ok(processor)
}

async fn autocomplete_pool(ctx: Context<'_>, partial: &'_ str) -> CreateAutocompleteResponse {
    let partial = partial.to_lowercase();
    let mut pools: Vec<String> = ctx
        .data()
        .dj_config_manager
        .pools
        .list()
        .into_iter()
        .filter(|name| name.to_lowercase().contains(&partial))
        .collect();
    pools.sort();
    pools.truncate(25);
    autocomplete_response(pools)
}

async fn autocomplete_profile(ctx: Context<'_>, partial: &'_ str) -> CreateAutocompleteResponse {
    let mut profiles: Vec<String> = ctx
        .data()
        .profile_manager
        .list()
        .into_iter()
        .filter(|name| name.starts_with(partial))
        .collect();
    profiles.sort();
    autocomplete_response(profiles)
}

fn optional_seconds(seconds: Option<f32>, name: &str) -> anyhow::Result<Option<Duration>> {
    seconds
        .map(|seconds| {
            Duration::try_from_secs_f32(seconds)
                .map_err(|_| anyhow!("{name} must be a non-negative number of seconds"))
        })
        .transpose()
}

fn for_duration(duration: Option<Duration>) -> String {
    duration
        .map(|duration| format!(" for {}s", duration.as_secs_f32()))
        .unwrap_or_default()
}

async fn set_dj_profile(
    ctx: Context<'_>,
    profile: Option<String>,
    fade_duration: Option<f32>,
    duration: Option<f32>,
    label: &str,
    tier: ConfigurableTier,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let guild_id = require_guild(ctx)?;

    let fade = optional_seconds(fade_duration, "Fade duration")?;
    let duration = optional_seconds(duration, "Duration")?;
    let fade = fade.or(default_profile_fade(tier));

    if let Some(profile) = &profile
        && ctx.data().profile_manager.get(profile).is_none()
    {
        bail!("Profile '{profile}' not found");
    }

    let message = match &profile {
        Some(profile) => format!("DJ {label} set to '{profile}'{}", for_duration(duration)),
        None => format!("DJ {label} cleared"),
    };
    let playback = profile.map(|profile| {
        TimedValue::new(
            SignalProfilePlayback {
                name: profile,
                fade_in_duration: fade,
                fade_out_duration: fade,
            },
            duration,
        )
    });
    send_to_dj(
        ctx.data(),
        guild_id,
        DJCommand::SetSignalProfile(tier, playback),
    )
    .with_context(|| format!("Failed to update DJ {label}"))?;
    ctx.say(message).await?;

    Ok(())
}

/// Set or clear the DJ's background signal profile, which replaces rotation
#[poise::command(slash_command, guild_only)]
pub async fn profile(
    ctx: Context<'_>,
    #[description = "Profile name (omit to return to rotation)"]
    #[autocomplete = "autocomplete_profile"]
    profile: Option<String>,
    #[description = "Fade duration in seconds when setting"] fade_duration: Option<f32>,
    #[description = "Seconds the profile lasts (default: until cleared)"] duration: Option<f32>,
) -> anyhow::Result<()> {
    set_dj_profile(
        ctx,
        profile,
        fade_duration,
        duration,
        "background profile",
        ConfigurableTier::Manual,
    )
    .await
}

/// Set or clear the DJ's signal profile override, which beats every other profile
#[poise::command(slash_command, guild_only, rename = "override")]
pub async fn override_profile(
    ctx: Context<'_>,
    #[description = "Profile name (omit to clear)"]
    #[autocomplete = "autocomplete_profile"]
    profile: Option<String>,
    #[description = "Fade duration in seconds when setting (default: 2.0)"] fade_duration: Option<
        f32,
    >,
    #[description = "Seconds the override lasts (default: until cleared)"] duration: Option<f32>,
) -> anyhow::Result<()> {
    set_dj_profile(
        ctx,
        profile,
        fade_duration,
        duration,
        "profile override",
        ConfigurableTier::Override,
    )
    .await
}

/// Set or clear the DJ's background channel status, which replaces rotation
#[poise::command(slash_command, guild_only)]
pub async fn channel_status(
    ctx: Context<'_>,
    #[description = "Channel status (omit to return to rotation)"] status: Option<String>,
    #[description = "Seconds the status lasts (default: until cleared)"] duration: Option<f32>,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let guild_id = require_guild(ctx)?;

    let duration = optional_seconds(duration, "Duration")?;
    let message = match &status {
        Some(status) => format!(
            "DJ background channel status set to {status:?}{}",
            for_duration(duration)
        ),
        None => "DJ background channel status cleared".to_string(),
    };
    let status = status.map(|status| TimedValue::new(ChannelStatus::Fixed(status), duration));
    send_to_dj(
        ctx.data(),
        guild_id,
        DJCommand::SetChannelStatus(ConfigurableTier::Manual, status),
    )
    .context("Failed to update DJ background channel status")?;
    ctx.say(message).await?;

    Ok(())
}

#[derive(poise::ChoiceParameter)]
pub enum ChannelStatusOverrideMode {
    #[name = "Clear (use the background channel status)"]
    Clear,
    #[name = "Unmanaged (leave the channel status as it is)"]
    Unmanaged,
    #[name = "Fixed (set the status given)"]
    Fixed,
}

/// Set or clear the DJ's channel status override, which beats every other channel status
#[poise::command(slash_command, guild_only, rename = "channel_status_override")]
pub async fn override_channel_status(
    ctx: Context<'_>,
    #[description = "What the override does"] mode: ChannelStatusOverrideMode,
    #[description = "Channel status for the fixed mode"] status: Option<String>,
    #[description = "Seconds the override lasts (default: until cleared)"] duration: Option<f32>,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let guild_id = require_guild(ctx)?;

    let duration = optional_seconds(duration, "Duration")?;
    let (status_override, message) = match (mode, status) {
        (ChannelStatusOverrideMode::Fixed, Some(status)) => (
            Some(ChannelStatus::Fixed(status.clone())),
            format!(
                "DJ channel status override set to {status:?}{}",
                for_duration(duration)
            ),
        ),
        (ChannelStatusOverrideMode::Fixed, None) => bail!("The fixed mode needs a status"),
        (ChannelStatusOverrideMode::Unmanaged, _) => (
            Some(ChannelStatus::Unmanaged),
            format!(
                "DJ channel status override set to unmanaged{}; the current status stays as it is",
                for_duration(duration)
            ),
        ),
        (ChannelStatusOverrideMode::Clear, _) => {
            (None, "DJ channel status override cleared".to_string())
        }
    };
    let status_override =
        status_override.map(|status_override| TimedValue::new(status_override, duration));
    send_to_dj(
        ctx.data(),
        guild_id,
        DJCommand::SetChannelStatus(ConfigurableTier::Override, status_override),
    )
    .context("Failed to update DJ channel status override")?;
    ctx.say(message).await?;

    Ok(())
}

/// Control the radio DJ
#[poise::command(
    slash_command,
    guild_only,
    default_member_permissions = "ADMINISTRATOR",
    subcommands(
        "skip",
        "idle",
        "track",
        "hex",
        "status",
        "configure",
        "announcements",
        "profile",
        "override_profile",
        "channel_status",
        "override_channel_status"
    )
)]
pub async fn dj(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

/// Restart the DJ with a configuration
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn configure(
    ctx: Context<'_>,
    #[description = "DJ configuration name"] config: String,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!(
        "User {} executed dj configure with config {:?}",
        ctx.author().id,
        config,
    );

    ctx.defer_ephemeral().await?;

    ctx.data()
        .dj_config_manager
        .subscribe(&config)
        .and_then(|loaded| send_to_dj(ctx.data(), guild_id, DJCommand::SetConfig(loaded)))
        .context("Failed to configure DJ")?;

    ctx.say(format!("DJ configured: configuration '{config}'"))
        .await?;

    Ok(())
}

/// Choose the announcement channel or turn announcements off
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn announcements(
    ctx: Context<'_>,
    #[description = "Text channel for announcements (omit to turn announcements off)"]
    announcement_channel: Option<ChannelId>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!(
        "User {} executed dj announcements with channel {:?}",
        ctx.author().id,
        announcement_channel,
    );

    ctx.defer_ephemeral().await?;

    send_to_dj(
        ctx.data(),
        guild_id,
        DJCommand::SetAnnouncementChannel(announcement_channel),
    )
    .context("Failed to configure DJ")?;

    let message = match announcement_channel {
        Some(channel) => format!("DJ announcements in <#{channel}>"),
        None => "DJ announcements off".to_string(),
    };
    ctx.say(message).await?;

    Ok(())
}

/// Skip to the DJ's next state
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Only advance to a segment from this pool (default: any)"]
    #[autocomplete = "autocomplete_pool"]
    pool: Option<String>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!("User {} executed dj skip {:?}", ctx.author().id, pool);

    ctx.defer_ephemeral().await?;

    pool.as_deref()
        .map(|pool| ctx.data().dj_config_manager.pools.require(pool))
        .transpose()
        .and_then(|_| send_to_dj(ctx.data(), guild_id, DJCommand::ForceAdvance(pool.clone())))
        .context("Failed to skip")?;

    let message = match pool {
        Some(pool) => format!("Skipping to a segment from pool {pool:?}"),
        None => "Skipping".to_string(),
    };
    ctx.say(message).await?;

    Ok(())
}

/// Get the current state of the DJ
#[poise::command(slash_command, guild_only)]
#[tracing::instrument(skip_all, fields(guild_id = ctx.guild_id().map(|id| id.get())))]
pub async fn status(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!("User {} executed dj status", user_id);

    ctx.defer_ephemeral().await?;

    let state_arc = ctx.data().dj_manager(guild_id).state();

    let state = state_arc.read().await;

    // Get current signal profile information
    let profile = state.signal_profile();
    let profile_info = format!(
        "\nSignal Profile: **{}** ({})",
        profile.value.name, profile.tier
    );

    let display = state.segment.state_display();
    let elapsed = state.segment.elapsed().as_secs();
    let time = match display.total {
        Some(total) => format!("{elapsed}/{}s", total.as_secs()),
        None => format!("{elapsed}s elapsed"),
    };
    let state_description = match display.subject {
        Some(subject) => format!("{}: **{subject}** ({time})", display.name),
        None => format!("{} ({time})", display.name),
    };

    ctx.say(format!("DJ State: {}{}", state_description, profile_info))
        .await?;

    Ok(())
}
