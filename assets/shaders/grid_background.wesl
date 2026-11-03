import bevy_sprite_render::mesh2d::vertex_output::VertexOutput;
import bevy_sprite_render::sprite_mesh::functions as sprite_functions;

@group(2) @binding(100)
var<uniform> cell_color: vec4<f32>;
@group(2) @binding(101)
var<uniform> border_color: vec4<f32>;
@group(2) @binding(102)
var<uniform> grid_size: vec2<f32>;

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let cell_uv = in.uv * grid_size;
    let f = fract(cell_uv);
    let dist_to_border = min(f, 1.0 - f);
    let aa = fwidth(cell_uv);
    let border_half_width = vec2<f32>(0.045);
    let inner = smoothstep(border_half_width - aa * 0.5, border_half_width + aa * 0.5, dist_to_border);
    let cell_mask = inner.x * inner.y;
    let color = mix(border_color, cell_color, cell_mask);
    return sprite_functions::get_final_color(color, in.instance_index);
}
