use crate::models::*;
use chrono::Utc;
use rand::Rng;
use std::f64::consts::PI;
use tracing::{debug, info, warn};
use uuid::Uuid;

pub struct CastingSimulator;

impl CastingSimulator {
    pub fn simulate(
        drum_id: String,
        drum_diameter_cm: f64,
        drum_height_cm: f64,
        req: &CastingSimulationRequest,
    ) -> CastingSimulationResult {
        info!("Running casting simulation for drum: {}", drum_id);

        let alloy = req.alloy.clone().unwrap_or_else(AlloyComposition::standard_bronze);
        let pour_temp = req.pour_temperature_c.unwrap_or(1180.0);
        let mold_temp = req.mold_temperature_c.unwrap_or(300.0);
        let cooling_time = req.cooling_time_s.unwrap_or(3600.0);
        let resolution = req.mesh_resolution.unwrap_or(32);

        let (solidus_temp, liquidus_temp) = Self::calc_phase_diagram(&alloy);

        let (shrinkage_risk_map, cooling_rate_map) =
            Self::run_heat_transfer_simulation(
                drum_diameter_cm,
                drum_height_cm,
                &alloy,
                pour_temp,
                mold_temp,
                cooling_time,
                solidus_temp,
                liquidus_temp,
                resolution,
            );

        let defects = Self::predict_defects(
            &shrinkage_risk_map,
            &cooling_rate_map,
            solidus_temp,
            liquidus_temp,
            pour_temp,
            mold_temp,
            drum_diameter_cm,
        );

        let quality_score = Self::calculate_quality_score(&defects, &shrinkage_risk_map);

        let overall_risk = if quality_score >= 0.85 {
            "LOW".to_string()
        } else if quality_score >= 0.65 {
            "MEDIUM".to_string()
        } else if quality_score >= 0.4 {
            "HIGH".to_string()
        } else {
            "CRITICAL".to_string()
        };

        CastingSimulationResult {
            sim_id: Uuid::new_v4().to_string(),
            drum_id,
            created_at: Utc::now(),
            alloy,
            pour_temperature_c: pour_temp,
            mold_temperature_c: mold_temp,
            cooling_time_s: cooling_time,
            solidus_temperature_c: solidus_temp,
            liquidus_temperature_c: liquidus_temp,
            shrinkage_risk_map,
            cooling_rate_map,
            defects,
            quality_score,
            overall_risk,
        }
    }

    fn calc_phase_diagram(alloy: &AlloyComposition) -> (f64, f64) {
        let cu_sn = alloy.tin_pct;
        let cu_pb = alloy.lead_pct;
        let cu_zn = alloy.zinc_pct;

        let base_liquidus = 1083.0;
        let base_solidus = 1020.0;

        let liquidus = base_liquidus
            - cu_sn * 8.5
            - cu_pb * 2.3
            - cu_zn * 5.0
            + (cu_sn * cu_sn) * 0.05;

        let solidus = base_solidus
            - cu_sn * 12.0
            - cu_pb * 3.5
            - cu_zn * 7.0
            + (cu_sn * cu_sn) * 0.08;

        (solidus.max(700.0), liquidus.max(800.0))
    }

    fn run_heat_transfer_simulation(
        diameter_cm: f64,
        height_cm: f64,
        alloy: &AlloyComposition,
        pour_temp: f64,
        mold_temp: f64,
        total_time_s: f64,
        solidus_temp: f64,
        liquidus_temp: f64,
        resolution: usize,
    ) -> (Vec<(f64, f64, f64)>, Vec<(f64, f64, f64)>) {
        let alpha = Self::calculate_thermal_diffusivity(alloy);
        let latent_heat_factor = 1.0 + Self::calculate_latent_heat_contribution(alloy);

        let radius_cm = diameter_cm / 2.0;
        let aspect_ratio = height_cm / diameter_cm;

        let sun_ray_angles: Vec<f64> = (0..12)
            .map(|i| (i as f64) / 12.0 * 2.0 * PI)
            .collect();

        let eval_risk = |x_frac: f64, y_frac: f64| -> (f64, f64) {
            let r_norm = ((x_frac - 0.5).powi(2) + (y_frac - 0.5).powi(2)).sqrt() * 2.0;
            let r_norm = r_norm.min(1.0);

            let center_factor = 1.0 - r_norm.powi(2);

            let edge_distance = (1.0 - r_norm).max(0.01);
            let modulus = (diameter_cm * 0.01 * height_cm * 0.01)
                / (2.0 * PI * radius_cm * 0.01 * (edge_distance + 0.05));

            let chvorinov_time = modulus.powi(2) / alpha * 1.2;
            let normalized_time = total_time_s / (chvorinov_time + 1.0);

            let thermal_grad = (pour_temp - mold_temp) / (edge_distance * 100.0 + 5.0);

            let solidification_time = (1.0 + (1.0 - center_factor) * 2.0) * chvorinov_time
                * latent_heat_factor;

            let cooling_r = (pour_temp - solidus_temp) / (solidification_time + 1.0)
                * (1.0 + (1.0 - edge_distance) * 0.5);

            let time_pressure = if normalized_time > 1.5 { 0.0 } else { 1.0 - normalized_time / 1.5 };

            let feed_distance = Self::calculate_feeding_distance(alloy, thermal_grad, cooling_r);
            let feed_ratio = (edge_distance * radius_cm) / (feed_distance + 0.1);

            let macroporosity_risk =
                (1.0 - thermal_grad / 200.0).max(0.0) * time_pressure * (1.0 - feed_ratio.min(1.0));

            let microporosity_risk =
                (cooling_r / 50.0).min(1.0) * center_factor * (1.0 + (alloy.tin_pct - 15.0) * 0.02);

            let hot_tear_factor = if cooling_r > 80.0 {
                (cooling_r - 80.0) / 100.0 * (1.0 + (alloy.tin_pct / 100.0) * 2.0)
            } else {
                0.0
            };

            let total_risk = (macroporosity_risk * 0.5
                + microporosity_risk * 0.35
                + hot_tear_factor * 0.15)
                .min(1.0);

            let wall_geometry_factor = Self::wall_thickness_geometry_factor(
                x_frac,
                y_frac,
                aspect_ratio,
            );

            let sun_pattern_factor = Self::sun_pattern_refinement_factor(
                x_frac, y_frac, &sun_ray_angles,
            );

            let final_risk = (total_risk * wall_geometry_factor * sun_pattern_factor).min(1.0);

            (final_risk, cooling_r)
        };

        let fine_grid = Self::adaptive_quadtree_refine(
            0.0, 0.0, 1.0, 1.0,
            &sun_ray_angles,
            0,
            3,
            &eval_risk,
        );

        let mut shrinkage_risk = Vec::with_capacity(resolution * resolution);
        let mut cooling_rate = Vec::with_capacity(resolution * resolution);

        for i in 0..resolution {
            for j in 0..resolution {
                let x_frac = (i as f64 + 0.5) / resolution as f64;
                let y_frac = (j as f64 + 0.5) / resolution as f64;

                let (risk, cool) = Self::sample_quadtree(&fine_grid, x_frac, y_frac);

                shrinkage_risk.push((x_frac, y_frac, risk));
                cooling_rate.push((x_frac, y_frac, cool));
            }
        }

        (shrinkage_risk, cooling_rate)
    }

    fn sun_pattern_refinement_factor(
        x_frac: f64,
        y_frac: f64,
        ray_angles: &[f64],
    ) -> f64 {
        let dx = x_frac - 0.5;
        let dy = y_frac - 0.5;
        let r = (dx * dx + dy * dy).sqrt() * 2.0;

        if r > 0.45 {
            return 1.0;
        }

        let mut ray_enhancement: f64 = 0.0;
        let theta = dy.atan2(dx);

        for &ray_angle in ray_angles {
            let angle_diff = ((theta - ray_angle).sin()).abs();
            let radial_dist = (r - 0.22).abs();
            let ray_intensity = (-radial_dist / 0.06).exp()
                * (-angle_diff * 8.0).exp();
            ray_enhancement = ray_enhancement.max(ray_intensity);
        }

        let boss_enhancement = if r < 0.12 {
            (-r / 0.04).exp() * 0.8
        } else {
            0.0
        };

        let ring_enhancement = ((r - 0.32).abs() / 0.03).exp().recip() * 0.4;

        1.0 + (ray_enhancement * 0.7 + boss_enhancement * 0.9 + ring_enhancement * 0.5)
    }

    fn adaptive_quadtree_refine(
        x0: f64,
        y0: f64,
        w: f64,
        h: f64,
        ray_angles: &[f64],
        depth: usize,
        max_depth: usize,
        eval_fn: &dyn Fn(f64, f64) -> (f64, f64),
    ) -> Vec<(f64, f64, f64, f64, f64, f64)> {
        let cx = x0 + w / 2.0;
        let cy = y0 + h / 2.0;

        let need_refine = if depth >= max_depth {
            false
        } else {
            Self::cell_needs_refinement(x0, y0, w, h, ray_angles, depth)
        };

        if need_refine {
            let hw = w / 2.0;
            let hh = h / 2.0;
            let mut cells = Vec::new();

            let sub_cells = [
                (x0, y0),
                (x0 + hw, y0),
                (x0, y0 + hh),
                (x0 + hw, y0 + hh),
            ];

            for (sx, sy) in sub_cells {
                let sub = Self::adaptive_quadtree_refine(
                    sx, sy, hw, hh,
                    ray_angles,
                    depth + 1,
                    max_depth,
                    eval_fn,
                );
                cells.extend(sub);
            }
            cells
        } else {
            let (risk, cool) = eval_fn(cx, cy);
            vec![(x0, y0, w, h, risk, cool)]
        }
    }

    fn cell_needs_refinement(
        x0: f64,
        y0: f64,
        w: f64,
        h: f64,
        ray_angles: &[f64],
        depth: usize,
    ) -> bool {
        let sample_pts = [
            (x0 + w * 0.25, y0 + h * 0.25),
            (x0 + w * 0.75, y0 + h * 0.25),
            (x0 + w * 0.25, y0 + h * 0.75),
            (x0 + w * 0.75, y0 + h * 0.75),
            (x0 + w * 0.5, y0 + h * 0.5),
        ];

        let mut factors = [0.0; 5];
        for (i, (x, y)) in sample_pts.iter().enumerate() {
            factors[i] = Self::sun_pattern_refinement_factor(*x, *y, ray_angles);
        }

        let max_f = factors.iter().cloned().fold(f64::NAN, f64::max);
        let min_f = factors.iter().cloned().fold(f64::NAN, f64::min);
        let gradient = (max_f - min_f) / max_f.max(0.01);

        if depth < 1 {
            true
        } else if depth < 2 {
            gradient > 0.05
        } else {
            gradient > 0.1
        }
    }

    fn sample_quadtree(
        cells: &[(f64, f64, f64, f64, f64, f64)],
        x: f64,
        y: f64,
    ) -> (f64, f64) {
        for &(cx, cy, cw, ch, risk, cool) in cells {
            if x >= cx && x < cx + cw && y >= cy && y < cy + ch {
                return (risk, cool);
            }
        }

        let mut nearest_risk = 0.0;
        let mut nearest_cool = 0.0;
        let mut min_dist = f64::MAX;
        for &(cx, cy, cw, ch, risk, cool) in cells {
            let ccx = cx + cw / 2.0;
            let ccy = cy + ch / 2.0;
            let d = (x - ccx).powi(2) + (y - ccy).powi(2);
            if d < min_dist {
                min_dist = d;
                nearest_risk = risk;
                nearest_cool = cool;
            }
        }
        (nearest_risk, nearest_cool)
    }

    fn calculate_thermal_diffusivity(alloy: &AlloyComposition) -> f64 {
        let k_copper = 401.0;
        let k_tin = 66.6;
        let k_lead = 35.3;
        let k_zinc = 116.0;

        let cp_copper = 385.0;
        let rho_copper = 8960.0;
        let cp_avg = 380.0;
        let rho_avg = rho_copper * (alloy.copper_pct / 100.0)
            + 7310.0 * (alloy.tin_pct / 100.0)
            + 11340.0 * (alloy.lead_pct / 100.0)
            + 7140.0 * (alloy.zinc_pct / 100.0);

        let k_avg = k_copper * (alloy.copper_pct / 100.0)
            + k_tin * (alloy.tin_pct / 100.0)
            + k_lead * (alloy.lead_pct / 100.0)
            + k_zinc * (alloy.zinc_pct / 100.0);

        k_avg / (rho_avg * cp_avg) * 1e6
    }

    fn calculate_latent_heat_contribution(alloy: &AlloyComposition) -> f64 {
        let l_copper = 205.0;
        let l_tin = 59.0;
        let weighted_l = l_copper * (alloy.copper_pct / 100.0) + l_tin * (alloy.tin_pct / 100.0);
        weighted_l / 380.0 / 200.0
    }

    fn calculate_feeding_distance(alloy: &AlloyComposition, grad: f64, cooling_rate: f64) -> f64 {
        let base_feed = 4.5;
        let tin_correction = 1.0 + (alloy.tin_pct - 15.0).min(10.0) * 0.01;
        let grad_factor = (grad / 100.0 + 0.5).max(0.3);
        let cooling_factor = (1.0 - cooling_rate / 200.0).max(0.4);
        base_feed * tin_correction * grad_factor * cooling_factor
    }

    fn wall_thickness_geometry_factor(x_frac: f64, y_frac: f64, aspect_ratio: f64) -> f64 {
        let cx = 0.5;
        let cy = 0.5;
        let dx = x_frac - cx;
        let dy = (y_frac - cy) / aspect_ratio.max(0.5);
        let dist = (dx * dx + dy * dy).sqrt();
        let ring_1 = (0.05f64 - (dist - 0.1).abs()).max(0.0) / 0.05;
        let ring_2 = (0.08f64 - (dist - 0.4).abs()).max(0.0) / 0.08;
        let boss = (0.1f64 - dist).max(0.0) / 0.1;
        1.0 + (ring_1 * 0.6 + ring_2 * 0.4 + boss * 0.3)
    }

    fn predict_defects(
        shrinkage: &Vec<(f64, f64, f64)>,
        cooling: &Vec<(f64, f64, f64)>,
        solidus: f64,
        liquidus: f64,
        pour: f64,
        mold: f64,
        diameter_cm: f64,
    ) -> Vec<CastingDefect> {
        let mut rng = rand::thread_rng();
        let mut defects = Vec::new();

        let sorted_shrinkage: Vec<&(f64, f64, f64)> = {
            let mut v: Vec<&(f64, f64, f64)> = shrinkage.iter().collect();
            v.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap());
            v
        };

        for pt in sorted_shrinkage.iter().take(10) {
            let risk = pt.2;
            if risk < 0.3 {
                continue;
            }

            let (defect_type, description, severity_adj) = if risk > 0.8 {
                (
                    DefectType::ShrinkageCavity,
                    "大型集中缩孔，需设置冒口补偿".to_string(),
                    1.0,
                )
            } else if risk > 0.6 {
                (
                    DefectType::Porosity,
                    "分散性缩松缺陷，降低致密度和声品质".to_string(),
                    0.7,
                )
            } else if risk > 0.45 {
                (
                    DefectType::HotTear,
                    "热裂纹风险区，合金凝固收缩受约束".to_string(),
                    0.6,
                )
            } else {
                (
                    DefectType::Porosity,
                    "微小气孔聚集区".to_string(),
                    0.4,
                )
            };

            let zone = Self::classify_zone(pt.0, pt.1);
            let severity = (risk * severity_adj).min(1.0);

            defects.push(CastingDefect {
                defect_id: Uuid::new_v4().to_string(),
                defect_type,
                zone,
                x_frac: pt.0,
                y_frac: pt.1,
                severity,
                description,
            });
        }

        let superheat = pour - liquidus;
        if superheat < 30.0 {
            let severity = ((30.0 - superheat) / 30.0).min(1.0);
            defects.push(CastingDefect {
                defect_id: Uuid::new_v4().to_string(),
                defect_type: DefectType::ColdShut,
                zone: "鼓面边缘".to_string(),
                x_frac: 0.95,
                y_frac: 0.5,
                severity: severity * 0.8,
                description: format!("过热度不足({:.1}°C)，可能产生冷隔或浇不足", superheat),
            });
        }

        let avg_cooling: f64 = cooling.iter().map(|c| c.2).sum::<f64>() / cooling.len() as f64;
        if avg_cooling > 150.0 && mold < 200.0 {
            defects.push(CastingDefect {
                defect_id: Uuid::new_v4().to_string(),
                defect_type: DefectType::HotTear,
                zone: "鼓腰过渡区".to_string(),
                x_frac: 0.5,
                y_frac: 0.7,
                severity: ((avg_cooling - 150.0) / 100.0).min(0.9),
                description: "模温过低+冷却速率过大，过渡区热应力致热裂风险".to_string(),
            });
        }

        if diameter_cm > 60.0 && defects.iter().filter(|d| matches!(d.defect_type, DefectType::ShrinkageCavity)).count() == 0 {
            if rng.gen::<f64>() < 0.3 {
                defects.push(CastingDefect {
                    defect_id: Uuid::new_v4().to_string(),
                    defect_type: DefectType::IncompleteFilling,
                    zone: "耳部/纹饰区".to_string(),
                    x_frac: rng.gen_range(0.1..0.9),
                    y_frac: rng.gen_range(0.1..0.9),
                    severity: rng.gen_range(0.3..0.6),
                    description: "大尺寸铜鼓复杂纹饰区充型不足".to_string(),
                });
            }
        }

        defects.sort_by(|a, b| b.severity.partial_cmp(&a.severity).unwrap());
        defects
    }

    fn classify_zone(x: f64, y: f64) -> String {
        let cx = 0.5;
        let cy = 0.5;
        let dist = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() * 2.0;

        if dist < 0.15 {
            "鼓心/太阳纹区".to_string()
        } else if dist < 0.4 {
            "主晕圈/羽人纹区".to_string()
        } else if dist < 0.7 {
            "鼓面外圈/立蛙区".to_string()
        } else if dist < 0.9 {
            "鼓腰/胴部".to_string()
        } else {
            "鼓足/底部边缘".to_string()
        }
    }

    fn calculate_quality_score(
        defects: &Vec<CastingDefect>,
        shrinkage: &Vec<(f64, f64, f64)>,
    ) -> f64 {
        let avg_risk = shrinkage.iter().map(|s| s.2).sum::<f64>() / shrinkage.len() as f64;

        let severity_penalty: f64 = defects
            .iter()
            .map(|d| match d.defect_type {
                DefectType::ShrinkageCavity => d.severity * 0.35,
                DefectType::HotTear => d.severity * 0.3,
                DefectType::ColdShut => d.severity * 0.2,
                DefectType::IncompleteFilling => d.severity * 0.25,
                DefectType::Porosity => d.severity * 0.15,
            })
            .sum();

        let count_penalty = (defects.len() as f64 * 0.02).min(0.15);

        let risk_penalty = avg_risk * 0.25;

        (1.0 - severity_penalty - count_penalty - risk_penalty).max(0.0).min(1.0)
    }
}
