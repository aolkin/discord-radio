# Discord Bot - Rust Implementation

A Discord bot built with Rust using Serenity and Songbird for voice functionality.

## Prerequisites

- Rust 1.88 or later
- System dependencies:
  - `cmake`
  - `libopus-dev`
  - `pkg-config`

### Installing System Dependencies

**Ubuntu/Debian:**

```bash
sudo apt update && sudo apt install -y cmake libopus-dev pkg-config
```

## Setup

1. **Environment Variables**
   Create a `.env` file or set the following environment variables:

   ```
   DISCORD_TOKEN=your_discord_bot_token_here
   WEB_PORT=3000  # Optional, defaults to 3000
   ```

2. **Build the Project**

   ```bash
   cargo build --release
   ```

## Usage

```bash
./target/release/radio-bot <content_path>
```

Where `<content_path>` is the path to the directory containing bot content (e.g. hex message audio files).

To check the build version:

```bash
./target/release/radio-bot --version
```

This will display the GitHub Actions run number and commit hash that the binary was built from.

## Development

Run these before committing:

1. **Format your code**
   ```bash
   cargo fmt
   ```

2. **Run the linter**
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```
   All warnings must be fixed before committing.

3. **Run the tests**
   ```bash
   cargo test
   ```

## Web Portal

The bot includes a built-in web portal for monitoring status and configuration. Once the bot is running, access it at:

```
http://localhost:3000
```

The portal provides:
- Real-time status updates via WebSocket
- Guild overview showing all active guilds
- Audio track monitoring with volume indicators
- Hex playback status with loop counters
- DJ state visualization

See [WEB_PORTAL.md](WEB_PORTAL.md) for more details.

## Commands

### Voice Commands
- `/join_voice_channel` - Join a voice channel to broadcast audio
- `/leave_voice_channel` - Leave the current voice channel
- `/change_track_state` - Start or stop an audio track with fade transition
- `/get_current_tracks` - Display all currently playing audio tracks

### DJ Commands
- `/dj skip` - Skip the DJ to its next state now, optionally to a segment from a named pool in the DJ config
- `/dj idle` - Have the DJ go idle now, optionally playing a noise profile, for a number of seconds or until the DJ is advanced
- `/dj track` - Have the DJ play an audio track now
- `/dj hex` - Have the DJ play a text message as hex audio, optionally with the hex defaults of a named pool in the DJ config
- `/dj status` - Get the current status of the DJ
- `/dj configure` - Restart the DJ with a chosen configuration (the DJ runs whenever the bot is in a voice channel)
- `/dj announcements` - Choose the DJ announcement channel, or turn announcements off by omitting it
- `/dj profile` - Set the DJ's background signal profile with an optional fade used to fade in and out of it, or clear it to return to rotation
- `/dj override` - Set the DJ's signal profile override with an optional fade used to fade in and out of it (0 switches instantly), or clear it
- `/dj channel_status` - Turn the DJ's management of the voice channel status on or off; turning it off leaves the current status as it is

### Messaging Commands
- `/register_channel` - Register a channel for sending messages from the web portal
- `/speak` - Send a message with optional embed to the current channel
- `/set_status` - Set the bot's custom status

## License

This project is provided as-is for development purposes.
