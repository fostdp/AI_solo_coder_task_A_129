use crate::config::AppConfig;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Packet};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tracing::{debug, error, info, warn};

pub struct MqttClient {
    config: Arc<AppConfig>,
    client: parking_lot::Mutex<Option<AsyncClient>>,
    alarm_tx: broadcast::Sender<crate::models::Alarm>,
    sensor_tx: broadcast::Sender<Vec<u8>>,
}

impl MqttClient {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let (alarm_tx, _) = broadcast::channel::<crate::models::Alarm>(1024);
        let (sensor_tx, _) = broadcast::channel::<Vec<u8>>(1024);
        Self {
            config,
            client: parking_lot::Mutex::new(None),
            alarm_tx,
            sensor_tx,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting MQTT client connection to {}:{}", self.config.mqtt_host, self.config.mqtt_port);
        
        let mut mqttoptions = MqttOptions::new(
            self.config.mqtt_client_id.clone(),
            self.config.mqtt_host.clone(),
            self.config.mqtt_port,
        );
        mqttoptions.set_keep_alive(std::time::Duration::from_secs(60));
        
        if let (Some(username), Some(password)) = (self.config.mqtt_username.clone(), self.config.mqtt_password.clone()) {
            mqttoptions.set_credentials(username, password);
        }

        let (client, mut eventloop) = AsyncClient::new(mqttoptions, 100);
        let sensor_topic = self.config.mqtt_sensor_topic.clone();

        match client.subscribe(sensor_topic.clone(), QoS::AtLeastOnce).await {
            Ok(_) => info!("Subscribed to MQTT sensor topic: {}", sensor_topic),
            Err(e) => warn!("Failed to subscribe to MQTT sensor topic (will retry): {:?}", e),
        }

        let sensor_tx_clone = self.sensor_tx.clone();
        let config_clone = self.config.clone();
        tokio::spawn(async move {
            let mut reconnect_attempts = 0;
            loop {
                match eventloop.poll().await {
                    Ok(notification) => {
                        if let Event::Incoming(Packet::Publish(publish)) = notification {
                            debug!("MQTT incoming message on topic: {}", publish.topic);
                            if publish.topic == sensor_topic {
                                debug!("Sensor data received via MQTT: {} bytes", publish.payload.len());
                                let _ = sensor_tx_clone.send(publish.payload.to_vec());
                            }
                        }
                    }
                    Err(e) => {
                        error!("MQTT eventloop error (attempt {}): {:?}", reconnect_attempts, e);
                        reconnect_attempts += 1;
                        tokio::time::sleep(std::time::Duration::from_secs(
                            std::cmp::min(10 * reconnect_attempts, 60)
                        )).await;
                        
                        let mut mqttoptions = MqttOptions::new(
                            config_clone.mqtt_client_id.clone(),
                            config_clone.mqtt_host.clone(),
                            config_clone.mqtt_port,
                        );
                        mqttoptions.set_keep_alive(std::time::Duration::from_secs(60));
                        if let (Some(username), Some(password)) = 
                            (config_clone.mqtt_username.clone(), config_clone.mqtt_password.clone()) {
                            mqttoptions.set_credentials(username, password);
                        }
                        let (new_client, new_eventloop) = AsyncClient::new(mqttoptions, 100);
                        let _ = new_client.subscribe(sensor_topic.clone(), QoS::AtLeastOnce).await;
                        eventloop = new_eventloop;
                    }
                }
            }
        });

        *self.client.lock() = Some(client);

        Ok(())
    }

    pub fn subscribe_alarms(&self) -> broadcast::Receiver<crate::models::Alarm> {
        self.alarm_tx.subscribe()
    }

    pub fn subscribe_sensor_events(&self) -> broadcast::Receiver<Vec<u8>> {
        self.sensor_tx.subscribe()
    }

    pub async fn publish_alarm(&self, alarm: &crate::models::Alarm) -> Result<()> {
        let client_opt = self.client.lock().clone();
        if let Some(client) = client_opt {
            let payload = serde_json::to_string(alarm)?;
            let topic = format!("{}/{}", self.config.mqtt_alarm_topic, alarm.drum_id);
            client
                .publish(topic.clone(), QoS::AtLeastOnce, false, payload.as_bytes())
                .await
                .with_context(|| format!("Failed to publish alarm to {}", topic))?;
            debug!("Published alarm to MQTT topic: {}", topic);
        }
        let _ = self.alarm_tx.send(alarm.clone());
        Ok(())
    }

    pub async fn publish_sensor_reading(&self, reading: &crate::models::SensorReading) -> Result<()> {
        let client_opt = self.client.lock().clone();
        if let Some(client) = client_opt {
            let payload = serde_json::to_string(reading)?;
            let topic = format!("{}/{}", self.config.mqtt_sensor_topic, reading.drum_id);
            client
                .publish(topic.clone(), QoS::AtLeastOnce, false, payload.as_bytes())
                .await
                .with_context(|| format!("Failed to publish sensor reading to {}", topic))?;
            debug!("Published sensor reading to MQTT topic: {}", topic);
        }
        Ok(())
    }
}
