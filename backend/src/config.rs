use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::Path;
use anyhow::{Result, Context};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialThermal {
    pub thermal_conductivity_wmk: f64,
    pub specific_heat_jkgk: f64,
    pub thermal_expansion_perk: f64,
    pub latent_heat_jkg: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialCasting {
    pub solidus_temperature_c: f64,
    pub liquidus_temperature_c: f64,
    pub pour_temperature_min_c: f64,
    pub pour_temperature_max_c: f64,
    pub shrinkage_pct: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseDiagramParams {
    pub tin_liquidus_slope: f64,
    pub lead_depression_per_pct: f64,
    pub eutectic_tin_pct: f64,
    pub eutectic_temp_c: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridRefinementParams {
    pub max_depth: usize,
    pub default_size: usize,
    pub sun_pattern_rays: usize,
    pub boss_inner_radius_frac: f64,
    pub ray_band_radius_frac: f64,
    pub halo_radius_frac: f64,
    pub refinement_threshold: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MaterialConfig {
    pub name: String,
    pub description: String,
    pub youngs_modulus_pa: f64,
    pub poissons_ratio: f64,
    pub density_kgm3: f64,
    pub thermal: MaterialThermal,
    pub casting: MaterialCasting,
    pub phase_diagram: PhaseDiagramParams,
    pub chvorinov_constant_sqrt: f64,
    pub n: f64,
    pub hot_tear_threshold_cooling_rate_ks: f64,
    pub cold_shut_min_superheat_c: f64,
    pub grid_refinement: GridRefinementParams,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EigenvalueEntry {
    pub m: usize,
    pub n: usize,
    pub lambda: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArcLengthParams {
    pub initial_delta_s: f64,
    pub max_load_steps: usize,
    pub max_iterations_per_step: usize,
    pub tolerance: f64,
    pub arc_length_scaling_alpha: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NonlinearParams {
    pub arc_length_method: ArcLengthParams,
    pub large_deflection_ratio: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SoundRadiationParams {
    pub field_hemisphere_radius_m: f64,
    pub grid_resolution: usize,
    pub reference_pascals_pa: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcousticsConfig {
    pub air_sound_speed_ms: f64,
    pub air_density_kgm3: f64,
    pub mode_count: usize,
    pub eigenvalues_lambda: Vec<EigenvalueEntry>,
    pub nonlinear: NonlinearParams,
    pub sound_radiation: SoundRadiationParams,
    pub reference_frequencies_hz: Vec<f64>,
}

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
    #[serde(skip)]
    pub material: Option<MaterialConfig>,
    #[serde(skip)]
    pub acoustics: Option<AcousticsConfig>,
}

impl AppConfig {
    pub fn load() -> Self {
        let mut config = Self::from_env();
        let _ = config.load_json_configs();
        config
    }

    fn from_env() -> Self {
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
            material: None,
            acoustics: None,
        }
    }

    pub fn load_json_configs(&mut self) -> Result<()> {
        let config_dir = env::var("CONFIG_DIR").unwrap_or_else(|_| "config".to_string());
        let config_path = Path::new(&config_dir);

        if config_path.join("materials.json").exists() {
            let materials_path = config_path.join("materials.json");
            let materials_str = fs::read_to_string(&materials_path)
                .with_context(|| format!("Failed to read {}", materials_path.display()))?;
            #[derive(Deserialize)]
            struct MaterialsRoot {
                materials: Vec<MaterialJson>,
            }
            #[derive(Deserialize)]
            struct MaterialJson {
                name: String,
                description: String,
                mechanical: MechanicalJson,
                thermal: MaterialThermal,
                casting: MaterialCasting,
                phase_diagram: PhaseDiagramParams,
                chvorinov_constant_sqrt: f64,
                n: f64,
                hot_tear_threshold_cooling_rate_ks: f64,
                cold_shut_min_superheat_c: f64,
                grid_refinement: GridRefinementParams,
            }
            #[derive(Deserialize)]
            struct MechanicalJson {
                youngs_modulus_pa: f64,
                poissons_ratio: f64,
                density_kgm3: f64,
            }
            let root: MaterialsRoot = serde_json::from_str(&materials_str)
                .with_context(|| "Failed to parse materials.json")?;
            if let Some(mat_json) = root.materials.into_iter().next() {
                let mat = MaterialConfig {
                    name: mat_json.name,
                    description: mat_json.description,
                    youngs_modulus_pa: mat_json.mechanical.youngs_modulus_pa,
                    poissons_ratio: mat_json.mechanical.poissons_ratio,
                    density_kgm3: mat_json.mechanical.density_kgm3,
                    thermal: mat_json.thermal,
                    casting: mat_json.casting,
                    phase_diagram: mat_json.phase_diagram,
                    chvorinov_constant_sqrt: mat_json.chvorinov_constant_sqrt,
                    n: mat_json.n,
                    hot_tear_threshold_cooling_rate_ks: mat_json.hot_tear_threshold_cooling_rate_ks,
                    cold_shut_min_superheat_c: mat_json.cold_shut_min_superheat_c,
                    grid_refinement: mat_json.grid_refinement,
                };
                self.material = Some(mat);
            }
        }

        if config_path.join("acoustics_params.json").exists() {
            let ac_path = config_path.join("acoustics_params.json");
            let ac_str = fs::read_to_string(&ac_path)
                .with_context(|| format!("Failed to read {}", ac_path.display()))?;
            #[derive(Deserialize)]
            struct AcRoot {
                air: AirParams,
                vibration: VibParams,
                circular_plate: PlateParams,
                nonlinear: NonlinearParams,
                sound_radiation: RadiationParams,
                quality_metrics: QualityParams,
            }
            #[derive(Deserialize)]
            struct AirParams { sound_speed_ms: f64, density_kgm3: f64 }
            #[derive(Deserialize)]
            struct VibParams { mode_count: usize }
            #[derive(Deserialize)]
            struct PlateParams { eigenvalues_lambda: Vec<EigenvalueEntry> }
            #[derive(Deserialize)]
            struct RadiationParams { field_hemisphere_radius_m: f64, grid_resolution: usize, reference_pascals_pa: f64 }
            #[derive(Deserialize)]
            struct QualityParams { reference_frequencies_hz: Vec<f64> }

            let root: AcRoot = serde_json::from_str(&ac_str)
                .with_context(|| "Failed to parse acoustics_params.json")?;
            self.acoustics = Some(AcousticsConfig {
                air_sound_speed_ms: root.air.sound_speed_ms,
                air_density_kgm3: root.air.density_kgm3,
                mode_count: root.vibration.mode_count,
                eigenvalues_lambda: root.circular_plate.eigenvalues_lambda,
                nonlinear: root.nonlinear,
                sound_radiation: SoundRadiationParams {
                    field_hemisphere_radius_m: root.sound_radiation.field_hemisphere_radius_m,
                    grid_resolution: root.sound_radiation.grid_resolution,
                    reference_pascals_pa: root.sound_radiation.reference_pascals_pa,
                },
                reference_frequencies_hz: root.quality_metrics.reference_frequencies_hz,
            });
        }

        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self::load()
    }
}
