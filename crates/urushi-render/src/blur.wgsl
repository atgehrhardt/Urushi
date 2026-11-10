// Frosted glass. A frame with backdrops is drawn into an offscreen texture. At each
// backdrop, what's drawn so far is blurred with a dual-Kawase chain (down to 1/2, 1/4,
// … resolution, then back up to 1/2), and the blurred copy is drawn inside the
// backdrop's rounded rectangle. At the end the texture is copied to the target.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var src_sampler: sampler;

struct FullOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// One triangle that covers the whole target.
@vertex
fn full_vs(@builtin(vertex_index) vi: u32) -> FullOut {
    let p = vec2<f32>(f32((vi << 1u) & 2u), f32(vi & 2u));
    var out: FullOut;
    out.position = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(p.x, 1.0 - p.y);
    return out;
}

fn texel() -> vec2<f32> {
    return 1.0 / vec2<f32>(textureDimensions(src));
}

// Halves the resolution: the centre plus four diagonal taps, each a bilinear average.
@fragment
fn down_fs(in: FullOut) -> @location(0) vec4<f32> {
    let o = texel();
    var sum = textureSample(src, src_sampler, in.uv) * 4.0;
    sum += textureSample(src, src_sampler, in.uv - o);
    sum += textureSample(src, src_sampler, in.uv + o);
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(o.x, -o.y));
    sum += textureSample(src, src_sampler, in.uv - vec2<f32>(o.x, -o.y));
    return sum / 8.0;
}

// Doubles the resolution with an eight-tap tent.
@fragment
fn up_fs(in: FullOut) -> @location(0) vec4<f32> {
    let o = texel();
    var sum = textureSample(src, src_sampler, in.uv + vec2<f32>(-2.0 * o.x, 0.0));
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(-o.x, o.y)) * 2.0;
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(0.0, 2.0 * o.y));
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(o.x, o.y)) * 2.0;
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(2.0 * o.x, 0.0));
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(o.x, -o.y)) * 2.0;
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(0.0, -2.0 * o.y));
    sum += textureSample(src, src_sampler, in.uv + vec2<f32>(-o.x, -o.y)) * 2.0;
    return sum / 12.0;
}

// Copies the finished frame to the target.
@fragment
fn blit_fs(in: FullOut) -> @location(0) vec4<f32> {
    return textureSample(src, src_sampler, in.uv);
}

struct Globals {
    screen: vec2<f32>,
};

@group(1) @binding(0) var<uniform> globals: Globals;

struct BackdropIn {
    @location(0) rect: vec4<f32>,
    @location(1) params: vec4<f32>, // radius, unused × 3
};

struct BackdropOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) local: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) radius: f32,
};

@vertex
fn composite_vs(@builtin(vertex_index) vi: u32, b: BackdropIn) -> BackdropOut {
    let c = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let pos = b.rect.xy + c * b.rect.zw;
    var out: BackdropOut;
    out.position = vec4<f32>(
        pos.x / globals.screen.x * 2.0 - 1.0,
        1.0 - pos.y / globals.screen.y * 2.0,
        0.0,
        1.0,
    );
    out.uv = pos / globals.screen;
    out.half_size = b.rect.zw * 0.5;
    out.local = c * b.rect.zw - out.half_size;
    out.radius = b.params.x;
    return out;
}

@fragment
fn composite_fs(in: BackdropOut) -> @location(0) vec4<f32> {
    let r = min(in.radius, min(in.half_size.x, in.half_size.y));
    let q = abs(in.local) - (in.half_size - vec2<f32>(r));
    let d = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    let color = textureSample(src, src_sampler, in.uv);
    return vec4<f32>(color.rgb, coverage);
}
