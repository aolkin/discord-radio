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
   Create a `.env` file or set the following environment variable:

   ```
   DISCORD_TOKEN=your_discord_bot_token_here
   ```

2. **Build the Project**

   ```bash
   cargo build --release
   ```

## Usage

```bash
./target/release/discord-bot <hex_audio_dir>
```

Where `<hex_audio_dir>` is the path to the directory containing hex message audio files.

## License

This project is provided as-is for development purposes.
