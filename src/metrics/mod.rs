use opentelemetry::{
    KeyValue,
    metrics::{Counter, Gauge, Histogram, Meter, MeterProvider as _},
};
use opentelemetry_otlp::{MetricExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::info;

/// Metrics collector for the Discord bot
#[derive(Clone)]
pub struct BotMetrics {
    _meter: Meter,

    // DJ state metrics
    dj_state_transitions: Counter<u64>,
    dj_state_duration: Histogram<f64>,

    // Hex message metrics
    hex_message_started: Counter<u64>,
    hex_message_completed: Counter<u64>,
    hex_message_loops: Histogram<u64>,

    // Liveness metric
    heartbeat: Counter<u64>,

    // Active state gauges
    active_guilds: Gauge<u64>,
    active_voice_connections: Gauge<u64>,
}

impl BotMetrics {
    /// Create a new metrics instance with the given meter
    pub fn new(meter: Meter) -> Self {
        let dj_state_transitions = meter
            .u64_counter("dj_state_transitions")
            .with_description("Number of DJ state transitions")
            .with_unit("transitions")
            .build();

        let dj_state_duration = meter
            .f64_histogram("dj_state_duration")
            .with_description("Duration spent in each DJ state")
            .with_unit("s")
            .build();

        let hex_message_started = meter
            .u64_counter("hex_message_started")
            .with_description("Number of hex messages started")
            .with_unit("messages")
            .build();

        let hex_message_completed = meter
            .u64_counter("hex_message_completed")
            .with_description("Number of hex messages completed")
            .with_unit("messages")
            .build();

        let hex_message_loops = meter
            .u64_histogram("hex_message_loops")
            .with_description("Number of loops for hex messages")
            .with_unit("loops")
            .build();

        let heartbeat = meter
            .u64_counter("bot_heartbeat")
            .with_description("Bot liveness heartbeat")
            .with_unit("heartbeats")
            .build();

        let active_guilds = meter
            .u64_gauge("active_guilds")
            .with_description("Number of active guilds")
            .with_unit("guilds")
            .build();

        let active_voice_connections = meter
            .u64_gauge("active_voice_connections")
            .with_description("Number of active voice connections")
            .with_unit("connections")
            .build();

        Self {
            _meter: meter,
            dj_state_transitions,
            dj_state_duration,
            hex_message_started,
            hex_message_completed,
            hex_message_loops,
            heartbeat,
            active_guilds,
            active_voice_connections,
        }
    }

    // DJ state metrics
    pub fn record_dj_state_transition(&self, guild_id: u64, from_state: &str, to_state: &str) {
        self.dj_state_transitions.add(
            1,
            &[
                KeyValue::new("guild_id", guild_id.to_string()),
                KeyValue::new("from_state", from_state.to_string()),
                KeyValue::new("to_state", to_state.to_string()),
            ],
        );
    }

    pub fn record_dj_state_duration(
        &self,
        guild_id: u64,
        state: &str,
        signal_profile: Option<&str>,
        duration_secs: f64,
    ) {
        let mut attributes = vec![
            KeyValue::new("guild_id", guild_id.to_string()),
            KeyValue::new("state", state.to_string()),
        ];
        if let Some(signal_profile) = signal_profile {
            attributes.push(KeyValue::new("signal_profile", signal_profile.to_string()));
        }
        self.dj_state_duration.record(duration_secs, &attributes);
    }

    // Hex message metrics
    pub fn record_hex_message_started(&self, guild_id: u64) {
        self.hex_message_started
            .add(1, &[KeyValue::new("guild_id", guild_id.to_string())]);
    }

    pub fn record_hex_message_completed(&self, guild_id: u64, loops: u64) {
        self.hex_message_completed
            .add(1, &[KeyValue::new("guild_id", guild_id.to_string())]);
        self.hex_message_loops
            .record(loops, &[KeyValue::new("guild_id", guild_id.to_string())]);
    }

    // Liveness metric
    pub fn record_heartbeat(&self) {
        self.heartbeat.add(1, &[]);
    }

    // Gauge updates
    pub fn update_active_guilds(&self, count: u64) {
        self.active_guilds.record(count, &[]);
    }

    pub fn update_active_voice_connections(&self, count: u64) {
        self.active_voice_connections.record(count, &[]);
    }
}

/// Initialize metrics with OTLP HTTP exporter for Grafana Cloud
pub fn init_metrics(
    metrics_url: String,
    api_key: String,
) -> Result<(SdkMeterProvider, BotMetrics), Box<dyn std::error::Error + Send + Sync>> {
    info!("Initializing metrics with OTLP HTTP exporter");

    // Create OTLP exporter
    let exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(&metrics_url)
        .with_headers(
            [("Authorization".to_string(), format!("Basic {}", api_key))]
                .into_iter()
                .collect(),
        )
        .with_timeout(Duration::from_secs(10))
        .build()?;

    // Read metrics interval from env var, default to 60 seconds
    let metrics_interval_secs = std::env::var("METRICS_INTERVAL_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(60);

    info!(
        "Metrics collection interval: {} seconds",
        metrics_interval_secs
    );

    // Create periodic reader that exports at the configured interval
    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_interval(Duration::from_secs(metrics_interval_secs))
        .build();

    // Create meter provider with resource attributes
    let provider = SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(Resource::new(vec![
            KeyValue::new("service.name", "radio-bot"),
            KeyValue::new("service.version", env!("BUILD_RUN_NUMBER")),
        ]))
        .build();

    let meter = provider.meter("radio-bot");
    let metrics = BotMetrics::new(meter);

    info!("Metrics initialized successfully");
    Ok((provider, metrics))
}

/// Shared metrics state that can be accessed throughout the application
pub type MetricsHandle = Arc<RwLock<Option<BotMetrics>>>;

/// Create a new metrics handle
pub fn create_metrics_handle() -> MetricsHandle {
    Arc::new(RwLock::new(None))
}

/// Start the heartbeat task that sends a liveness metric every minute
pub async fn start_heartbeat_task(metrics_handle: MetricsHandle) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            if let Some(metrics) = metrics_handle.read().await.as_ref() {
                metrics.record_heartbeat();
            }
        }
    });
}

/// Start a task to periodically update gauge metrics
pub async fn start_gauge_update_task(metrics_handle: MetricsHandle, bot_state: crate::state::Data) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;

            if let Some(metrics) = metrics_handle.read().await.as_ref() {
                // Update active guilds count
                let voice_connections = bot_state.voice_connections.read().await;
                let guild_count = voice_connections.len() as u64;
                metrics.update_active_guilds(guild_count);
                metrics.update_active_voice_connections(guild_count);
            }
        }
    });
}
