use crate::models::*;
use anyhow::{Context, Result};
use chrono::Utc;
use clickhouse::Client;
use clickhouse::Row;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct DrumRow {
    pub drum_id: String,
    pub name: String,
    pub ethnic_group: String,
    pub origin_region: String,
    pub estimated_era: String,
    pub diameter_cm: f64,
    pub height_cm: f64,
    pub mass_kg: f64,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub notes: Option<String>,
}

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct SensorReadingRow {
    pub reading_id: String,
    pub drum_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub copper_pct: f64,
    pub tin_pct: f64,
    pub lead_pct: f64,
    pub zinc_pct: f64,
    pub other_pct: f64,
    pub wall_thickness: String,
    pub tap_spectrum: String,
    pub temperature_c: f64,
    pub humidity_pct: f64,
    pub sensor_ids: String,
}

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct CastingSimRow {
    pub sim_id: String,
    pub drum_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub copper_pct: f64,
    pub tin_pct: f64,
    pub lead_pct: f64,
    pub zinc_pct: f64,
    pub other_pct: f64,
    pub pour_temp: f64,
    pub mold_temp: f64,
    pub cooling_time: f64,
    pub solidus_temp: f64,
    pub liquidus_temp: f64,
    pub shrinkage_map: String,
    pub cooling_rate_map: String,
    pub defects: String,
    pub quality_score: f64,
    pub overall_risk: String,
}

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AcousticAnalysisRow {
    pub analysis_id: String,
    pub drum_id: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub youngs_modulus: f64,
    pub poissons_ratio: f64,
    pub density: f64,
    pub sound_speed: f64,
    pub air_density: f64,
    pub vibration_modes: String,
    pub radiated_power: f64,
    pub resonance_freqs: String,
    pub sound_field: String,
    pub sound_quality: f64,
}

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct AlarmRow {
    pub alarm_id: String,
    pub drum_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub alarm_type: String,
    pub severity: String,
    pub message: String,
    pub measured_value: f64,
    pub threshold_value: f64,
    pub metadata: String,
    pub acknowledged: u8,
}

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct WallThicknessRow {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub zone: String,
    pub x_frac: f64,
    pub y_frac: f64,
    pub thickness_mm: f64,
}

#[derive(Row, Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "snake_case")]
pub struct WallThicknessInsertRow {
    pub drum_id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub zone: String,
    pub x_frac: f64,
    pub y_frac: f64,
    pub thickness_mm: f64,
}

pub struct ClickHouseClient {
    client: Client,
    url: String,
}

impl ClickHouseClient {
    pub fn new(url: String) -> Self {
        let client = Client::default()
            .with_url(&url);
        Self { client, url }
    }

    pub fn with_database(&self, database: &str) -> Client {
        self.client.clone().with_database(database)
    }

    pub async fn ensure_database(&self) -> Result<()> {
        info!("Ensuring ClickHouse database exists");
        let query = "CREATE DATABASE IF NOT EXISTS bronze_drum ENGINE = Atomic";
        self.client.query(query).execute().await
            .context("Failed to create database")?;
        info!("Database bronze_drum is ready");
        Ok(())
    }

    pub async fn ensure_tables(&self) -> Result<()> {
        let db = self.with_database("bronze_drum");

        let tables = vec![
            ("drums", r#"
                CREATE TABLE IF NOT EXISTS drums (
                    drum_id String,
                    name String,
                    ethnic_group String,
                    origin_region String,
                    estimated_era String,
                    diameter_cm Float64,
                    height_cm Float64,
                    mass_kg Float64,
                    created_at DateTime64(9, 'UTC'),
                    notes Nullable(String)
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(created_at)
                ORDER BY (drum_id, created_at)
                PRIMARY KEY drum_id
            "#),
            ("sensor_readings", r#"
                CREATE TABLE IF NOT EXISTS sensor_readings (
                    reading_id String,
                    drum_id String,
                    timestamp DateTime64(9, 'UTC'),
                    copper_pct Float64,
                    tin_pct Float64,
                    lead_pct Float64,
                    zinc_pct Float64,
                    other_pct Float64,
                    wall_thickness String,
                    tap_spectrum String,
                    temperature_c Float64,
                    humidity_pct Float64,
                    sensor_ids String
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (drum_id, timestamp, reading_id)
                PRIMARY KEY (drum_id, timestamp)
            "#),
            ("casting_simulations", r#"
                CREATE TABLE IF NOT EXISTS casting_simulations (
                    sim_id String,
                    drum_id String,
                    created_at DateTime64(9, 'UTC'),
                    copper_pct Float64,
                    tin_pct Float64,
                    lead_pct Float64,
                    zinc_pct Float64,
                    other_pct Float64,
                    pour_temp Float64,
                    mold_temp Float64,
                    cooling_time Float64,
                    solidus_temp Float64,
                    liquidus_temp Float64,
                    shrinkage_map String,
                    cooling_rate_map String,
                    defects String,
                    quality_score Float64,
                    overall_risk String
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(created_at)
                ORDER BY (drum_id, created_at, sim_id)
                PRIMARY KEY (drum_id, created_at)
            "#),
            ("acoustic_analyses", r#"
                CREATE TABLE IF NOT EXISTS acoustic_analyses (
                    analysis_id String,
                    drum_id String,
                    created_at DateTime64(9, 'UTC'),
                    youngs_modulus Float64,
                    poissons_ratio Float64,
                    density Float64,
                    sound_speed Float64,
                    air_density Float64,
                    vibration_modes String,
                    radiated_power Float64,
                    resonance_freqs String,
                    sound_field String,
                    sound_quality Float64
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(created_at)
                ORDER BY (drum_id, created_at, analysis_id)
                PRIMARY KEY (drum_id, created_at)
            "#),
            ("alarms", r#"
                CREATE TABLE IF NOT EXISTS alarms (
                    alarm_id String,
                    drum_id String,
                    timestamp DateTime64(9, 'UTC'),
                    alarm_type String,
                    severity String,
                    message String,
                    measured_value Float64,
                    threshold_value Float64,
                    metadata String,
                    acknowledged UInt8 DEFAULT 0
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (drum_id, timestamp, alarm_id)
                PRIMARY KEY (drum_id, timestamp)
            "#),
            ("wall_thickness_history", r#"
                CREATE TABLE IF NOT EXISTS wall_thickness_history (
                    drum_id String,
                    timestamp DateTime64(9, 'UTC'),
                    zone String,
                    x_frac Float64,
                    y_frac Float64,
                    thickness_mm Float64
                ) ENGINE = MergeTree()
                PARTITION BY toYYYYMM(timestamp)
                ORDER BY (drum_id, zone, timestamp)
            "#),
        ];

        for (name, sql) in tables {
            debug!("Creating table: {}", name);
            db.query(sql).execute().await
                .with_context(|| format!("Failed to create table {}", name))?;
            info!("Table {} is ready", name);
        }

        Ok(())
    }

    pub async fn insert_drum(&self, drum: &Drum) -> Result<()> {
        let db = self.with_database("bronze_drum");
        let mut insert = db.insert("drums")?;
        let row = DrumRow {
            drum_id: drum.drum_id.clone(),
            name: drum.name.clone(),
            ethnic_group: drum.ethnic_group.clone(),
            origin_region: drum.origin_region.clone(),
            estimated_era: drum.estimated_era.clone(),
            diameter_cm: drum.diameter_cm,
            height_cm: drum.height_cm,
            mass_kg: drum.mass_kg,
            created_at: drum.created_at,
            notes: drum.notes.clone(),
        };
        insert.write(&row).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_drums(&self) -> Result<Vec<Drum>> {
        let db = self.with_database("bronze_drum");
        let rows: Vec<DrumRow> = db
            .query("SELECT drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes FROM drums ORDER BY created_at DESC")
            .fetch_all::<DrumRow>()
            .await?;

        Ok(rows
            .into_iter()
            .map(|row| {
                Drum {
                    drum_id: row.drum_id,
                    name: row.name,
                    ethnic_group: row.ethnic_group,
                    origin_region: row.origin_region,
                    estimated_era: row.estimated_era,
                    diameter_cm: row.diameter_cm,
                    height_cm: row.height_cm,
                    mass_kg: row.mass_kg,
                    created_at: row.created_at,
                    notes: row.notes,
                }
            },
            )
            .collect())
    }

    pub async fn get_drum(&self, drum_id: &str) -> Result<Option<Drum>> {
        let db = self.with_database("bronze_drum");
        let row: Option<DrumRow> = db
            .query("SELECT drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes FROM drums WHERE drum_id = ?")
            .bind(drum_id)
            .fetch_optional::<DrumRow>()
            .await?;

        Ok(row.map(
            |row| {
                Drum {
                    drum_id: row.drum_id,
                    name: row.name,
                    ethnic_group: row.ethnic_group,
                    origin_region: row.origin_region,
                    estimated_era: row.estimated_era,
                    diameter_cm: row.diameter_cm,
                    height_cm: row.height_cm,
                    mass_kg: row.mass_kg,
                    created_at: row.created_at,
                    notes: row.notes,
                }
            },
        ))
    }

    pub async fn insert_sensor_reading(&self, reading: &SensorReading) -> Result<()> {
        let db = self.with_database("bronze_drum");
        let wt_json = serde_json::to_string(&reading.wall_thickness)?;
        let sp_json = serde_json::to_string(&reading.tap_spectrum)?;
        let sid_json = serde_json::to_string(&reading.sensor_ids)?;

        let mut insert = db.insert("sensor_readings")?;
        let row = SensorReadingRow {
            reading_id: reading.reading_id.clone(),
            drum_id: reading.drum_id.clone(),
            timestamp: reading.timestamp,
            copper_pct: reading.alloy.copper_pct,
            tin_pct: reading.alloy.tin_pct,
            lead_pct: reading.alloy.lead_pct,
            zinc_pct: reading.alloy.zinc_pct,
            other_pct: reading.alloy.other_impurities_pct,
            wall_thickness: wt_json,
            tap_spectrum: sp_json,
            temperature_c: reading.temperature_c,
            humidity_pct: reading.ambient_humidity_pct,
            sensor_ids: sid_json,
        };
        insert.write(&row).await?;
        insert.end().await?;

        let mut wt_insert = db.insert("wall_thickness_history")?;
        for tp in &reading.wall_thickness {
            let wt_row = WallThicknessInsertRow {
                drum_id: reading.drum_id.clone(),
                timestamp: reading.timestamp,
                zone: tp.zone.clone(),
                x_frac: tp.x_frac,
                y_frac: tp.y_frac,
                thickness_mm: tp.thickness_mm,
            };
            wt_insert.write(&wt_row).await?;
        }
        wt_insert.end().await?;

        Ok(())
    }

    pub async fn get_sensor_readings(&self, drum_id: &str, limit: usize) -> Result<Vec<SensorReading>> {
        let db = self.with_database("bronze_drum");
        let raw_rows: Vec<SensorReadingRow> = db
            .query("SELECT reading_id, drum_id, timestamp, copper_pct, tin_pct, lead_pct, zinc_pct, other_pct, wall_thickness, tap_spectrum, temperature_c, humidity_pct, sensor_ids FROM sensor_readings WHERE drum_id = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(drum_id)
            .bind(limit as u64)
            .fetch_all::<SensorReadingRow>()
            .await?;

        let mut result = Vec::new();
        for row in raw_rows {
            let wall_thickness: Vec<ThicknessPoint> = serde_json::from_str(&row.wall_thickness).unwrap_or_default();
            let tap_spectrum: Vec<SpectrumBin> = serde_json::from_str(&row.tap_spectrum).unwrap_or_default();
            let sensor_ids: Vec<String> = serde_json::from_str(&row.sensor_ids).unwrap_or_default();

            result.push(SensorReading {
                reading_id: row.reading_id,
                drum_id: row.drum_id,
                timestamp: row.timestamp,
                alloy: AlloyComposition {
                    copper_pct: row.copper_pct,
                    tin_pct: row.tin_pct,
                    lead_pct: row.lead_pct,
                    zinc_pct: row.zinc_pct,
                    other_impurities_pct: row.other_pct,
                },
                wall_thickness,
                tap_spectrum,
                temperature_c: row.temperature_c,
                ambient_humidity_pct: row.humidity_pct,
                sensor_ids,
            });
        }
        Ok(result)
    }

    pub async fn get_wall_thickness_history(&self, drum_id: &str, limit: usize) -> Result<Vec<(chrono::DateTime<Utc>, Vec<ThicknessPoint>)>> {
        let db = self.with_database("bronze_drum");
        let raw_rows: Vec<WallThicknessRow> = db
            .query("SELECT timestamp, zone, x_frac, y_frac, thickness_mm FROM wall_thickness_history WHERE drum_id = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(drum_id)
            .bind(limit as u64)
            .fetch_all::<WallThicknessRow>()
            .await?;

        let mut grouped: std::collections::HashMap<chrono::DateTime<Utc>, Vec<ThicknessPoint>> = std::collections::HashMap::new();
        for row in raw_rows {
            grouped.entry(row.timestamp).or_insert_with(Vec::new).push(ThicknessPoint {
                zone: row.zone,
                x_frac: row.x_frac,
                y_frac: row.y_frac,
                thickness_mm: row.thickness_mm,
            });
        }

        let mut sorted: Vec<_> = grouped.into_iter().collect();
        sorted.sort_by(|a, b| b.0.cmp(&a.0));
        Ok(sorted)
    }

    pub async fn insert_casting_simulation(&self, sim: &CastingSimulationResult) -> Result<()> {
        let db = self.with_database("bronze_drum");
        let sm_json = serde_json::to_string(&sim.shrinkage_risk_map)?;
        let cr_json = serde_json::to_string(&sim.cooling_rate_map)?;
        let df_json = serde_json::to_string(&sim.defects)?;

        let mut insert = db.insert("casting_simulations")?;
        let row = CastingSimRow {
            sim_id: sim.sim_id.clone(),
            drum_id: sim.drum_id.clone(),
            created_at: sim.created_at,
            copper_pct: sim.alloy.copper_pct,
            tin_pct: sim.alloy.tin_pct,
            lead_pct: sim.alloy.lead_pct,
            zinc_pct: sim.alloy.zinc_pct,
            other_pct: sim.alloy.other_impurities_pct,
            pour_temp: sim.pour_temperature_c,
            mold_temp: sim.mold_temperature_c,
            cooling_time: sim.cooling_time_s,
            solidus_temp: sim.solidus_temperature_c,
            liquidus_temp: sim.liquidus_temperature_c,
            shrinkage_map: sm_json,
            cooling_rate_map: cr_json,
            defects: df_json,
            quality_score: sim.quality_score,
            overall_risk: sim.overall_risk.clone(),
        };
        insert.write(&row).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_casting_result(&self, drum_id: &str) -> Result<Option<CastingSimulationResult>> {
        let db = self.with_database("bronze_drum");
        let row: Option<CastingSimRow> = db
            .query("SELECT sim_id, drum_id, created_at, copper_pct, tin_pct, lead_pct, zinc_pct, other_pct, pour_temp, mold_temp, cooling_time, solidus_temp, liquidus_temp, shrinkage_map, cooling_rate_map, defects, quality_score, overall_risk FROM casting_simulations WHERE drum_id = ? ORDER BY created_at DESC LIMIT 1")
            .bind(drum_id)
            .fetch_optional::<CastingSimRow>()
            .await?;

        Ok(row.map(|r| {
            let shrinkage_risk_map: Vec<(f64, f64, f64)> = serde_json::from_str(&r.shrinkage_map).unwrap_or_default();
            let cooling_rate_map: Vec<(f64, f64, f64)> = serde_json::from_str(&r.cooling_rate_map).unwrap_or_default();
            let defects: Vec<CastingDefect> = serde_json::from_str(&r.defects).unwrap_or_default();

            CastingSimulationResult {
                sim_id: r.sim_id,
                drum_id: r.drum_id,
                created_at: r.created_at,
                alloy: AlloyComposition {
                    copper_pct: r.copper_pct,
                    tin_pct: r.tin_pct,
                    lead_pct: r.lead_pct,
                    zinc_pct: r.zinc_pct,
                    other_impurities_pct: r.other_pct,
                },
                pour_temperature_c: r.pour_temp,
                mold_temperature_c: r.mold_temp,
                cooling_time_s: r.cooling_time,
                solidus_temperature_c: r.solidus_temp,
                liquidus_temperature_c: r.liquidus_temp,
                shrinkage_risk_map,
                cooling_rate_map,
                defects,
                quality_score: r.quality_score,
                overall_risk: r.overall_risk,
            }
        }))
    }

    pub async fn insert_acoustic_analysis(&self, analysis: &AcousticAnalysisResult) -> Result<()> {
        let db = self.with_database("bronze_drum");
        let vm_json = serde_json::to_string(&analysis.vibration_modes)?;
        let rf_json = serde_json::to_string(&analysis.resonance_frequencies_hz)?;
        let sf_json = serde_json::to_string(&analysis.sound_field)?;

        let mut insert = db.insert("acoustic_analyses")?;
        let row = AcousticAnalysisRow {
            analysis_id: analysis.analysis_id.clone(),
            drum_id: analysis.drum_id.clone(),
            created_at: analysis.created_at,
            youngs_modulus: analysis.youngs_modulus_pa,
            poissons_ratio: analysis.poissons_ratio,
            density: analysis.density_kgm3,
            sound_speed: analysis.sound_speed_air_ms,
            air_density: analysis.air_density_kgm3,
            vibration_modes: vm_json,
            radiated_power: analysis.radiated_sound_power_w,
            resonance_freqs: rf_json,
            sound_field: sf_json,
            sound_quality: analysis.sound_quality_metric,
        };
        insert.write(&row).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_acoustic_result(&self, drum_id: &str) -> Result<Option<AcousticAnalysisResult>> {
        let db = self.with_database("bronze_drum");
        let row: Option<AcousticAnalysisRow> = db
            .query("SELECT analysis_id, drum_id, created_at, youngs_modulus, poissons_ratio, density, sound_speed, air_density, vibration_modes, radiated_power, resonance_freqs, sound_field, sound_quality FROM acoustic_analyses WHERE drum_id = ? ORDER BY created_at DESC LIMIT 1")
            .bind(drum_id)
            .fetch_optional::<AcousticAnalysisRow>()
            .await?;

        Ok(row.map(|r| {
            let vibration_modes: Vec<VibrationMode> = serde_json::from_str(&r.vibration_modes).unwrap_or_default();
            let resonance_frequencies_hz: Vec<f64> = serde_json::from_str(&r.resonance_freqs).unwrap_or_default();
            let sound_field: Vec<SoundFieldPoint> = serde_json::from_str(&r.sound_field).unwrap_or_default();

            AcousticAnalysisResult {
                analysis_id: r.analysis_id,
                drum_id: r.drum_id,
                created_at: r.created_at,
                youngs_modulus_pa: r.youngs_modulus,
                poissons_ratio: r.poissons_ratio,
                density_kgm3: r.density,
                sound_speed_air_ms: r.sound_speed,
                air_density_kgm3: r.air_density,
                vibration_modes,
                radiated_sound_power_w: r.radiated_power,
                resonance_frequencies_hz,
                sound_field,
                sound_quality_metric: r.sound_quality,
            }
        }))
    }

    pub async fn insert_alarm(&self, alarm: &Alarm) -> Result<()> {
        let db = self.with_database("bronze_drum");
        let md_json = serde_json::to_string(&alarm.metadata)?;
        let at_str = format!("{:?}", alarm.alarm_type);
        let sv_str = format!("{:?}", alarm.severity);
        let ack = if alarm.acknowledged { 1u8 } else { 0u8 };

        let mut insert = db.insert("alarms")?;
        let row = AlarmRow {
            alarm_id: alarm.alarm_id.clone(),
            drum_id: alarm.drum_id.clone(),
            timestamp: alarm.timestamp,
            alarm_type: at_str,
            severity: sv_str,
            message: alarm.message.clone(),
            measured_value: alarm.measured_value,
            threshold_value: alarm.threshold_value,
            metadata: md_json,
            acknowledged: ack,
        };
        insert.write(&row).await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_alarms(&self, drum_id: &str, limit: usize) -> Result<Vec<Alarm>> {
        let db = self.with_database("bronze_drum");
        let raw_rows: Vec<AlarmRow> = db
            .query("SELECT alarm_id, drum_id, timestamp, alarm_type, severity, message, measured_value, threshold_value, metadata, acknowledged FROM alarms WHERE drum_id = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(drum_id)
            .bind(limit as u64)
            .fetch_all::<AlarmRow>()
            .await?;

        let mut result = Vec::new();
        for row in raw_rows {
            let metadata: serde_json::Value = serde_json::from_str(&row.metadata).unwrap_or(serde_json::json!({}));
            let alarm_type = match row.alarm_type.as_str() {
                "FrequencyDeviation" => AlarmType::FrequencyDeviation,
                "ShrinkageDefect" => AlarmType::ShrinkageDefect,
                "ThicknessAnomaly" => AlarmType::ThicknessAnomaly,
                "AlloyAnomaly" => AlarmType::AlloyAnomaly,
                "StructuralFailureRisk" => AlarmType::StructuralFailureRisk,
                "SoundQualityDegradation" => AlarmType::SoundQualityDegradation,
                _ => AlarmType::Info,
            };
            let severity = match row.severity.as_str() {
                "Info" => AlarmSeverity::Info,
                "Warning" => AlarmSeverity::Warning,
                "Critical" => AlarmSeverity::Critical,
                "Fatal" => AlarmSeverity::Fatal,
                _ => AlarmSeverity::Info,
            };
            result.push(Alarm {
                alarm_id: row.alarm_id,
                drum_id: row.drum_id,
                timestamp: row.timestamp,
                alarm_type,
                severity,
                message: row.message,
                measured_value: row.measured_value,
                threshold_value: row.threshold_value,
                metadata,
                acknowledged: row.acknowledged == 1,
            });
        }
        Ok(result)
    }
}
