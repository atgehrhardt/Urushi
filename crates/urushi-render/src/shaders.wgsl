// Quads: rounded, bordered rectangles drawn with a signed distance field.
// Glyphs: textured quads sampling an RGBA atlas (masks are white + alpha).

struct Globals {
    screen: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

fn to_clip(p: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(p.x / globals.screen.x * 2.0 - 1.0, 1.0 - p.y / globals.screen.y * 2.0, 0.0, 1.0);
}

fn corner(index: u32) -> vec2<f32> {
    // Triangle strip: (0,0) (1,0) (0,1) (1,1)
    return vec2<f32>(f32(index & 1u), f32(index >> 1u));
}

struct QuadIn {
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
    @location(2) border_color: vec4<f32>,
    @location(3) params: vec2<f32>, // radius, border width
};

struct QuadOut {
    @builtin(position) position: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) params: vec2<f32>,
};

@vertex
fn quad_vs(@builtin(vertex_index) vi: u32, q: QuadIn) -> QuadOut {
    let c = corner(vi);
    let pos = q.rect.xy + c * q.rect.zw;
    var out: QuadOut;
    out.position = to_clip(pos);
    out.half_size = q.rect.zw * 0.5;
    out.local = c * q.rect.zw - out.half_size;
    out.color = q.color;
    out.border_color = q.border_color;
    out.params = q.params;
    return out;
}

fn rounded_box(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
    let r = min(radius, min(half_size.x, half_size.y));
    let q = abs(p) - (half_size - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

@fragment
fn quad_fs(in: QuadOut) -> @location(0) vec4<f32> {
    let d = rounded_box(in.local, in.half_size, in.params.x);
    let coverage = clamp(0.5 - d, 0.0, 1.0);
    var color = in.color;
    if (in.params.y > 0.0) {
        // Inside the border band (from the edge inwards by border width) use the border colour.
        let inner = clamp(0.5 - (d + in.params.y), 0.0, 1.0);
        color = mix(in.border_color, in.color, inner);
    }
    return vec4<f32>(color.rgb, color.a * coverage);
}

@group(1) @binding(0) var atlas: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct GlyphIn {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) is_color: f32,
};

struct GlyphOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) is_color: f32,
};

@vertex
fn glyph_vs(@builtin(vertex_index) vi: u32, g: GlyphIn) -> GlyphOut {
    let c = corner(vi);
    var out: GlyphOut;
    out.position = to_clip(g.rect.xy + c * g.rect.zw);
    out.uv = mix(g.uv.xy, g.uv.zw, c);
    out.color = g.color;
    out.is_color = g.is_color;
    return out;
}

@fragment
fn glyph_fs(in: GlyphOut) -> @location(0) vec4<f32> {
    let texel = textureSample(atlas, atlas_sampler, in.uv);
    if (in.is_color > 0.5) {
        return vec4<f32>(texel.rgb, texel.a * in.color.a);
    }
    return vec4<f32>(in.color.rgb, in.color.a * texel.a);
}
