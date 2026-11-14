//! GPU compute (M15b): the tissue Monte Carlo walk of `optobench_analysis::photon` as a
//! wgpu compute kernel, one invocation per photon packet.
//!
//! Layer L4. The GPU is optional: [`Gpu::new`] gives `None` when no adapter is found, and
//! callers then run the CPU walk, which gives the same results within their statistical
//! errors (the check of M15b). The kernel (`walk.wgsl`) follows the CPU walk step by step
//! in f32 with xoshiro128** for its random numbers (the CPU uses xoshiro256** in f64), so
//! the two agree in distribution, not packet by packet. Deposits are summed in 64-bit
//! fixed point (2⁻³⁰ of the largest launch weight; two u32 atomics with carry), so the sums
//! do not depend on the order the invocations finish in.

#![forbid(unsafe_code)]

use optobench_analysis::diffusion::RzGrid;
use optobench_analysis::photon::{Launch, Tissue, Transport, WalkSettings};

/// The fixed point's unit, 2⁻³⁰ of the largest launch weight.
const SCALE: f64 = (1u64 << 30) as f64;

/// Invocations per dispatch, at most (65 535 workgroups of 64).
const PER_DISPATCH: u32 = 65_535 * 64;

/// A GPU ready to walk photons.
pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    /// The adapter's name, for reports.
    pub name: String,
}

fn storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn bytes_f32(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

impl Gpu {
    /// The first adapter the platform offers, with the kernel compiled; `None` without one.
    pub async fn new() -> Option<Gpu> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .ok()?;
        let name = adapter.get_info().name;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("optobench"),
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .ok()?;
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("walk"),
            source: wgpu::ShaderSource::Wgsl(include_str!("walk.wgsl").into()),
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("walk"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                storage(1, true),
                storage(2, true),
                storage(3, false),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("walk"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("walk"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Some(Gpu {
            device,
            queue,
            pipeline,
            layout,
            name,
        })
    }

    /// [`Gpu::new`], waited for (not on the web).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn blocking() -> Option<Gpu> {
        pollster::block_on(Self::new())
    }

    /// The walk of `optobench_analysis::photon::walk` on the GPU: `settings.packets`
    /// packets per launch through `tissue`, recorded on `grid`, the stream picked by
    /// `batch`.
    pub async fn walk(
        &self,
        tissue: &Tissue,
        grid: &RzGrid,
        launches: &[Launch],
        settings: &WalkSettings,
        batch: u64,
    ) -> Result<Transport, String> {
        let layers = tissue.layers.len();
        let cells = grid.cells();
        let total = launches.len() as u64 * settings.packets as u64;
        if layers == 0 || total == 0 {
            return Ok(empty(*grid, layers));
        }
        let total = u32::try_from(total).map_err(|_| "too many packets for one GPU run")?;
        // Packet weights relative to the largest.
        let unit = launches
            .iter()
            .map(|l| l.power / settings.packets as f64)
            .fold(0.0, f64::max);
        if unit <= 0.0 {
            return Ok(empty(*grid, layers));
        }
        let layer_data: Vec<f32> = tissue
            .layers
            .iter()
            .flat_map(|l| {
                [
                    l.top as f32,
                    l.bottom as f32,
                    l.index as f32,
                    l.absorption as f32,
                    l.scattering as f32,
                    l.anisotropy as f32,
                ]
            })
            .collect();
        let launch_data: Vec<f32> = launches
            .iter()
            .flat_map(|l| {
                let d = l.direction;
                [
                    l.position.x as f32,
                    l.position.y as f32,
                    l.position.z as f32,
                    d.x as f32,
                    d.y as f32,
                    d.z as f32,
                    (l.power / settings.packets as f64 / unit) as f32,
                ]
            })
            .collect();
        let counters = 4 * cells + 2 * (6 + layers);
        let size = (counters * 4) as u64;
        let buffer = |label: &str, data: &[u8], usage: wgpu::BufferUsages| {
            let b = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: data.len().max(4) as u64,
                usage: usage | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.queue.write_buffer(&b, 0, data);
            b
        };
        let layer_buffer = buffer(
            "layers",
            &bytes_f32(&layer_data),
            wgpu::BufferUsages::STORAGE,
        );
        let launch_buffer = buffer(
            "launches",
            &bytes_f32(&launch_data),
            wgpu::BufferUsages::STORAGE,
        );
        let sums = buffer(
            "sums",
            &vec![0u8; counters * 4],
            wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        );
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let seed = settings.seed;
        let batch32 = (batch ^ (batch >> 32)) as u32;
        let mut offset = 0u32;
        while offset < total {
            let count = (total - offset).min(PER_DISPATCH);
            let mut params: Vec<u8> = Vec::with_capacity(96);
            for v in [
                layers as u32,
                launches.len() as u32,
                settings.packets as u32,
                grid.rings as u32,
                grid.slices as u32,
                settings.chance,
                seed as u32,
                (seed >> 32) as u32,
                batch32,
                offset,
                total,
                u32::from(settings.track),
            ] {
                params.extend_from_slice(&v.to_le_bytes());
            }
            for v in [
                tissue.above as f32,
                tissue.below as f32,
                grid.radius as f32,
                grid.z.0 as f32,
                grid.z.1 as f32,
                settings.threshold as f32,
                (0.5 * grid.dr().min(grid.dz())) as f32,
                0.0,
            ] {
                params.extend_from_slice(&v.to_le_bytes());
            }
            let uniform = buffer("params", &params, wgpu::BufferUsages::UNIFORM);
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("walk"),
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: layer_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: launch_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: sums.as_entire_binding(),
                    },
                ],
            });
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("walk"),
                });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("walk"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &group, &[]);
                pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
            }
            self.queue.submit([encoder.finish()]);
            offset += count;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("read"),
            });
        encoder.copy_buffer_to_buffer(&sums, 0, &readback, 0, size);
        self.queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback.map_async(wgpu::MapMode::Read, .., move |r| {
            let _ = sender.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| format!("the GPU did not finish: {e}"))?;
        receiver
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("reading the GPU's results: {e}"))?;
        let words: Vec<u32> = {
            let view = readback
                .get_mapped_range(..)
                .map_err(|e| format!("reading the GPU's results: {e:?}"))?;
            view.chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        };
        readback.unmap();
        let at = |k: usize| -> f64 {
            let value = (words[2 * k] as u64) | ((words[2 * k + 1] as u64) << 32);
            value as f64 / SCALE * unit
        };
        let mut out = empty(*grid, layers);
        out.tracking = settings.track;
        out.launched = launches.iter().map(|l| l.power).sum();
        for c in 0..cells {
            out.absorbed[c] = at(c);
            out.track[c] = at(cells + c) * grid.dz();
        }
        let scalar = |k: usize| at(2 * cells + k);
        out.absorbed_outside = scalar(0);
        out.reflected = scalar(1);
        out.transmitted = scalar(2);
        out.roulette = scalar(3) - scalar(4);
        // Each interaction added one unit of the fixed point.
        out.interactions = (scalar(5) / unit * SCALE).round() as u64;
        for (k, absorbed) in out.absorbed_by_layer.iter_mut().enumerate() {
            *absorbed = scalar(6 + k);
        }
        Ok(out)
    }

    /// [`Gpu::walk`], waited for (not on the web).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn walk_blocking(
        &self,
        tissue: &Tissue,
        grid: &RzGrid,
        launches: &[Launch],
        settings: &WalkSettings,
        batch: u64,
    ) -> Result<Transport, String> {
        pollster::block_on(self.walk(tissue, grid, launches, settings, batch))
    }
}

fn empty(grid: RzGrid, layers: usize) -> Transport {
    Transport::empty(grid, layers)
}
