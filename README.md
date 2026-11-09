# Discord Bot - Rust Implementation

A Discord bot built with Rust using Serenity and Songbird for voice functionality.

## Features

- Broadcast an automated "radio station" into a Discord voice channel.
- Shuffle across uploaded audio files with repeat avoidance.
- Play messages as read-aloud voicing of their hexadecimal ASCII encoding.
- Add static and signal processing to simulate an intermittent or weak connection.
- Dynamically update the voice channel status based on the currently playing segment.
- Post "announcements" to a text channel at the start of selected segments.

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

Where `<content_path>` is the path to the directory containing bot content (e.g. hex message audio files). Hex audio files are named `hex_<char>.wav` for `0`-`9` and uppercase `A`-`F`, and are resolved from the folder named by the `HEX_AUDIO_FOLDER` environment variable (default `audio/hex/`, must end with `/`; a value such as `s3://hex/` loads them from object storage). A hex config in a pool can set `hex_audio_folder` to use another folder. A missing file is skipped with a warning.

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

### DJ Commands
- `/dj join` - Join a voice channel to broadcast audio
- `/dj leave` - Leave the current voice channel
- `/dj skip` - Skip the DJ to its next state now, optionally to a segment from a named pool in the DJ config
- `/dj idle` - Have the DJ go idle now, optionally playing a noise profile, for a number of seconds or until the DJ is advanced
- `/dj track` - Have the DJ play an audio track now
- `/dj hex` - Have the DJ play a text message as hex audio, optionally with the segment defaults of a named pool in the DJ config
- `/dj status` - Get the current status of the DJ
- `/dj configure` - Restart the DJ with a chosen configuration (the DJ runs whenever the bot is in a voice channel)
- `/dj announcements` - Choose the DJ announcement channel, or turn announcements off by omitting it
- `/dj profile` - Set the DJ's background signal profile with an optional fade used to fade in and out of it and an optional duration in seconds, or clear it to return to rotation
- `/dj override` - Set the DJ's signal profile override with an optional fade used to fade in and out of it (0 switches instantly) and an optional duration in seconds, or clear it
- `/dj channel_status` - Set the DJ's background voice channel status with an optional duration in seconds, or clear it to return to rotation
- `/dj channel_status_override` - Override the voice channel status with a fixed status or leave it unmanaged (the DJ does not touch it), with an optional duration in seconds, or clear the override

### Messaging Commands
- `/message register` - Register a channel for sending messages from the web portal
- `/message speak` - Send a message with optional embed to the current channel
- `/activity` - Set the bot's custom status

## License

This project is provided as-is for development purposes.
