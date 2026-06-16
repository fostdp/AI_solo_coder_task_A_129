use serde::{Deserialize, Serialize};
use std::env;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server_port: u16,
    pub clickhouse_url: String,
    pub clickhouse_database: String,
    pub mqtt_host: String,
    pub mqtt_port: u16,
    pub mqtt_client_id: String,
    pub mqtt_username: Option<String>,
    pub mqtt_password: Option<String>,
    pub mqtt_alarm_topic: String,
    pub mqtt_sensor_topic: String,
    pub threshold_frequency_deviation_hz: f64,
    pub threshold_shrinkage_risk: f64,
    pub threshold_thickness_deviation_pct: f64,
}

impl AppConfig {
    pub fn load() -> Self {
        let server_port = env::var("SERVER_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(8080);

        let clickhouse_url = env::var("CLICKHOUSE_URL")
            .unwrap_or_else(|_| "tcp://127.0.0.1:9000".to_string());

        let clickhouse_database = env::var("CLICKHOUSE_DATABASE")
            .unwrap_or_else(|_| "bronze_drum".to_string());

        let mqtt_host = env::var("MQTT_HOST")
            .unwrap_or_else(|_| "127.0.0.1".to_string());

        let mqtt_port = env::var("MQTT_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(1883);

        let mqtt_client_id = env::var("MQTT_CLIENT_ID")
            .unwrap_or_else(|_| "bronze-drum-backend".to_string());

        let mqtt_username = env::var("MQTT_USERNAME").ok();
        let mqtt_password = env::var("MQTT_PASSWORD").ok();

        let mqtt_alarm_topic = env::var("MQTT_ALARM_TOPIC")
            .unwrap_or_else(|_| "bronze-drum/alarms".to_string());

        let mqtt_sensor_topic = env::var("MQTT_SENSOR_TOPIC")
            .unwrap_or_else(|_| "bronze-drum/sensors".to_string());

        let threshold_frequency_deviation_hz = env::var("THRESHOLD_FREQ_DEV_HZ")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(5.0);

        let threshold_shrinkage_risk = env::var("THRESHOLD_SHRINKAGE")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(0.7);

        let threshold_thickness_deviation_pct = env::var("THRESHOLD_THICKNESS_PCT")
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .unwrap_or(15.0);

        AppConfig {
            server_port,
            clickhouse_url,
            clickhouse_database,
            mqtt_host,
            mqtt_port,
            mqtt_client_id,
            mqtt_username,
            mqtt_password,
            mqtt_alarm_topic,
            mqtt_sensor_topic,
            threshold_frequency_deviation_hz,
            threshold_shrinkage_risk,
            threshold_thickness_deviation_pct,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::load()
    }
}
