// The tissue Monte Carlo walk (optobench_analysis::photon) as a compute kernel: one
// invocation per photon packet. f32 throughout; deposits accumulated in 64-bit fixed
// point (two u32 atomics with carry), in units of 2^-30 of the largest launch weight.

struct Params {
    layers: u32,
    launches: u32,
    packets: u32,
    rings: u32,
    slices: u32,
    chance: u32,
    seed_low: u32,
    seed_high: u32,
    batch: u32,
    offset: u32,
    total: u32,
    pad0: u32,
    above: f32,
    below: f32,
    radius: f32,
    z0: f32,
    z1: f32,
    threshold: f32,
    piece: f32,
    pad1: f32,
};

@group(0) @binding(0) var<uniform> p: Params;
// Per layer: top, bottom, index, µa, µs, g.
@group(0) @binding(1) var<storage, read> layers: array<f32>;
// Per launch: x, y, z, ux, uy, uz, weight (relative to the largest, ≤ 1).
@group(0) @binding(2) var<storage, read> launches: array<f32>;
// 64-bit counters, low then high word: absorbed per cell, track per cell, then
// outside, reflected, transmitted, roulette ended, roulette added, interactions, and
// absorbed per layer.
@group(0) @binding(3) var<storage, read_write> sums: array<atomic<u32>>;

const SCALE: f32 = 1073741824.0; // 2^30

var<private> state: vec4<u32>;

fn rotl(x: u32, k: u32) -> u32 {
    return (x << k) | (x >> (32u - k));
}

// xoshiro128** (D. Blackman and S. Vigna, ACM Trans. Math. Softw. 47, 36 (2021)).
fn next() -> u32 {
    let result = rotl(state.y * 5u, 7u) * 9u;
    let t = state.y << 9u;
    state.z = state.z ^ state.x;
    state.w = state.w ^ state.y;
    state.y = state.y ^ state.z;
    state.x = state.x ^ state.w;
    state.z = state.z ^ t;
    state.w = rotl(state.w, 11u);
    return result;
}

// Uniform on [0, 1), 24 bits.
fn unit_random() -> f32 {
    return f32(next() >> 8u) * (1.0 / 16777216.0);
}

// A 32-bit mixer (the finaliser of MurmurHash3, A. Appleby, public domain).
fn mix(x0: u32) -> u32 {
    var x = x0;
    x = x ^ (x >> 16u);
    x = x * 0x85ebca6bu;
    x = x ^ (x >> 13u);
    x = x * 0xc2b2ae35u;
    x = x ^ (x >> 16u);
    return x;
}

fn seed(id: u32) {
    var h = mix(p.seed_low ^ mix(p.seed_high ^ mix(p.batch ^ mix(id + 0x9e3779b9u))));
    for (var k = 0u; k < 4u; k = k + 1u) {
        h = mix(h + 0x9e3779b9u);
        state[k] = h | 1u; // never all zero
    }
}

fn add(index: u32, value: f32) {
    if (value <= 0.0) {
        return;
    }
    let q = u32(value * SCALE + 0.5);
    let old = atomicAdd(&sums[index], q);
    if (old + q < old) {
        atomicAdd(&sums[index + 1u], 1u);
    }
}

fn cells() -> u32 {
    return p.rings * p.slices;
}

fn cell(position: vec3<f32>) -> i32 {
    let r = length(position.xy);
    if (r >= p.radius || position.z < p.z0 || position.z >= p.z1) {
        return -1;
    }
    let i = min(u32(r / (p.radius / f32(p.rings))), p.rings - 1u);
    let j = min(u32((position.z - p.z0) / ((p.z1 - p.z0) / f32(p.slices))), p.slices - 1u);
    return i32(j * p.rings + i);
}

fn scalar(k: u32) -> u32 {
    return 4u * cells() + 2u * k;
}

// Weighted track of the segment a → b, in pieces, in units of the slice thickness.
fn track(a: vec3<f32>, b: vec3<f32>, w: f32) {
    let length_ab = length(b - a);
    if (length_ab <= 0.0) {
        return;
    }
    let n = max(u32(ceil(length_ab / p.piece)), 1u);
    let step = length_ab / f32(n);
    let dz = (p.z1 - p.z0) / f32(p.slices);
    for (var k = 0u; k < min(n, 4096u); k = k + 1u) {
        let mid = a + (b - a) * ((f32(k) + 0.5) / f32(n));
        let c = cell(mid);
        if (c >= 0) {
            add(2u * cells() + 2u * u32(c), w * step / dz);
        }
    }
}

fn henyey_greenstein(g: f32, xi: f32) -> f32 {
    if (abs(g) < 1e-6) {
        return 2.0 * xi - 1.0;
    }
    let f = (1.0 - g * g) / (1.0 - g + 2.0 * g * xi);
    return clamp((1.0 + g * g - f * f) / (2.0 * g), -1.0, 1.0);
}

fn turn(u: vec3<f32>, cos_theta: f32, phi: f32) -> vec3<f32> {
    let sin_theta = sqrt(max(1.0 - cos_theta * cos_theta, 0.0));
    let sp = sin(phi);
    let cp = cos(phi);
    var v: vec3<f32>;
    if (abs(u.z) > 0.99999) {
        v = vec3<f32>(sin_theta * cp, sin_theta * sp, cos_theta * sign(u.z));
    } else {
        let t = sqrt(1.0 - u.z * u.z);
        v = vec3<f32>(
            sin_theta * (u.x * u.z * cp - u.y * sp) / t + u.x * cos_theta,
            sin_theta * (u.y * u.z * cp + u.x * sp) / t + u.y * cos_theta,
            -sin_theta * cp * t + u.z * cos_theta,
        );
    }
    return normalize(v);
}

// Unpolarised Fresnel reflectance; 1 beyond the critical angle.
fn reflectance(ci: f32, n1: f32, n2: f32) -> f32 {
    if (abs(n1 - n2) < 1e-7) {
        return 0.0;
    }
    let ratio = n1 / n2;
    let s2 = ratio * ratio * (1.0 - ci * ci);
    if (s2 >= 1.0) {
        return 1.0;
    }
    let ct = sqrt(1.0 - s2);
    let rs = (n1 * ci - n2 * ct) / (n1 * ci + n2 * ct);
    let rp = (n2 * ci - n1 * ct) / (n2 * ci + n1 * ct);
    return 0.5 * (rs * rs + rp * rp);
}

fn layer_of(z: f32) -> u32 {
    for (var k = 0u; k < p.layers; k = k + 1u) {
        if (z >= layers[6u * k] && z < layers[6u * k + 1u]) {
            return k;
        }
    }
    if (z < layers[0]) {
        return 0u;
    }
    return p.layers - 1u;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let id = p.offset + gid.x;
    if (id >= p.total) {
        return;
    }
    seed(id);
    let launch = id / p.packets;
    let base = 7u * launch;
    var position = vec3<f32>(launches[base], launches[base + 1u], launches[base + 2u]);
    var u = normalize(vec3<f32>(launches[base + 3u], launches[base + 4u], launches[base + 5u]));
    let w0 = launches[base + 6u];
    var w = w0;
    var layer = layer_of(position.z);
    var depth = -log(1.0 - unit_random());
    for (var guard = 0u; guard < 1000000u; guard = guard + 1u) {
        let l = 6u * layer;
        let top = layers[l];
        let bottom = layers[l + 1u];
        let index = layers[l + 2u];
        let mu_a = layers[l + 3u];
        let mu_s = layers[l + 4u];
        let g = layers[l + 5u];
        let mu_t = mu_a + mu_s;
        var to_boundary = 3.0e38;
        if (u.z > 0.0) {
            to_boundary = (bottom - position.z) / u.z;
        } else if (u.z < 0.0) {
            to_boundary = (top - position.z) / u.z;
        }
        var step = 3.0e38;
        if (mu_t > 0.0) {
            step = depth / mu_t;
        }
        if (step < to_boundary) {
            let q = position + u * step;
            track(position, q, w);
            position = q;
            depth = -log(1.0 - unit_random());
            add(scalar(5u), 1.0 / SCALE); // one interaction
            let dropped = w * mu_a / mu_t;
            w = w - dropped;
            add(scalar(6u + layer), dropped);
            let c = cell(position);
            if (c >= 0) {
                add(2u * u32(c), dropped);
            } else {
                add(scalar(0u), dropped);
            }
            if (w <= 0.0) {
                return;
            }
            let cos_theta = henyey_greenstein(g, unit_random());
            let phi = 6.283185307179586 * unit_random();
            u = turn(u, cos_theta, phi);
            if (w < p.threshold * w0) {
                if (unit_random() * f32(p.chance) <= 1.0) {
                    add(scalar(4u), w * f32(p.chance - 1u));
                    w = w * f32(p.chance);
                } else {
                    add(scalar(3u), w);
                    return;
                }
            }
            continue;
        }
        let q = position + u * to_boundary;
        track(position, q, w);
        position = q;
        if (mu_t > 0.0) {
            depth = depth - to_boundary * mu_t;
        }
        let downwards = u.z > 0.0;
        var leaving = false;
        var next_layer = layer;
        var n2 = 1.0;
        if (downwards) {
            if (layer + 1u < p.layers) {
                next_layer = layer + 1u;
                n2 = layers[6u * next_layer + 2u];
            } else {
                leaving = true;
                n2 = p.below;
            }
        } else {
            if (layer > 0u) {
                next_layer = layer - 1u;
                n2 = layers[6u * next_layer + 2u];
            } else {
                leaving = true;
                n2 = p.above;
            }
        }
        let ci = abs(u.z);
        if (unit_random() < reflectance(ci, index, n2)) {
            u = vec3<f32>(u.x, u.y, -u.z);
            continue;
        }
        let ratio = index / n2;
        let s2 = ratio * ratio * (1.0 - ci * ci);
        u = normalize(vec3<f32>(u.x * ratio, u.y * ratio, sqrt(max(1.0 - s2, 0.0)) * sign(u.z)));
        if (leaving) {
            if (downwards) {
                add(scalar(2u), w);
            } else {
                add(scalar(1u), w);
            }
            return;
        }
        layer = next_layer;
    }
}
