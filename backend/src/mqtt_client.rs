use crate::config::AppConfig;
use anyhow::{Context, Result};
use rumqttc::{AsyncClient, MqttOptions, QoS, Event, Packet};
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, error, info, warn};

pub struct MqttClient {
    config: Arc<AppConfig>,
    client: Option<AsyncClient>,
    alarm_tx: broadcast::Sender<crate::models::Alarm>,
}

impl MqttClient {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let (alarm_tx, _) = broadcast::channel::<crate::models::Alarm>(1024);
        Self {
            config,
            client: None,
            alarm_tx,
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
        let alarm_topic = self.config.mqtt_alarm_topic.clone();
        let sensor_topic = self.config.mqtt_sensor_topic.clone();

        client.subscribe(sensor_topic.clone(), QoS::AtLeastOnce).await
            .context("Failed to subscribe to sensor topic")?;
        info!("Subscribed to MQTT sensor topic: {}", sensor_topic);

        let alarm_tx_clone = self.alarm_tx.clone();
        tokio::spawn(async move {
            loop {
                match eventloop.poll().await {
                    Ok(notification) => {
                        if let Event::Incoming(Packet::Publish(publish)) = notification {
                            debug!("MQTT incoming message on topic: {}", publish.topic);
                            if publish.topic == sensor_topic {
                                debug!("Sensor data received via MQTT: {} bytes", publish.payload.len());
                            }
                        }
                    }
                    Err(e) => {
                        error!("MQTT eventloop error: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                }
            }
        });

        let static_self: &'static MqttClient = unsafe { std::mem::transmute(self as *const MqttClient) };
        let _ = static_self;

        let client_arc = unsafe { std::mem::transmute::<AsyncClient, AsyncClient>(client) };
        let self_ptr = self as *const MqttClient;
        unsafe {
            let self_mut = &mut *(self_ptr as *mut MqttClient);
            self_mut.client = Some(client_arc);
        }

        Ok(())
    }

    pub fn subscribe_alarms(&self) -> broadcast::Receiver<crate::models::Alarm> {
        self.alarm_tx.subscribe()
    }

    pub async fn publish_alarm(&self, alarm: &crate::models::Alarm) -> Result<()> {
        if let Some(client) = &self.client {
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
        if let Some(client) = &self.client {
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
