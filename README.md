# Discord Bot - Rust Implementation

A Discord bot built with Rust using Serenity and Songbird for voice functionality.

## Features

- **Voice Channel Broadcasting**: Join voice channels and broadcast audio files on repeat
- **Admin Commands**: Voice channel management restricted to server administrators
- **In-Memory State**: Maintains voice connections and track handles in memory

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
   Create a `.env` file or set the following environment variable:

   ```
   DISCORD_TOKEN=your_discord_bot_token_here
   ```

2. **Build the Project**

   ```bash
   cargo build --release
   ```

3. **Run the Bot**

   The bot requires an audio file path as a command line argument:

   ```bash
   cargo run -- <path_to_audio_file>
   ```

   For example:

   ```bash
   cargo run -- audio/files/sample.mp3
   ```

## Commands

All commands require administrator permissions:

- `/join_voice_channel <channel>` - Join and start broadcasting to a voice channel
- `/leave_voice_channel` - Leave the current voice channel

## Configuration

### Audio Files

The audio file to broadcast is specified as a command line argument when starting the bot.

**Supported audio formats:**

- MP3 (`.mp3`)
- Ogg Vorbis (`.ogg`)
- WAV (`.wav`)

Place your audio files in any directory (e.g., `audio/files/`) and provide the path when starting the bot.

## Project Structure

```
bot/
├── src/
│   ├── main.rs              # Entry point and bot initialization
│   ├── state.rs             # In-memory state management and constants
│   ├── commands/
│   │   ├── mod.rs
│   │   └── admin.rs         # Admin-only voice channel management commands
│   ├── handlers/
│   │   ├── mod.rs
│   │   └── voice.rs         # Voice channel management (placeholder)
│   ├── audio/
│   │   ├── mod.rs
│   │   └── manager.rs       # Audio streaming management with looping
│   └── utils/
│       ├── mod.rs
│       └── permissions.rs   # Admin permission checking
└── audio/
    └── files/               # Directory for audio files
```

## License

This project is provided as-is for development purposes.
