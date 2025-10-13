# Web Portal

The Discord bot includes a built-in web portal for monitoring bot status and configuration.

## Features

- **Real-time status updates** via WebSocket
- **Guild overview** showing all active guilds
- **Audio track monitoring** with volume indicators
- **Hex playback status** with loop counters
- **DJ state visualization** showing current activity

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

## Technology Stack

- **Backend**: Axum web framework
- **Frontend**: Vanilla HTML/CSS/JavaScript (no React)
- **Real-time**: WebSocket with automatic reconnection
- **Styling**: Gradient-based modern design

## API Endpoints

- `GET /` - Web portal UI
- `GET /api/status` - JSON snapshot of bot state
- `GET /api/health` - Health check endpoint
- `GET /ws` - WebSocket endpoint for live updates
