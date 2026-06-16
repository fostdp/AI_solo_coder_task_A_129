use crate::models::*;
use crate::simulation::CastingSimulator;
use crate::acoustics::AcousticAnalyzer;
use crate::AppState;
use axum::{
    extract::{Path, State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::{IntoResponse, Response},
    Json,
    http::StatusCode,
};
use futures::{sink::SinkExt, stream::StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub async fn health_check() -> Json<ApiResponse<serde_json::Value>> {
    Json(ApiResponse::ok(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": "1.0.0",
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

    let mut alarms_vec = Vec::new();

    {
        let mut sessions = state.drum_sessions.write();
        if !sessions.contains_key(&drum_id) {
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
        }

        if let Some(session) = sessions.get_mut(&drum_id) {
            alarms_vec = state.alarm_engine.check_sensor_reading(&reading, session).await;
        }
    }

    match state.clickhouse.insert_sensor_reading(&reading).await {
        Ok(_) => {
            let pending = state.alarm_engine.drain_pending();
            for alarm in &pending {
                if let Err(e) = state.clickhouse.insert_alarm(alarm).await {
                    error!("Failed to insert alarm: {:?}", e);
                }
            }
            let all_alarms: Vec<Alarm> = alarms_vec.into_iter().chain(pending).collect();
            Json(ApiResponse::ok(all_alarms))
        }
        Err(e) => {
            error!("Error inserting sensor reading: {:?}", e);
            Json(ApiResponse::err(&format!("Failed to store reading: {}", e)))
        }
    }
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

    let drum = match state.clickhouse.get_drum(&drum_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return Json(ApiResponse::err("Drum not found")),
        Err(e) => return Json(ApiResponse::err(&format!("Database error: {}", e))),
    };

    let result = CastingSimulator::simulate(
        drum_id.clone(),
        drum.diameter_cm,
        drum.height_cm,
        &req,
    );

    {
        let mut sessions = state.drum_sessions.write();
        if !sessions.contains_key(&drum_id) {
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
        }
        if let Some(session) = sessions.get_mut(&drum_id) {
            state.alarm_engine.analyze_casting_result(&result, session).await;
        }
    }

    match state.clickhouse.insert_casting_simulation(&result).await {
        Ok(_) => {
            let pending = state.alarm_engine.drain_pending();
            for alarm in &pending {
                if let Err(e) = state.clickhouse.insert_alarm(alarm).await {
                    error!("Failed to insert alarm: {:?}", e);
                }
            }
            info!("Casting simulation completed: {} defects found, quality={:.2}",
                result.defects.len(), result.quality_score);
            Json(ApiResponse::ok(result))
        }
        Err(e) => {
            error!("Error storing casting simulation: {:?}", e);
            Json(ApiResponse::err(&format!("Failed to store sim: {}", e)))
        }
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

    let drum = match state.clickhouse.get_drum(&drum_id).await {
        Ok(Some(d)) => d,
        Ok(None) => return Json(ApiResponse::err("Drum not found")),
        Err(e) => return Json(ApiResponse::err(&format!("Database error: {}", e))),
    };

    let wall_thickness = if req.use_sensor_calibration.unwrap_or(false) {
        match state.clickhouse.get_sensor_readings(&drum_id, 1).await {
            Ok(readings) if !readings.is_empty() => Some(readings[0].wall_thickness.clone()),
            _ => None,
        }
    } else {
        None
    };

    let wt_ref = wall_thickness.as_ref();
    let result = AcousticAnalyzer::analyze(
        drum_id.clone(),
        drum.diameter_cm,
        drum.height_cm,
        drum.mass_kg,
        &req,
        wt_ref,
    );

    {
        let mut sessions = state.drum_sessions.write();
        if !sessions.contains_key(&drum_id) {
            sessions.insert(drum_id.clone(), DrumSession::new(drum_id.clone()));
        }
        if let Some(session) = sessions.get_mut(&drum_id) {
            state.alarm_engine.analyze_acoustic_result(&result, session).await;
        }
    }

    match state.clickhouse.insert_acoustic_analysis(&result).await {
        Ok(_) => {
            let pending = state.alarm_engine.drain_pending();
            for alarm in &pending {
                if let Err(e) = state.clickhouse.insert_alarm(alarm).await {
                    error!("Failed to insert alarm: {:?}", e);
                }
            }
            info!("Acoustic analysis completed: {} modes, quality={:.2}",
                result.vibration_modes.len(), result.sound_quality_metric);
            Json(ApiResponse::ok(result))
        }
        Err(e) => {
            error!("Error storing acoustic analysis: {:?}", e);
            Json(ApiResponse::err(&format!("Failed to store analysis: {}", e)))
        }
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
        handle_alarm_socket(socket, state.mqtt.clone()).await
    })
}

async fn handle_alarm_socket(mut socket: WebSocket, mqtt: Arc<crate::mqtt_client::MqttClient>) {
    let mut receiver = mqtt.subscribe_alarms();
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
