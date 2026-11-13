//! The photon budget of an emitter (M11c): where the light of a point in the sample
//! goes, from emission to the counts of a detector.
//!
//! The emitter is an isotropic source (`SourceKind::Isotropic`) placed in the sample,
//! for now at the excited volume's centroid; its power is what the fluorophores emit in
//! all directions (M11b will give it from the excitation rate and the quantum yield). It
//! is traced on its own, back through the sample's layers, an ideal objective (entered
//! at its front plane, `Role::ObjectiveFront`), the emission path with its filters
//! (measured curves) and onto detectors, which count photons by their quantum
//! efficiency.
//!
//! Every amount is given in watts and in photons per second, P λ / (h c) for each ray's
//! vacuum wavelength λ. The budget closes: what was emitted equals the part outside the
//! traced cone plus everything the trace booked (detected, clipped, absorbed, scattered,
//! reflected and not followed, escaped, stopped).

use optobench_scene::{Built, Owner, Scene, SceneTrace, SourceKind};
use optobench_trace::nonsequential::{End, Role};

/// Planck's constant (J s), exact in the SI since 2019.
pub(crate) const PLANCK: f64 = 6.626_070_15e-34;
/// The speed of light (m/s), exact.
pub(crate) const LIGHT_SPEED: f64 = 299_792_458.0;

/// An amount of light: power and photon rate.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Amount {
    /// W.
    pub watts: f64,
    /// Photons per second.
    pub photons: f64,
}

impl Amount {
    /// `watts` at vacuum wavelength `wavelength` (µm).
    pub fn of(watts: f64, wavelength: f64) -> Self {
        Self {
            watts,
            photons: watts * wavelength * 1e-6 / (PLANCK * LIGHT_SPEED),
        }
    }

    fn add(&mut self, other: Amount) {
        self.watts += other.watts;
        self.photons += other.photons;
    }
}

/// What one part of a sample took out of the emitted light.
#[derive(Debug, Clone, PartialEq)]
pub struct PartLoss {
    /// The sample's id.
    pub sample: String,
    /// `above`, `layer 1`, …, `below`.
    pub part: String,
    /// Absorbed in its bulk.
    pub absorbed: Amount,
    /// Scattered out of the rays (not followed: lost to the budget).
    pub scattered: Amount,
}

/// What one element, proxy, solid or detector took out.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemLoss {
    /// Its id.
    pub id: String,
    /// Not transmitted at its refracting faces: Fresnel reflection and a filter's
    /// rejection (reflected, followed or not, and absorbed in its coatings).
    pub at_faces: Amount,
    /// Stopped by it (outside a clear aperture, a mount, an objective's NA).
    pub clipped: Amount,
}

/// What reached one detector.
#[derive(Debug, Clone, PartialEq)]
pub struct DetectorCount {
    /// Its id.
    pub id: String,
    /// Arriving.
    pub arriving: Amount,
    /// Counted: photons times the quantum efficiency at their wavelength (watts in the
    /// same proportion); `None` if the detector has no quantum efficiency.
    pub counted: Option<Amount>,
    /// Where its quantum efficiency comes from.
    pub source: Option<String>,
}

/// Where the traced light ended, as in the trace's ledger.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ends {
    /// On detectors.
    pub detected: Amount,
    /// Clipped.
    pub clipped: Amount,
    /// Absorbed in the bulk of samples' parts.
    pub absorbed_in_samples: Amount,
    /// Absorbed in the bulk of other media (glass).
    pub absorbed_in_other_media: Amount,
    /// Absorbed in coatings (absorbing filters).
    pub absorbed_in_coatings: Amount,
    /// Scattered out of the rays in turbid media.
    pub scattered: Amount,
    /// Reflected and not followed (splitting off, or below its cutoff).
    pub reflected: Amount,
    /// Left the scene.
    pub escaped: Amount,
    /// Stopped by the interaction limit or an undefined medium.
    pub stopped: Amount,
}

/// The photon budget.
#[derive(Debug, Clone, PartialEq)]
pub struct Budget {
    /// The emitter's id.
    pub emitter: String,
    /// Its wavelengths (µm).
    pub wavelengths: Vec<f64>,
    /// Emitted in all directions.
    pub emitted: Amount,
    /// Emitted outside the traced cone (not traced: away from the objective).
    pub outside_cone: Amount,
    /// Traced.
    pub traced: Amount,
    /// Taken in by an objective (through its front plane, within its NA).
    pub collected: Amount,
    /// Per part of each sample.
    pub sample: Vec<PartLoss>,
    /// Per item that took light out, in order of first loss.
    pub items: Vec<ItemLoss>,
    /// Per detector, in scene order.
    pub detectors: Vec<DetectorCount>,
    /// Where the traced light ended.
    pub ends: Ends,
    /// emitted − (outside the cone + every end) (W); zero up to rounding.
    pub imbalance: f64,
}

pub(crate) fn owner_id(scene: &Scene, owner: Owner) -> &str {
    match owner {
        Owner::Element(i) => &scene.elements[i].id,
        Owner::Proxy(i) => &scene.proxies[i].id,
        Owner::Detector(i) => &scene.detectors[i].id,
        Owner::Solid(i) => &scene.solids[i].id,
    }
}

/// The entry for item `id`, added if new.
fn item(items: &mut Vec<ItemLoss>, id: &str) -> usize {
    match items.iter().position(|i| i.id == id) {
        Some(k) => k,
        None => {
            items.push(ItemLoss {
                id: id.to_owned(),
                at_faces: Amount::default(),
                clipped: Amount::default(),
            });
            items.len() - 1
        }
    }
}

/// The photon budget of source `emitter` (an index into the scene's sources, an
/// isotropic source), traced on its own.
pub fn budget(scene: &Scene, emitter: usize) -> Result<Budget, String> {
    let source = scene.sources.get(emitter).ok_or("no such source")?;
    let SourceKind::Isotropic { half_angle } = source.kind else {
        return Err(format!(
            "`{}` is not an isotropic source (kind = \"isotropic\"): an emitter radiates in every direction",
            source.id
        ));
    };
    let mut alone = scene.clone();
    alone.sources = vec![source.clone()];
    let built: Built = alone.build().map_err(|e| e.to_string())?;
    let trace: SceneTrace = alone.trace_built(&built);

    // The lines and their share of the power, as `Scene::launch` shares it.
    let lines: Vec<(f64, f64)> = if source.spectrum.is_empty() {
        vec![(source.wavelength, 1.0)]
    } else {
        let total: f64 = source.spectrum.iter().map(|l| l.1).sum();
        source
            .spectrum
            .iter()
            .map(|&(w, p)| (w, p / total))
            .collect()
    };
    let cone = (1.0 - half_angle.min(core::f64::consts::PI).cos()) / 2.0;
    let mut emitted = Amount::default();
    let mut outside_cone = Amount::default();
    for &(wavelength, share) in &lines {
        emitted.add(Amount::of(source.power * share, wavelength));
        outside_cone.add(Amount::of(source.power * share * (1.0 - cone), wavelength));
    }

    let faces = &built.system.faces;
    let sample_medium =
        |medium: usize| built.sample_media.iter().position(|(_, _, m)| *m == medium);
    let mut traced = Amount::default();
    let mut collected = Amount::default();
    let mut ends = Ends::default();
    let mut sample: Vec<PartLoss> = built
        .sample_media
        .iter()
        .map(|(element, part, _)| PartLoss {
            sample: alone.elements[*element].id.clone(),
            part: part.clone(),
            absorbed: Amount::default(),
            scattered: Amount::default(),
        })
        .collect();
    let mut items: Vec<ItemLoss> = Vec::new();
    let mut detectors: Vec<DetectorCount> = alone
        .detectors
        .iter()
        .map(|d| DetectorCount {
            id: d.id.clone(),
            arriving: Amount::default(),
            counted: d.quantum_efficiency.as_ref().map(|_| Amount::default()),
            source: d.quantum_efficiency.as_ref().map(|q| q.source.clone()),
        })
        .collect();

    for record in &trace.records {
        let wavelength = record.launched.wavelength;
        let of = |watts: f64| Amount::of(watts, wavelength);
        if record.generation == 0 {
            traced.add(of(record.launched.power));
        }
        // An objective passed backwards: its front face twice in a row.
        for k in 1..record.faces.len() {
            let f = record.faces[k];
            if record.faces[k - 1] == f && matches!(faces[f].role, Role::ObjectiveFront { .. }) {
                collected.add(of(record.powers[k]));
            }
        }
        // Bulk deposits.
        let mut bulk_absorbed = 0.0;
        for d in &record.deposits {
            bulk_absorbed += d.absorbed;
            match sample_medium(d.medium) {
                Some(p) => {
                    sample[p].absorbed.add(of(d.absorbed));
                    sample[p].scattered.add(of(d.scattered));
                    ends.absorbed_in_samples.add(of(d.absorbed));
                }
                None => ends.absorbed_in_other_media.add(of(d.absorbed)),
            }
        }
        ends.absorbed_in_coatings
            .add(of((record.absorbed_power - bulk_absorbed).max(0.0)));
        ends.scattered.add(of(record.scattered_power));
        ends.reflected
            .add(of(record.reflected_power + record.below_cutoff_power));
        for &(face, lost) in &record.face_losses {
            let k = item(
                &mut items,
                owner_id(&alone, built.owners[faces[face].owner]),
            );
            items[k].at_faces.add(of(lost));
        }
        let remaining = record.ray.power;
        match record.end {
            End::Detected { detector, .. } => {
                ends.detected.add(of(remaining));
                let count = &mut detectors[detector];
                count.arriving.add(of(remaining));
                if let (Some(counted), Some(q)) = (
                    count.counted.as_mut(),
                    alone.detectors[detector].quantum_efficiency.as_ref(),
                ) {
                    let efficiency = q.curve.at(wavelength).map_err(|e| {
                        format!(
                            "detector `{}`: no quantum efficiency at {wavelength} µm ({e})",
                            count.id
                        )
                    })?;
                    counted.add(of(remaining * efficiency));
                }
            }
            End::Clipped { face } => {
                ends.clipped.add(of(remaining));
                let k = item(
                    &mut items,
                    owner_id(&alone, built.owners[faces[face].owner]),
                );
                items[k].clipped.add(of(remaining));
            }
            End::Escaped => ends.escaped.add(of(remaining)),
            End::Stopped | End::MaterialUndefined { .. } => ends.stopped.add(of(remaining)),
        }
    }
    let booked = [
        ends.detected,
        ends.clipped,
        ends.absorbed_in_samples,
        ends.absorbed_in_other_media,
        ends.absorbed_in_coatings,
        ends.scattered,
        ends.reflected,
        ends.escaped,
        ends.stopped,
    ]
    .iter()
    .map(|a| a.watts)
    .sum::<f64>();
    let imbalance = emitted.watts - outside_cone.watts - booked;
    Ok(Budget {
        emitter: source.id.clone(),
        wavelengths: lines.iter().map(|l| l.0).collect(),
        emitted,
        outside_cone,
        traced,
        collected,
        sample,
        items,
        detectors,
        ends,
        imbalance,
    })
}

impl Budget {
    /// Adds another budget of the same emitter in the same scene (another place of it).
    fn absorb(&mut self, other: &Budget) {
        self.emitted.add(other.emitted);
        self.outside_cone.add(other.outside_cone);
        self.traced.add(other.traced);
        self.collected.add(other.collected);
        for (mine, theirs) in self.sample.iter_mut().zip(&other.sample) {
            mine.absorbed.add(theirs.absorbed);
            mine.scattered.add(theirs.scattered);
        }
        for theirs in &other.items {
            let k = item(&mut self.items, &theirs.id);
            self.items[k].at_faces.add(theirs.at_faces);
            self.items[k].clipped.add(theirs.clipped);
        }
        for (mine, theirs) in self.detectors.iter_mut().zip(&other.detectors) {
            mine.arriving.add(theirs.arriving);
            if let (Some(a), Some(b)) = (mine.counted.as_mut(), theirs.counted) {
                a.add(b);
            }
        }
        let (e, o) = (&mut self.ends, &other.ends);
        for (a, b) in [
            (&mut e.detected, o.detected),
            (&mut e.clipped, o.clipped),
            (&mut e.absorbed_in_samples, o.absorbed_in_samples),
            (&mut e.absorbed_in_other_media, o.absorbed_in_other_media),
            (&mut e.absorbed_in_coatings, o.absorbed_in_coatings),
            (&mut e.scattered, o.scattered),
            (&mut e.reflected, o.reflected),
            (&mut e.escaped, o.escaped),
            (&mut e.stopped, o.stopped),
        ] {
            a.add(b);
        }
        self.imbalance += other.imbalance;
    }
}

/// The photon budget of emitter `emitter` (an isotropic source) spread through an excited
/// volume: placed in turn at each of `points` (scene frame, shares adding up to 1), with
/// `photons` emitted per second in all (each place its share), its own direction, cone,
/// spectrum and sampling kept; the budgets added up. The power per photon is the mean
/// over the emitter's spectral lines, h c Σ wᵢ / λᵢ.
pub fn budget_through(
    scene: &Scene,
    emitter: usize,
    points: &[(optobench_math::Vec3, f64)],
    photons: f64,
) -> Result<Budget, String> {
    let source = scene.sources.get(emitter).ok_or("no such source")?;
    if points.is_empty() {
        return Err("no emitter places: the excited volume is empty".into());
    }
    let lines: Vec<(f64, f64)> = if source.spectrum.is_empty() {
        vec![(source.wavelength, 1.0)]
    } else {
        let total: f64 = source.spectrum.iter().map(|l| l.1).sum();
        source
            .spectrum
            .iter()
            .map(|&(w, p)| (w, p / total))
            .collect()
    };
    let energy: f64 = lines
        .iter()
        .map(|&(w, share)| share * PLANCK * LIGHT_SPEED / (w * 1e-6))
        .sum();
    let [x, _, z] = source.frame.axes();
    let mut total: Option<Budget> = None;
    for &(point, share) in points {
        let mut moved = scene.clone();
        let placed = &mut moved.sources[emitter];
        placed.frame =
            optobench_math::Frame::new(point, z, x).ok_or("a degenerate emitter frame")?;
        placed.power = photons * share * energy;
        let b = budget(&moved, emitter)?;
        match total.as_mut() {
            None => total = Some(b),
            Some(t) => t.absorb(&b),
        }
    }
    total.ok_or_else(|| "no emitter places".into())
}
