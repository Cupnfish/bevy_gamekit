// Adds a two-pixel black outline around opaque pixels in a rendered layer.

@group(0) @binding(0) var material_texture: texture_2d<f32>;
@group(0) @binding(1) var material_sampler: sampler;

@fragment
fn fragment(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    let src = textureSampleLevel(material_texture, material_sampler, uv, 0.0);
    let size = textureDimensions(material_texture);
    let px = 1.0 / vec2<f32>(size);

    // Linear samples halfway between texels collapse the 5x5 kernel exactly:
    // corners cover four texels, edge samples cover two, and src is the center.
    // This keeps the generated GLES shader small enough for Mini Game drivers.
    let corners =
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>(-1.5, -1.5) * px, 0.0).a +
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>( 1.5, -1.5) * px, 0.0).a +
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>(-1.5,  1.5) * px, 0.0).a +
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>( 1.5,  1.5) * px, 0.0).a;
    let edges =
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>( 0.0, -1.5) * px, 0.0).a +
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>( 0.0,  1.5) * px, 0.0).a +
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>(-1.5,  0.0) * px, 0.0).a +
        textureSampleLevel(material_texture, material_sampler, uv + vec2<f32>( 1.5,  0.0) * px, 0.0).a;
    let neighbor_a = min(corners * 4.0 + edges * 2.0 + src.a, 1.0);

    let outline_a = clamp(neighbor_a - src.a, 0.0, 1.0);
    let rgb = mix(vec3<f32>(0.0), src.rgb, src.a);
    return vec4<f32>(rgb, max(outline_a, src.a));
}
