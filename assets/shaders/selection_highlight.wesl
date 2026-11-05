import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
import bevy_sprite_render::sprite_mesh::functions as sprite_functions;

@group(2) @binding(100)
var<uniform> ring_color: vec4<f32>;
@group(2) @binding(101)
var<uniform> outline_color: vec4<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let p = in.uv * 2.0 - vec2<f32>(1.0);
    let dist = length(p);
    let aa = fwidth(dist);

    let inner_edge = 0.68;
    let outer_edge = 0.84;
    let outline_width = 0.05;

    if (dist < inner_edge - outline_width || dist > outer_edge + outline_width) {
        discard;
    }

    let in_ring = smoothstep(inner_edge - aa, inner_edge + aa, dist)
        * smoothstep(outer_edge + aa, outer_edge - aa, dist);
    let color = mix(outline_color, ring_color, in_ring);

    return sprite_functions::get_final_color(color, in.instance_index);
}
