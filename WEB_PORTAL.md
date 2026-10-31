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
  - Select audio files from object storage
- **Signal profile management**
  - Set or clear the DJ's background profile and profile override
  - List, create, and edit signal profiles
- **DJ config editing**
  - List, create, and edit DJ configs in a form
- **Audio file browser**
  - Browse the folders and files under `tracks/` in object storage, create folders, and upload audio files by choosing or dropping them
- **Per-guild bot profile**
  - Set the bot's nickname, avatar and banner in each guild, picking or uploading images under `images/` in object storage

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

- **Advance DJ State**: Force the DJ to advance to the next state, optionally from the DJ config pool named `track`, `hex` or `noise`

- **Play Hex Message**: Queue a custom hex message to play via the DJ with default settings

- **Play Track**: Force the DJ to play a chosen audio file now, with optional volume, start position, and duration

### Audio Track Management

Control audio tracks for any guild through the "Manage Tracks" button:

- **Start New Track**: Begin playing an audio file
  - Select from the audio files in the `tracks/` folder of object storage
  - Set initial volume and loop behavior
  - Configure fade-in time
- **Stop Track**: Stop a playing track with configurable fade-out
- **Update Track**: Modify settings of an existing track
  - Adjust volume with crossfade
  - Toggle looping on/off

### Signal Profile Management

The DJ plays the profile override if set, else the current segment's forced profile, else its background profile. The background profile is either one set by hand or a rotating pick from the DJ config; clearing a hand-set background profile returns the DJ to rotation. The DJ saves both settings with its state.

The Signal Profiles panel lists every profile by name. Saving a new or edited profile writes it to object storage; a DJ picks up the new parameters the next time it transitions to that profile.

### DJ Configs

The bot loads DJ configs from the `DJ_CONFIGS_PATH` directory (default `dj_configs/`; the value must end with a slash), under both the content root and the object storage bucket root. A config in object storage replaces a local config of the same name. A DJ uses the config named `default` until one is chosen with `/dj configure`. The DJ Configs panel lists every config by name and opens a form with every field of the config, showing the server's reason when it rejects a save. Saving a config writes it to object storage; a running DJ using that config switches to the saved copy the next time it advances to a new segment.

A config's `pools` maps pool names to settings, as `{"<name>": {"weight": <weight>}}`; the weight sets how often that config picks the pool and must be above 0. A pool's settings can also hold `segment_defaults`; each field set there replaces the same field of the pool file's segment defaults for that config. The bot loads pool files from the `POOLS_PATH` directory (default `pools/`; the value must end with a slash) in the same way as configs. A pool file holds `name`, `entries`, `segment_defaults` and `history_penalty_multiplier`; `/dj skip` and `/dj hex` match the `name`. Several configs can reference the same pool. A config fails to load when a pool it references is missing or invalid, or has no entry of weight above 0. The `name` stored in a config, pool or signal profile file must match its file name without `.json`; the bot skips a file where they differ.

Each pool can set segment defaults (a hex config, a signal profile and a channel status). A segment from the pool uses its entry's own setting, then the pool's segment default, then the built-in hex config with no signal profile or channel status. The pool's signal profile overrides the rotating background profile while its tracks and hex messages play. The DJ config's pool history penalty multiplier applies only to choosing which pool plays next, and a pool's own history penalty multiplier applies only to choosing among its entries.

### Audio File Browser

The Audio Files panel opens a browser over the `tracks/` folder of object storage. Dropping or choosing files uploads them to the current folder one after another, with a progress bar per file and the server's message when a file is rejected. Breadcrumbs and Up move between folders, and each file shows its full key, size and last modified time. The same browser opens in a mode that picks a file or a folder and hands the choice back to the section that opened it.

### Bot Profile

Each guild card's Bot Profile button opens the bot's current nickname, avatar and banner in that guild, as Discord reports them. Choosing an avatar or banner opens the file browser over the `images/` folder, where you can pick an image or upload a PNG, JPEG or GIF. Save sends only the changed fields to Discord and shows Discord's error when it rejects them, such as for a too-large image or a rate limit.

## Technology Stack

- **Backend**: Axum web framework
- **Real-time**: WebSocket with automatic reconnection
- **Styling**: Gradient-based modern design

## API Endpoints

- `GET /` - Web portal UI
- `GET /api/status` - JSON snapshot of bot state (includes version and commit hash)
- `GET /api/health` - Health check endpoint
- `POST /api/guilds/:guild_id/dj/advance` - Advance DJ state, optionally to a segment from the DJ config pool named `state_type`
- `POST /api/guilds/:guild_id/dj/idle` - Make the DJ go idle now, optionally playing a `noise_profile`, for `duration` seconds or, without one, until the DJ is advanced
- `POST /api/guilds/:guild_id/hex/play` - Play hex message via DJ, with the segment defaults of the pool named `pool` when set
- `POST /api/guilds/:guild_id/track/play` - Play audio track via DJ
- `POST /api/guilds/:guild_id/tracks` - Manage track state (start, stop, update)
- `POST /api/guilds/:guild_id/dj/profile` - Set the DJ background profile with an optional `fade_duration` in seconds, used to fade in and out of it, or clear it (`{"profile": null}`)
- `POST /api/guilds/:guild_id/dj/profile-override` - Set the DJ profile override with an optional `fade_duration` in seconds, used to fade in and out of it (default 2), or clear it (`{"profile": null}`)
- `POST /api/guilds/:guild_id/dj/channel-status` - Turn the DJ's management of the voice channel status on or off (`{"enabled": false}`); turning it off leaves the current status as it is
- `GET /api/guilds/:guild_id/bot-profile` - Get the bot's current per-guild profile from Discord (`{"nick", "avatar_url", "banner_url"}`)
- `PUT /api/guilds/:guild_id/bot-profile` - Set the bot's per-guild profile on Discord. Body fields `nick`, `avatar`, `banner` are each optional; omitted fields are left unchanged, and at least one is required. `avatar` and `banner` are file references resolved like track filenames. Returns 429 when Discord rate limits the request
- `GET /api/profiles` - List available signal profiles
- `GET /api/profiles/:name` - Get a signal profile's parameters. With `?refresh=true`, re-fetch the profile from object storage first and keep the fetched copy; returns 404 when object storage has no such profile
- `PUT /api/profiles/:name` - Create or replace a signal profile in object storage. The body holds the profile's parameters; the name comes from `:name`, which may contain only letters, digits, `-` and `_`. Returns 503 when object storage is not configured
- `GET /api/dj-configs` - List available DJ configs (`{"configs": [...]}`)
- `GET /api/dj-configs/:name` - Get a DJ config. With `?refresh=true`, re-fetch the config from object storage first and keep the fetched copy; returns 404 when object storage has no such config
- `PUT /api/dj-configs/:name` - Create or replace a DJ config in object storage. The body holds the config in the same JSON form as the files under `DJ_CONFIGS_PATH`; the name comes from `:name`, which may contain only letters, digits, `-` and `_`. Returns 422 with the reason when the body is not a valid DJ config, and 503 when object storage is not configured
- `GET /api/files` - Browse object storage one folder level at a time. `?prefix=` selects the folder; returns `{"prefix", "folders", "files"}`, where `folders` are the sub-folder prefixes and each file has `key`, `size` and `last_modified`. Returns 503 when object storage is not configured
- `POST /api/files/upload` - Upload a file to object storage as `{prefix}{filename}`. `?prefix=` must be a folder under `tracks/` or `images/`. The body is `multipart/form-data` with the file in a `file` field, at most 100 MiB. Under `tracks/` it must be .mp3, .ogg or .wav, mono or stereo; a file at any sample rate other than 48kHz is resampled to 48kHz and stored as Ogg Vorbis under `{prefix}{stem}.ogg`. Under `images/` it must be .png, .jpg or .gif. Returns the stored `{"key"}`; 400 for a bad prefix, name or type, 422 for an undecodable audio file or one with more than two channels, 503 when object storage is not configured
- `POST /api/files/folders` - Create a folder under `tracks/` or `images/` in object storage (`{"prefix": "tracks/", "name": "my-folder"}`). The name may contain only letters, digits, `-` and `_`. Returns `{"key"}` of the folder marker
- `GET /ws` - WebSocket endpoint for live updates (includes version tracking)
