use crate::config::AppConfig;
use crate::models::{Alarm, AlarmType, AlarmSeverity, DrumSession};
use crate::mqtt_client::MqttClient;
use crate::casting_simulator::CastingEvent;
use crate::acoustic_analyzer::AcousticsEvent;
use crate::dtu_receiver::DtuEvent;
use std::sync::Arc;
use tokio::sync::{mpsc, broadcast};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;

#[derive(Debug, Clone)]
pub enum AlarmCommand {
    ProcessCastingEvent {
        event: CastingEvent,
    },
    ProcessAcousticsEvent {
        event: AcousticsEvent,
    },
    ProcessDtuEvent {
        event: DtuEvent,
        session: DrumSession,
    },
    EvaluateFrequencyDeviation {
        drum_id: String,
        deviations: Vec<(f64, f64, f64)>,
    },
    FlushPending,
    Shutdown,
}

pub struct AlarmMqttService {
    config: Arc<AppConfig>,
    mqtt: Arc<MqttClient>,
    pub alarm_tx: broadcast::Sender<Alarm>,
    command_rx: mpsc::Receiver<AlarmCommand>,
    pending_alarms: Arc<parking_lot::Mutex<Vec<Alarm>>>,
}

impl AlarmMqttService {
    pub fn new(
        config: Arc<AppConfig>,
        mqtt: Arc<MqttClient>,
        command_rx: mpsc::Receiver<AlarmCommand>,
    ) -> Self {
        let (alarm_tx, _) = broadcast::channel::<Alarm>(1024);
        Self {
            config,
            mqtt,
            alarm_tx,
            command_rx,
            pending_alarms: Arc::new(parking_lot::Mutex::new(Vec::new())),
        }
    }

    pub fn clone_for_drain(&self) -> Self {
        Self {
            config: self.config.clone(),
            mqtt: self.mqtt.clone(),
            alarm_tx: self.alarm_tx.clone(),
            command_rx: mpsc::channel(1).1,
            pending_alarms: self.pending_alarms.clone(),
        }
    }

    pub fn subscribe_alarms(&self) -> broadcast::Receiver<Alarm> {
        self.alarm_tx.subscribe()
    }

    pub async fn run(mut self) {
        info!("Alarm MQTT service started");
        while let Some(cmd) = self.command_rx.recv().await {
            match cmd {
                AlarmCommand::ProcessCastingEvent { event } => {
                    self.handle_casting_event(event).await;
                }
                AlarmCommand::ProcessAcousticsEvent { event } => {
                    self.handle_acoustics_event(event).await;
                }
                AlarmCommand::ProcessDtuEvent { event, session } => {
                    self.handle_dtu_event(event, session).await;
                }
                AlarmCommand::EvaluateFrequencyDeviation { drum_id, deviations } => {
                    self.handle_frequency_deviation(&drum_id, &deviations);
                }
                AlarmCommand::FlushPending => {
                    self.flush_pending();
                }
                AlarmCommand::Shutdown => {
                    info!("Alarm MQTT service shutting down");
                    self.flush_pending();
                    break;
                }
            }
        }
    }

    async fn handle_casting_event(&self, event: CastingEvent) {
        match event {
            CastingEvent::SimulationComplete { result, mut session } => {
                info!("Processing casting simulation result for drum: {}", result.drum_id);
                let mut alarms = Vec::new();

                if let Some(s) = session.as_mut() {
                    s.last_casting_sim = Some(result.sim_id.clone());
                }

                for defect in &result.defects {
                    if defect.severity >= self.config.threshold_shrinkage_risk {
                        let severity = match defect.defect_type {
                            crate::models::DefectType::ShrinkageCavity => {
                                if defect.severity > 0.85 {
                                    AlarmSeverity::Fatal
                                } else if defect.severity > 0.7 {
                                    AlarmSeverity::Critical
                                } else {
                                    AlarmSeverity::Warning
                                }
                            }
                            crate::models::DefectType::HotTear => {
                                if defect.severity > 0.8 {
                                    AlarmSeverity::Critical
                                } else {
                                    AlarmSeverity::Warning
                                }
                            }
                            crate::models::DefectType::ColdShut
                            | crate::models::DefectType::IncompleteFilling => {
                                if defect.severity > 0.7 {
                                    AlarmSeverity::Critical
                                } else {
                                    AlarmSeverity::Warning
                                }
                            }
                            crate::models::DefectType::Porosity => {
                                if defect.severity > 0.8 {
                                    AlarmSeverity::Warning
                                } else {
                                    AlarmSeverity::Info
                                }
                            }
                        };

                        let alarm_type = match defect.defect_type {
                            crate::models::DefectType::ShrinkageCavity
                            | crate::models::DefectType::Porosity => AlarmType::ShrinkageDefect,
                            crate::models::DefectType::HotTear => AlarmType::StructuralFailureRisk,
                            _ => AlarmType::ShrinkageDefect,
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

                self.dispatch_alarms(alarms, session).await;
            }
            CastingEvent::SimulationError { drum_id, message } => {
                error!("Casting simulation error for drum {}: {}", drum_id, message);
                let alarm = Alarm::new(
                    drum_id.clone(),
                    AlarmType::Info,
                    AlarmSeverity::Warning,
                    format!("铸造仿真执行失败: {}", message),
                    0.0,
                    0.0,
                    serde_json::json!({ "error": message }),
                );
                self.dispatch_alarms(vec![alarm], None).await;
            }
        }
    }

    async fn handle_acoustics_event(&self, event: AcousticsEvent) {
        match event {
            AcousticsEvent::AnalysisComplete { result, mut session } => {
                info!("Processing acoustic analysis result for drum: {}", result.drum_id);
                let mut alarms = Vec::new();

                if let Some(s) = session.as_mut() {
                    s.last_acoustic_analysis = Some(result.analysis_id.clone());
                }

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

                self.dispatch_alarms(alarms, session).await;
            }
            AcousticsEvent::AnalysisError { drum_id, message } => {
                error!("Acoustic analysis error for drum {}: {}", drum_id, message);
                let alarm = Alarm::new(
                    drum_id.clone(),
                    AlarmType::Info,
                    AlarmSeverity::Warning,
                    format!("声学分析执行失败: {}", message),
                    0.0,
                    0.0,
                    serde_json::json!({ "error": message }),
                );
                self.dispatch_alarms(vec![alarm], None).await;
            }
            AcousticsEvent::FrequencyDeviationResult { drum_id, deviations } => {
                self.handle_frequency_deviation(&drum_id, &deviations);
            }
        }
    }

    async fn handle_dtu_event(&self, event: DtuEvent, _session: DrumSession) {
        match event {
            DtuEvent::SensorReading { reading, validated: _ } => {
                let freq_deviations = crate::acoustic_analyzer::AcousticAnalyzerService::check_frequency_deviation(
                    &reading.tap_spectrum,
                    &self.config.acoustics.as_ref()
                        .map(|a| a.reference_frequencies_hz.clone())
                        .unwrap_or_default(),
                    self.config.threshold_frequency_deviation_hz,
                );
                self.handle_frequency_deviation(&reading.drum_id, &freq_deviations);

                if let Some(alarm) = self.check_wall_thickness_anomaly(&reading.wall_thickness, &reading.drum_id) {
                    self.dispatch_alarms(vec![alarm], None).await;
                }

                if let Some(alarm) = self.check_alloy_composition(&reading.alloy, &reading.drum_id) {
                    self.dispatch_alarms(vec![alarm], None).await;
                }
            }
            DtuEvent::ValidationError { drum_id, field, message } => {
                error!("DTU validation error for drum {}: field={}, msg={}", drum_id, field, message);
                let alarm = Alarm::new(
                    drum_id.clone(),
                    AlarmType::Info,
                    AlarmSeverity::Warning,
                    format!("传感器数据校验失败 [{}]: {}", field, message),
                    0.0,
                    0.0,
                    serde_json::json!({ "field": field, "message": message }),
                );
                self.dispatch_alarms(vec![alarm], None).await;
            }
        }
    }

    fn handle_frequency_deviation(&self, drum_id: &str, deviations: &[(f64, f64, f64)]) {
        if deviations.is_empty() {
            return;
        }
        let mut alarms = Vec::new();
        for (ref_f, meas_f, dev) in deviations {
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
                drum_id.to_string(),
                AlarmType::FrequencyDeviation,
                severity,
                format!("音准偏差: 参考频率{:.1}Hz, 实测{:.1}Hz, 偏差{:+.2}Hz", ref_f, meas_f, dev),
                dev.abs(),
                self.config.threshold_frequency_deviation_hz,
                metadata,
            ));
        }

        let _ = self.dispatch_alarms(alarms, None);
    }

    fn check_wall_thickness_anomaly(
        &self,
        thickness: &[crate::models::ThicknessPoint],
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

    fn check_alloy_composition(
        &self,
        alloy: &crate::models::AlloyComposition,
        drum_id: &str,
    ) -> Option<Alarm> {
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

    async fn dispatch_alarms(&self, alarms: Vec<Alarm>, mut session: Option<DrumSession>) {
        if alarms.is_empty() {
            return;
        }

        if let Some(s) = session.as_mut() {
            s.active_alarms += alarms.len();
        }

        {
            let mut pending = self.pending_alarms.lock();
            pending.extend(alarms.clone());
        }

        for alarm in &alarms {
            let _ = self.alarm_tx.send(alarm.clone());
            if let Err(e) = self.mqtt.publish_alarm(alarm).await {
                error!("Failed to publish alarm via MQTT: {}", e);
            }
            info!("Alarm dispatched [{}] {} for drum {}: {}",
                format!("{:?}", alarm.severity),
                format!("{:?}", alarm.alarm_type),
                alarm.drum_id,
                alarm.message);
        }
    }

    fn flush_pending(&self) -> Vec<Alarm> {
        let mut pending = self.pending_alarms.lock();
        pending.drain(..).collect()
    }

    pub fn drain_pending(&self) -> Vec<Alarm> {
        self.flush_pending()
    }
}
