use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drum {
    pub drum_id: String,
    pub name: String,
    pub ethnic_group: String,
    pub origin_region: String,
    pub estimated_era: String,
    pub diameter_cm: f64,
    pub height_cm: f64,
    pub mass_kg: f64,
    pub created_at: DateTime<Utc>,
    pub notes: Option<String>,
}

impl Drum {
    pub fn new(
        name: String,
        ethnic_group: String,
        origin_region: String,
        estimated_era: String,
        diameter_cm: f64,
        height_cm: f64,
        mass_kg: f64,
        notes: Option<String>,
    ) -> Self {
        Self {
            drum_id: Uuid::new_v4().to_string(),
            name,
            ethnic_group,
            origin_region,
            estimated_era,
            diameter_cm,
            height_cm,
            mass_kg,
            created_at: Utc::now(),
            notes,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlloyComposition {
    pub copper_pct: f64,
    pub tin_pct: f64,
    pub lead_pct: f64,
    pub zinc_pct: f64,
    pub other_impurities_pct: f64,
}

impl AlloyComposition {
    pub fn standard_bronze() -> Self {
        Self {
            copper_pct: 78.0,
            tin_pct: 18.0,
            lead_pct: 3.0,
            zinc_pct: 0.5,
            other_impurities_pct: 0.5,
        }
    }

    pub fn total(&self) -> f64 {
        self.copper_pct + self.tin_pct + self.lead_pct + self.zinc_pct + self.other_impurities_pct
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThicknessPoint {
    pub zone: String,
    pub x_frac: f64,
    pub y_frac: f64,
    pub thickness_mm: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpectrumBin {
    pub frequency_hz: f64,
    pub amplitude_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensorReading {
    pub reading_id: String,
    pub drum_id: String,
    pub timestamp: DateTime<Utc>,
    pub alloy: AlloyComposition,
    pub wall_thickness: Vec<ThicknessPoint>,
    pub tap_spectrum: Vec<SpectrumBin>,
    pub temperature_c: f64,
    pub ambient_humidity_pct: f64,
    pub sensor_ids: Vec<String>,
}

impl SensorReading {
    pub fn new(drum_id: String) -> Self {
        Self {
            reading_id: Uuid::new_v4().to_string(),
            drum_id,
            timestamp: Utc::now(),
            alloy: AlloyComposition::standard_bronze(),
            wall_thickness: Vec::new(),
            tap_spectrum: Vec::new(),
            temperature_c: 25.0,
            ambient_humidity_pct: 50.0,
            sensor_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DefectType {
    ShrinkageCavity,
    Porosity,
    HotTear,
    ColdShut,
    IncompleteFilling,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastingDefect {
    pub defect_id: String,
    pub defect_type: DefectType,
    pub zone: String,
    pub x_frac: f64,
    pub y_frac: f64,
    pub severity: f64,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastingSimulationResult {
    pub sim_id: String,
    pub drum_id: String,
    pub created_at: DateTime<Utc>,
    pub alloy: AlloyComposition,
    pub pour_temperature_c: f64,
    pub mold_temperature_c: f64,
    pub cooling_time_s: f64,
    pub solidus_temperature_c: f64,
    pub liquidus_temperature_c: f64,
    pub shrinkage_risk_map: Vec<(f64, f64, f64)>,
    pub cooling_rate_map: Vec<(f64, f64, f64)>,
    pub defects: Vec<CastingDefect>,
    pub quality_score: f64,
    pub overall_risk: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastingSimulationRequest {
    pub drum_id: String,
    pub alloy: Option<AlloyComposition>,
    pub pour_temperature_c: Option<f64>,
    pub mold_temperature_c: Option<f64>,
    pub cooling_time_s: Option<f64>,
    pub mesh_resolution: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VibrationMode {
    pub mode_order: usize,
    pub frequency_hz: f64,
    pub damping_ratio: f64,
    pub node_pattern: String,
    pub modal_displacements: Vec<(f64, f64, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundFieldPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub pressure_pa: f64,
    pub spl_db: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcousticAnalysisResult {
    pub analysis_id: String,
    pub drum_id: String,
    pub created_at: DateTime<Utc>,
    pub youngs_modulus_pa: f64,
    pub poissons_ratio: f64,
    pub density_kgm3: f64,
    pub sound_speed_air_ms: f64,
    pub air_density_kgm3: f64,
    pub vibration_modes: Vec<VibrationMode>,
    pub radiated_sound_power_w: f64,
    pub resonance_frequencies_hz: Vec<f64>,
    pub sound_field: Vec<SoundFieldPoint>,
    pub sound_quality_metric: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcousticAnalysisRequest {
    pub drum_id: String,
    pub youngs_modulus_pa: Option<f64>,
    pub poissons_ratio: Option<f64>,
    pub density_kgm3: Option<f64>,
    pub use_sensor_calibration: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Clone)]
pub enum AlarmSeverity {
    Info,
    Warning,
    Critical,
    Fatal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Clone)]
pub enum AlarmType {
    FrequencyDeviation,
    ShrinkageDefect,
    ThicknessAnomaly,
    AlloyAnomaly,
    StructuralFailureRisk,
    SoundQualityDegradation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alarm {
    pub alarm_id: String,
    pub drum_id: String,
    pub timestamp: DateTime<Utc>,
    pub alarm_type: AlarmType,
    pub severity: AlarmSeverity,
    pub message: String,
    pub measured_value: f64,
    pub threshold_value: f64,
    pub metadata: serde_json::Value,
    pub acknowledged: bool,
}

impl Alarm {
    pub fn new(
        drum_id: String,
        alarm_type: AlarmType,
        severity: AlarmSeverity,
        message: String,
        measured_value: f64,
        threshold_value: f64,
        metadata: serde_json::Value,
    ) -> Self {
        Self {
            alarm_id: Uuid::new_v4().to_string(),
            drum_id,
            timestamp: Utc::now(),
            alarm_type,
            severity,
            message,
            measured_value,
            threshold_value,
            metadata,
            acknowledged: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrumSession {
    pub drum_id: String,
    pub last_reading_time: Option<DateTime<Utc>>,
    pub last_casting_sim: Option<String>,
    pub last_acoustic_analysis: Option<String>,
    pub active_alarms: usize,
    pub reference_frequencies_hz: Vec<f64>,
}

impl DrumSession {
    pub fn new(drum_id: String) -> Self {
        Self {
            drum_id,
            last_reading_time: None,
            last_casting_sim: None,
            last_acoustic_analysis: None,
            active_alarms: 0,
            reference_frequencies_hz: vec![523.25, 659.25, 783.99, 1046.50, 1318.51],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDrumRequest {
    pub name: String,
    pub ethnic_group: String,
    pub origin_region: String,
    pub estimated_era: String,
    pub diameter_cm: f64,
    pub height_cm: f64,
    pub mass_kg: f64,
    pub notes: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub message: String,
    pub data: Option<T>,
    pub error: Option<String>,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn ok(data: T) -> Self {
        Self {
            success: true,
            message: "OK".to_string(),
            data: Some(data),
            error: None,
        }
    }

    pub fn ok_msg(message: &str) -> Self {
        Self {
            success: true,
            message: message.to_string(),
            data: None,
            error: None,
        }
    }

    pub fn err(error: &str) -> Self {
        Self {
            success: false,
            message: "Error".to_string(),
            data: None,
            error: Some(error.to_string()),
        }
    }
}
