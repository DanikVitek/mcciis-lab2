pub mod camera;
pub mod pacing;
pub mod sprites;
pub mod ui;

use std::time::Duration;

use bevy::prelude::*;

pub use self::pacing::{AppState, SimulationPlaybackState, SimulationSpeedMode};
use self::{
    camera::{CameraPlugin, CameraSystems},
    pacing::SimulationPacingPlugin,
    sprites::SpritesPlugin,
    ui::{UiPlugin, UiSystems},
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
        .add_plugins((
            SimulationPacingPlugin::new(self.initial_tick_interval),
            SpritesPlugin,
            CameraPlugin,
            UiPlugin,
        ));
    }
}
