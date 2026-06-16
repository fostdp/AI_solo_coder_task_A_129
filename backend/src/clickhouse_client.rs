use crate::models::*;
use anyhow::{Context, Result};
use chrono::Utc;
use clickhouse::Client;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

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
        insert
            .write((
                drum.drum_id.clone(),
                drum.name.clone(),
                drum.ethnic_group.clone(),
                drum.origin_region.clone(),
                drum.estimated_era.clone(),
                drum.diameter_cm,
                drum.height_cm,
                drum.mass_kg,
                drum.created_at,
                drum.notes.clone(),
            ))
            .await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_drums(&self) -> Result<Vec<Drum>> {
        let db = self.with_database("bronze_drum");
        let rows: Vec<(String, String, String, String, String, f64, f64, f64, chrono::DateTime<Utc>, Option<String>)> = db
            .query("SELECT drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes FROM drums ORDER BY created_at DESC")
            .fetch_all()
            .await?;

        Ok(rows
            .into_iter()
            .map(
                |(drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes)| {
                    Drum {
                        drum_id,
                        name,
                        ethnic_group,
                        origin_region,
                        estimated_era,
                        diameter_cm,
                        height_cm,
                        mass_kg,
                        created_at,
                        notes,
                    }
                },
            )
            .collect())
    }

    pub async fn get_drum(&self, drum_id: &str) -> Result<Option<Drum>> {
        let db = self.with_database("bronze_drum");
        let row: Option<(String, String, String, String, String, f64, f64, f64, chrono::DateTime<Utc>, Option<String>)> = db
            .query("SELECT drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes FROM drums WHERE drum_id = ?")
            .bind(drum_id)
            .fetch_optional()
            .await?;

        Ok(row.map(
            |(drum_id, name, ethnic_group, origin_region, estimated_era, diameter_cm, height_cm, mass_kg, created_at, notes)| {
                Drum {
                    drum_id,
                    name,
                    ethnic_group,
                    origin_region,
                    estimated_era,
                    diameter_cm,
                    height_cm,
                    mass_kg,
                    created_at,
                    notes,
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
        insert
            .write((
                reading.reading_id.clone(),
                reading.drum_id.clone(),
                reading.timestamp,
                reading.alloy.copper_pct,
                reading.alloy.tin_pct,
                reading.alloy.lead_pct,
                reading.alloy.zinc_pct,
                reading.alloy.other_impurities_pct,
                wt_json,
                sp_json,
                reading.temperature_c,
                reading.ambient_humidity_pct,
                sid_json,
            ))
            .await?;
        insert.end().await?;

        let mut wt_insert = db.insert("wall_thickness_history")?;
        for tp in &reading.wall_thickness {
            wt_insert
                .write((
                    reading.drum_id.clone(),
                    reading.timestamp,
                    tp.zone.clone(),
                    tp.x_frac,
                    tp.y_frac,
                    tp.thickness_mm,
                ))
                .await?;
        }
        wt_insert.end().await?;

        Ok(())
    }

    pub async fn get_sensor_readings(&self, drum_id: &str, limit: usize) -> Result<Vec<SensorReading>> {
        let db = self.with_database("bronze_drum");
        let raw_rows: Vec<(String, String, chrono::DateTime<Utc>, f64, f64, f64, f64, f64, String, String, f64, f64, String)> = db
            .query("SELECT reading_id, drum_id, timestamp, copper_pct, tin_pct, lead_pct, zinc_pct, other_pct, wall_thickness, tap_spectrum, temperature_c, humidity_pct, sensor_ids FROM sensor_readings WHERE drum_id = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(drum_id)
            .bind(limit as u64)
            .fetch_all()
            .await?;

        let mut result = Vec::new();
        for row in raw_rows {
            let wall_thickness: Vec<ThicknessPoint> = serde_json::from_str(&row.8).unwrap_or_default();
            let tap_spectrum: Vec<SpectrumBin> = serde_json::from_str(&row.9).unwrap_or_default();
            let sensor_ids: Vec<String> = serde_json::from_str(&row.12).unwrap_or_default();

            result.push(SensorReading {
                reading_id: row.0,
                drum_id: row.1,
                timestamp: row.2,
                alloy: AlloyComposition {
                    copper_pct: row.3,
                    tin_pct: row.4,
                    lead_pct: row.5,
                    zinc_pct: row.6,
                    other_impurities_pct: row.7,
                },
                wall_thickness,
                tap_spectrum,
                temperature_c: row.10,
                ambient_humidity_pct: row.11,
                sensor_ids,
            });
        }
        Ok(result)
    }

    pub async fn get_wall_thickness_history(&self, drum_id: &str, limit: usize) -> Result<Vec<(chrono::DateTime<Utc>, Vec<ThicknessPoint>)>> {
        let db = self.with_database("bronze_drum");
        let raw_rows: Vec<(chrono::DateTime<Utc>, String, f64, f64, f64)> = db
            .query("SELECT timestamp, zone, x_frac, y_frac, thickness_mm FROM wall_thickness_history WHERE drum_id = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(drum_id)
            .bind(limit as u64)
            .fetch_all()
            .await?;

        let mut grouped: std::collections::HashMap<chrono::DateTime<Utc>, Vec<ThicknessPoint>> = std::collections::HashMap::new();
        for row in raw_rows {
            grouped.entry(row.0).or_insert_with(Vec::new).push(ThicknessPoint {
                zone: row.1,
                x_frac: row.2,
                y_frac: row.3,
                thickness_mm: row.4,
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
        insert
            .write((
                sim.sim_id.clone(),
                sim.drum_id.clone(),
                sim.created_at,
                sim.alloy.copper_pct,
                sim.alloy.tin_pct,
                sim.alloy.lead_pct,
                sim.alloy.zinc_pct,
                sim.alloy.other_impurities_pct,
                sim.pour_temperature_c,
                sim.mold_temperature_c,
                sim.cooling_time_s,
                sim.solidus_temperature_c,
                sim.liquidus_temperature_c,
                sm_json,
                cr_json,
                df_json,
                sim.quality_score,
                sim.overall_risk.clone(),
            ))
            .await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_casting_result(&self, drum_id: &str) -> Result<Option<CastingSimulationResult>> {
        let db = self.with_database("bronze_drum");
        let row: Option<(String, String, chrono::DateTime<Utc>, f64, f64, f64, f64, f64, f64, f64, f64, f64, f64, String, String, String, f64, String)> = db
            .query("SELECT sim_id, drum_id, created_at, copper_pct, tin_pct, lead_pct, zinc_pct, other_pct, pour_temp, mold_temp, cooling_time, solidus_temp, liquidus_temp, shrinkage_map, cooling_rate_map, defects, quality_score, overall_risk FROM casting_simulations WHERE drum_id = ? ORDER BY created_at DESC LIMIT 1")
            .bind(drum_id)
            .fetch_optional()
            .await?;

        Ok(row.map(|r| {
            let shrinkage_risk_map: Vec<(f64, f64, f64)> = serde_json::from_str(&r.13).unwrap_or_default();
            let cooling_rate_map: Vec<(f64, f64, f64)> = serde_json::from_str(&r.14).unwrap_or_default();
            let defects: Vec<CastingDefect> = serde_json::from_str(&r.15).unwrap_or_default();

            CastingSimulationResult {
                sim_id: r.0,
                drum_id: r.1,
                created_at: r.2,
                alloy: AlloyComposition {
                    copper_pct: r.3,
                    tin_pct: r.4,
                    lead_pct: r.5,
                    zinc_pct: r.6,
                    other_impurities_pct: r.7,
                },
                pour_temperature_c: r.8,
                mold_temperature_c: r.9,
                cooling_time_s: r.10,
                solidus_temperature_c: r.11,
                liquidus_temperature_c: r.12,
                shrinkage_risk_map,
                cooling_rate_map,
                defects,
                quality_score: r.16,
                overall_risk: r.17,
            }
        }))
    }

    pub async fn insert_acoustic_analysis(&self, analysis: &AcousticAnalysisResult) -> Result<()> {
        let db = self.with_database("bronze_drum");
        let vm_json = serde_json::to_string(&analysis.vibration_modes)?;
        let rf_json = serde_json::to_string(&analysis.resonance_frequencies_hz)?;
        let sf_json = serde_json::to_string(&analysis.sound_field)?;

        let mut insert = db.insert("acoustic_analyses")?;
        insert
            .write((
                analysis.analysis_id.clone(),
                analysis.drum_id.clone(),
                analysis.created_at,
                analysis.youngs_modulus_pa,
                analysis.poissons_ratio,
                analysis.density_kgm3,
                analysis.sound_speed_air_ms,
                analysis.air_density_kgm3,
                vm_json,
                analysis.radiated_sound_power_w,
                rf_json,
                sf_json,
                analysis.sound_quality_metric,
            ))
            .await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_acoustic_result(&self, drum_id: &str) -> Result<Option<AcousticAnalysisResult>> {
        let db = self.with_database("bronze_drum");
        let row: Option<(String, String, chrono::DateTime<Utc>, f64, f64, f64, f64, f64, String, f64, String, String, f64)> = db
            .query("SELECT analysis_id, drum_id, created_at, youngs_modulus, poissons_ratio, density, sound_speed, air_density, vibration_modes, radiated_power, resonance_freqs, sound_field, sound_quality FROM acoustic_analyses WHERE drum_id = ? ORDER BY created_at DESC LIMIT 1")
            .bind(drum_id)
            .fetch_optional()
            .await?;

        Ok(row.map(|r| {
            let vibration_modes: Vec<VibrationMode> = serde_json::from_str(&r.8).unwrap_or_default();
            let resonance_frequencies_hz: Vec<f64> = serde_json::from_str(&r.10).unwrap_or_default();
            let sound_field: Vec<SoundFieldPoint> = serde_json::from_str(&r.11).unwrap_or_default();

            AcousticAnalysisResult {
                analysis_id: r.0,
                drum_id: r.1,
                created_at: r.2,
                youngs_modulus_pa: r.3,
                poissons_ratio: r.4,
                density_kgm3: r.5,
                sound_speed_air_ms: r.6,
                air_density_kgm3: r.7,
                vibration_modes,
                radiated_sound_power_w: r.9,
                resonance_frequencies_hz,
                sound_field,
                sound_quality_metric: r.12,
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
        insert
            .write((
                alarm.alarm_id.clone(),
                alarm.drum_id.clone(),
                alarm.timestamp,
                at_str,
                sv_str,
                alarm.message.clone(),
                alarm.measured_value,
                alarm.threshold_value,
                md_json,
                ack,
            ))
            .await?;
        insert.end().await?;
        Ok(())
    }

    pub async fn get_alarms(&self, drum_id: &str, limit: usize) -> Result<Vec<Alarm>> {
        let db = self.with_database("bronze_drum");
        let raw_rows: Vec<(String, String, chrono::DateTime<Utc>, String, String, String, f64, f64, String, u8)> = db
            .query("SELECT alarm_id, drum_id, timestamp, alarm_type, severity, message, measured_value, threshold_value, metadata, acknowledged FROM alarms WHERE drum_id = ? ORDER BY timestamp DESC LIMIT ?")
            .bind(drum_id)
            .bind(limit as u64)
            .fetch_all()
            .await?;

        let mut result = Vec::new();
        for row in raw_rows {
            let metadata: serde_json::Value = serde_json::from_str(&row.8).unwrap_or(serde_json::json!({}));
            let alarm_type = match row.3.as_str() {
                "FrequencyDeviation" => AlarmType::FrequencyDeviation,
                "ShrinkageDefect" => AlarmType::ShrinkageDefect,
                "ThicknessAnomaly" => AlarmType::ThicknessAnomaly,
                "AlloyAnomaly" => AlarmType::AlloyAnomaly,
                "StructuralFailureRisk" => AlarmType::StructuralFailureRisk,
                "SoundQualityDegradation" => AlarmType::SoundQualityDegradation,
                _ => AlarmType::Info,
            };
            let severity = match row.4.as_str() {
                "Info" => AlarmSeverity::Info,
                "Warning" => AlarmSeverity::Warning,
                "Critical" => AlarmSeverity::Critical,
                "Fatal" => AlarmSeverity::Fatal,
                _ => AlarmSeverity::Info,
            };
            result.push(Alarm {
                alarm_id: row.0,
                drum_id: row.1,
                timestamp: row.2,
                alarm_type,
                severity,
                message: row.5,
                measured_value: row.6,
                threshold_value: row.7,
                metadata,
                acknowledged: row.9 == 1,
            });
        }
        Ok(result)
    }
}
