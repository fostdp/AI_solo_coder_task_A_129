use crate::config::{AppConfig, AcousticsConfig, MaterialConfig};
use crate::models::{
    AcousticAnalysisRequest, AcousticAnalysisResult, VibrationMode, SoundFieldPoint,
    DrumSession, ThicknessPoint, SpectrumBin,
};
use crate::alarm_mqtt::AlarmCommand;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use chrono::Utc;

#[derive(Debug)]
pub enum AcousticsCommand {
    RunAnalysis {
        request: AcousticAnalysisRequest,
        diameter_cm: f64,
        height_cm: f64,
        mass_kg: f64,
        wall_thickness: Option<Vec<ThicknessPoint>>,
        session: Option<DrumSession>,
        result_tx: oneshot::Sender<Result<AcousticAnalysisResult, String>>,
    },
    CheckFrequencyDeviation {
        spectrum: Vec<SpectrumBin>,
        reference_hz: Vec<f64>,
        drum_id: String,
        threshold_hz: f64,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum AcousticsEvent {
    AnalysisComplete {
        result: AcousticAnalysisResult,
        session: Option<DrumSession>,
    },
    AnalysisError {
        drum_id: String,
        message: String,
    },
    FrequencyDeviationResult {
        drum_id: String,
        deviations: Vec<(f64, f64, f64)>,
    },
}

pub struct AcousticAnalyzerService {
    config: Arc<AppConfig>,
    acoustics: Option<AcousticsConfig>,
    material: Option<MaterialConfig>,
    command_rx: mpsc::Receiver<AcousticsCommand>,
    alarm_tx: mpsc::Sender<AlarmCommand>,
}

impl AcousticAnalyzerService {
    pub fn new(
        config: Arc<AppConfig>,
        command_rx: mpsc::Receiver<AcousticsCommand>,
        alarm_tx: mpsc::Sender<AlarmCommand>,
    ) -> Self {
        let acoustics = config.acoustics.clone();
        let material = config.material.clone();
        Self {
            config,
            acoustics,
            material,
            command_rx,
            alarm_tx,
        }
    }

    pub async fn run(mut self) {
        info!("Acoustic Analyzer service started");
        while let Some(cmd) = self.command_rx.recv().await {
            match cmd {
                AcousticsCommand::RunAnalysis {
                    request, diameter_cm, height_cm, mass_kg, wall_thickness, session, result_tx,
                } => {
                    let drum_id = request.drum_id.clone();
                    let result = self.process_analysis(
                        &request, diameter_cm, height_cm, mass_kg, wall_thickness,
                    ).await;
                    match result {
                        Ok(analysis_result) => {
                            let mut session_out = session.clone();
                            if let Some(s) = session_out.as_mut() {
                                s.last_acoustic_analysis = Some(analysis_result.analysis_id.clone());
                            }
                            let event = AcousticsEvent::AnalysisComplete {
                                result: analysis_result.clone(),
                                session: session_out,
                            };
                            let _ = result_tx.send(Ok(analysis_result));
                            let _ = self.alarm_tx.send(AlarmCommand::ProcessAcousticsEvent { event }).await;
                        }
                        Err(e) => {
                            let event = AcousticsEvent::AnalysisError {
                                drum_id: drum_id.clone(),
                                message: e.clone(),
                            };
                            let _ = result_tx.send(Err(e));
                            let _ = self.alarm_tx.send(AlarmCommand::ProcessAcousticsEvent { event }).await;
                        }
                    }
                }
                AcousticsCommand::CheckFrequencyDeviation {
                    spectrum, reference_hz, drum_id, threshold_hz,
                } => {
                    let deviations = Self::check_frequency_deviation(&spectrum, &reference_hz, threshold_hz);
                    if !deviations.is_empty() {
                        let _ = self.alarm_tx.send(AlarmCommand::EvaluateFrequencyDeviation {
                            drum_id,
                            deviations,
                        }).await;
                    }
                }
                AcousticsCommand::Shutdown => {
                    info!("Acoustic Analyzer service shutting down");
                    break;
                }
            }
        }
    }

    async fn process_analysis(
        &self,
        req: &AcousticAnalysisRequest,
        diameter_cm: f64,
        _height_cm: f64,
        mass_kg: f64,
        wall_thickness: Option<Vec<ThicknessPoint>>,
    ) -> Result<AcousticAnalysisResult, String> {
        info!("Processing acoustic analysis for drum: {}", req.drum_id);

        let ac = self.acoustics.as_ref().ok_or_else(|| "Acoustics config not loaded".to_string())?;
        let mat = self.material.as_ref().ok_or_else(|| "Material config not loaded".to_string())?;

        let a = (diameter_cm / 2.0) / 100.0;
        let h = self.get_effective_thickness(mass_kg, a, &wall_thickness, mat);

        let mut result = AcousticAnalysisResult {
            analysis_id: Uuid::new_v4().to_string(),
            drum_id: req.drum_id.clone(),
            created_at: Utc::now(),
            youngs_modulus_pa: mat.youngs_modulus_pa,
            poissons_ratio: mat.poissons_ratio,
            density_kgm3: mat.density_kgm3,
            sound_speed_air_ms: ac.air_sound_speed_ms,
            air_density_kgm3: ac.air_density_kgm3,
            vibration_modes: Vec::new(),
            radiated_sound_power_w: 0.0,
            resonance_frequencies_hz: Vec::new(),
            sound_field: Vec::new(),
            sound_quality_metric: 0.0,
        };

        let modes = self.compute_vibration_modes(a, h, mat, ac)?;
        result.vibration_modes = modes;
        result.resonance_frequencies_hz = result.vibration_modes
            .iter()
            .map(|m| m.nonlinear_frequency_hz)
            .collect();

        let (sound_field, radiated_power) = self.compute_sound_radiation(
            a, &result.vibration_modes, ac,
        );
        result.sound_field = sound_field;
        result.radiated_sound_power_w = radiated_power;

        result.sound_quality_metric = self.evaluate_sound_quality(&result, ac);

        info!("Acoustic analysis {} complete: {} modes, quality={:.2}",
            result.analysis_id, result.vibration_modes.len(), result.sound_quality_metric);

        Ok(result)
    }

    fn get_effective_thickness(
        &self,
        mass_kg: f64,
        a: f64,
        wall_thickness: &Option<Vec<ThicknessPoint>>,
        mat: &MaterialConfig,
    ) -> f64 {
        if let Some(wt) = wall_thickness {
            if !wt.is_empty() {
                let sum: f64 = wt.iter().map(|t| t.thickness_mm).sum();
                let avg_mm = sum / wt.len() as f64;
                return avg_mm / 1000.0;
            }
        }
        let area = std::f64::consts::PI * a * a;
        let volume = mass_kg / mat.density_kgm3;
        volume / (area * 0.85)
    }

    fn compute_vibration_modes(
        &self,
        a: f64, h: f64,
        mat: &MaterialConfig,
        ac: &AcousticsConfig,
    ) -> Result<Vec<VibrationMode>, String> {
        let e = mat.youngs_modulus_pa;
        let nu = mat.poissons_ratio;
        let rho = mat.density_kgm3;
        let d = e * h * h * h / (12.0 * (1.0 - nu * nu));

        let mut modes: Vec<VibrationMode> = ac.eigenvalues_lambda
            .iter()
            .take(ac.mode_count)
            .map(|entry| {
                let lambda = entry.lambda;
                let frequency = (lambda * lambda / (2.0 * std::f64::consts::PI * a * a))
                    * (d / (rho * h)).sqrt();

                let damping = 0.002 + 0.008 * (entry.n as f64) / 6.0;
                let pattern = format!("({}, {})", entry.m, entry.n);
                let displacements = self.build_mode_shape(entry.m, entry.n, lambda, a, 24);
                let force_amp = 1.0 / (entry.n as f64);

                VibrationMode {
                    mode_order: entry.n * 7 + entry.m + 1,
                    frequency_hz: frequency,
                    nonlinear_frequency_hz: frequency,
                    damping_ratio: damping,
                    node_pattern: pattern,
                    modal_displacements: displacements,
                    effective_force_amplitude: force_amp,
                    arc_length_converged: false,
                }
            })
            .collect();

        modes.sort_by(|a, b| a.frequency_hz.partial_cmp(&b.frequency_hz).unwrap_or(std::cmp::Ordering::Equal));

        let nl_params = &ac.nonlinear.arc_length_method;
        for mode in modes.iter_mut() {
            let (nl_freq, converged, _eff_amp) = self.riks_arc_length_solve(
                mode, a, h, e, nu, rho, d, nl_params,
            );
            mode.nonlinear_frequency_hz = nl_freq;
            mode.arc_length_converged = converged;
        }

        Ok(modes)
    }

    fn build_mode_shape(
        &self,
        m: usize, n: usize, lambda: f64, a: f64, grid_size: usize,
    ) -> Vec<(f64, f64, f64)> {
        let mut result = Vec::with_capacity(grid_size * grid_size);
        let step = 2.0 / (grid_size as f64 - 1.0);

        for i in 0..grid_size {
            for j in 0..grid_size {
                let x = -1.0 + (i as f64) * step;
                let y = -1.0 + (j as f64) * step;
                let r = (x * x + y * y).sqrt();
                let theta = y.atan2(x);

                let w = if r > 1.0 {
                    0.0
                } else {
                    let rho_norm = lambda * r / a;
                    let jm = Self::bessel_j(m as i32, rho_norm);
                    let im = Self::bessel_i(m as i32, rho_norm);
                    let jm_lambda = Self::bessel_j(m as i32, lambda);
                    let im_lambda = Self::bessel_i(m as i32, lambda);

                    let sign = if m % 2 == 0 { 1.0 } else { -1.0 };
                    let bending = jm + sign * im * jm_lambda / im_lambda;
                    let angular = (m as f64 * theta).cos();
                    bending * angular
                };

                let w_norm = (w / 1.5).max(-1.0).min(1.0);
                result.push((x, y, w_norm));
            }
        }

        result
    }

    fn riks_arc_length_solve(
        &self,
        mode: &VibrationMode,
        a: f64, h: f64,
        e: f64, nu: f64, rho: f64, d: f64,
        params: &crate::config::ArcLengthParams,
    ) -> (f64, bool, f64) {
        let n = mode.modal_displacements.len();
        let mut displ: Vec<f64> = vec![0.0; n];
        let force_ref: Vec<f64> = mode.modal_displacements.iter()
            .map(|&(_, _, w)| w)
            .collect();

        let mut lambda = 0.0;
        let delta_s = params.initial_delta_s;
        let alpha = params.arc_length_scaling_alpha;
        let mut converged_steps = 0;
        let mut effective_force = 0.0;

        for _step in 0..params.max_load_steps {
            let k_t = self.tangent_stiffness(&displ, &force_ref, a, h, e, nu, d, n);
            let residual = self.compute_residual(&displ, &force_ref, lambda, a, h, e, nu, d, n);

            let mut converged = false;
            for _iter in 0..params.max_iterations_per_step {
                let res_norm: f64 = residual.iter().map(|&r| r * r).sum::<f64>().sqrt();
                if res_norm < params.tolerance {
                    converged = true;
                    break;
                }

                let delta_u_t = self.solve_linear_system(&k_t, &residual, n);
                let delta_u_z = self.solve_linear_system(&k_t, &force_ref, n);

                let du_dot = delta_u_t.iter().zip(delta_u_z.iter())
                    .map(|(&a, &b)| a * b).sum::<f64>();
                let uu_dot = delta_u_t.iter().map(|&v| v * v).sum::<f64>();
                let ff_dot = force_ref.iter().map(|&v| v * v).sum::<f64>();

                let discriminant = du_dot * du_dot - uu_dot * (uu_dot + alpha * alpha * ff_dot
                    - delta_s * delta_s);

                if discriminant < 0.0 {
                    break;
                }

                let disc_sqrt = discriminant.sqrt();
                let denom = uu_dot + alpha * alpha * ff_dot;
                let dl1 = (-du_dot + disc_sqrt) / denom;
                let dl2 = (-du_dot - disc_sqrt) / denom;

                let delta_lambda = if dl1.abs() < dl2.abs() { dl1 } else { dl2 };

                for i in 0..n {
                    displ[i] += delta_u_t[i] + delta_lambda * delta_u_z[i];
                }
                lambda += delta_lambda;
            }

            if converged {
                converged_steps += 1;
                effective_force = lambda;
            } else {
                break;
            }
        }

        let k_eff = self.effective_modal_stiffness(&displ, &mode.modal_displacements, n);
        let nonlinear_freq = if converged_steps >= 3 {
            (k_eff / (rho * h)).sqrt() / (2.0 * std::f64::consts::PI)
        } else {
            mode.frequency_hz
        };

        (nonlinear_freq, converged_steps >= 3, effective_force)
    }

    fn tangent_stiffness(
        &self,
        displ: &[f64], force_ref: &[f64],
        a: f64, h: f64, e: f64, nu: f64, d: f64, n: usize,
    ) -> Vec<Vec<f64>> {
        let mut k = vec![vec![0.0; n]; n];
        let base_k = d / (a * a * a * a);

        for i in 0..n {
            k[i][i] = base_k * (1.0 + nu) * 2.0;
        }

        let max_disp = displ.iter()
            .map(|&v| v.abs())
            .fold(f64::NEG_INFINITY, f64::max);

        if max_disp > 0.01 * h {
            let nonlinear_factor = e * h / (1.0 - nu * nu);
            for i in 0..n {
                let w = displ[i] / h;
                let f = force_ref[i];
                let nl_stiff = nonlinear_factor * w * w * 3.0;
                k[i][i] += nl_stiff * f.abs();
            }
        }

        k
    }

    fn compute_residual(
        &self,
        displ: &[f64], force_ref: &[f64], lambda: f64,
        _a: f64, _h: f64, _e: f64, _nu: f64, d: f64, n: usize,
    ) -> Vec<f64> {
        let mut r = vec![0.0; n];
        let k_linear = d * 1.0e8;

        for i in 0..n {
            r[i] = k_linear * displ[i] - lambda * force_ref[i];
        }

        r
    }

    fn solve_linear_system(
        &self,
        a: &[Vec<f64>], b: &[f64], n: usize,
    ) -> Vec<f64> {
        let mut aug = vec![vec![0.0; n + 1]; n];
        for i in 0..n {
            for j in 0..n {
                aug[i][j] = a[i][j];
            }
            aug[i][n] = b[i];
        }

        for col in 0..n {
            let mut max_row = col;
            let mut max_val = aug[col][col].abs();
            for row in col + 1..n {
                if aug[row][col].abs() > max_val {
                    max_val = aug[row][col].abs();
                    max_row = row;
                }
            }
            if max_row != col {
                aug.swap(col, max_row);
            }

            let pivot = aug[col][col];
            if pivot.abs() < 1e-10 {
                for i in 0..n {
                    aug[i][n] = if i == col { 0.0 } else { b[i] };
                }
                return aug.iter().map(|row| row[n]).collect();
            }

            for row in col + 1..n {
                let factor = aug[row][col] / pivot;
                for j in col..=n {
                    aug[row][j] -= factor * aug[col][j];
                }
            }
        }

        let mut x = vec![0.0; n];
        for i in (0..n).rev() {
            let mut sum = aug[i][n];
            for j in i + 1..n {
                sum -= aug[i][j] * x[j];
            }
            x[i] = sum / aug[i][i];
        }

        x
    }

    fn effective_modal_stiffness(
        &self,
        displ: &[f64], mode_shape: &[(f64, f64, f64)], n: usize,
    ) -> f64 {
        let mut num = 0.0;
        let mut den = 0.0;
        for i in 0..n {
            let phi = mode_shape[i].2;
            num += displ[i] * phi;
            den += phi * phi;
        }
        if den > 1e-10 {
            num / den * 1e8
        } else {
            1e8
        }
    }

    fn compute_sound_radiation(
        &self,
        a: f64,
        modes: &[VibrationMode],
        ac: &AcousticsConfig,
    ) -> (Vec<SoundFieldPoint>, f64) {
        let res = ac.sound_radiation.grid_resolution;
        let r_obs = ac.sound_radiation.field_hemisphere_radius_m;
        let c0 = ac.air_sound_speed_ms;
        let rho0 = ac.air_density_kgm3;
        let p_ref = ac.sound_radiation.reference_pascals_pa;

        let mut field = Vec::with_capacity(res * res);
        let mut total_power = 0.0;

        let mode_weight = |f: f64| {
            let ka = 2.0 * std::f64::consts::PI * f * a / c0;
            if ka < 0.5 {
                (ka * ka) * 0.8
            } else if ka < 5.0 {
                1.0 - (-ka).exp()
            } else {
                1.0
            }
        };

        for i in 0..res {
            for j in 0..res {
                let theta = std::f64::consts::PI * 0.5 * (i as f64) / ((res - 1) as f64);
                let phi = 2.0 * std::f64::consts::PI * (j as f64) / ((res - 1) as f64);

                let x_obs = r_obs * theta.sin() * phi.cos();
                let y_obs = r_obs * theta.sin() * phi.sin();
                let z_obs = r_obs * theta.cos();

                let mut p_complex = 0.0_f64;
                let mut total_amp = 0.0;

                for mode in modes.iter().take(5) {
                    let f = mode.nonlinear_frequency_hz;
                    let omega = 2.0 * std::f64::consts::PI * f;
                    let k = omega / c0;
                    let amp = mode.effective_force_amplitude * mode_weight(f);

                    let max_disp = mode.modal_displacements.iter()
                        .map(|&(_, _, w)| w.abs())
                        .fold(f64::NEG_INFINITY, f64::max);

                    let w0 = max_disp * 0.001;
                    let sigma_r = mode_weight(f);

                    let mut vel_integral = 0.0_f64;
                    for &(dx, dy, w) in &mode.modal_displacements {
                        let r_source = (dx * dx + dy * dy).sqrt() * a;
                        if r_source > a {
                            continue;
                        }
                        let dot = (x_obs * dx + y_obs * dy + z_obs * 0.0) / (r_obs * r_source.max(0.01));
                        let kernel = (k * r_source * dot).sin() / (k * r_source * dot + 1e-6);
                        vel_integral += w * kernel;
                    }

                    let p_amp = rho0 * c0 * k * w0 * a * a * sigma_r * vel_integral.abs() / (2.0 * std::f64::consts::PI * r_obs);
                    p_complex += p_amp * amp;
                    total_amp += amp;
                }

                let p_rms = p_complex / 2.0_f64.sqrt();
                let spl = if p_rms > p_ref {
                    20.0 * (p_rms / p_ref).log10()
                } else {
                    -60.0
                };

                let intensity = p_rms * p_rms / (rho0 * c0);
                total_power += intensity * r_obs * r_obs * theta.sin()
                    * (std::f64::consts::PI * 0.5 / (res as f64))
                    * (2.0 * std::f64::consts::PI / (res as f64));

                field.push(SoundFieldPoint {
                    x: x_obs,
                    y: y_obs,
                    z: z_obs,
                    pressure_pa: p_rms,
                    spl_db: spl.max(-60.0),
                    intensity_wm2: intensity,
                });
            }
        }

        (field, total_power * 4.0 * std::f64::consts::PI * r_obs * r_obs)
    }

    fn evaluate_sound_quality(
        &self,
        result: &AcousticAnalysisResult,
        ac: &AcousticsConfig,
    ) -> f64 {
        let mut scores = [0.0; 4];

        let freqs: Vec<f64> = result.vibration_modes.iter()
            .take(5)
            .map(|m| m.nonlinear_frequency_hz)
            .collect();
        let mut harmonic_score = 0.0;
        for (i, &f) in freqs.iter().enumerate() {
            if i > 0 {
                let ratio = f / freqs[0];
                let ideal = (i + 1) as f64;
                let deviation = (ratio - ideal).abs();
                harmonic_score += (1.0 - deviation / 0.2).max(0.0);
            }
        }
        scores[0] = harmonic_score / (freqs.len().max(1) as f64);

        let damping_scores: Vec<f64> = result.vibration_modes.iter()
            .take(5)
            .map(|m| 1.0 - (m.damping_ratio - 0.005).abs() / 0.01)
            .collect();
        scores[1] = damping_scores.iter().sum::<f64>() / damping_scores.len().max(1) as f64;

        scores[2] = if result.vibration_modes.len() >= 5 { 1.0 } else {
            result.vibration_modes.len() as f64 / 5.0
        };

        let power_db = 10.0 * (result.radiated_sound_power_w / 1e-12).log10();
        scores[3] = if power_db > 85.0 {
            1.0 - (power_db - 85.0) / 20.0
        } else if power_db > 60.0 {
            1.0
        } else {
            power_db / 60.0
        }.max(0.0);

        (scores[0] * 0.4 + scores[1] * 0.3 + scores[2] * 0.15 + scores[3] * 0.15)
            .max(0.0).min(1.0)
    }

    pub fn check_frequency_deviation(
        spectrum: &[SpectrumBin],
        reference_hz: &[f64],
        threshold_hz: f64,
    ) -> Vec<(f64, f64, f64)> {
        let mut deviations = Vec::new();
        if spectrum.is_empty() || reference_hz.is_empty() {
            return deviations;
        }

        let peaks: Vec<f64> = spectrum.windows(3)
            .filter(|w| w[1].amplitude_db > w[0].amplitude_db
                && w[1].amplitude_db > w[2].amplitude_db
                && w[1].amplitude_db > -20.0
                && w[1].frequency_hz > 80.0
                && w[1].frequency_hz < 4000.0)
            .map(|w| w[1].frequency_hz)
            .collect();

        for &ref_f in reference_hz {
            let mut best_match = None;
            let mut best_dist = f64::INFINITY;
            for &peak in &peaks {
                let octave_ratio = peak / ref_f;
                let harmonic = octave_ratio.log2().round();
                let expected = ref_f * 2.0_f64.powf(harmonic);
                let dist = (peak - expected).abs();
                if dist < best_dist && dist < threshold_hz * 3.0 {
                    best_dist = dist;
                    best_match = Some((peak, peak - expected));
                }
            }
            if let Some((measured, deviation)) = best_match {
                if deviation.abs() > threshold_hz {
                    deviations.push((ref_f, measured, deviation));
                }
            }
        }

        deviations
    }

    fn bessel_j(n: i32, x: f64) -> f64 {
        if x.abs() < 1e-10 {
            return if n == 0 { 1.0 } else { 0.0 };
        }
        let mut j = 0.0_f64;
        for k in 0..30 {
            let term = (-1.0_f64).powi(k as i32)
                * (x / 2.0).powi(2 * k + n);
            let den = Self::gamma(k as f64 + 1.0) * Self::gamma(k as f64 + n as f64 + 1.0);
            let t = term / den;
            j += t;
            if t.abs() < 1e-12 {
                break;
            }
        }
        j
    }

    fn bessel_i(n: i32, x: f64) -> f64 {
        if x.abs() < 1e-10 {
            return if n == 0 { 1.0 } else { 0.0 };
        }
        let mut i = 0.0_f64;
        for k in 0..30 {
            let term = (x / 2.0).powi(2 * k + n);
            let den = Self::gamma(k as f64 + 1.0) * Self::gamma(k as f64 + n as f64 + 1.0);
            let t = term / den;
            i += t;
            if t.abs() < 1e-12 {
                break;
            }
        }
        i
    }

    fn gamma(x: f64) -> f64 {
        if x <= 0.0 {
            return std::f64::INFINITY;
        }
        let g = 7.0;
        let c = [
            0.99999999999980993,
            676.5203681218851,
            -1259.1392167224028,
            771.32342877765313,
            -176.61502916214059,
            12.507343278686905,
            -0.13857109526572012,
            9.9843695780195716e-6,
            1.5056327351493116e-7,
        ];
        if x < 0.5 {
            std::f64::consts::PI
                / ((std::f64::consts::PI * x).sin() * Self::gamma(1.0 - x))
        } else {
            let x = x - 1.0;
            let mut a = c[0];
            let t = x + g + 0.5;
            for i in 1..9 {
                a += c[i] / (x + i as f64);
            }
            (2.0 * std::f64::consts::PI).sqrt()
                * t.powf(x + 0.5)
                * (-t).exp()
                * a
        }
    }
}
