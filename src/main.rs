pub mod camera;
pub mod character;
pub mod grid;
pub mod world;

use bevy::prelude::*;

use self::{character::SpatialGrid, grid::GridSize, world::DistributionConfig};

fn main() {
    let grid_size = GridSize::new(100, 100);
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_systems(Startup, (camera::system::setup, world::system::setup))
        .add_systems(
            Update,
            (
                camera::system::zoom_camera_to_cursor,
                camera::system::pan_camera_on_drag,
            ),
        )
        .add_systems(FixedUpdate, (character::system::step_characters,))
        .add_systems(
            FixedPostUpdate,
            (
                grid::system::sync_translation_with_grid,
                grid::system::sync_rotation_with_grid,
            )
                .before(TransformSystems::Propagate),
        )
        .insert_resource(Time::<Fixed>::from_seconds(0.5))
        .insert_resource(
            DistributionConfig::builder()
                .plants_fraction(0.1)
                .herbivores_fraction(0.1)
                .predators_fraction(0.1)
                .build()
                .unwrap(),
        )
        .insert_resource(SpatialGrid::new(grid_size))
        .insert_resource(grid_size)
        .run();
}
