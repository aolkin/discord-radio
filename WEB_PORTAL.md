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
  - Switch between audio processing profiles
  - Configurable fade duration
  - Bypass option for unprocessed audio

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

When a guild has DJ enabled, you can control it directly from the web portal:

- **Advance DJ State**: Force the DJ to advance to the next state, with optional filtering:
  - Track - Force to play a track
  - Hex Message - Force to play a hex message
  - Noise - Force to play noise
  - Any (default) - Advance to next scheduled state

- **Play Hex Message**: Queue a custom hex message to play via the DJ with default settings

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

Switch between different audio processing profiles:

- Select from available DSP profiles
- Configure fade duration for smooth transitions
- Enable bypass mode for unprocessed audio passthrough
- Changes are persisted across bot restarts

## Technology Stack

- **Backend**: Axum web framework
- **Frontend**: Vanilla HTML/CSS/JavaScript (no React)
- **Real-time**: WebSocket with automatic reconnection
- **Styling**: Gradient-based modern design

## API Endpoints

- `GET /` - Web portal UI
- `GET /api/status` - JSON snapshot of bot state (includes version and commit hash)
- `GET /api/health` - Health check endpoint
- `POST /api/guilds/:guild_id/dj/advance` - Advance DJ state (optionally to specific state type)
- `POST /api/guilds/:guild_id/hex/play` - Play hex message via DJ
- `POST /api/guilds/:guild_id/tracks` - Manage track state (start, stop, update)
- `POST /api/guilds/:guild_id/profile` - Change signal profile
- `GET /api/profiles` - List available signal profiles
- `GET /api/audio-files` - List available audio files in content directory
- `GET /ws` - WebSocket endpoint for live updates (includes version tracking)
