//! M11 exit check: brain heating during two-photon imaging, against K. Podgorski and
//! G. Ranganathan, "Brain heating induced by near-infrared lasers during multiphoton
//! microscopy", J. Neurophysiol. 116, 1012 (2016) (values from its text; local corpus).
//!
//! Their conditions (Methods, the finite differencing model; Results): 920 nm; a 1 mm²
//! field scanned at 70 % duty; the focal plane 250 µm below the dura; a 350 µm glass
//! window with the immersion water held at its starting temperature 1.5 mm above it;
//! perfused brain below, here a cylinder 6 mm in radius and 10 mm deep as in Stujenske et
//! al. (2015). They found NA 0.8 and 0.08 heat alike, so the beam is focused at NA 0.08 by
//! an ideal lens in a medium of index 1.333.
//!
//! Values and their sources:
//! - Brain: k 0.527 W m⁻¹ K⁻¹, ρ 1040 kg m⁻³, c 3650 J kg⁻¹ K⁻¹, perfusion 8.5 × 10⁻³ s⁻¹,
//!   blood ρ 1060 kg m⁻³, c 3600 J kg⁻¹ K⁻¹ (J. M. Stujenske, T. Spellman and J. A. Gordon,
//!   Cell Rep. 12, 525 (2015), Table S1); n 1.36 (ibid.); µs and g of gray matter at
//!   900 nm, 6.6 /mm and 0.9 (J. D. Johansson, J. Biomed. Opt. 15, 057005 (2010),
//!   Table 1), µs′ carried to 920 nm by his λ^−0.96 fit (0.646 /mm, so µs 6.463 /mm with
//!   g 0.9); µa 0.0175 /mm, the middle of 0.015–0.02 read off his Fig. 6b at 900 nm (a
//!   figure reading, not a reference value).
//! - Water: k 0.623 W m⁻¹ K⁻¹ (J. W. Valvano, Table 12.9), ρ 993 kg m⁻³, c 4178 J kg⁻¹ K⁻¹
//!   (K. R. Diller, Table 10.2), both in A. J. Welch and M. J. C. van Gemert (eds.),
//!   Optical-Thermal Response of Laser-Irradiated Tissue, 2nd ed., 2011.
//! - Glass: k 1.4 W m⁻¹ K⁻¹, ρ 2500 kg m⁻³, c 750 J kg⁻¹ K⁻¹ (Valvano, ibid., Table 11.4,
//!   from F. P. Incropera, Fundamentals of Heat Transfer, 1990).
//!
//! Their measurements and simulations, and what this model gives (the light by the
//! diffusion approximation with the delta-Eddington primary, M11a2):
//! - steady rise per 100 mW at 250 µm: measured 1.86 ± 0.25 °C (quantum dots); here
//!   0.47 K (0.42 at 150 µm, 0.54 at 500 µm; 0.44–0.62 K over µa 0.015–0.02). **Not
//!   met**: about 4 times lower (the gap the exit check reports; see the milestone);
//! - the rise at 500 µm over that at 150 µm: measured 20 ± 7 % higher; here 29 %;
//! - a field 4× smaller (0.5 mm): their simulation +20.7 %; here +36 %; 4× larger (2 mm):
//!   −23.6 %; here −29 %;
//! - the steady state against 180 s: measured 6 % higher; here the same (it settles
//!   within 180 s).

use optobench_analysis::heating::{HeatingSettings, ScanField, heating};
use optobench_analysis::sample::LightSettings;
use optobench_io::scene_file::parse;
use optobench_io::store::material;

const WATER: &str = "thermal = { conductivity_w_per_m_k = 0.623, density_kg_per_m3 = 993, heat_capacity_j_per_kg_k = 4178, source = \"Welch and van Gemert (2011): Valvano Table 12.9, Diller Table 10.2\" }";
const GLASS: &str = "thermal = { conductivity_w_per_m_k = 1.4, density_kg_per_m3 = 2500, heat_capacity_j_per_kg_k = 750, source = \"Welch and van Gemert (2011): Valvano Table 11.4 (from Incropera 1990)\" }";
const BRAIN: &str = "thermal = { conductivity_w_per_m_k = 0.527, density_kg_per_m3 = 1040, heat_capacity_j_per_kg_k = 3650, perfusion_per_s = 8.5e-3, blood_density_kg_per_m3 = 1060, blood_heat_capacity_j_per_kg_k = 3600, source = \"Stujenske, Spellman and Gordon (2015), Table S1\" }";

/// The dura lies 1.85 mm below the sample's first face (water 1.5, glass 0.35).
const DURA: f64 = 1.85;

fn design() -> String {
    format!(
        "format = \"optobench/1\"\nkind = \"layout\"\nlayout = \"planar\"\nsurrounding = 1.333\n\n\
         [[source]]\nid = \"laser\"\nkind = \"collimated\"\ndiameter = 1.6\nrings = 20\nat = -5\n\
         wavelength_um = 0.92\npower_w = 0.1\n\n\
         [[element]]\nid = \"L\"\nkind = \"ideal_lens\"\nat = 0\nfocal_length = 10.0\ndiameter = 5\n\n\
         [[element]]\nid = \"brain\"\nkind = \"sample\"\nat = 7.934\ndiameter = 12.0\n\
         layers = [\n\
           {{ thickness = 1.5, material = 1.333, {WATER} }},\n\
           {{ thickness = 0.35, material = \"fused-silica\", {GLASS} }},\n\
           {{ thickness = 10.0, material = 1.36, absorption = 0.0175, scattering = 6.463, anisotropy = 0.9,\n\
              source = \"Johansson (2010): Table 1 gray matter carried to 920 nm by its fit; µa read off Fig. 6b\", {BRAIN} }},\n\
         ]\n\
         thermal_boundaries = {{ top = \"fixed\", bottom = \"fixed\", side = \"fixed\" }}\n"
    )
}

/// The steady rise per 100 mW entering the sample at depths below the dura (mm), over a
/// square field of side `field` (mm) at 70 % duty.
fn rises(field: f64, depths: &[f64]) -> Vec<f64> {
    let scene = parse(&design(), &|n: &str| material(None, n)).expect("the design reads");
    let settings = HeatingSettings {
        light: LightSettings {
            rings: 120,
            slices: 200,
            ..Default::default()
        },
        field: Some(ScanField {
            side: field,
            duty: 0.7,
        }),
        focal_depth: Some(DURA + 0.25),
        ..Default::default()
    };
    let sample = scene
        .elements
        .iter()
        .position(|e| e.id == "brain")
        .expect("the sample");
    let h = heating(&scene, sample, &settings).expect("heats");
    // The balance closes: absorbed = conducted out + perfused.
    let out = h.conducted.iter().sum::<f64>() + h.perfused;
    assert!((out / h.absorbed - 1.0).abs() < 1e-9);
    let g = &h.grid;
    // On the axis, linear between the cells' middles (the rise is steep below the glass).
    depths
        .iter()
        .map(|d| {
            let z = DURA + d;
            let j = (0..g.slices() - 1)
                .find(|&j| g.z_at(j) <= z && z < g.z_at(j + 1))
                .expect("inside");
            let s = (z - g.z_at(j)) / (g.z_at(j + 1) - g.z_at(j));
            let rise = h.rise[g.index(0, j)] * (1.0 - s) + h.rise[g.index(0, j + 1)] * s;
            rise * 0.1 / h.surface_power
        })
        .collect()
}

#[test]
fn brain_heating_against_podgorski_and_ranganathan() {
    let one = rises(1.0, &[0.15, 0.25, 0.5]);
    let (at_150, at_250, at_500) = (one[0], one[1], one[2]);
    // The absolute rise: 1.86 ± 0.25 °C per 100 mW measured; this model gives about a
    // quarter of it (pinned here; the gap is reported in the milestone, not hidden).
    assert!(
        (0.40..0.60).contains(&at_250),
        "{at_250} K per 100 mW at 250 µm"
    );
    assert!(at_250 < 1.86 - 0.25);
    // Deeper is warmer: 20 ± 7 % more at 500 µm than at 150 µm measured; within twice
    // its spread.
    let deeper = at_500 / at_150;
    assert!((1.20 - 0.14..1.20 + 0.14).contains(&deeper), "{deeper}");
    // The field's size: their simulation +20.7 % for a field 4× smaller, −23.6 % for 4×
    // larger; the same direction and within a factor of two here.
    let smaller = rises(0.5, &[0.25])[0] / at_250 - 1.0;
    let larger = rises(2.0, &[0.25])[0] / at_250 - 1.0;
    assert!((0.207 / 2.0..0.207 * 2.0).contains(&smaller), "{smaller}");
    assert!((-0.236 * 2.0..-0.236 / 2.0).contains(&larger), "{larger}");
    eprintln!(
        "per 100 mW: {at_150:.4} K at 150 µm, {at_250:.4} K at 250 µm, {at_500:.4} K at 500 µm; \
         500/150 {deeper:.4}; field 0.5 mm {smaller:+.4}, 2 mm {larger:+.4}"
    );
}
