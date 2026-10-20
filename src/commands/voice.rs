use crate::audio::connection::connect_voice_channel;
use crate::audio::dj::manager::{DJCommand, DJStateType};
use crate::audio::dj::segments::SignalProfilePlayback;
use crate::audio::dj::state_machine::DEFAULT_PROFILE_FADE;
use crate::audio::processing::processor::ProcessorRef;
use crate::commands::utils;
use crate::commands::utils::{Context, require_guild};
use anyhow::bail;
use serenity::all::{ChannelId, ChannelType, CreateAutocompleteResponse, GuildId};
use std::path::Path;
use std::time::Duration;

fn send_to_dj(
    bot_state: crate::state::Data,
    guild_id: GuildId,
    command: DJCommand,
) -> anyhow::Result<()> {
    if !bot_state.send_dj_command(guild_id, command)? {
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
pub async fn join_voice_channel(
    ctx: Context<'_>,
    #[description = "Voice channel to join and broadcast audio"] channel: ChannelId,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed join_voice_channel for channel {} in guild {}",
        user_id,
        channel,
        guild_id
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

    if let Err(e) = join_voice_channel_helper(ctx, guild_id, channel).await {
        ctx.say(format!("Failed to join the voice channel: {e}"))
            .await?;
        return Ok(());
    }

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
pub async fn leave_voice_channel(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed leave_voice_channel in guild {}",
        user_id,
        guild_id
    );

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
pub async fn hex(
    ctx: Context<'_>,
    #[description = "Message to convert to hex and play"] message: String,
    #[description = "Volume 0.0-2.0 (0=silence, 1.0=unity, 2.0=+18dB boost, default 1.0)"]
    volume: Option<f32>,
    #[description = "Times to loop the message (default from DJ config)"] loops: Option<u32>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!(
        "User {} executed dj hex with message '{}' in guild {}",
        user_id,
        message,
        guild_id
    );

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
    };
    let content = match send_to_dj(ctx.data(), guild_id, DJCommand::ForceHexMessage(request)) {
        Ok(()) => {
            let loops_text = match loops {
                Some(n) => format!("{n} loops"),
                None => "DJ default loop count".to_string(),
            };
            format!(
                "DJ is now playing message: \"{}\" ({}, volume {})",
                message,
                loops_text,
                volume.unwrap_or(1.0)
            )
        }
        Err(e) => format!("Failed to use DJ for message playback: {}", e),
    };
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
        "User {} executed dj track with file '{}' in guild {}",
        user_id,
        filename,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let request = crate::audio::dj::manager::TrackRequest {
        filename: filename.clone(),
        volume,
        start_position,
        duration,
    };
    if let Err(e) = request.validate() {
        ctx.say(format!("{e:#}")).await?;
        return Ok(());
    }

    if let Err(e) = ctx.data().file_resolver.resolve(&filename).await {
        ctx.say(format!("Failed to resolve audio file: {e:#}"))
            .await?;
        return Ok(());
    }

    let content = match send_to_dj(ctx.data(), guild_id, DJCommand::ForceTrack(request)) {
        Ok(()) => format!(
            "DJ is now playing track: {} (volume {})",
            filename,
            volume.unwrap_or(1.0)
        ),
        Err(e) => format!("Failed to use DJ for track playback: {}", e),
    };
    ctx.say(content).await?;

    Ok(())
}

/// Have the DJ go idle now, optionally playing a noise profile
#[poise::command(slash_command, guild_only)]
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
        "User {} executed dj idle with noise profile {:?} in guild {}",
        ctx.author().id,
        noise_profile,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let request = crate::audio::dj::manager::IdleRequest {
        noise_profile: noise_profile.clone(),
        duration,
    };
    if let Err(e) = request.validate() {
        ctx.say(format!("{e:#}")).await?;
        return Ok(());
    }

    if let Some(profile) = &noise_profile
        && ctx.data().profile_manager.get(profile).is_none()
    {
        ctx.say(format!("Profile '{profile}' not found")).await?;
        return Ok(());
    }

    let content = match send_to_dj(ctx.data(), guild_id, DJCommand::ForceIdle(request)) {
        Ok(()) => {
            let what = match &noise_profile {
                Some(profile) => format!("playing noise profile '{profile}'"),
                None => "idle".to_string(),
            };
            let until = match duration {
                Some(seconds) => format!("for {seconds}s"),
                None => "until advanced".to_string(),
            };
            format!("DJ is now {what} {until}")
        }
        Err(e) => format!("Failed to make the DJ idle: {e:#}"),
    };
    ctx.say(content).await?;

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
pub async fn change_track_state(
    ctx: Context<'_>,
    #[description = "Track name identifier"] name: String,
    #[description = "Track state: start or stop (omit to update existing)"] state: Option<String>,
    #[description = "Audio filename (required for start)"]
    #[autocomplete = "autocomplete_audio_file"]
    filename: Option<String>,
    #[description = "Volume 0.0-2.0 (0=silence, 1.0=unity, 2.0=+18dB boost)"] volume: Option<f32>,
    #[description = "Loop track"] loops: Option<bool>,
    #[description = "Fade time in seconds (default 1.0)"] fade_time: Option<f32>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    ctx.defer_ephemeral().await?;

    let manager = ctx.data().track_manager(guild_id).await;
    let fade_time = fade_time.unwrap_or(1.0);

    let state_str = state.as_deref().map(|s| s.to_lowercase());
    match state_str.as_deref() {
        Some("start") => {
            let Some(filename) = filename else {
                ctx.say("filename is required for start").await?;
                return Ok(());
            };

            if let Err(e) = ctx.data().file_resolver.resolve(&filename).await {
                ctx.say(format!("Failed to resolve audio file: {e:#}"))
                    .await?;
                return Ok(());
            }

            let volume = volume.unwrap_or(1.0);
            let loops = loops.unwrap_or(true);

            if let Err(e) = manager
                .start_track(crate::audio::tracks::StartTrackArgs {
                    name: name.clone(),
                    filename,
                    volume,
                    fade_time,
                    loops,
                    start_position: None,
                })
                .await
            {
                ctx.say(format!("Failed to start track: {}", e)).await?;
            } else {
                ctx.say(format!(
                    "Started track '{}' with volume {}, {} second fade, looping: {}",
                    name, volume, fade_time, loops
                ))
                .await?;
            }
        }
        Some("stop") => {
            if let Err(e) = manager.stop_track(&name, fade_time).await {
                ctx.say(format!("Failed to stop track: {}", e)).await?;
            } else {
                ctx.say(format!(
                    "Stopped track '{}' with {} second fade",
                    name, fade_time
                ))
                .await?;
            }
        }
        None => {
            if !manager.has_track(&name).await {
                ctx.say(format!(
                    "Track '{}' not found. Use state=start to create a new track.",
                    name
                ))
                .await?;
                return Ok(());
            }

            let mut updated = Vec::new();

            if let Some(new_volume) = volume {
                if let Err(e) = manager
                    .update_track_volume(&name, new_volume, fade_time)
                    .await
                {
                    ctx.say(format!("Failed to update volume: {}", e)).await?;
                    return Ok(());
                }
                updated.push(format!("volume to {}", new_volume));
            }

            if let Some(new_loops) = loops {
                if let Err(e) = manager.update_track_loops(&name, new_loops).await {
                    ctx.say(format!("Failed to update loops: {}", e)).await?;
                    return Ok(());
                }
                updated.push(format!("loops to {}", new_loops));
            }

            if updated.is_empty() {
                ctx.say("No updates specified. Provide volume and/or loops to update.")
                    .await?;
            } else {
                ctx.say(format!("Updated track '{}': {}", name, updated.join(", ")))
                    .await?;
            }
        }
        _ => {
            ctx.say("Invalid state. Use 'start' or 'stop'").await?;
        }
    }

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

async fn set_dj_profile(
    ctx: Context<'_>,
    profile: Option<String>,
    fade_duration: Option<f32>,
    default_fade: Option<Duration>,
    label: &str,
    command: fn(Option<SignalProfilePlayback>) -> DJCommand,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let guild_id = require_guild(ctx)?;

    if profile.is_none() && fade_duration.is_some() {
        ctx.say(format!("Fade duration only applies when setting a {label}"))
            .await?;
        return Ok(());
    }
    let fade = match fade_duration.map(Duration::try_from_secs_f32) {
        Some(Ok(fade)) => Some(fade),
        Some(Err(_)) => {
            ctx.say("Fade duration must be a non-negative number of seconds")
                .await?;
            return Ok(());
        }
        None => default_fade,
    };

    if let Some(profile) = &profile
        && ctx.data().profile_manager.get(profile).is_none()
    {
        ctx.say(format!("Profile '{profile}' not found")).await?;
        return Ok(());
    }

    let message = match &profile {
        Some(profile) => format!("DJ {label} set to '{profile}'"),
        None => format!("DJ {label} cleared"),
    };
    let playback = profile.map(|profile| SignalProfilePlayback {
        name: profile,
        fade_in_duration: fade,
        fade_out_duration: fade,
    });
    if let Err(e) = send_to_dj(ctx.data(), guild_id, command(playback)) {
        ctx.say(format!("Failed to update DJ {label}: {e:#}"))
            .await?;
        return Ok(());
    }
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
) -> anyhow::Result<()> {
    set_dj_profile(
        ctx,
        profile,
        fade_duration,
        None,
        "background profile",
        DJCommand::SetBackgroundProfile,
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
) -> anyhow::Result<()> {
    set_dj_profile(
        ctx,
        profile,
        fade_duration,
        Some(DEFAULT_PROFILE_FADE),
        "profile override",
        DJCommand::SetOverrideProfile,
    )
    .await
}

/// Turn the DJ's management of the voice channel status on or off
#[poise::command(slash_command, guild_only)]
pub async fn channel_status(
    ctx: Context<'_>,
    #[description = "Whether the DJ sets the voice channel status"] enabled: bool,
) -> anyhow::Result<()> {
    ctx.defer_ephemeral().await?;

    let guild_id = require_guild(ctx)?;

    ctx.data()
        .dj_settings
        .update(guild_id, |settings| {
            settings.channel_status_disabled = !enabled;
            Ok(())
        })
        .await?;
    let message = if enabled {
        "DJ channel status management enabled"
    } else {
        "DJ channel status management disabled; the current status stays as it is"
    };
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
        "channel_status"
    )
)]
pub async fn dj(_ctx: Context<'_>) -> anyhow::Result<()> {
    Ok(())
}

/// Restart the DJ with a configuration
#[poise::command(slash_command, guild_only)]
pub async fn configure(
    ctx: Context<'_>,
    #[description = "DJ configuration name"] config: String,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!(
        "User {} executed dj configure with config {:?} in guild {}",
        ctx.author().id,
        config,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    let Some(dj_manager) = ctx.data().existing_dj_manager(guild_id) else {
        ctx.say("Failed to configure DJ: Not in a voice channel")
            .await?;
        return Ok(());
    };
    if let Err(e) = dj_manager.restart_with_config(config.clone()).await {
        ctx.say(format!("Failed to configure DJ: {e:#}")).await?;
        return Ok(());
    }

    ctx.say(format!("DJ configured: configuration '{config}'"))
        .await?;

    Ok(())
}

/// Choose the announcement channel or turn announcements off
#[poise::command(slash_command, guild_only)]
pub async fn announcements(
    ctx: Context<'_>,
    #[description = "Text channel for announcements (omit to turn announcements off)"]
    announcement_channel: Option<ChannelId>,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!(
        "User {} executed dj announcements with channel {:?} in guild {}",
        ctx.author().id,
        announcement_channel,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    if let Err(e) = send_to_dj(
        ctx.data(),
        guild_id,
        DJCommand::SetAnnouncementChannel(announcement_channel),
    ) {
        ctx.say(format!("Failed to configure DJ: {e:#}")).await?;
        return Ok(());
    }

    let message = match announcement_channel {
        Some(channel) => format!("DJ announcements in <#{channel}>"),
        None => "DJ announcements off".to_string(),
    };
    ctx.say(message).await?;

    Ok(())
}

/// Skip to the DJ's next state
#[poise::command(slash_command, guild_only)]
pub async fn skip(
    ctx: Context<'_>,
    #[description = "Only advance to this kind of state (default: any)"] to_state: Option<
        DJStateType,
    >,
) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;

    tracing::info!(
        "User {} executed dj skip {:?} in guild {}",
        ctx.author().id,
        to_state,
        guild_id
    );

    ctx.defer_ephemeral().await?;

    if let Err(e) = send_to_dj(ctx.data(), guild_id, DJCommand::ForceAdvance(to_state)) {
        ctx.say(format!("Failed to force DJ state advance: {e:#}"))
            .await?;
        return Ok(());
    }

    let message = match to_state {
        Some(filter) => format!("DJ advancing to {:?} state", filter),
        None => "DJ state advance triggered".to_string(),
    };
    ctx.say(message).await?;

    Ok(())
}

/// Get the current state of the DJ
#[poise::command(slash_command, guild_only)]
pub async fn status(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_id = require_guild(ctx)?;
    let user_id = ctx.author().id;

    tracing::info!("User {} executed dj status in guild {}", user_id, guild_id);

    ctx.defer_ephemeral().await?;

    let Some(state_arc) = ctx.data().dj_state(guild_id) else {
        ctx.say("DJ state not available").await?;
        return Ok(());
    };

    let state = state_arc.read().await;

    // Get current signal profile information
    let profile = state.effective_profile().name;
    let source = state.profile_source();
    let profile_info = format!("\nSignal Profile: **{profile}** ({source})");

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
