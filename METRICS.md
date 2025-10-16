# Metrics Implementation

This document describes the metrics instrumentation added to the Discord bot.

## Overview

The bot now emits OpenTelemetry metrics in OTLP format via HTTP to Grafana Cloud (or any compatible OTLP endpoint). Metrics are exported every 60 seconds automatically.

## Configuration

Set the following environment variables to enable metrics:

```bash
# Grafana Cloud OTLP endpoint (adjust region as needed)
GRAFANA_METRICS_URL=https://otlp-gateway-prod-us-central-0.grafana.net/otlp/v1/metrics

# Your Grafana Cloud API key (also called "instance ID" and "token" in some docs)
GRAFANA_API_KEY=your_grafana_cloud_api_key
```

If these environment variables are not set, the bot will run normally without metrics.

## Metrics Collected

### DJ State Metrics

- **dj_state_transitions** (counter): Number of DJ state transitions
  - Labels: `guild_id`, `from_state`, `to_state`
  
- **dj_state_duration** (histogram): Time spent in each DJ state (in seconds)
  - Labels: `guild_id`, `state`
  - States: `playing_track`, `playing_hex_message`, `playing_noise`, `idle`, `stopped`

### Track Playback Metrics

- **track_playback_started** (counter): Number of tracks started
  - Labels: `guild_id`, `track_name`

- **track_playback_stopped** (counter): Number of tracks stopped
  - Labels: `guild_id`, `track_name`

### Hex Message Metrics

- **hex_message_started** (counter): Number of hex messages started
  - Labels: `guild_id`

- **hex_message_completed** (counter): Number of hex messages completed
  - Labels: `guild_id`

- **hex_message_loops** (histogram): Number of loops for hex messages
  - Labels: `guild_id`

### Noise State Metrics

- **noise_state_changes** (counter): Number of noise state changes
  - Labels: `guild_id`, `noise_profile`

- **noise_state_duration** (histogram): Duration spent in noise states (in seconds)
  - Labels: `guild_id`, `noise_profile`

### System Metrics

- **bot_heartbeat** (counter): Bot liveness heartbeat (incremented every 60 seconds)
  - No labels

- **active_guilds** (gauge): Number of guilds with active voice connections
  - No labels

- **active_voice_connections** (gauge): Number of active voice connections
  - No labels

## Implementation Details

The metrics implementation:

1. Uses the `opentelemetry` and `opentelemetry-otlp` crates
2. Exports metrics via HTTP using the OTLP protocol
3. Sends metrics to Grafana Cloud every 60 seconds via a periodic reader
4. Includes resource attributes: `service.name=discord-bot`, `service.version=<build_number>`

## Example Grafana Queries

Here are some example PromQL queries you can use in Grafana:

### DJ State Transitions
```promql
rate(dj_state_transitions_total[5m])
```

### Average DJ State Duration
```promql
rate(dj_state_duration_sum[5m]) / rate(dj_state_duration_count[5m])
```

### Track Playback Rate
```promql
rate(track_playback_started_total[5m])
```

### Bot Uptime (via heartbeats)
```promql
increase(bot_heartbeat_total[1h])
```

## Troubleshooting

If metrics are not appearing in Grafana Cloud:

1. Verify the `GRAFANA_METRICS_URL` and `GRAFANA_API_KEY` environment variables are set correctly
2. Check the bot logs for metrics initialization messages
3. Ensure the Grafana Cloud endpoint URL is correct for your region
4. Verify your API key has the necessary permissions

The bot will log the following on startup if metrics are configured:
```
INFO Initializing metrics with OTLP HTTP exporter
INFO Metrics initialized successfully
```

If not configured, you'll see:
```
INFO Metrics not configured (GRAFANA_METRICS_URL and GRAFANA_API_KEY not set)
```
