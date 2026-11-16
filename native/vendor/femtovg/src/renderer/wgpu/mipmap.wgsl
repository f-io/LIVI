// Mipmap generation: each level is the level above sampled bilinearly at
// its own texel centres, the 2x2 box average glGenerateMipmap produces.

struct MipVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@group(0) @binding(0) var mip_source: texture_2d<f32>;
@group(0) @binding(1) var mip_sampler: sampler;

// One triangle covering the level: uv (0,0), (2,0), (0,2).
@vertex
fn vs_mipmap(@builtin(vertex_index) index: u32) -> MipVertex {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return MipVertex(vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0), uv);
}

@fragment
fn fs_mipmap(vertex: MipVertex) -> @location(0) vec4<f32> {
    return textureSample(mip_source, mip_sampler, vertex.uv);
}
