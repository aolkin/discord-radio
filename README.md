# Discord Bot - Rust Implementation

A Discord bot built with Rust using Serenity and Songbird for voice functionality.

## Prerequisites

- Rust 1.74 or later
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
./target/release/discord-bot <content_path>
```

Where `<content_path>` is the path to the directory containing bot content (e.g. hex message audio files).

To check the build version:

```bash
./target/release/discord-bot --version
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
- `/play_message` - Convert a text message to hex and play it as audio in voice
- `/stop_message` - Stop the currently playing message
- `/change_track_state` - Start or stop an audio track with fade transition
- `/get_current_tracks` - Display all currently playing audio tracks
- `/signal_profile` - Change the audio signal processing profile
- `/manage_dj` - Manage the radio DJ for automated playback
- `/get_dj_state` - Get the current state of the DJ
- `/advance_dj_state` - Force the DJ to advance to the next state (for testing)

### Messaging Commands
- `/register_channel` - Register a channel for sending messages from the web portal
- `/speak` - Send a message with optional embed to the current channel
- `/set_status` - Set the bot's custom status

## License

This project is provided as-is for development purposes.
