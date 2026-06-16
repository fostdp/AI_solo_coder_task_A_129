use crate::models::*;
use chrono::Utc;
use nalgebra::{DMatrix, DVector, Matrix4, Vector3, SymmetricEigen};
use rand::Rng;
use std::f64::consts::PI;
use tracing::{debug, info};
use uuid::Uuid;

pub struct AcousticAnalyzer;

impl AcousticAnalyzer {
    pub fn analyze(
        drum_id: String,
        drum_diameter_cm: f64,
        drum_height_cm: f64,
        drum_mass_kg: f64,
        req: &AcousticAnalysisRequest,
        wall_thickness: Option<&Vec<ThicknessPoint>>,
    ) -> AcousticAnalysisResult {
        info!("Running acoustic analysis for drum: {}", drum_id);

        let youngs_modulus = req.youngs_modulus_pa.unwrap_or(110e9);
        let poissons_ratio = req.poissons_ratio.unwrap_or(0.34);
        let density = req.density_kgm3.unwrap_or(8800.0);

        let sound_speed_air = 343.2;
        let air_density = 1.204;

        let effective_thickness = Self::get_effective_thickness(
            drum_diameter_cm,
            wall_thickness,
        );

        let vibration_modes = Self::compute_vibration_modes(
            drum_diameter_cm,
            drum_height_cm,
            effective_thickness,
            youngs_modulus,
            poissons_ratio,
            density,
        );

        let resonance_frequencies_hz: Vec<f64> = vibration_modes
            .iter()
            .map(|m| m.frequency_hz)
            .collect();

        let (sound_field, radiated_power) = Self::compute_sound_radiation(
            &vibration_modes,
            drum_diameter_cm,
            drum_height_cm,
            density,
            sound_speed_air,
            air_density,
        );

        let sound_quality_metric = Self::evaluate_sound_quality(
            &vibration_modes,
            &resonance_frequencies_hz,
            radiated_power,
        );

        AcousticAnalysisResult {
            analysis_id: Uuid::new_v4().to_string(),
            drum_id,
            created_at: Utc::now(),
            youngs_modulus_pa: youngs_modulus,
            poissons_ratio,
            density_kgm3: density,
            sound_speed_air_ms: sound_speed_air,
            air_density_kgm3: air_density,
            vibration_modes,
            radiated_sound_power_w: radiated_power,
            resonance_frequencies_hz,
            sound_field,
            sound_quality_metric,
        }
    }

    fn get_effective_thickness(
        drum_diameter_cm: f64,
        wall_thickness: Option<&Vec<ThicknessPoint>>,
    ) -> f64 {
        if let Some(wt) = wall_thickness {
            if !wt.is_empty() {
                let avg: f64 = wt.iter().map(|p| p.thickness_mm).sum::<f64>() / wt.len() as f64;
                return avg / 1000.0;
            }
        }
        let std_thickness_mm = 3.0 + drum_diameter_cm * 0.05;
        std_thickness_mm / 1000.0
    }

    fn compute_vibration_modes(
        diameter_cm: f64,
        height_cm: f64,
        thickness_m: f64,
        e: f64,
        nu: f64,
        rho: f64,
    ) -> Vec<VibrationMode> {
        let radius_m = (diameter_cm / 2.0) / 100.0;
        let h = thickness_m;
        let a = radius_m;

        let d = e * h.powi(3) / (12.0 * (1.0 - nu.powi(2)));
        let rho_h = rho * h;

        let mut modes = Vec::new();
        let num_modes = 8;

        for m in 0..=3 {
            for n in 1..=4 {
                if modes.len() >= num_modes {
                    break;
                }

                let lambda_mn = Self::lambda_mn(m, n);
                let omega_sq = d / rho_h * (lambda_mn / a).powi(4);
                let frequency = omega_sq.sqrt() / (2.0 * PI);

                if frequency > 50.0 && frequency < 5000.0 {
                    let order = modes.len() + 1;
                    let damping = Self::modal_damping(order, frequency, h, a);
                    let displacements = Self::modal_displacement_field(m, n, a, lambda_mn);
                    let node_pattern = format!("({}, {})节径-节圆模式", m, n - 1);

                    modes.push(VibrationMode {
                        mode_order: order,
                        frequency_hz: frequency,
                        damping_ratio: damping,
                        node_pattern,
                        modal_displacements: displacements,
                    });
                }
            }
        }

        modes.sort_by(|a, b| a.frequency_hz.partial_cmp(&b.frequency_hz).unwrap());
        for (i, mode) in modes.iter_mut().enumerate() {
            mode.mode_order = i + 1;
        }
        modes
    }

    fn lambda_mn(m: usize, n: usize) -> f64 {
        let lambda_lookup = [
            [3.196, 4.611, 6.306, 7.970, 9.616, 11.250],
            [5.906, 7.799, 9.748, 11.650, 13.530, 15.380],
            [8.417, 10.536, 12.595, 14.620, 16.620, 18.600],
            [10.837, 13.095, 15.278, 17.410, 19.510, 21.590],
            [13.212, 15.579, 17.848, 20.070, 22.260, 24.430],
        ];
        if m < lambda_lookup.len() && n > 0 && n <= lambda_lookup[0].len() {
            lambda_lookup[m][n - 1]
        } else {
            let m = m as f64;
            let n = n as f64;
            PI * (n - 0.75 + 0.5 * m)
        }
    }

    fn modal_damping(order: usize, freq: f64, h: f64, a: f64) -> f64 {
        let damping_viscous = 0.0003 * (1.0 + 0.05 * order as f64);
        let freq_factor = (freq / 500.0).powf(-0.3).min(3.0);
        let thickness_factor = (h / a * 100.0 + 0.5).powf(-0.2);
        (damping_viscous * freq_factor * thickness_factor).max(0.0005).min(0.05)
    }

    fn modal_displacement_field(
        m: usize,
        n: usize,
        a: f64,
        lambda: f64,
    ) -> Vec<(f64, f64, f64)> {
        let resolution = 24;
        let mut displacements = Vec::with_capacity(resolution * resolution);
        let m = m as f64;
        let lambda_a = lambda / a;

        for i in 0..resolution {
            for j in 0..resolution {
                let x_frac = (i as f64 + 0.5) / resolution as f64;
                let y_frac = (j as f64 + 0.5) / resolution as f64;

                let dx = x_frac - 0.5;
                let dy = y_frac - 0.5;
                let r = (dx.powi(2) + dy.powi(2)).sqrt() * 2.0 * a;
                let theta = dy.atan2(dx);

                let r_norm = r / a;
                if r_norm > 1.0 {
                    displacements.push((x_frac, y_frac, 0.0));
                    continue;
                }

                let xi = lambda_a * r;
                let w = Self::bessel_j(m, xi)
                    + ((-1.0f64).powi(m as i32) * Self::bessel_i(m, xi)) / Self::bessel_i(m, lambda)
                        * Self::bessel_j(m, lambda);

                let angular = (m * theta).cos();
                let displ = w * angular * (1.0 - r_norm.powi(4));

                displacements.push((x_frac, y_frac, displ));
            }
        }
        displacements
    }

    fn bessel_j(n: f64, x: f64) -> f64 {
        if x < 0.0 {
            return (n as i32 % 2) as f64 * -1.0 + 1.0;
        }
        if x.abs() < 1e-10 {
            return if n.abs() < 1e-10 { 1.0 } else { 0.0 };
        }

        let x2 = x / 2.0;
        let mut sum = 0.0;
        for k in 0..=20 {
            let kf = k as f64;
            let gamma_nk = Self::gamma_approx(n + kf + 1.0);
            let gamma_k1 = Self::gamma_approx(kf + 1.0);
            let term = (-1.0f64).powi(k) * (x2.powi(2 * k + n as i32)) / (gamma_nk * gamma_k1);
            sum += term;
            if term.abs() < 1e-12 {
                break;
            }
        }
        sum
    }

    fn bessel_i(n: f64, x: f64) -> f64 {
        if x.abs() < 1e-10 {
            return if n.abs() < 1e-10 { 1.0 } else { 0.0 };
        }
        let x2 = x / 2.0;
        let mut sum = 0.0;
        for k in 0..=20 {
            let kf = k as f64;
            let gamma_nk = Self::gamma_approx(n + kf + 1.0);
            let gamma_k1 = Self::gamma_approx(kf + 1.0);
            let term = x2.powi(2 * k + n as i32) / (gamma_nk * gamma_k1);
            sum += term;
            if term.abs() < 1e-12 {
                break;
            }
        }
        sum
    }

    fn gamma_approx(z: f64) -> f64 {
        if z < 0.5 {
            return PI / ((PI * z).sin() * Self::gamma_approx(1.0 - z));
        }
        let z = z - 1.0;
        let p = [
            676.5203681218851,
            -1259.1392167224028,
            771.32342877765313,
            -176.61502916214059,
            12.507343278686905,
            -0.13857109526572012,
            9.9843695780195716e-6,
            1.5056327351493116e-7,
        ];
        let mut x = 0.99999999999980993;
        for (i, pi) in p.iter().enumerate() {
            x += pi / (z + i as f64 + 1.0);
        }
        let t = z + p.len() as f64 - 0.5;
        (2.0 * PI).sqrt() * t.powf(z + 0.5) * (-t).exp() * x
    }

    fn compute_sound_radiation(
        modes: &Vec<VibrationMode>,
        diameter_cm: f64,
        height_cm: f64,
        rho_material: f64,
        c_air: f64,
        rho_air: f64,
    ) -> (Vec<SoundFieldPoint>, f64) {
        let a = (diameter_cm / 2.0) / 100.0;
        let h_drum = height_cm / 100.0;
        let S_drum = PI * a.powi(2);

        let mut field_points = Vec::new();
        let grid_size = 12;
        let observation_distance = (diameter_cm.max(30.0)) / 100.0 * 3.0;

        let mut total_radiated_power = 0.0;

        for mode in modes.iter().take(5) {
            let k = 2.0 * PI * mode.frequency_hz / c_air;
            let ka = k * a;

            let radiation_efficiency = if ka < 1.0 {
                (ka.powi(2) / 2.0).min(1.0)
            } else if ka < 3.0 {
                0.5 + 0.5 * (1.0 - (-(ka - 1.0) * 1.5).exp())
            } else {
                1.0 - 1.0 / (ka * ka).sqrt()
            };

            let amplitude = 1.0e-6 * (1.0 / mode.mode_order as f64).sqrt();
            let modal_velocity = 2.0 * PI * mode.frequency_hz * amplitude;
            let v_rms = modal_velocity / 2.0f64.sqrt();

            let sigma = radiation_efficiency;
            let modal_power = rho_air * c_air * S_drum * sigma * v_rms.powi(2)
                * (1.0 / (mode.mode_order as f64));

            total_radiated_power += modal_power;

            for ig in 0..grid_size {
                let theta = (ig as f64 + 0.5) / grid_size as f64 * PI / 2.0;
                for jg in 0..grid_size {
                    let phi = (jg as f64 + 0.5) / grid_size as f64 * 2.0 * PI;

                    let r = observation_distance;
                    let x = r * theta.sin() * phi.cos();
                    let y = r * theta.sin() * phi.sin();
                    let z = r * theta.cos();

                    let (x_frac, y_frac, z_frac) = (
                        (x + observation_distance) / (2.0 * observation_distance),
                        (y + observation_distance) / (2.0 * observation_distance),
                        (z + observation_distance) / (2.0 * observation_distance),
                    );

                    let far_field = (rho_air * c_air * k * v_rms * S_drum * sigma)
                        / (2.0 * PI * r)
                        * ((ka * theta.sin()).sin() / ((ka * theta.sin()) + 1e-6)).abs()
                        * (1.0 + theta.cos()) / 2.0;

                    let p = far_field * (1.0 / mode.mode_order as f64).sqrt();

                    let idx = ig * grid_size + jg;
                    if mode.mode_order == 1 {
                        let spl = if p > 0.0 {
                            20.0 * (p / 2.0e-5).log10()
                        } else {
                            0.0
                        };
                        field_points.push(SoundFieldPoint {
                            x: x_frac,
                            y: y_frac,
                            z: z_frac,
                            pressure_pa: p,
                            spl_db: spl.max(0.0),
                        });
                    } else {
                        if let Some(pt) = field_points.get_mut(idx) {
                            pt.pressure_pa += p;
                            if pt.pressure_pa > 0.0 {
                                pt.spl_db = 20.0 * (pt.pressure_pa / 2.0e-5).log10();
                            }
                        }
                    }
                }
            }
        }

        let total_power = total_radiated_power;
        (field_points, total_power)
    }

    fn evaluate_sound_quality(
        modes: &Vec<VibrationMode>,
        resonances: &Vec<f64>,
        radiated_power: f64,
    ) -> f64 {
        let harmonic_score = Self::harmonicity_score(resonances);
        let damping_score = 1.0 - (modes.iter().take(5)
            .map(|m| m.damping_ratio)
            .sum::<f64>() / 5.0 / 0.02).min(1.0);

        let mode_count_score = (modes.len() as f64 / 8.0).min(1.0);

        let power_score = {
            let p_log = (radiated_power + 1e-12).log10();
            let normalized = (p_log + 8.0) / 4.0;
            normalized.max(0.0).min(1.0)
        };

        0.4 * harmonic_score + 0.25 * damping_score + 0.15 * mode_count_score + 0.2 * power_score
    }

    fn harmonicity_score(frequencies: &Vec<f64>) -> f64 {
        if frequencies.len() < 3 {
            return 0.3;
        }
        let f0 = frequencies[0];
        let mut harmony = 0.0;
        let mut count = 0.0;
        for (i, f) in frequencies.iter().enumerate().skip(1) {
            let harmonic_expected = f0 * (i + 1) as f64;
            let deviation_ratio = (f - harmonic_expected).abs() / harmonic_expected;
            let partial = (1.0 - deviation_ratio * 5.0).max(0.0);
            harmony += partial;
            count += 1.0;
        }
        let std_ref = [523.25, 659.25, 783.99, 1046.50, 1318.51, 1568.0];
        let mut in_tune = 0.0;
        for f in frequencies.iter().take(6) {
            let mut best = f64::MAX;
            for &ref_freq in &std_ref {
                let dev = (f - ref_freq).abs() / ref_freq * 100.0;
                best = best.min(dev);
            }
            in_tune += (1.0 - best / 3.0).max(0.0);
        }
        in_tune = in_tune / 6.0;

        if count > 0.0 {
            0.5 * (harmony / count) + 0.5 * in_tune
        } else {
            in_tune * 0.5
        }
    }

    pub fn check_frequency_deviation(
        measured_spectrum: &Vec<SpectrumBin>,
        reference_freqs: &Vec<f64>,
        threshold_hz: f64,
    ) -> Vec<(f64, f64, f64)> {
        let measured_peaks: Vec<f64> = Self::extract_peak_frequencies(measured_spectrum);
        let mut deviations = Vec::new();

        for (i, &ref_f) in reference_freqs.iter().enumerate() {
            if let Some(meas_f) = measured_peaks.get(i) {
                let dev = meas_f - ref_f;
                if dev.abs() > threshold_hz {
                    deviations.push((ref_f, *meas_f, dev));
                }
            }
        }
        deviations
    }

    fn extract_peak_frequencies(spectrum: &Vec<SpectrumBin>) -> Vec<f64> {
        if spectrum.is_empty() {
            return Vec::new();
        }
        let mut peaks: Vec<(f64, f64)> = Vec::new();
        for i in 1..spectrum.len() - 1 {
            let a = spectrum[i - 1].amplitude_db;
            let b = spectrum[i].amplitude_db;
            let c = spectrum[i + 1].amplitude_db;
            if b > a + 3.0 && b > c + 3.0 {
                peaks.push((spectrum[i].frequency_hz, b));
            }
        }
        peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        peaks.truncate(8);
        peaks.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        peaks.into_iter().map(|(f, _)| f).collect()
    }
}
