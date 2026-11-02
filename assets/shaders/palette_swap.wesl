import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
import bevy_sprite_render::sprite_mesh::functions as sprite_functions;

struct Palette {
    colors: array<vec4<f32>, 3>,
}

@group(2) @binding(100)
var<uniform> palette: Palette;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let base_sample = sprite_functions::sample_sprite_texture(in.uv, in.instance_index);

    if (base_sample.a < 0.01) {
        discard;
    }

    // Map the sampled red channel to palette index 0, 1, or 2
    let index = u32(round(base_sample.r * 2.0));

    let swapped_color = palette.colors[index];

    return sprite_functions::get_final_color(swapped_color, in.instance_index);
}
