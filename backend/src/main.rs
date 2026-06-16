use axum::{
    routing::{get, post},
    Router,
    extract::State,
    response::Json,
};
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::sync::mpsc;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod config;
pub mod models;
pub mod clickhouse_client;
pub mod mqtt_client;
pub mod api;
pub mod dtu_receiver;
pub mod casting_simulator;
pub mod acoustic_analyzer;
pub mod alarm_mqtt;

use config::AppConfig;
use clickhouse_client::ClickHouseClient;
use mqtt_client::MqttClient;
use dtu_receiver::DtuEvent;
use casting_simulator::{CastingCommand, CastingSimulatorService};
use acoustic_analyzer::{AcousticsCommand, AcousticAnalyzerService};
use alarm_mqtt::{AlarmCommand, AlarmMqttService};

const CHANNEL_CAPACITY: usize = 64;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub clickhouse: Arc<ClickHouseClient>,
    pub mqtt: Arc<MqttClient>,
    pub alarm_broadcast: tokio::sync::broadcast::Sender<models::Alarm>,
    pub drum_sessions: Arc<RwLock<std::collections::HashMap<String, models::DrumSession>>>,
    pub dtu_tx: mpsc::Sender<DtuEvent>,
    pub casting_tx: mpsc::Sender<CastingCommand>,
    pub acoustics_tx: mpsc::Sender<AcousticsCommand>,
    pub alarm_tx: mpsc::Sender<AlarmCommand>,
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
    info!("Starting Bronze Drum System v2.0.0 [mpsc microservice architecture]");

    if let Some(mat) = &config.material {
        info!("Material config loaded: {} (E={:.2e}Pa, ρ={:.0}kg/m³)",
            mat.name, mat.youngs_modulus_pa, mat.density_kgm3);
    }
    if let Some(ac) = &config.acoustics {
        info!("Acoustics config loaded: {} modes, {} eigenvalues, c0={:.0}m/s",
            ac.mode_count, ac.eigenvalues_lambda.len(), ac.air_sound_speed_ms);
    }

    let clickhouse = Arc::new(ClickHouseClient::new(config.clickhouse_url.clone()));
    match clickhouse.ensure_database().await {
        Ok(_) => info!("ClickHouse database initialized successfully"),
        Err(e) => warn!("ClickHouse database init failed (continuing without DB): {:#}", e),
    }
    match clickhouse.ensure_tables().await {
        Ok(_) => info!("ClickHouse tables initialized successfully"),
        Err(e) => warn!("ClickHouse tables init failed (continuing without DB): {:#}", e),
    }

    let mqtt = Arc::new(MqttClient::new(config.clone()));
    match mqtt.start().await {
        Ok(_) => info!("MQTT client started successfully"),
        Err(e) => warn!("MQTT client start failed (continuing without MQTT): {:#}", e),
    }

    let (dtu_tx, dtu_rx) = mpsc::channel::<DtuEvent>(CHANNEL_CAPACITY);
    let (casting_tx, casting_rx) = mpsc::channel::<CastingCommand>(CHANNEL_CAPACITY);
    let (acoustics_tx, acoustics_rx) = mpsc::channel::<AcousticsCommand>(CHANNEL_CAPACITY);
    let (alarm_tx, alarm_rx) = mpsc::channel::<AlarmCommand>(CHANNEL_CAPACITY);

    let alarm_service = AlarmMqttService::new(
        config.clone(), mqtt.clone(), alarm_rx,
    );
    let alarm_broadcast = alarm_service.alarm_tx.clone();
    let alarm_service_clone_drain = alarm_service.clone_for_drain();

    let alarm_handle = tokio::spawn(async move {
        alarm_service.run().await;
    });

    let casting_service = CastingSimulatorService::new(
        config.clone(), casting_rx, alarm_tx.clone(),
    );
    let casting_handle = tokio::spawn(async move {
        casting_service.run().await;
    });

    let acoustics_service = AcousticAnalyzerService::new(
        config.clone(), acoustics_rx, alarm_tx.clone(),
    );
    let acoustics_handle = tokio::spawn(async move {
        acoustics_service.run().await;
    });

    let alarm_tx_for_casting = alarm_tx.clone();
    tokio::spawn(async move {
        let mut casting_events = dtu_rx;
        while let Some(event) = casting_events.recv().await {
            let _ = alarm_tx_for_casting.send(AlarmCommand::ProcessDtuEvent {
                event,
                session: models::DrumSession::new("unknown".to_string()),
            }).await;
        }
    });

    let app_state = AppState {
        config: config.clone(),
        clickhouse: clickhouse.clone(),
        mqtt: mqtt.clone(),
        alarm_broadcast,
        drum_sessions: Arc::new(RwLock::new(std::collections::HashMap::new())),
        dtu_tx,
        casting_tx,
        acoustics_tx,
        alarm_tx,
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
    info!("==================================================");
    info!("Bronze Drum System v2.0.0 - Module Architecture");
    info!("==================================================");
    info!("  [dtu_receiver]     channel: dtu_tx → alarm_mqtt");
    info!("  [casting_sim]      channel: casting_tx → alarm_mqtt");
    info!("  [acoustic_analyzer] channel: acoustics_tx → alarm_mqtt");
    info!("  [alarm_mqtt]       channel: alarm_rx → MQTT + WS");
    info!("==================================================");
    info!("Server listening on http://0.0.0.0:{}", config.server_port);
    info!("Health check: http://0.0.0.0:{}/api/health", config.server_port);
    info!("==================================================");
    
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
        loop {
            interval.tick().await;
            let _ = alarm_service_clone_drain.drain_pending();
        }
    });

    let server_handle = axum::serve(listener, app);
    tokio::select! {
        res = server_handle => {
            if let Err(e) = res {
                error!("Server error: {:?}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl-C received, shutting down gracefully");
        }
    }

    info!("Sending shutdown signals to services...");
    let _ = app_state.casting_tx.send(CastingCommand::Shutdown).await;
    let _ = app_state.acoustics_tx.send(AcousticsCommand::Shutdown).await;
    let _ = app_state.alarm_tx.send(AlarmCommand::Shutdown).await;

    info!("Waiting for service tasks to complete...");
    let _ = tokio::join!(alarm_handle, casting_handle, acoustics_handle);

    info!("All services shut down. Goodbye!");

    Ok(())
}
