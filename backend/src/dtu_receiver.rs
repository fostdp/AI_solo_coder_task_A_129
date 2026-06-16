use crate::config::AppConfig;
use crate::models::{SensorReading, ThicknessPoint, AlloyComposition};
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;

#[derive(Debug, Clone)]
pub enum DtuEvent {
    SensorReading {
        reading: SensorReading,
        validated: bool,
    },
    ValidationError {
        drum_id: String,
        field: String,
        message: String,
    },
}

#[derive(Debug, Clone)]
pub struct DtuReceiver {
    config: Arc<AppConfig>,
    sensor_tx: mpsc::Sender<DtuEvent>,
}

impl DtuReceiver {
    pub fn new(config: Arc<AppConfig>, sensor_tx: mpsc::Sender<DtuEvent>) -> Self {
        Self {
            config,
            sensor_tx,
        }
    }

    pub fn get_sender(&self) -> mpsc::Sender<DtuEvent> {
        self.sensor_tx.clone()
    }

    pub async fn receive_and_validate(
        &self,
        mut reading: SensorReading,
    ) -> Result<SensorReading, Vec<String>> {
        if reading.reading_id.is_empty() {
            reading.reading_id = Uuid::new_v4().to_string();
        }
        if reading.timestamp.timestamp() <= 0 {
            reading.timestamp = Utc::now();
        }

        let mut errors = Vec::new();

        if reading.drum_id.is_empty() {
            errors.push("drum_id cannot be empty".to_string());
        }

        self.validate_alloy(&reading.alloy, &mut errors);
        self.validate_wall_thickness(&reading.wall_thickness, &mut errors);
        self.validate_environment(reading.temperature_c, reading.ambient_humidity_pct, &mut errors);
        self.validate_spectrum(&reading.tap_spectrum, &mut errors);

        if errors.is_empty() {
            debug!("Sensor reading validated: {} for drum {}", reading.reading_id, reading.drum_id);
            let event = DtuEvent::SensorReading {
                reading: reading.clone(),
                validated: true,
            };
            if let Err(e) = self.sensor_tx.send(event).await {
                warn!("Failed to send validated reading to channel: {}", e);
            }
            Ok(reading)
        } else {
            let error_msg = errors.join(", ");
            error!("Sensor reading validation failed for drum {}: {}", reading.drum_id, error_msg);
            let event = DtuEvent::ValidationError {
                drum_id: reading.drum_id.clone(),
                field: "multiple".to_string(),
                message: error_msg.clone(),
            };
            let _ = self.sensor_tx.send(event).await;
            Err(errors)
        }
    }

    fn validate_alloy(&self, alloy: &AlloyComposition, errors: &mut Vec<String>) {
        let total = alloy.copper_pct + alloy.tin_pct + alloy.lead_pct + alloy.zinc_pct + alloy.other_impurities_pct;
        let diff = (total - 100.0).abs();

        if diff > 10.0 {
            errors.push(format!(
                "Alloy composition total {}% deviates from 100% by {:.2}% (max allowed 10%)",
                total, diff
            ));
        }

        if alloy.copper_pct < 50.0 || alloy.copper_pct > 95.0 {
            errors.push(format!(
                "Copper percentage {:.2}% is outside valid range [50%, 95%]",
                alloy.copper_pct
            ));
        }

        if alloy.tin_pct < 0.0 || alloy.tin_pct > 30.0 {
            errors.push(format!(
                "Tin percentage {:.2}% is outside valid range [0%, 30%]",
                alloy.tin_pct
            ));
        }

        if alloy.lead_pct < 0.0 || alloy.lead_pct > 25.0 {
            errors.push(format!(
                "Lead percentage {:.2}% is outside valid range [0%, 25%]",
                alloy.lead_pct
            ));
        }
    }

    fn validate_wall_thickness(&self, thickness: &[ThicknessPoint], errors: &mut Vec<String>) {
        if thickness.is_empty() {
            errors.push("Wall thickness data is empty".to_string());
            return;
        }

        let expected_zones = ["center", "mid_radius", "edge", "boss", "ray_band", "halo"];
        for (i, tp) in thickness.iter().enumerate() {
            if tp.x_frac < 0.0 || tp.x_frac > 1.0 {
                errors.push(format!(
                    "Wall thickness point {} has invalid x_frac {:.3} (must be [0, 1])",
                    i, tp.x_frac
                ));
            }
            if tp.y_frac < 0.0 || tp.y_frac > 1.0 {
                errors.push(format!(
                    "Wall thickness point {} has invalid y_frac {:.3} (must be [0, 1])",
                    i, tp.y_frac
                ));
            }
            if tp.thickness_mm < 1.0 || tp.thickness_mm > 20.0 {
                errors.push(format!(
                    "Wall thickness point {} has invalid thickness {:.2}mm (must be [1, 20])",
                    i, tp.thickness_mm
                ));
            }
            if !expected_zones.contains(&tp.zone.as_str()) && !tp.zone.starts_with("zone_") {
                warn!("Unknown thickness zone '{}' at point {}", tp.zone, i);
            }
        }

        let values: Vec<f64> = thickness.iter().map(|t| t.thickness_mm).collect();
        let avg = values.iter().sum::<f64>() / values.len() as f64;
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        if max / min > 3.0 {
            errors.push(format!(
                "Wall thickness variation too large: max {:.2}mm / min {:.2}mm = {:.2}x (max allowed 3x)",
                max, min, max / min
            ));
        }

        debug!("Wall thickness stats: avg={:.2}mm, min={:.2}mm, max={:.2}mm, count={}",
            avg, min, max, thickness.len());
    }

    fn validate_environment(&self, temp_c: f64, humidity_pct: f64, errors: &mut Vec<String>) {
        if temp_c < -40.0 || temp_c > 85.0 {
            errors.push(format!(
                "Temperature {:.1}°C is outside valid range [-40, 85]",
                temp_c
            ));
        }
        if humidity_pct < 0.0 || humidity_pct > 100.0 {
            errors.push(format!(
                "Humidity {:.1}% is outside valid range [0, 100]",
                humidity_pct
            ));
        }
    }

    fn validate_spectrum(&self, spectrum: &[crate::models::SpectrumBin], errors: &mut Vec<String>) {
        if spectrum.is_empty() {
            warn!("Tap spectrum is empty, skipping validation");
            return;
        }

        let expected_size = 256;
        if spectrum.len() != expected_size {
            warn!("Tap spectrum size {} differs from expected {}", spectrum.len(), expected_size);
        }

        for (i, bin) in spectrum.iter().enumerate() {
            if bin.amplitude_db < -120.0 || bin.amplitude_db > 10.0 {
                errors.push(format!(
                    "Spectrum bin {} has invalid amplitude {:.2}dB (must be [-120, 10])",
                    i, bin.amplitude_db
                ));
            }
            if bin.frequency_hz < 0.0 || bin.frequency_hz > 20000.0 {
                errors.push(format!(
                    "Spectrum bin {} has invalid frequency {:.1}Hz (must be [0, 20000])",
                    i, bin.frequency_hz
                ));
            }
        }

        let max_amp = spectrum.iter()
            .map(|b| b.amplitude_db)
            .fold(f64::NEG_INFINITY, f64::max);
        debug!("Tap spectrum peak: {:.2}dB, bins: {}", max_amp, spectrum.len());
    }

    pub async fn start_mqtt_listener(
        &self,
        mqtt_client: Arc<crate::mqtt_client::MqttClient>,
    ) {
        info!("DTU MQTT listener started, subscribing to sensor data");
        let mut eventloop = mqtt_client.subscribe_sensor_events();
        let tx = self.sensor_tx.clone();

        tokio::spawn(async move {
            loop {
                match eventloop.recv().await {
                    Ok(payload) => {
                        match serde_json::from_slice::<SensorReading>(&payload) {
                            Ok(reading) => {
                                info!("Received sensor reading via MQTT for drum: {}", reading.drum_id);
                                let _ = tx.send(DtuEvent::SensorReading {
                                    reading,
                                    validated: false,
                                }).await;
                            }
                            Err(e) => {
                                error!("Failed to deserialize MQTT sensor message: {}", e);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        warn!("MQTT sensor event lagged, lost {} messages", n);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        info!("MQTT sensor event channel closed");
                        break;
                    }
                }
            }
        });
    }
}
