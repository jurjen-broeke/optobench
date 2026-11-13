//! Excitation of fluorophores (M11b): one- and two-photon rates per molecule, the pulse
//! factor of a pulsed laser, and the dispersion that stretches its pulses on the way to
//! the sample.
//!
//! - **One photon.** A molecule in irradiance I absorbs σ₁ I / (h ν) photons per second,
//!   σ₁ its absorption cross-section; each absorption is one excitation. The fraction of
//!   time it spends excited stays small while the rate times its lifetime, k τ_f, is well
//!   below 1 (saturation sets in near 1).
//! - **Two photons.** The rate is ½ σ₂ ⟨F²⟩, F = I / (h ν) the photon flux and σ₂ the
//!   two-photon absorption cross-section δ, which counts two photons per excitation
//!   (C. Xu and W. W. Webb, J. Opt. Soc. Am. B 13, 481 (1996), Eqs. (1) to (5): F =
//!   ½ φ η₂ N_abs, "the factor 1/2 simply reflects the fact that two photons are needed
//!   for each excitation event"). Its companion, C. Xu, W. Zipfel, J. B. Shear,
//!   R. M. Williams and W. W. Webb, Proc. Natl. Acad. Sci. USA 93, 10763 (1996), gives σ₂
//!   in GM (10⁻⁵⁰ cm⁴ s per photon), publishes most spectra as action cross-sections
//!   η σ₂, finds the same emission spectrum after one-, two- and three-photon excitation,
//!   and puts saturation at σₙ I_peakⁿ τ ≈ 1 per pulse (its Eq. 1, I in photons per area
//!   and time); its Eq. 3 for the saturation power prints (σₙ τ)^(−1/n) in the
//!   denominator where combining its Eqs. 1 and 2 puts it in the numerator, so it is not
//!   used. For pulses of width τ (FWHM)
//!   at repetition rate f, ⟨F²⟩ = g_p ⟨F⟩² / (f τ), with g_p = τ ∫ I² dt / (∫ I dt)² of one
//!   pulse (Xu and Webb, Eqs. (13), (14)): √(2 ln 2 / π) = 0.664 for a Gaussian,
//!   2 ln(1 + √2) / 3 = 0.588 for sech² (their values), computed here from the shapes.
//!   Xu and Webb measured 0.41 ± 0.09 for their own laser (Sect. 4): a real pulse need
//!   not have its nominal shape. Light that does not vary in time has g_p / (f τ) = 1.
//!   The chance of excitation per pulse, the rate over f, must stay well below 1.
//! - **Dispersion.** A transform-limited Gaussian pulse of width τ that picks up a
//!   group-delay dispersion D leaves with width τ √(1 + (4 ln 2 · D / τ²)²) (J.-C. Diels
//!   and W. Rudolph, Ultrashort Laser Pulse Phenomena, 2nd ed., 2006, ch. 1); its shape
//!   stays Gaussian, so g_p is kept. The same widening is applied to sech² pulses, which
//!   it describes approximately. D adds up along the chief ray: the source's prechirp, the
//!   group-velocity dispersion of every medium crossed times its length
//!   (`optobench_materials::dispersion`, from the same models the trace uses), and what
//!   the maker states for an ideal objective, which has no glass of its own.
//! - **The focus** ([`excitation`]): the vectorial PSF (M9c2) at the actual depth, each
//!   plane wave attenuated by its ray's ballistic loss, gives the mean irradiance; the
//!   rates follow at its peak. Two-photon excitation in the whole focus is
//!   ½ σ₂ C g ∫ F² dV, the integral over the whole depth (`PsfSettings::volume`), so the
//!   layer is taken as much thicker than the focus (Xu and Webb's thick-sample limit).
//!   One-photon excitation spreads through the whole cone and is given per depth at the
//!   focal plane; the diffuse light (M11a2) excites too, which is not counted here.

use optobench_materials::dispersion::group_velocity_dispersion;
use optobench_scene::{ElementKind, Owner, PulseShape, Scene, SourceKind};
use optobench_trace::Ray;
use optobench_trace::nonsequential::{End, NO_MEDIUM};

use crate::emission::{LIGHT_SPEED, PLANCK, owner_id};
use crate::{Polarization, Psf, PsfSettings, psf};

/// Avogadro's constant (1/mol), exact in the SI since 2019.
const AVOGADRO: f64 = 6.022_140_76e23;

/// The energy of one photon at vacuum wavelength `wavelength` (µm), in J.
pub fn photon_energy(wavelength: f64) -> f64 {
    PLANCK * LIGHT_SPEED / (wavelength * 1e-6)
}

/// The pulse factor g_p = τ ∫ I² dt / (∫ I dt)² of one pulse of the given shape, τ its
/// full width at half maximum.
pub fn pulse_factor(shape: PulseShape) -> f64 {
    match shape {
        // I = exp(−4 ln 2 t²/τ²): ∫ I = τ √(π / (4 ln 2)), ∫ I² = τ √(π / (8 ln 2)).
        PulseShape::Gaussian => (2.0 * core::f64::consts::LN_2 / core::f64::consts::PI).sqrt(),
        // I = sech²(t/T), τ = 2 ln(1 + √2) T: ∫ I = 2T, ∫ I² = 4T/3.
        PulseShape::SechSquared => 2.0 * (1.0 + 2f64.sqrt()).ln() / 3.0,
    }
}

/// The width (FWHM, s) of a transform-limited Gaussian pulse of width `width` after it
/// picks up a group-delay dispersion `dispersion` (s²).
pub fn broadened_width(width: f64, dispersion: f64) -> f64 {
    let x = 4.0 * core::f64::consts::LN_2 * dispersion / (width * width);
    width * (1.0 + x * x).sqrt()
}

/// One-photon excitations per second of one molecule of cross-section `cross_section`
/// (mm²) in irradiance `irradiance` (W/mm²) at vacuum wavelength `wavelength` (µm).
pub fn one_photon_rate(cross_section: f64, irradiance: f64, wavelength: f64) -> f64 {
    cross_section * irradiance / photon_energy(wavelength)
}

/// How a laser's light is spread in time, for two-photon excitation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Timing {
    /// Constant in time (continuous light).
    Continuous,
    /// Pulses.
    Pulsed {
        /// Pulses per second (Hz).
        repetition_rate: f64,
        /// FWHM at the molecule (s), after the path's dispersion.
        width: f64,
        /// Their shape.
        shape: PulseShape,
    },
}

impl Timing {
    /// ⟨I²⟩ / ⟨I⟩²: g_p / (f τ) for pulses, 1 for continuous light.
    pub fn squared_mean_ratio(self) -> f64 {
        match self {
            Timing::Continuous => 1.0,
            Timing::Pulsed {
                repetition_rate,
                width,
                shape,
            } => pulse_factor(shape) / (repetition_rate * width),
        }
    }
}

/// Two-photon excitations per second of one molecule of cross-section `cross_section`
/// (mm⁴ s, two photons per excitation) in mean irradiance `irradiance` (W/mm²) at the
/// laser's vacuum wavelength `wavelength` (µm): ½ σ₂ ⟨F²⟩.
pub fn two_photon_rate(
    cross_section: f64,
    irradiance: f64,
    wavelength: f64,
    timing: Timing,
) -> f64 {
    let flux = irradiance / photon_energy(wavelength);
    0.5 * cross_section * flux * flux * timing.squared_mean_ratio()
}

/// One step of the chief ray's path, for the dispersion.
#[derive(Debug, Clone, PartialEq)]
pub struct PathStep {
    /// The item where it starts (the source's id for the first).
    pub from: String,
    /// The item where it ends.
    pub to: String,
    /// Its length (mm); 0 through an ideal objective.
    pub length: f64,
    /// The group-delay dispersion it adds (s²).
    pub dispersion: f64,
    /// Total group-delay dispersion after it, the prechirp included (s²).
    pub total: f64,
    /// The pulse width after it (FWHM, s).
    pub width: f64,
}

/// The pulse width along the chief ray of a pulsed source.
#[derive(Debug, Clone, PartialEq)]
pub struct PulsePath {
    /// The source's id.
    pub source: String,
    /// The width as the laser makes it (s).
    pub width: f64,
    /// The prechirp (s²).
    pub prechirp: f64,
    /// The width as it leaves the source, with the prechirp (s).
    pub start: f64,
    /// Step by step.
    pub steps: Vec<PathStep>,
    /// Where the chief ray ended: the detector's id, or why not.
    pub end: String,
}

/// Traces the chief ray of pulsed source `source` (an index into the scene's sources)
/// from its origin along its axis, and adds up the dispersion on the way.
pub fn pulse_path(scene: &Scene, source: usize) -> Result<PulsePath, String> {
    let src = scene.sources.get(source).ok_or("no such source")?;
    let pulse = src
        .pulse
        .ok_or_else(|| format!("source `{}` is not pulsed (no `pulse`)", src.id))?;
    if !src.spectrum.is_empty() {
        return Err(format!(
            "source `{}` has a spectrum; the pulse is followed at one wavelength",
            src.id
        ));
    }
    let built = scene.build().map_err(|e| e.to_string())?;
    let [_, _, axis] = src.frame.axes();
    let ray = Ray::new(src.frame.origin(), axis, src.wavelength, src.power)
        .ok_or("a source axis of zero length")?;
    let record = built.system.trace(ray);
    let faces = &built.system.faces;
    let owner_of = |k: usize| built.owners[faces[record.faces[k]].owner];
    let mut total = pulse.prechirp;
    let mut steps = Vec::with_capacity(record.media.len());
    let mut from = src.id.clone();
    for (k, &medium) in record.media.iter().enumerate() {
        let to = owner_id(scene, owner_of(k)).to_owned();
        let length = (record.points[k + 1] - record.points[k]).length();
        let dispersion = if medium == NO_MEDIUM {
            match owner_of(k) {
                Owner::Element(i) => match &scene.elements[i].kind {
                    ElementKind::Objective {
                        group_delay_dispersion,
                        ..
                    } => *group_delay_dispersion,
                    _ => 0.0,
                },
                _ => 0.0,
            }
        } else if length > 0.0 {
            let material = built.system.media[medium].material();
            group_velocity_dispersion(material, src.wavelength)
                .map_err(|e| format!("between `{from}` and `{to}`: {e}"))?
                * length
        } else {
            0.0
        };
        total += dispersion;
        steps.push(PathStep {
            from: core::mem::replace(&mut from, to.clone()),
            to,
            length: if medium == NO_MEDIUM { 0.0 } else { length },
            dispersion,
            total,
            width: broadened_width(pulse.width, total),
        });
    }
    let end = match record.end {
        End::Detected { detector, .. } => scene.detectors[detector].id.clone(),
        End::Clipped { face } => format!(
            "clipped by `{}`",
            owner_id(scene, built.owners[faces[face].owner])
        ),
        End::Escaped => "escaped".into(),
        End::Stopped => "stopped".into(),
        End::MaterialUndefined { error, .. } => format!("stopped: {error}"),
    };
    Ok(PulsePath {
        source: src.id.clone(),
        width: pulse.width,
        prechirp: pulse.prechirp,
        start: broadened_width(pulse.width, pulse.prechirp),
        steps,
        end,
    })
}

/// How to compute the excitation of a focus.
#[derive(Debug, Clone, PartialEq)]
pub struct ExcitationSettings {
    /// Rays across the laser's beam (the PSF's samples).
    pub samples: usize,
    /// The pupil field's polarization.
    pub polarization: Polarization,
    /// Planes across the focus for the volume integral, and points across each.
    pub volume: (usize, usize),
    /// An x–z section of the excitation through the peak, (columns, rows).
    pub section: Option<(usize, usize)>,
    /// Emitter points through the excited volume (`PsfSettings::cloud`): points across,
    /// points along, half-extent in widths of I².
    pub cloud: Option<(usize, usize, f64)>,
}

impl Default for ExcitationSettings {
    fn default() -> Self {
        Self {
            samples: 96,
            polarization: Polarization::X,
            volume: (32, 32),
            section: None,
            cloud: None,
        }
    }
}

/// One-photon excitation at a focus.
#[derive(Debug, Clone, PartialEq)]
pub struct OnePhoton {
    /// σ₁ at the laser's wavelength (mm²).
    pub cross_section: f64,
    /// Excitations per second of one molecule at the peak.
    pub peak_rate: f64,
    /// The peak rate times the lifetime: well below 1 unsaturated; `None` without a
    /// lifetime.
    pub saturation: Option<f64>,
    /// Excitations per second per mm of depth at the focal plane, σ₁ C P / (h ν) (P the
    /// ballistic power there): one-photon excitation is not confined to the focus, so it
    /// is given per depth.
    pub per_depth: Option<f64>,
}

/// Two-photon excitation at a focus.
#[derive(Debug, Clone, PartialEq)]
pub struct TwoPhoton {
    /// σ₂ at the laser's wavelength (mm⁴ s).
    pub cross_section: f64,
    /// Excitations per second of one molecule at the peak.
    pub peak_rate: f64,
    /// The chance of excitation per pulse at the peak, the rate over the repetition rate;
    /// `None` for continuous light.
    pub per_pulse: Option<f64>,
    /// Excitations per second in the whole focus, ½ σ₂ C g ∫ F² dV, the layer taken as
    /// much thicker than the focus; `None` without a concentration.
    pub total: Option<f64>,
}

/// The excitation of a fluorophore at a laser's focus (M11b).
#[derive(Debug, Clone, PartialEq)]
pub struct Excitation {
    /// The laser's id.
    pub source: String,
    /// The detector at the focus.
    pub detector: String,
    /// The fluorophore's id.
    pub fluorophore: String,
    /// The laser's vacuum wavelength (µm).
    pub wavelength: f64,
    /// Where the focus is: (sample id, `layer k`), if in a sample.
    pub place: Option<(String, String)>,
    /// The fluorophore's concentration there (mol/L); `None` if it is not there.
    pub molar: Option<f64>,
    /// How the light varies in time at the focus (the pulse width after the path's
    /// dispersion).
    pub timing: Timing,
    /// The pulse along the chief ray, for a pulsed laser.
    pub pulse: Option<PulsePath>,
    /// Mean power at the objective's pupil (W).
    pub pupil_power: f64,
    /// The ballistic transmission from the objective to the focus.
    pub transmission: f64,
    /// Mean irradiance at the peak (W/mm²), ballistic.
    pub peak_irradiance: f64,
    /// One-photon excitation; `None` without σ₁ at this wavelength.
    pub one_photon: Option<OnePhoton>,
    /// Two-photon excitation; `None` without σ₂ at this wavelength.
    pub two_photon: Option<TwoPhoton>,
    /// Photons emitted per second (excitations times the quantum yield): two-photon
    /// excitations in the focus, else none.
    pub emitted: Option<f64>,
    /// Emitters through the excited volume (if asked for): points in the scene frame and
    /// their shares of the excitation (adding up to 1), weighted by I² for two-photon
    /// excitation, else by I.
    pub emitters: Vec<(optobench_math::Vec3, f64)>,
    /// The share of the focus's two-photon excitation, ½ σ₂ C g ∫ F² dV, that the
    /// emitters' grid covers; `None` without two-photon excitation or a grid.
    pub represented: Option<f64>,
    /// What was left out and why.
    pub notes: Vec<String>,
    /// The focus (ballistic), with its volume integrals.
    pub psf: Psf,
}

/// The fluorophore `fluorophore` (index into the scene's fluorophores) excited by laser
/// `source` at its focus near detector `detector`: the vectorial PSF at the actual depth
/// with every plane wave attenuated by its own ballistic loss, the pulse widened by the
/// path's dispersion, and the rates, totals and saturation checks.
pub fn excitation(
    scene: &Scene,
    source: usize,
    detector: usize,
    fluorophore: usize,
    settings: &ExcitationSettings,
) -> Result<Excitation, String> {
    let src = scene.sources.get(source).ok_or("no such source")?;
    let dye = scene
        .fluorophores
        .get(fluorophore)
        .ok_or("no such fluorophore")?;
    let det = scene.detectors.get(detector).ok_or("no such detector")?;
    if !matches!(
        src.kind,
        SourceKind::Collimated { .. } | SourceKind::Gaussian { .. }
    ) {
        return Err(format!(
            "source `{}`: excitation needs a laser beam, a collimated or Gaussian source",
            src.id
        ));
    }
    let psf = psf(
        scene,
        source,
        detector,
        &PsfSettings {
            samples: settings.samples,
            polarization: settings.polarization,
            section: settings.section,
            attenuated: true,
            volume: Some(settings.volume),
            cloud: settings.cloud,
            ..Default::default()
        },
    )?;
    let mut notes = Vec::new();

    // Where the focus is: the medium the chief ray reaches the detector in.
    let built = scene.build().map_err(|e| e.to_string())?;
    let [_, _, axis] = src.frame.axes();
    let chief = built.system.trace(
        Ray::new(src.frame.origin(), axis, src.wavelength, src.power)
            .ok_or("a source axis of zero length")?,
    );
    let place = chief.media.last().and_then(|&m| {
        built
            .sample_media
            .iter()
            .find(|(_, _, medium)| *medium == m)
            .map(|(element, part, _)| (scene.elements[*element].id.clone(), part.clone()))
    });
    let molar = place.as_ref().and_then(|(sample, part)| {
        dye.concentrations
            .iter()
            .find(|c| &c.sample == sample && *part == format!("layer {}", c.layer))
            .map(|c| c.molar)
    });
    match (&place, molar) {
        (None, _) => notes.push(format!(
            "the focus at `{}` is not in a sample layer: rates per molecule only",
            det.id
        )),
        (Some((sample, part)), None) => notes.push(format!(
            "`{}` is not in `{sample}` {part}, where the focus is: rates per molecule only",
            dye.id
        )),
        _ => {}
    }
    let molecules = molar.map(|m| m * AVOGADRO * 1e-6); // per mm³

    let pulse = match src.pulse {
        Some(_) => Some(pulse_path(scene, source)?),
        None => None,
    };
    let timing = match (src.pulse, &pulse) {
        (Some(p), Some(path)) => Timing::Pulsed {
            repetition_rate: p.repetition_rate,
            width: path.steps.last().map_or(path.start, |s| s.width),
            shape: p.shape,
        },
        _ => Timing::Continuous,
    };
    let wavelength = src.wavelength;
    let energy = photon_energy(wavelength);
    let peak_irradiance = psf.peak_intensity;
    let focal_power = psf.power * psf.transmission;

    let at =
        |spectrum: &Option<optobench_materials::Spectrum>, what: &str, notes: &mut Vec<String>| {
            match spectrum {
                None => None,
                Some(s) => match s.at(wavelength) {
                    Ok(v) => Some(v),
                    Err(e) => {
                        notes.push(format!("no {what} cross-section at {wavelength} µm: {e}"));
                        None
                    }
                },
            }
        };
    let one_photon = at(&dye.one_photon, "one-photon", &mut notes).map(|sigma| {
        let peak_rate = one_photon_rate(sigma, peak_irradiance, wavelength);
        OnePhoton {
            cross_section: sigma,
            peak_rate,
            saturation: dye.lifetime.map(|t| peak_rate * t),
            per_depth: molecules.map(|c| sigma * c * focal_power / energy),
        }
    });
    let two_photon = at(&dye.two_photon, "two-photon", &mut notes).map(|sigma| {
        let peak_rate = two_photon_rate(sigma, peak_irradiance, wavelength, timing);
        let squared = psf.volume.as_ref().map_or(0.0, |v| v.squared);
        TwoPhoton {
            cross_section: sigma,
            peak_rate,
            per_pulse: match timing {
                Timing::Pulsed {
                    repetition_rate, ..
                } => Some(peak_rate / repetition_rate),
                Timing::Continuous => None,
            },
            total: molecules.map(|c| {
                0.5 * sigma * c * timing.squared_mean_ratio() * squared / (energy * energy)
            }),
        }
    });
    let emitted = two_photon
        .as_ref()
        .and_then(|t| t.total)
        .map(|total| total * dye.quantum_yield);
    // Emitters through the excited volume.
    let squared = two_photon.is_some();
    let weight = |i: f64| if squared { i * i } else { i };
    let sum: f64 = psf.cloud.iter().map(|(_, i)| weight(*i)).sum();
    let emitters: Vec<(optobench_math::Vec3, f64)> = if sum > 0.0 {
        psf.cloud
            .iter()
            .filter(|(_, i)| weight(*i) > 0.0)
            .map(|(r, i)| (det.frame.to_parent_point(*r), weight(*i) / sum))
            .collect()
    } else {
        Vec::new()
    };
    let represented = match (&psf.volume, squared && sum > 0.0) {
        (Some(v), true) if v.squared > 0.0 => Some(sum * psf.cloud_cell / v.squared),
        _ => None,
    };
    Ok(Excitation {
        source: src.id.clone(),
        detector: det.id.clone(),
        fluorophore: dye.id.clone(),
        wavelength,
        place,
        molar,
        timing,
        pulse,
        pupil_power: psf.power,
        transmission: psf.transmission,
        peak_irradiance,
        one_photon,
        two_photon,
        emitted,
        emitters,
        represented,
        notes,
        psf,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// g_p by numerical integration of the shapes, against the closed forms.
    #[test]
    fn the_pulse_factors_are_those_of_the_shapes() {
        let integrate = |f: &dyn Fn(f64) -> f64| {
            // Midpoint rule over ±20 FWHM, fine enough for 1e-10.
            let (n, half) = (400_000, 20.0);
            let h = 2.0 * half / n as f64;
            (0..n).map(|i| f(-half + (i as f64 + 0.5) * h)).sum::<f64>() * h
        };
        let factor = |i: &dyn Fn(f64) -> f64| {
            integrate(&|t| i(t) * i(t)) / integrate(i).powi(2) // τ = 1
        };
        let gaussian = |t: f64| (-4.0 * core::f64::consts::LN_2 * t * t).exp();
        let t = 1.0 / (2.0 * (1.0 + 2f64.sqrt()).ln()); // FWHM 1
        let sech = |x: f64| 1.0 / (x / t).cosh().powi(2);
        // Both shapes have FWHM 1.
        assert!((gaussian(0.5) - 0.5).abs() < 1e-12);
        assert!((sech(0.5) - 0.5).abs() < 1e-12);
        let g = pulse_factor(PulseShape::Gaussian);
        let s = pulse_factor(PulseShape::SechSquared);
        assert!((factor(&gaussian) - g).abs() < 1e-10);
        assert!((factor(&sech) - s).abs() < 1e-10);
        // Xu and Webb (1996), J. Opt. Soc. Am. B 13, 481, after Eq. (14): 0.664 and 0.588.
        assert!((g - 0.664).abs() < 5e-4 && (s - 0.588).abs() < 5e-4);
    }

    #[test]
    fn a_gaussian_pulse_widens_by_its_dispersion() {
        // Diels and Rudolph (2006), ch. 1: τ √(1 + (4 ln 2 D / τ²)²). At D = τ² / (4 ln 2)
        // the width grows by √2; widening is the same for ±D and tends to 4 ln 2 D / τ.
        let tau = 100e-15;
        let d = tau * tau / (4.0 * core::f64::consts::LN_2);
        assert!((broadened_width(tau, d) / tau - 2f64.sqrt()).abs() < 1e-14);
        assert_eq!(broadened_width(tau, d), broadened_width(tau, -d));
        assert_eq!(broadened_width(tau, 0.0), tau);
        let large = 1e-22;
        let ratio = broadened_width(tau, large) / (4.0 * core::f64::consts::LN_2 * large / tau);
        assert!((ratio - 1.0).abs() < 1e-6);
        // From first principles: dispersion D adds the spectral phase ½ D ω² to the
        // field's spectrum; the intensity's FWHM after it, found by summing the chirped
        // spectrum back into time, is the closed form's.
        let width_by_fourier = |tau: f64, d: f64| {
            // E(ω) = exp(−ω² τ² / (8 ln 2)) exp(i D ω² / 2); intensity |E(t)|² by direct sum.
            let l2 = core::f64::consts::LN_2;
            let n = 4000;
            let omega_max = 8.0 * (2.0 * l2).sqrt() / tau * 2.0;
            let dw = 2.0 * omega_max / n as f64;
            let intensity = |t: f64| {
                let (mut re, mut im) = (0.0, 0.0);
                for i in 0..n {
                    let w = -omega_max + (i as f64 + 0.5) * dw;
                    let amplitude = (-w * w * tau * tau / (8.0 * l2)).exp();
                    let phase = 0.5 * d * w * w - w * t;
                    re += amplitude * phase.cos();
                    im += amplitude * phase.sin();
                }
                re * re + im * im
            };
            let peak = intensity(0.0);
            // Half maximum by bisection on t > 0.
            let (mut a, mut b) = (0.0, 20.0 * tau);
            for _ in 0..80 {
                let m = 0.5 * (a + b);
                if intensity(m) > 0.5 * peak {
                    a = m;
                } else {
                    b = m;
                }
            }
            a + b
        };
        let d = 3000e-30;
        let found = width_by_fourier(tau, d);
        let expected = broadened_width(tau, d);
        assert!((found / expected - 1.0).abs() < 1e-6, "{found} {expected}");
    }

    #[test]
    fn rates_follow_the_photon_flux() {
        // One photon: σ₁ I / (h ν). With I = h ν (W/mm²) the flux is one photon per mm² per
        // second, so the rate equals σ₁ in mm².
        let wavelength = 0.488;
        let e = photon_energy(wavelength);
        assert!((e - 6.626_070_15e-34 * 299_792_458.0 / 0.488e-6).abs() < 1e-30);
        assert!((one_photon_rate(2e-14, e, wavelength) - 2e-14).abs() < 1e-28);
        // Two photons, continuous: ½ σ₂ F²; pulsed: × g_p / (f τ).
        let (sigma, i, w) = (1e-47, 1.0, 0.92);
        let f = i / photon_energy(w);
        let cw = two_photon_rate(sigma, i, w, Timing::Continuous);
        assert!((cw - 0.5 * sigma * f * f).abs() < 1e-12 * cw);
        let timing = Timing::Pulsed {
            repetition_rate: 80e6,
            width: 120e-15,
            shape: PulseShape::SechSquared,
        };
        let pulsed = two_photon_rate(sigma, i, w, timing);
        let gain = pulse_factor(PulseShape::SechSquared) / (80e6 * 120e-15);
        assert!((pulsed / cw - gain).abs() < 1e-9 * gain);
        // About 6.1 × 10⁴ times the continuous rate at 80 MHz and 120 fs.
        assert!((gain - 61_250.0).abs() < 50.0, "{gain}");
    }
}
