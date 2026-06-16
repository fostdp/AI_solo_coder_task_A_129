use crate::config::{AppConfig, MaterialConfig};
use crate::models::{CastingSimulationRequest, CastingSimulationResult, CastingDefect, DefectType, AlloyComposition, DrumSession};
use crate::alarm_mqtt::AlarmCommand;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;

#[derive(Debug)]
pub enum CastingCommand {
    RunSimulation {
        request: CastingSimulationRequest,
        diameter_cm: f64,
        height_cm: f64,
        session: Option<DrumSession>,
        result_tx: oneshot::Sender<Result<CastingSimulationResult, String>>,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum CastingEvent {
    SimulationComplete {
        result: CastingSimulationResult,
        session: Option<DrumSession>,
    },
    SimulationError {
        drum_id: String,
        message: String,
    },
}

pub struct CastingSimulatorService {
    config: Arc<AppConfig>,
    material: Option<MaterialConfig>,
    command_rx: mpsc::Receiver<CastingCommand>,
    alarm_tx: mpsc::Sender<AlarmCommand>,
}

impl CastingSimulatorService {
    pub fn new(
        config: Arc<AppConfig>,
        command_rx: mpsc::Receiver<CastingCommand>,
        alarm_tx: mpsc::Sender<AlarmCommand>,
    ) -> Self {
        let material = config.material.clone();
        Self {
            config,
            material,
            command_rx,
            alarm_tx,
        }
    }

    pub async fn run(mut self) {
        info!("Casting Simulator service started");
        while let Some(cmd) = self.command_rx.recv().await {
            match cmd {
                CastingCommand::RunSimulation { request, diameter_cm, height_cm, session, result_tx } => {
                    let drum_id = request.drum_id.clone();
                    let result = self.process_simulation(request, diameter_cm, height_cm).await;
                    match result {
                        Ok(sim_result) => {
                            let mut session_out = session.clone();
                            if let Some(s) = session_out.as_mut() {
                                s.last_casting_sim = Some(sim_result.sim_id.clone());
                            }
                            let event = CastingEvent::SimulationComplete {
                                result: sim_result.clone(),
                                session: session_out,
                            };
                            let _ = result_tx.send(Ok(sim_result));
                            let _ = self.alarm_tx.send(AlarmCommand::ProcessCastingEvent { event }).await;
                        }
                        Err(e) => {
                            let event = CastingEvent::SimulationError {
                                drum_id: drum_id.clone(),
                                message: e.clone(),
                            };
                            let _ = result_tx.send(Err(e));
                            let _ = self.alarm_tx.send(AlarmCommand::ProcessCastingEvent { event }).await;
                        }
                    }
                }
                CastingCommand::Shutdown => {
                    info!("Casting Simulator service shutting down");
                    break;
                }
            }
        }
    }

    async fn process_simulation(
        &self,
        req: CastingSimulationRequest,
        diameter_cm: f64,
        height_cm: f64,
    ) -> Result<CastingSimulationResult, String> {
        info!("Processing casting simulation for drum: {}", req.drum_id);

        let material = self.material.as_ref().ok_or_else(|| "Material config not loaded".to_string())?;

        let alloy = req.alloy.unwrap_or_else(AlloyComposition::standard_bronze);
        let pour_temp = req.pour_temperature_c.unwrap_or(1180.0);
        let mold_temp = req.mold_temperature_c.unwrap_or(300.0);
        let cooling_time = req.cooling_time_s.unwrap_or(3600.0);

        let (solidus_temp, liquidus_temp) = self.calc_phase_diagram(
            alloy.tin_pct,
            alloy.lead_pct,
            material,
        );

        let radius_m = (diameter_cm / 2.0) / 100.0;
        let area_m2 = std::f64::consts::PI * radius_m * radius_m;
        let volume_m3 = area_m2 * (height_cm / 100.0) * 0.85;
        let surface_area_m2 = 2.0 * area_m2 + 2.0 * std::f64::consts::PI * radius_m * (height_cm / 100.0) * 0.6;
        let chvorinov_modulus = volume_m3 / surface_area_m2;

        let mut sim_result = CastingSimulationResult {
            sim_id: Uuid::new_v4().to_string(),
            drum_id: req.drum_id.clone(),
            created_at: Utc::now(),
            alloy: alloy.clone(),
            pour_temperature_c: pour_temp,
            mold_temperature_c: mold_temp,
            cooling_time_s: cooling_time,
            solidus_temperature_c: solidus_temp,
            liquidus_temperature_c: liquidus_temp,
            shrinkage_risk_map: Vec::new(),
            cooling_rate_map: Vec::new(),
            defects: Vec::new(),
            quality_score: 0.0,
            overall_risk: "LOW".to_string(),
        };

        self.run_heat_transfer_simulation(
            &mut sim_result,
            chvorinov_modulus,
            material,
        );

        let defects = self.predict_defects(&sim_result, material);
        let (quality, risk) = self.calculate_quality_score(&defects);
        sim_result.defects = defects;
        sim_result.quality_score = quality;
        sim_result.overall_risk = risk;

        info!("Casting simulation {} complete: {} defects, quality={:.2}, risk={}",
            sim_result.sim_id, sim_result.defects.len(), sim_result.quality_score, sim_result.overall_risk);

        Ok(sim_result)
    }

    fn calc_phase_diagram(
        &self,
        tin_pct: f64,
        lead_pct: f64,
        mat: &MaterialConfig,
    ) -> (f64, f64) {
        let pd = &mat.phase_diagram;
        let eutectic_temp = pd.eutectic_temp_c;
        let eutectic_tin = pd.eutectic_tin_pct;

        let tin_effect = if tin_pct < eutectic_tin {
            pd.tin_liquidus_slope * (eutectic_tin - tin_pct)
        } else {
            0.0
        };
        let lead_effect = pd.lead_depression_per_pct * lead_pct;

        let liquidus = 1083.0 - tin_effect - lead_effect;
        let solidus = eutectic_temp + (liquidus - eutectic_temp) * 0.15;

        (solidus.max(750.0), liquidus.max(900.0))
    }

    fn run_heat_transfer_simulation(
        &self,
        result: &mut CastingSimulationResult,
        modulus: f64,
        mat: &MaterialConfig,
    ) {
        let grid_size = mat.grid_refinement.default_size;
        let max_depth = mat.grid_refinement.max_depth;
        let rays = mat.grid_refinement.sun_pattern_rays;

        let ray_angles: Vec<f64> = (0..rays)
            .map(|i| (i as f64) * 2.0 * std::f64::consts::PI / (rays as f64))
            .collect();

        let cells = self.adaptive_quadtree_refine(
            grid_size, max_depth, &ray_angles, modulus, mat,
            result.liquidus_temperature_c, result.solidus_temperature_c,
        );

        let shrink_risk = self.sample_quadtree(&cells, |c| c.4, grid_size);
        let cooling_rate = self.sample_quadtree(&cells, |c| c.5, grid_size);

        result.shrinkage_risk_map = shrink_risk;
        result.cooling_rate_map = cooling_rate;
    }

    fn adaptive_quadtree_refine(
        &self,
        n: usize,
        max_depth: usize,
        ray_angles: &[f64],
        modulus: f64,
        mat: &MaterialConfig,
        liquidus_temp: f64,
        solidus_temp: f64,
    ) -> Vec<(f64, f64, f64, f64, f64, f64)> {
        let mut cells = Vec::new();
        let gr = &mat.grid_refinement;

        let eval_risk = |x_frac: f64, y_frac: f64| -> (f64, f64) {
            let r = ((x_frac - 0.5).powi(2) + (y_frac - 0.5).powi(2)).sqrt() * 2.0;
            if r > 1.0 {
                return (0.0, 0.0);
            }

            let time_constant = mat.chvorinov_constant_sqrt * (modulus * 1e6).powf(mat.n);
            let radial_factor = 1.0 - 0.3 * r;
            let mut solidification_time = time_constant * radial_factor;

            let ray_enhancement: f64 = ray_angles.iter()
                .map(|&theta| {
                    let dx = x_frac - 0.5;
                    let dy = y_frac - 0.5;
                    let ray_dx = theta.cos() * gr.ray_band_radius_frac;
                    let ray_dy = theta.sin() * gr.ray_band_radius_frac;
                    let cross = (dx * ray_dy - dy * ray_dx).abs();
                    let along = dx * ray_dx + dy * ray_dy;
                    if along > 0.0 && along < gr.ray_band_radius_frac && cross < 0.02 {
                        1.5 * (1.0 - cross / 0.02)
                    } else {
                        0.0
                    }
                })
                .sum();

            let boss_factor = if r < gr.boss_inner_radius_frac {
                1.8
            } else {
                0.0
            };

            let halo_factor = if (r - gr.halo_radius_frac).abs() < 0.03 {
                1.2
            } else {
                0.0
            };

            let total_enhancement = 1.0 + ray_enhancement.max(boss_factor).max(halo_factor);
            solidification_time *= total_enhancement;

            let cooling_rate = (liquidus_temp - solidus_temp) / solidification_time;
            let shrinkage_risk = (1.0 - (r * 0.5 + 0.3)) * (cooling_rate / 50.0).sqrt() * total_enhancement;
            let risk = shrinkage_risk.max(0.0).min(1.0);

            (risk, cooling_rate)
        };

        let refine_inner = |cells: &mut Vec<_>, depth: usize| {
            let mut new_cells = Vec::new();
            for cell in cells.drain(..) {
                let (x0, y0, w, h, _, _) = cell;
                if self.cell_needs_refinement(x0, y0, w, h, ray_angles, depth, max_depth, gr) {
                    let hw = w / 2.0;
                    let hh = h / 2.0;
                    for sub in [
                        (x0, y0, hw, hh),
                        (x0 + hw, y0, hw, hh),
                        (x0, y0 + hh, hw, hh),
                        (x0 + hw, y0 + hh, hw, hh),
                    ] {
                        let (risk, cr) = eval_risk(sub.0 + sub.2 / 2.0, sub.1 + sub.3 / 2.0);
                        new_cells.push((sub.0, sub.1, sub.2, sub.3, risk, cr));
                    }
                } else {
                    new_cells.push(cell);
                }
            }
            *cells = new_cells;
        };

        let cell_w = 1.0 / (n as f64);
        let cell_h = 1.0 / (n as f64);
        for i in 0..n {
            for j in 0..n {
                let x0 = (i as f64) * cell_w;
                let y0 = (j as f64) * cell_h;
                let (risk, cr) = eval_risk(x0 + cell_w / 2.0, y0 + cell_h / 2.0);
                cells.push((x0, y0, cell_w, cell_h, risk, cr));
            }
        }

        for depth in 0..max_depth {
            refine_inner(&mut cells, depth);
        }

        cells
    }

    fn cell_needs_refinement(
        &self,
        x0: f64, y0: f64, w: f64, h: f64,
        ray_angles: &[f64], depth: usize, max_depth: usize,
        gr: &crate::config::GridRefinementParams,
    ) -> bool {
        if depth >= max_depth {
            return false;
        }

        let samples = [
            (x0 + w * 0.25, y0 + h * 0.25),
            (x0 + w * 0.75, y0 + h * 0.25),
            (x0 + w * 0.5, y0 + h * 0.5),
            (x0 + w * 0.25, y0 + h * 0.75),
            (x0 + w * 0.75, y0 + h * 0.75),
        ];

        let mut values = Vec::new();
        for &(sx, sy) in &samples {
            let r = ((sx - 0.5).powi(2) + (sy - 0.5).powi(2)).sqrt() * 2.0;
            if r > 1.0 {
                values.push(0.0);
                continue;
            }
            let mut enhancement: f64 = 0.0;
            for &theta in ray_angles {
                let dx = sx - 0.5;
                let dy = sy - 0.5;
                let ray_dx = theta.cos() * gr.ray_band_radius_frac;
                let ray_dy = theta.sin() * gr.ray_band_radius_frac;
                let cross = (dx * ray_dy - dy * ray_dx).abs();
                let along = dx * ray_dx + dy * ray_dy;
                if along > 0.0 && along < gr.ray_band_radius_frac && cross < 0.02 {
                    enhancement = enhancement.max(1.5 * (1.0 - cross / 0.02));
                }
            }
            if r < gr.boss_inner_radius_frac {
                enhancement = enhancement.max(1.8);
            }
            if (r - gr.halo_radius_frac).abs() < 0.03 {
                enhancement = enhancement.max(1.2);
            }
            values.push(enhancement);
        }

        let max_val = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min_val = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let grad = max_val - min_val;

        grad > gr.refinement_threshold
    }

    fn sample_quadtree<F>(
        &self,
        cells: &[(f64, f64, f64, f64, f64, f64)],
        extractor: F,
        grid_size: usize,
    ) -> Vec<(f64, f64, f64)>
    where
        F: Fn(&(f64, f64, f64, f64, f64, f64)) -> f64 + Copy,
    {
        let mut result = Vec::with_capacity(grid_size * grid_size);
        let cell_size = 1.0 / (grid_size as f64);

        for i in 0..grid_size {
            for j in 0..grid_size {
                let x = (i as f64 + 0.5) * cell_size;
                let y = (j as f64 + 0.5) * cell_size;

                let mut best_val = 0.0;
                let mut best_dist = f64::INFINITY;

                for cell in cells {
                    let (cx0, cy0, cw, ch, _, _) = cell;
                    let cx = cx0 + cw / 2.0;
                    let cy = cy0 + ch / 2.0;
                    let dist = (cx - x).powi(2) + (cy - y).powi(2);

                    if x >= *cx0 && x < cx0 + cw && y >= *cy0 && y < cy0 + ch {
                        best_val = extractor(cell);
                        break;
                    }
                    if dist < best_dist {
                        best_dist = dist;
                        best_val = extractor(cell);
                    }
                }

                result.push((x, y, best_val));
            }
        }

        result
    }

    fn predict_defects(
        &self,
        result: &CastingSimulationResult,
        mat: &MaterialConfig,
    ) -> Vec<CastingDefect> {
        let mut defects = Vec::new();

        let superheat = result.pour_temperature_c - result.liquidus_temperature_c;
        if superheat < mat.cold_shut_min_superheat_c {
            defects.push(CastingDefect {
                defect_id: Uuid::new_v4().to_string(),
                defect_type: DefectType::ColdShut,
                zone: "pour_system".to_string(),
                x_frac: 0.5,
                y_frac: 0.5,
                severity: (mat.cold_shut_min_superheat_c - superheat) / mat.cold_shut_min_superheat_c,
                description: format!("过热度仅{:.1}℃，低于要求{:.1}℃，冷隔风险高",
                    superheat, mat.cold_shut_min_superheat_c),
            });
        }

        for &(x, y, risk) in &result.shrinkage_risk_map {
            if risk > self.config.threshold_shrinkage_risk {
                let r = ((x - 0.5).powi(2) + (y - 0.5).powi(2)).sqrt() * 2.0;
                let zone = if r < 0.15 {
                    "center_boss".to_string()
                } else if r < 0.35 {
                    "sun_ray_band".to_string()
                } else if r < 0.7 {
                    "plate_mid".to_string()
                } else {
                    "edge_ring".to_string()
                };

                let (dtype, desc) = if risk > 0.85 {
                    (DefectType::ShrinkageCavity,
                        format!("集中缩孔风险严重，位于{}区域，风险值{:.2}%", zone, risk * 100.0))
                } else {
                    (DefectType::Porosity,
                        format!("缩松风险，位于{}区域，风险值{:.2}%", zone, risk * 100.0))
                };

                defects.push(CastingDefect {
                    defect_id: Uuid::new_v4().to_string(),
                    defect_type: dtype,
                    zone,
                    x_frac: x,
                    y_frac: y,
                    severity: risk,
                    description: desc,
                });
            }
        }

        for &(_, _, cr) in &result.cooling_rate_map {
            if cr > mat.hot_tear_threshold_cooling_rate_ks && result.mold_temperature_c < 200.0 {
                defects.push(CastingDefect {
                    defect_id: Uuid::new_v4().to_string(),
                    defect_type: DefectType::HotTear,
                    zone: "edge_ring".to_string(),
                    x_frac: 0.9,
                    y_frac: 0.5,
                    severity: 0.7,
                    description: format!("冷却速率{:.1}℃/s过高且模温仅{:.1}℃，热裂风险存在",
                        cr, result.mold_temperature_c),
                });
                break;
            }
        }

        defects
    }

    fn calculate_quality_score(
        &self,
        defects: &[CastingDefect],
    ) -> (f64, String) {
        let mut penalty = 0.0;
        let mut has_critical = false;

        for d in defects {
            let weight = match d.defect_type {
                DefectType::ShrinkageCavity => 0.4,
                DefectType::HotTear => 0.3,
                DefectType::ColdShut => 0.25,
                DefectType::Porosity => 0.15,
                _ => 0.1,
            };
            penalty += weight * d.severity;
            if d.severity > 0.85 {
                has_critical = true;
            }
        }

        let quality = (1.0 - penalty).max(0.0).min(1.0);
        let risk = if has_critical || quality < 0.4 {
            "CRITICAL".to_string()
        } else if quality < 0.6 {
            "HIGH".to_string()
        } else if quality < 0.8 {
            "MEDIUM".to_string()
        } else {
            "LOW".to_string()
        };

        (quality, risk)
    }
}
