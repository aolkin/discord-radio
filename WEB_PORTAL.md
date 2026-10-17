# Web Portal

The Discord bot includes a built-in web portal for monitoring bot status and configuration.

## Features

- **Real-time status updates** via WebSocket
- **Bot version and commit tracking** with automatic page reload on updates
- **Guild overview** showing all active guilds
- **Audio track monitoring** with volume indicators
- **Hex playback status** with loop counters
- **DJ state visualization** showing current activity
  - Advance DJ state with optional state type selection
  - Play hex messages directly via DJ
- **Audio track management**
  - Start, stop, and update tracks
  - Adjust volume and loop settings
  - Select audio files from available content
- **Signal profile management**
  - Set or clear the DJ's background profile and profile override

## Configuration

Set the web portal port in your `.env` file (default: 3000):

```
WEB_PORT=3000
```

## Usage

The web portal starts automatically when the bot starts. Access it at:

```
http://localhost:3000
```

The portal uses WebSockets for live status updates, automatically reconnecting if the connection is lost.

### DJ Management

When the bot is in a guild's voice channel, you can control that guild's DJ directly from the web portal:

- **Advance DJ State**: Force the DJ to advance to the next state, with optional filtering:
  - Track - Force to play a track
  - Hex Message - Force to play a hex message
  - Noise - Force to play noise
  - Any (default) - Advance to next scheduled state

- **Play Hex Message**: Queue a custom hex message to play via the DJ with default settings

- **Play Track**: Force the DJ to play a chosen audio file now, with optional volume, start position, and duration

### DJ Config Overrides

Customize DJ behavior by overriding configuration values from the web portal:

- **Hex Messages**: Add, edit, or remove hex messages that the DJ can play
  - Set custom text, weight, signal profile, loop count, and announcements
  - Enable/disable the override to switch between custom and default messages
  
- **Hex Message Announcements**: Manage announcement text that can be used with hex messages
  - Add or remove custom announcements
  - Enable/disable the override independently
  
- **State Weights**: Adjust the probability of different DJ states
  - View default weights from the configuration file
  - Set custom weights for Track, Hex Message, and Noise states
  - Enable/disable the override to switch between custom and default weights

**Note**: All override changes are persisted to storage and take effect the next time the DJ starts.

### Audio Track Management

Control audio tracks for any guild through the "Manage Tracks" button:

- **Start New Track**: Begin playing an audio file
  - Select from available audio files in the content directory
  - Set initial volume and loop behavior
  - Configure fade-in time
- **Stop Track**: Stop a playing track with configurable fade-out
- **Update Track**: Modify settings of an existing track
  - Adjust volume with crossfade
  - Toggle looping on/off

### Signal Profile Management

The DJ plays the profile override if set, else the current segment's forced profile, else its background profile. The background profile is either one set by hand or a rotating pick from the DJ config; clearing a hand-set background profile returns the DJ to rotation. The DJ saves both settings with its state.

## Technology Stack

- **Backend**: Axum web framework
- **Real-time**: WebSocket with automatic reconnection
- **Styling**: Gradient-based modern design

## API Endpoints

- `GET /` - Web portal UI
- `GET /api/status` - JSON snapshot of bot state (includes version and commit hash)
- `GET /api/health` - Health check endpoint
- `POST /api/guilds/:guild_id/dj/advance` - Advance DJ state (optionally to specific state type)
- `POST /api/guilds/:guild_id/dj/idle` - Make the DJ go idle now, optionally playing a `noise_profile`, for `duration` seconds or, without one, until the DJ is advanced
- `POST /api/guilds/:guild_id/hex/play` - Play hex message via DJ
- `POST /api/guilds/:guild_id/track/play` - Play audio track via DJ
- `POST /api/guilds/:guild_id/tracks` - Manage track state (start, stop, update)
- `POST /api/guilds/:guild_id/dj/profile` - Set the DJ background profile with an optional `fade_duration` in seconds, used to fade in and out of it, or clear it (`{"profile": null}`)
- `POST /api/guilds/:guild_id/dj/profile-override` - Set the DJ profile override with an optional `fade_duration` in seconds, used to fade in and out of it (default 2), or clear it (`{"profile": null}`)
- `POST /api/guilds/:guild_id/dj/channel-status` - Turn the DJ's management of the voice channel status on or off (`{"enabled": false}`); turning it off leaves the current status as it is
- `GET /api/guilds/:guild_id/bot-profile` - Get the bot's current per-guild profile from Discord (`{"nick", "avatar_url", "banner_url"}`)
- `PUT /api/guilds/:guild_id/bot-profile` - Set the bot's per-guild profile on Discord. Body fields `nick`, `avatar`, `banner` are each optional; omitted fields are left unchanged, and at least one is required. `avatar` and `banner` are file references resolved like track filenames. Returns 429 when Discord rate limits the request
- `GET /api/profiles` - List available signal profiles
- `GET /api/profiles/:name` - Get a signal profile's parameters
- `PUT /api/profiles/:name` - Create or replace a signal profile in object storage. The body holds the profile's parameters; the name comes from `:name`, which may contain only letters, digits, `-` and `_`. Returns 503 when object storage is not configured
- `GET /api/audio-files` - List available audio files in content directory
- `GET /api/dj-config/overrides` - Get all DJ config overrides (hex messages, announcements, state weights)
- `GET /api/dj-config/default-state-weights` - Get default state weights from config file
- `POST /api/dj-config/overrides/hex-messages` - Add or update a hex message override
- `DELETE /api/dj-config/overrides/hex-messages/{index}` - Delete a hex message override
- `POST /api/dj-config/overrides/announcements` - Add or update an announcement override
- `DELETE /api/dj-config/overrides/announcements/{index}` - Delete an announcement override
- `POST /api/dj-config/overrides/state-weights` - Set state weights override
- `POST /api/dj-config/overrides/toggle` - Enable/disable a specific override category
- `GET /ws` - WebSocket endpoint for live updates (includes version tracking)
