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

## License

This project is provided as-is for development purposes.
