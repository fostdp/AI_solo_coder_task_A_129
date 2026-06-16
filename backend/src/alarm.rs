use crate::config::AppConfig;
use crate::models::*;
use crate::mqtt_client::MqttClient;
use crate::acoustics::AcousticAnalyzer;
use anyhow::Result;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{self, Duration};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;
use super::clickhouse_client::ClickHouseClient;

pub struct AlarmEngine {
    mqtt: Arc<MqttClient>,
    config: Arc<AppConfig>,
    pending_alarms: parking_lot::Mutex<Vec<Alarm>>,
}

impl AlarmEngine {
    pub fn new(mqtt: Arc<MqttClient>, config: Arc<AppConfig>) -> Self {
        Self {
            mqtt,
            config,
            pending_alarms: parking_lot::Mutex::new(Vec::new()),
        }
    }

    pub async fn start_background_monitor(
        &self,
        clickhouse: Arc<ClickHouseClient>,
        sessions: Arc<RwLock<HashMap<String, DrumSession>>>,
    ) {
        info!("Starting background alarm monitor");
        let mut interval = time::interval(Duration::from_secs(300));

        loop {
            interval.tick().await;
            debug!("Running periodic alarm checks");

            let drum_ids: Vec<String> = {
                let s = sessions.read();
                s.keys().cloned().collect()
            };

            for drum_id in &drum_ids {
                if let Err(e) = self.check_drum_alarms(drum_id, clickhouse.clone(), sessions.clone()).await {
                    error!("Error checking alarms for drum {}: {:?}", drum_id, e);
                }
            }

            self.flush_pending_alarms(clickhouse.clone()).await;
        }
    }

    pub async fn check_sensor_reading(
        &self,
        reading: &SensorReading,
        session: &mut DrumSession,
    ) -> Vec<Alarm> {
        let mut alarms = Vec::new();
        session.last_reading_time = Some(reading.timestamp);

        let freq_deviations = AcousticAnalyzer::check_frequency_deviation(
            &reading.tap_spectrum,
            &session.reference_frequencies_hz,
            self.config.threshold_frequency_deviation_hz,
        );

        for (ref_f, meas_f, dev) in &freq_deviations {
            let severity = if dev.abs() > self.config.threshold_frequency_deviation_hz * 3.0 {
                AlarmSeverity::Critical
            } else if dev.abs() > self.config.threshold_frequency_deviation_hz * 2.0 {
                AlarmSeverity::Warning
            } else {
                AlarmSeverity::Info
            };

            let metadata = serde_json::json!({
                "reference_frequency_hz": ref_f,
                "measured_frequency_hz": meas_f,
                "deviation_hz": dev,
                "direction": if *dev > 0.0 { "sharp" } else { "flat" },
            });

            alarms.push(Alarm::new(
                reading.drum_id.clone(),
                AlarmType::FrequencyDeviation,
                severity,
                format!("音准偏差: 参考频率{:.1}Hz, 实测{:.1}Hz, 偏差{:+.2}Hz", ref_f, meas_f, dev),
                dev.abs(),
                self.config.threshold_frequency_deviation_hz,
                metadata,
            ));
        }

        if let Some(thick_alarm) = self.check_wall_thickness_anomaly(
            &reading.wall_thickness,
            &reading.drum_id,
        ) {
            alarms.push(thick_alarm);
        }

        if let Some(alloy_alarm) = self.check_alloy_composition(&reading.alloy, &reading.drum_id) {
            alarms.push(alloy_alarm);
        }

        if !alarms.is_empty() {
            session.active_alarms += alarms.len();
            let mut pending = self.pending_alarms.lock();
            pending.extend(alarms.clone());
            drop(pending);

            for alarm in &alarms {
                if let Err(e) = self.mqtt.publish_alarm(alarm).await {
                    error!("Failed to publish alarm via MQTT: {:?}", e);
                }
            }
        }

        alarms
    }

    fn check_wall_thickness_anomaly(
        &self,
        thickness: &Vec<ThicknessPoint>,
        drum_id: &str,
    ) -> Option<Alarm> {
        if thickness.is_empty() {
            return None;
        }

        let thicknesses: Vec<f64> = thickness.iter().map(|t| t.thickness_mm).collect();
        let avg = thicknesses.iter().sum::<f64>() / thicknesses.len() as f64;
        let variance = thicknesses.iter()
            .map(|t| (t - avg).powi(2))
            .sum::<f64>() / thicknesses.len() as f64;
        let std_dev = variance.sqrt();
        let cv = std_dev / avg * 100.0;

        if cv > self.config.threshold_thickness_deviation_pct {
            let (min_idx, max_idx) = thicknesses.iter().enumerate().fold((0usize, 0usize), |(mi, ma), (i, &v)| {
                let new_mi = if v < thicknesses[mi] { i } else { mi };
                let new_ma = if v > thicknesses[ma] { i } else { ma };
                (new_mi, new_ma)
            });

            let severity = if cv > self.config.threshold_thickness_deviation_pct * 2.0 {
                AlarmSeverity::Critical
            } else if cv > self.config.threshold_thickness_deviation_pct * 1.5 {
                AlarmSeverity::Warning
            } else {
                AlarmSeverity::Info
            };

            let metadata = serde_json::json!({
                "average_mm": avg,
                "min_mm": thicknesses[min_idx],
                "max_mm": thicknesses[max_idx],
                "std_dev_mm": std_dev,
                "coefficient_of_variation_pct": cv,
                "thin_zone": thickness.get(min_idx).map(|t| t.zone.clone()),
                "thick_zone": thickness.get(max_idx).map(|t| t.zone.clone()),
            });

            return Some(Alarm::new(
                drum_id.to_string(),
                AlarmType::ThicknessAnomaly,
                severity,
                format!("壁厚均匀性异常: 变异系数CV={:.2}%, 阈值={:.2}%", cv, self.config.threshold_thickness_deviation_pct),
                cv,
                self.config.threshold_thickness_deviation_pct,
                metadata,
            ));
        }
        None
    }

    fn check_alloy_composition(&self, alloy: &AlloyComposition, drum_id: &str) -> Option<Alarm> {
        let total = alloy.total();
        let diff = (total - 100.0).abs();

        if diff > 2.0 {
            let severity = if diff > 5.0 {
                AlarmSeverity::Critical
            } else {
                AlarmSeverity::Warning
            };

            let metadata = serde_json::json!({
                "copper_pct": alloy.copper_pct,
                "tin_pct": alloy.tin_pct,
                "lead_pct": alloy.lead_pct,
                "zinc_pct": alloy.zinc_pct,
                "other_pct": alloy.other_impurities_pct,
                "total_pct": total,
            });

            return Some(Alarm::new(
                drum_id.to_string(),
                AlarmType::AlloyAnomaly,
                severity,
                format!("合金成分异常: 总计{:.2}%, 偏差{:.2}个百分点", total, diff),
                diff,
                2.0,
                metadata,
            ));
        }
        None
    }

    pub async fn analyze_casting_result(
        &self,
        result: &CastingSimulationResult,
        session: &mut DrumSession,
    ) -> Vec<Alarm> {
        let mut alarms = Vec::new();
        session.last_casting_sim = Some(result.sim_id.clone());

        for defect in &result.defects {
            if defect.severity >= self.config.threshold_shrinkage_risk {
                let severity = match defect.defect_type {
                    DefectType::ShrinkageCavity => {
                        if defect.severity > 0.85 {
                            AlarmSeverity::Fatal
                        } else if defect.severity > 0.7 {
                            AlarmSeverity::Critical
                        } else {
                            AlarmSeverity::Warning
                        }
                    }
                    DefectType::HotTear => {
                        if defect.severity > 0.8 {
                            AlarmSeverity::Critical
                        } else {
                            AlarmSeverity::Warning
                        }
                    }
                    DefectType::ColdShut | DefectType::IncompleteFilling => {
                        if defect.severity > 0.7 {
                            AlarmSeverity::Critical
                        } else {
                            AlarmSeverity::Warning
                        }
                    }
                    DefectType::Porosity => {
                        if defect.severity > 0.8 {
                            AlarmSeverity::Warning
                        } else {
                            AlarmSeverity::Info
                        }
                    }
                };

                let metadata = serde_json::json!({
                    "defect_type": format!("{:?}", defect.defect_type),
                    "defect_id": defect.defect_id,
                    "zone": defect.zone,
                    "x_frac": defect.x_frac,
                    "y_frac": defect.y_frac,
                    "severity": defect.severity,
                    "description": defect.description,
                    "overall_risk": result.overall_risk,
                    "quality_score": result.quality_score,
                });

                let alarm_type = match defect.defect_type {
                    DefectType::ShrinkageCavity | DefectType::Porosity => AlarmType::ShrinkageDefect,
                    DefectType::HotTear => AlarmType::StructuralFailureRisk,
                    _ => AlarmType::ShrinkageDefect,
                };

                alarms.push(Alarm::new(
                    result.drum_id.clone(),
                    alarm_type,
                    severity,
                    format!("铸造缺陷[{}] 区域: {}, 严重度: {:.2}% - {}",
                        format!("{:?}", defect.defect_type),
                        defect.zone,
                        defect.severity * 100.0,
                        defect.description
                    ),
                    defect.severity,
                    self.config.threshold_shrinkage_risk,
                    metadata,
                ));
            }
        }

        if result.overall_risk == "CRITICAL" {
            let metadata = serde_json::json!({
                "overall_risk": result.overall_risk,
                "quality_score": result.quality_score,
                "defect_count": result.defects.len(),
                "suggestion": "强烈建议修改工艺参数: 提高浇注温度、增加冒口、降低冷却速率或优化浇注系统",
            });
            alarms.push(Alarm::new(
                result.drum_id.clone(),
                AlarmType::StructuralFailureRisk,
                AlarmSeverity::Fatal,
                format!("整体铸造风险等级CRITICAL, 综合品质评分仅{:.1}分", result.quality_score * 100.0),
                1.0 - result.quality_score,
                0.6,
                metadata,
            ));
        } else if result.overall_risk == "HIGH" {
            let metadata = serde_json::json!({
                "overall_risk": result.overall_risk,
                "quality_score": result.quality_score,
                "defect_count": result.defects.len(),
                "suggestion": "建议优化工艺参数，检查高风险区域是否需要补贴或冷铁",
            });
            alarms.push(Alarm::new(
                result.drum_id.clone(),
                AlarmType::StructuralFailureRisk,
                AlarmSeverity::Critical,
                format!("整体铸造风险等级HIGH, 综合品质评分{:.1}分", result.quality_score * 100.0),
                1.0 - result.quality_score,
                0.6,
                metadata,
            ));
        }

        if !alarms.is_empty() {
            session.active_alarms += alarms.len();
            let mut pending = self.pending_alarms.lock();
            pending.extend(alarms.clone());
            drop(pending);

            for alarm in &alarms {
                if let Err(e) = self.mqtt.publish_alarm(alarm).await {
                    error!("Failed to publish casting alarm: {:?}", e);
                }
            }
        }

        alarms
    }

    pub async fn analyze_acoustic_result(
        &self,
        result: &AcousticAnalysisResult,
        session: &mut DrumSession,
    ) -> Vec<Alarm> {
        let mut alarms = Vec::new();
        session.last_acoustic_analysis = Some(result.analysis_id.clone());

        if result.sound_quality_metric < 0.5 {
            let severity = if result.sound_quality_metric < 0.3 {
                AlarmSeverity::Critical
            } else {
                AlarmSeverity::Warning
            };

            let metadata = serde_json::json!({
                "sound_quality_score": result.sound_quality_metric,
                "resonance_frequencies": result.resonance_frequencies_hz,
                "radiated_power_w": result.radiated_sound_power_w,
                "mode_count": result.vibration_modes.len(),
                "damping_ratios": result.vibration_modes.iter().map(|m| m.damping_ratio).collect::<Vec<_>>(),
                "reference_target": "理想铜鼓音准应接近C5、E5、G5、C6、E6",
            });

            alarms.push(Alarm::new(
                result.drum_id.clone(),
                AlarmType::SoundQualityDegradation,
                severity,
                format!("声学品质评分偏低: {:.1}/100, 与参考音准匹配度不足", result.sound_quality_metric * 100.0),
                1.0 - result.sound_quality_metric,
                0.5,
                metadata,
            ));
        }

        if !alarms.is_empty() {
            session.active_alarms += alarms.len();
            let mut pending = self.pending_alarms.lock();
            pending.extend(alarms.clone());
            drop(pending);

            for alarm in &alarms {
                if let Err(e) = self.mqtt.publish_alarm(alarm).await {
                    error!("Failed to publish acoustic alarm: {:?}", e);
                }
            }
        }

        alarms
    }

    async fn check_drum_alarms(
        &self,
        drum_id: &str,
        clickhouse: Arc<ClickHouseClient>,
        sessions: Arc<RwLock<HashMap<String, DrumSession>>>,
    ) -> Result<()> {
        debug!("Periodic alarm check for drum {}", drum_id);
        Ok(())
    }

    async fn flush_pending_alarms(&self, clickhouse: Arc<ClickHouseClient>) {
        let alarms: Vec<Alarm> = {
            let mut pending = self.pending_alarms.lock();
            let drained = pending.drain(..).collect();
            drained
        };

        if !alarms.is_empty() {
            info!("Flushing {} pending alarms to ClickHouse", alarms.len());
            for alarm in &alarms {
                if let Err(e) = clickhouse.insert_alarm(alarm).await {
                    error!("Failed to insert alarm {}: {:?}", alarm.alarm_id, e);
                }
            }
        }
    }

    pub fn drain_pending(&self) -> Vec<Alarm> {
        let mut pending = self.pending_alarms.lock();
        pending.drain(..).collect()
    }
}
