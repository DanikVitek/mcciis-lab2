pub mod camera;
pub mod character;
pub mod grid;
pub mod world;

use bevy::prelude::*;

use self::{camera::GameCameraPlugin, grid::GridPlugin, world::GameWorldPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .add_plugins((GridPlugin, GameWorldPlugin, GameCameraPlugin))
        .run();
}
