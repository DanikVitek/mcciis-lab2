use bevy::prelude::*;
use bevy::{
    camera,
    input::mouse::{AccumulatedMouseMotion, MouseScrollUnit, MouseWheel},
    window::PrimaryWindow,
};

use crate::grid::{GridSize, TILE_SIZE};

/// Dark-grassy green (#337c37)
const BACKGROUND_COLOR: Color = Color::oklch(0.5221, 0.1258, 144.53);

const ZOOM_SCALE_MIN: f32 = 0.1;
const ZOOM_SCALE_MAX: f32 = 1.0;

pub struct GameCameraPlugin;

impl Plugin for GameCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_camera)
            .add_systems(Update, (zoom_camera_to_cursor, pan_camera_on_drag));
    }
}

fn setup_camera(mut commands: Commands, field_size: Res<GridSize>) {
    let world_size = TILE_SIZE * field_size.as_vec2();

    commands.spawn((
        Camera2d,
        Camera {
            clear_color: ClearColorConfig::Custom(BACKGROUND_COLOR),
            ..default()
        },
        Projection::Orthographic(OrthographicProjection {
            scaling_mode: camera::ScalingMode::AutoMin {
                min_width: world_size.x,
                min_height: world_size.y,
            },
            scale: ZOOM_SCALE_MAX,
            ..OrthographicProjection::default_2d()
        }),
    ));
}

fn zoom_camera_to_cursor(
    mut mouse_wheel: MessageReader<MouseWheel>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera_query: Single<
        (&Camera, &GlobalTransform, &mut Projection, &mut Transform),
        With<Camera2d>,
    >,
    field_size: Res<GridSize>,
) {
    let mut scroll_delta = 0.0;
    for msg in mouse_wheel.read() {
        scroll_delta += match msg.unit {
            MouseScrollUnit::Line => msg.y * 0.12,
            MouseScrollUnit::Pixel => msg.y * 0.0015,
        };
    }

    if scroll_delta == 0.0 {
        return;
    }

    let (camera, camera_global, mut projection, mut camera_transform) = camera_query.into_inner();
    let Projection::Orthographic(ref mut projection) = *projection else {
        return;
    };

    let old_scale = projection.scale;

    // Exponentional zoom factor guarantees symmetrical zoom-in and zoom-out steps
    let new_scale = (old_scale * (-scroll_delta).exp()).clamp(ZOOM_SCALE_MIN, ZOOM_SCALE_MAX);

    if (new_scale - old_scale).abs() < f32::EPSILON {
        return;
    }

    // 1. Shift camera toward cursor if the cursor is inside the window
    let scale_ratio = new_scale / old_scale;
    if let Some(cursor_screen_pos) = window.cursor_position()
        && let Ok(cursor_world_pos) = camera.viewport_to_world_2d(camera_global, cursor_screen_pos)
    {
        let camera_xy = camera_transform.translation.truncate();
        let translation_delta = (cursor_world_pos - camera_xy) * (1.0 - scale_ratio);

        camera_transform.translation += translation_delta.extend(0.0);
    }

    // 2. Apply the new zoom scale
    projection.scale = new_scale;

    // 3. Clamp camera center so the camera cannot pan past the grid edges when zooming out.
    // `projection.area` holds the unscaled base bounds * `old_scale`, so we rescale it to `new_scale`
    let new_viewport_size = projection.area.size() * scale_ratio;
    clamp_camera_to_grid(&mut camera_transform, new_viewport_size, &field_size);
}

fn pan_camera_on_drag(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    camera_query: Single<(&Camera, &Projection, &mut Transform), With<Camera2d>>,
    field_size: Res<GridSize>,
) {
    // Pan while holding Left or Middle mouse button
    let is_dragging =
        mouse_buttons.pressed(MouseButton::Left) || mouse_buttons.pressed(MouseButton::Middle);

    if !is_dragging || mouse_motion.delta == Vec2::ZERO {
        return;
    }

    let (camera, projection, mut camera_transform) = camera_query.into_inner();
    let Projection::Orthographic(ref projection) = *projection else {
        return;
    };

    let Some(viewport_px) = camera.logical_viewport_size() else {
        return;
    };

    // World units per logical screen pixel (accounts for ScalingMode::AutoMin and ortho.scale)
    let world_units_per_px = projection.area.size() / viewport_px;

    // Screen +Y is down, World +Y is up; dragging the mouse right/down moves the camera left/up:
    camera_transform.translation.x -= mouse_motion.delta.x * world_units_per_px.x;
    camera_transform.translation.y += mouse_motion.delta.y * world_units_per_px.y;

    clamp_camera_to_grid(&mut camera_transform, projection.area.size(), &field_size);
}

/// Clamps the camera translation so the viewport never scrolls past the outer grid edges.
/// `viewport_world_size` is the visible world area at the current `ortho.scale`.
fn clamp_camera_to_grid(
    camera_transform: &mut Transform,
    viewport_world_size: Vec2,
    field_size: &GridSize,
) {
    let world_size = field_size.as_vec2() * TILE_SIZE;
    let max_offset = ((world_size - viewport_world_size) / 2.0).max(Vec2::ZERO);

    camera_transform.translation.x = camera_transform
        .translation
        .x
        .clamp(-max_offset.x, max_offset.x);
    camera_transform.translation.y = camera_transform
        .translation
        .y
        .clamp(-max_offset.y, max_offset.y);
}
