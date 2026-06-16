use axum::{
    routing::{get, post},
    Router,
    extract::State,
    response::Json,
    http::StatusCode,
};
use std::sync::Arc;
use parking_lot::RwLock;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod config;
pub mod models;
pub mod clickhouse_client;
pub mod mqtt_client;
pub mod api;
pub mod simulation;
pub mod acoustics;
pub mod alarm;

use config::AppConfig;
use clickhouse_client::ClickHouseClient;
use mqtt_client::MqttClient;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub clickhouse: Arc<ClickHouseClient>,
    pub mqtt: Arc<MqttClient>,
    pub alarm_engine: Arc<alarm::AlarmEngine>,
    pub drum_sessions: Arc<RwLock<std::collections::HashMap<String, models::DrumSession>>>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "bronze_drum_system=debug,tower_http=debug,axum=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let config = AppConfig::load();
    let config = Arc::new(config);

    let clickhouse = Arc::new(ClickHouseClient::new(config.clickhouse_url.clone()));
    clickhouse.ensure_database().await?;
    clickhouse.ensure_tables().await?;

    let mqtt = Arc::new(MqttClient::new(config.clone()));
    mqtt.start().await?;

    let alarm_engine = Arc::new(alarm::AlarmEngine::new(mqtt.clone(), config.clone()));

    let app_state = AppState {
        config: config.clone(),
        clickhouse: clickhouse.clone(),
        mqtt: mqtt.clone(),
        alarm_engine: alarm_engine.clone(),
        drum_sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
    };

    let app = Router::new()
        .route("/api/health", get(api::health_check))
        .route("/api/drums", get(api::list_drums))
        .route("/api/drums/:id", get(api::get_drum))
        .route("/api/drums", post(api::create_drum))
        .route("/api/sensor/readings", post(api::receive_sensor_reading))
        .route("/api/sensor/readings/:drum_id", get(api::get_sensor_readings))
        .route("/api/casting/simulate", post(api::run_casting_simulation))
        .route("/api/casting/:drum_id", get(api::get_casting_result))
        .route("/api/acoustics/analyze", post(api::run_acoustic_analysis))
        .route("/api/acoustics/:drum_id", get(api::get_acoustic_result))
        .route("/api/modes/:drum_id", get(api::get_vibration_modes))
        .route("/api/soundfield/:drum_id", get(api::get_sound_field))
        .route("/api/alarms/:drum_id", get(api::get_alarms))
        .route("/api/alarms/stream", get(api::alarm_stream))
        .route("/api/wall-thickness/:drum_id", get(api::get_wall_thickness))
        .with_state(app_state.clone());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", config.server_port)).await?;
    tracing::info!("Bronze Drum System server listening on port {}", config.server_port);
    
    let alarm_clone = alarm_engine.clone();
    let ch_clone = clickhouse.clone();
    let sessions_clone = app_state.drum_sessions.clone();
    tokio::spawn(async move {
        alarm_clone.start_background_monitor(ch_clone, sessions_clone).await;
    });

    axum::serve(listener, app).await?;

    Ok(())
}
