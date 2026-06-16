use crate::models::{self, *};
use crate::AppState;
use crate::casting_simulator::CastingCommand;
use crate::acoustic_analyzer::AcousticsCommand;
use crate::dtu_receiver::{DtuEvent, DtuReceiver};
use crate::alarm_mqtt::AlarmCommand;
use axum::{
    extract::{Path, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::{IntoResponse, Response},
    Json,
};
use futures::{sink::SinkExt, stream::StreamExt};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub async fn health_check() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse::ok(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": "2.0.0",
        "architecture": "microservices-mpsc",
    })))
}

pub async fn list_drums(
    State(state): State<AppState>,
) -> Json<ApiResponse<Vec<Drum>>> {
    match state.clickhouse.get_drums().await {
        Ok(drums) => Json(ApiResponse::ok(drums)),
        Err(e) => {
            error!("Error listing drums: {:?}", e);
            Json(ApiResponse::err(&format!("Failed to list drums: {}", e)))
        }
    }
}

pub async fn get_drum(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<ApiResponse<Drum>> {
    match state.clickhouse.get_drum(&id).await {
        Ok(Some(drum)) => Json(ApiResponse::ok(drum)),
        Ok(None) => Json(ApiResponse::err("Drum not found")),
        Err(e) => {
            error!("Error getting drum {}: {:?}", id, e);
            Json(ApiResponse::err(&format!("Failed to get drum: {}", e)))
        }
    }
}

pub async fn create_drum(
    State(state): State<AppState>,
    Json(req): Json<CreateDrumRequest>,
) -> Json<ApiResponse<Drum>> {
    let drum = Drum::new(
        req.name,
        req.ethnic_group,
        req.origin_region,
        req.estimated_era,
        req.diameter_cm,
        req.height_cm,
        req.mass_kg,
        req.notes,
    );

    let drum_id = drum.drum_id.clone();
    match state.clickhouse.insert_drum(&drum).await {
        Ok(_) => {
            let mut sessions = state.drum_sessions.write();
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
            drop(sessions);
            info!("Created new drum: {} ({})", drum.name, drum.drum_id);
            Json(ApiResponse::ok(drum))
        }
        Err(e) => {
            error!("Error creating drum: {:?}", e);
            Json(ApiResponse::err(&format!("Failed to create drum: {}", e)))
        }
    }
}

pub async fn receive_sensor_reading(
    State(state): State<AppState>,
    Json(mut reading): Json<SensorReading>,
) -> Json<ApiResponse<Vec<Alarm>>> {
    if reading.reading_id.is_empty() {
        reading.reading_id = Uuid::new_v4().to_string();
    }
    if reading.timestamp.timestamp() <= 0 {
        reading.timestamp = chrono::Utc::now();
    }

    let drum_id = reading.drum_id.clone();
    info!("Received sensor reading for drum: {}", drum_id);

    let dtu_receiver = DtuReceiver::new(state.config.clone(), state.dtu_tx.clone());
    let validated = match dtu_receiver.receive_and_validate(reading.clone()).await {
        Ok(v) => v,
        Err(_) => reading.clone(),
    };

    {
        let mut sessions = state.drum_sessions.write();
        if !sessions.contains_key(&drum_id) {
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
        }
        if let Some(session) = sessions.get_mut(&drum_id) {
            session.last_reading_time = Some(validated.timestamp);
            session.alloy_history.push(validated.alloy.clone());
        }
    }

    if let Ok(_v) = dtu_receiver.receive_and_validate(reading.clone()).await {
        let freq_dev_cmd = AcousticsCommand::CheckFrequencyDeviation {
            spectrum: validated.tap_spectrum.clone(),
            reference_hz: state.config.acoustics.as_ref()
                .map(|a| a.reference_frequencies_hz.clone())
                .unwrap_or_default(),
            drum_id: drum_id.clone(),
            threshold_hz: state.config.threshold_frequency_deviation_hz,
        };
        let _ = state.acoustics_tx.send(freq_dev_cmd).await;
    }

    let _ = state.clickhouse.insert_sensor_reading(&validated).await;
    let _ = state.alarm_tx.send(AlarmCommand::FlushPending).await;
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    let alarms: Vec<models::Alarm> = Vec::new();
    Json(ApiResponse::ok(alarms))
}

pub async fn get_sensor_readings(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<Vec<SensorReading>>> {
    match state.clickhouse.get_sensor_readings(&drum_id, 100).await {
        Ok(readings) => Json(ApiResponse::ok(readings)),
        Err(e) => {
            error!("Error getting sensor readings: {:?}", e);
            Json(ApiResponse::err(&format!("Failed to get readings: {}", e)))
        }
    }
}

pub async fn run_casting_simulation(
    State(state): State<AppState>,
    Json(req): Json<CastingSimulationRequest>,
) -> Json<ApiResponse<CastingSimulationResult>> {
    let drum_id = req.drum_id.clone();
    info!("Running casting simulation for drum: {}", drum_id);

    let (diameter_cm, height_cm) = match state.clickhouse.get_drum(&drum_id).await {
        Ok(Some(d)) => (d.diameter_cm, d.height_cm),
        _ => {
            warn!("Drum not found or DB unavailable, using default dimensions: 50cm x 30cm");
            (50.0, 30.0)
        }
    };

    let session_opt = {
        let mut sessions = state.drum_sessions.write();
        if !sessions.contains_key(&drum_id) {
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
        }
        sessions.get(&drum_id).cloned()
    };

    let (result_tx, result_rx) = oneshot::channel::<Result<CastingSimulationResult, String>>();

    state.casting_tx.send(CastingCommand::RunSimulation {
        request: req.clone(),
        diameter_cm,
        height_cm,
        session: session_opt.clone(),
        result_tx,
    }).await.map_err(|e| format!("Failed to send casting command: {}", e)).ok();

    let result = tokio::select! {
        res = result_rx => {
            match res {
                Ok(Ok(sim_result)) => Ok(sim_result),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("Simulation channel closed".to_string()),
            }
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
            Err("Simulation timeout after 30s".to_string())
        }
    };

    match result {
        Ok(sim_result) => {
            let _ = state.clickhouse.insert_casting_simulation(&sim_result).await;
            let _ = state.alarm_tx.send(AlarmCommand::FlushPending).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            info!("Casting simulation completed: {} defects found, quality={:.2}",
                sim_result.defects.len(), sim_result.quality_score);
            Json(ApiResponse::ok(sim_result))
        }
        Err(e) => Json(ApiResponse::err(&e)),
    }
}

pub async fn get_casting_result(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<CastingSimulationResult>> {
    match state.clickhouse.get_casting_result(&drum_id).await {
        Ok(Some(result)) => Json(ApiResponse::ok(result)),
        Ok(None) => Json(ApiResponse::err("No casting simulation found")),
        Err(e) => Json(ApiResponse::err(&format!("Database error: {}", e))),
    }
}

pub async fn run_acoustic_analysis(
    State(state): State<AppState>,
    Json(req): Json<AcousticAnalysisRequest>,
) -> Json<ApiResponse<AcousticAnalysisResult>> {
    let drum_id = req.drum_id.clone();
    info!("Running acoustic analysis for drum: {}", drum_id);

    let (diameter_cm, height_cm, mass_kg) = match state.clickhouse.get_drum(&drum_id).await {
        Ok(Some(d)) => (d.diameter_cm, d.height_cm, d.mass_kg),
        _ => {
            warn!("Drum not found or DB unavailable, using default dimensions: 50cm x 30cm, 15kg");
            (50.0, 30.0, 15.0)
        }
    };

    let wall_thickness = if req.use_sensor_calibration.unwrap_or(false) {
        match state.clickhouse.get_sensor_readings(&drum_id, 1).await {
            Ok(readings) if !readings.is_empty() => Some(readings[0].wall_thickness.clone()),
            _ => None,
        }
    } else {
        None
    };

    let session_opt = {
        let mut sessions = state.drum_sessions.write();
        if !sessions.contains_key(&drum_id) {
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
        }
        sessions.get(&drum_id).cloned()
    };

    let (result_tx, result_rx) = oneshot::channel::<Result<AcousticAnalysisResult, String>>();

    state.acoustics_tx.send(AcousticsCommand::RunAnalysis {
        request: req.clone(),
        diameter_cm,
        height_cm,
        mass_kg,
        wall_thickness,
        session: session_opt.clone(),
        result_tx,
    }).await.map_err(|e| format!("Failed to send acoustics command: {}", e)).ok();

    let result = tokio::select! {
        res = result_rx => {
            match res {
                Ok(Ok(analysis_result)) => Ok(analysis_result),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("Analysis channel closed".to_string()),
            }
        }
        _ = tokio::time::sleep(tokio::time::Duration::from_secs(30)) => {
            Err("Acoustic analysis timeout after 30s".to_string())
        }
    };

    match result {
        Ok(analysis_result) => {
            let _ = state.clickhouse.insert_acoustic_analysis(&analysis_result).await;
            let _ = state.alarm_tx.send(AlarmCommand::FlushPending).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            info!("Acoustic analysis completed: {} modes, quality={:.2}",
                analysis_result.vibration_modes.len(), analysis_result.sound_quality_metric);
            Json(ApiResponse::ok(analysis_result))
        }
        Err(e) => Json(ApiResponse::err(&e)),
    }
}

pub async fn get_acoustic_result(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<AcousticAnalysisResult>> {
    match state.clickhouse.get_acoustic_result(&drum_id).await {
        Ok(Some(result)) => Json(ApiResponse::ok(result)),
        Ok(None) => Json(ApiResponse::err("No acoustic analysis found")),
        Err(e) => Json(ApiResponse::err(&format!("Database error: {}", e))),
    }
}

pub async fn get_vibration_modes(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<Vec<VibrationMode>>> {
    match state.clickhouse.get_acoustic_result(&drum_id).await {
        Ok(Some(result)) => Json(ApiResponse::ok(result.vibration_modes)),
        Ok(None) => Json(ApiResponse::err("No acoustic analysis found")),
        Err(e) => Json(ApiResponse::err(&format!("Database error: {}", e))),
    }
}

pub async fn get_sound_field(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<Vec<SoundFieldPoint>>> {
    match state.clickhouse.get_acoustic_result(&drum_id).await {
        Ok(Some(result)) => Json(ApiResponse::ok(result.sound_field)),
        Ok(None) => Json(ApiResponse::err("No acoustic analysis found")),
        Err(e) => Json(ApiResponse::err(&format!("Database error: {}", e))),
    }
}

pub async fn get_alarms(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<Vec<Alarm>>> {
    match state.clickhouse.get_alarms(&drum_id, 200).await {
        Ok(alarms) => Json(ApiResponse::ok(alarms)),
        Err(e) => Json(ApiResponse::err(&format!("Database error: {}", e))),
    }
}

pub async fn alarm_stream(
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        handle_alarm_socket(socket, state.alarm_broadcast.clone()).await
    })
}

async fn handle_alarm_socket(socket: WebSocket, alarm_broadcast: tokio::sync::broadcast::Sender<models::Alarm>) {
    let mut receiver = alarm_broadcast.subscribe();
    let (mut sender, mut receiver_ws) = socket.split();

    let mut send_task = tokio::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(alarm) => {
                    let msg = match serde_json::to_string(&alarm) {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    if sender.send(Message::Text(msg)).await.is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver_ws.next().await {
            debug!("WebSocket received: {:?}", msg);
        }
    });

    tokio::select! {
        _ = (&mut send_task) => recv_task.abort(),
        _ = (&mut recv_task) => send_task.abort(),
    };
    info!("Alarm WebSocket connection closed");
}

pub async fn get_wall_thickness(
    State(state): State<AppState>,
    Path(drum_id): Path<String>,
) -> Json<ApiResponse<Vec<(String, Vec<ThicknessPoint>)>>> {
    match state.clickhouse.get_wall_thickness_history(&drum_id, 500).await {
        Ok(history) => {
            let formatted: Vec<(String, Vec<ThicknessPoint>)> = history
                .into_iter()
                .map(|(ts, pts)| (ts.to_rfc3339(), pts))
                .collect();
            Json(ApiResponse::ok(formatted))
        }
        Err(e) => Json(ApiResponse::err(&format!("Database error: {}", e))),
    }
}
