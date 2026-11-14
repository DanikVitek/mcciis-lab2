pub mod camera;
pub mod pacing;
pub mod sprites;
pub mod ui;

use std::time::Duration;

use bevy::asset::AssetEventSystems;
use bevy::prelude::*;
use bevy_egui::EguiPostUpdateSet;

use self::camera::{
    CameraPlugin,
    CameraSystems,
};
use self::pacing::SimulationPacingPlugin;
pub use self::pacing::{
    AppState,
    SimulationPlaybackState,
    SimulationSpeedMode,
};
use self::sprites::{
    SpriteTransformSystems,
    SpritesPlugin,
};
use self::ui::{
    UiPlugin,
    UiSystems,
};

/// Top-level presentation plugin bundling visual pacing, sprites, camera controls, and UI.
pub struct PresentationPlugin {
    pub initial_tick_interval: Duration,
}

impl PresentationPlugin {
    pub const fn new(initial_tick_interval: Duration) -> Self {
        Self {
            initial_tick_interval,
        }
    }
}

impl Default for PresentationPlugin {
    fn default() -> Self {
        Self::new(Duration::from_secs_f64(0.5))
    }
}

impl Plugin for PresentationPlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            Update,
            (
                UiSystems::Config,
                UiSystems::Inspector.run_if(in_state(AppState::Simulation)),
                CameraSystems.run_if(in_state(AppState::Simulation)),
            )
                .chain_weak(),
        )
        .configure_sets(
            PostUpdate,
            (
                AssetEventSystems,
                EguiPostUpdateSet::EndPass,
                SpriteTransformSystems,
            )
                .chain(),
        )
        .add_plugins((
            SimulationPacingPlugin::new(self.initial_tick_interval),
            SpritesPlugin,
            CameraPlugin,
            UiPlugin,
        ));
    }
}
