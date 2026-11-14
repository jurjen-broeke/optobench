//! Light in a sample by Monte Carlo (M15d): the walk of `optobench_analysis::photon` on
//! the GPU when one is found (`optobench_gpu`), else on the CPU's threads, for `fluence`
//! and `heating` with `transport = "monte-carlo"`.

use std::sync::OnceLock;

use optobench_analysis::photon::{WalkSettings, sample_launches, sample_tissue, walk};
use optobench_analysis::sample::{
    LightSettings, SampleLight, light_from_transport, monte_carlo_grid,
};
use optobench_scene::{Built, Scene, SceneTrace};
use serde_json::Value;

use crate::{CommandError, ErrorCode, Parameters};

/// How to get the light in a sample.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Transport {
    /// Monte Carlo (else the diffusion approximation).
    pub(crate) monte_carlo: bool,
    /// Packets per traced ray.
    pub(crate) packets: usize,
    pub(crate) seed: u64,
    /// Use a GPU if one is found.
    pub(crate) gpu: bool,
    pub(crate) threads: usize,
}

fn invalid(message: impl Into<String>) -> CommandError {
    CommandError::new(ErrorCode::InvalidParameters, message)
}

/// The transport the parameters ask for: `transport` (`diffusion`, default, or
/// `monte-carlo`), `packets` per traced ray (default 200), `seed` (default 1), `gpu`
/// (default true: use one if found) and `threads` (the CPU's, default all).
pub(crate) fn choice(parameters: &Parameters) -> Result<Transport, CommandError> {
    let monte_carlo = match parameters.get("transport").and_then(Value::as_str) {
        None | Some("diffusion") => false,
        Some("monte-carlo") => true,
        Some(other) => {
            return Err(invalid(format!(
                "`transport` `{other}` is not known; use \"diffusion\" (default) or \"monte-carlo\""
            )));
        }
    };
    let count = |key: &str, default: u64, most: u64| -> Result<u64, CommandError> {
        match parameters.get(key) {
            None => Ok(default),
            Some(v) => v
                .as_u64()
                .filter(|n| (1..=most).contains(n))
                .ok_or_else(|| invalid(format!("`{key}` must be a whole number from 1 to {most}"))),
        }
    };
    let machine = std::thread::available_parallelism().map_or(1, |n| n.get()) as u64;
    let gpu = match parameters.get("gpu") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(invalid("`gpu` is true or false")),
    };
    if !monte_carlo {
        for key in ["packets", "gpu", "threads"] {
            if parameters.contains_key(key) {
                return Err(invalid(format!(
                    "`{key}` goes with `transport = \"monte-carlo\"`"
                )));
            }
        }
    }
    Ok(Transport {
        monte_carlo,
        packets: count("packets", 200, 100_000)? as usize,
        seed: match parameters.get("seed") {
            None => 1,
            Some(v) => v
                .as_u64()
                .ok_or_else(|| invalid("`seed` must be a whole number ≥ 0"))?,
        },
        gpu,
        threads: count("threads", machine, 256)? as usize,
    })
}

/// The GPU, found once per process.
fn gpu() -> Option<&'static optobench_gpu::Gpu> {
    static GPU: OnceLock<Option<optobench_gpu::Gpu>> = OnceLock::new();
    GPU.get_or_init(optobench_gpu::Gpu::blocking).as_ref()
}

/// The light in sample `sample` by Monte Carlo, packets launched from the traced rays,
/// on the GPU if asked for and found, else on the CPU's threads (each thread a batch of
/// its own stream). `track`: record the fluence too (heating needs only absorption).
pub(crate) fn monte_carlo_light(
    scene: &Scene,
    built: &Built,
    trace: &SceneTrace,
    sample: usize,
    settings: &LightSettings,
    choice: &Transport,
    track: bool,
) -> Result<SampleLight, String> {
    let (launches, wavelength) = sample_launches(scene, built, trace, sample)?;
    if launches.is_empty() {
        return Err("no traced ray enters the sample".into());
    }
    let tissue = sample_tissue(scene, sample, wavelength)?;
    let grid = monte_carlo_grid(scene, sample, settings)?;
    let walk_settings = WalkSettings {
        packets: choice.packets,
        seed: choice.seed,
        track,
        ..Default::default()
    };
    let packets = (launches.len() * choice.packets) as u64;
    if choice.gpu {
        if let Some(g) = gpu() {
            let t = g.walk_blocking(&tissue, &grid, &launches, &walk_settings, 0)?;
            return light_from_transport(
                scene,
                sample,
                &t,
                wavelength,
                packets,
                Some(g.name.clone()),
            );
        }
    }
    // On the CPU: one batch per thread, the packets shared, each batch carrying its share
    // of the power.
    let threads = choice.threads.clamp(1, choice.packets);
    let per = choice.packets.div_ceil(threads);
    let shared: Vec<optobench_analysis::photon::Launch> = launches
        .iter()
        .map(|l| optobench_analysis::photon::Launch {
            power: l.power / threads as f64,
            ..*l
        })
        .collect();
    let parts: Vec<optobench_analysis::photon::Transport> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|b| {
                let (tissue, grid, launches) = (&tissue, &grid, &shared);
                let settings = WalkSettings {
                    packets: per,
                    ..walk_settings
                };
                scope.spawn(move || walk(tissue, grid, launches, &settings, b as u64))
            })
            .collect();
        workers.into_iter().filter_map(|w| w.join().ok()).collect()
    });
    let mut total = optobench_analysis::photon::Transport::empty(grid, tissue.layers.len());
    total.tracking = track;
    for p in &parts {
        total.absorb(p);
    }
    light_from_transport(
        scene,
        sample,
        &total,
        wavelength,
        (launches.len() * per * threads) as u64,
        None,
    )
}
