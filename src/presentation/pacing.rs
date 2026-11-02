use std::time::Duration;

use bevy::prelude::*;

use crate::simulation::SimulationTick;

/// Visual playback state controlling whether [`SimulationTick`] advances automatically.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum SimulationPlaybackState {
    #[default]
    Running,
    Paused,
}

/// Controls how [`SimulationTick`] is paced while [`SimulationPlaybackState::Running`] is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Resource)]
pub enum SimulationSpeedMode {
    /// Runs [`SimulationTick`] on Bevy's `FixedUpdate` schedule using [`Time<Fixed>`].
    #[default]
    FixedStep,
    /// Runs [`SimulationTick`] once per rendered `Update` frame without waiting on [`Time<Fixed>`].
    Uncapped,
}

/// Drives [`SimulationTick`] according to [`SimulationPlaybackState`] and [`SimulationSpeedMode`].
pub struct SimulationPacingPlugin {
    pub initial_tick_interval: Duration,
}

impl SimulationPacingPlugin {
    pub const fn new(initial_tick_interval: Duration) -> Self {
        Self {
            initial_tick_interval,
        }
    }
}

impl Default for SimulationPacingPlugin {
    fn default() -> Self {
        Self::new(Duration::from_secs_f64(0.5))
    }
}

impl Plugin for SimulationPacingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_duration(self.initial_tick_interval))
            .init_state::<SimulationPlaybackState>()
            .init_resource::<SimulationSpeedMode>()
            .add_systems(
                FixedUpdate,
                run_simulation_tick
                    .run_if(in_state(SimulationPlaybackState::Running))
                    .run_if(resource_equals(SimulationSpeedMode::FixedStep)),
            )
            .add_systems(
                Update,
                run_simulation_tick
                    .run_if(in_state(SimulationPlaybackState::Running))
                    .run_if(resource_equals(SimulationSpeedMode::Uncapped)),
            );
    }
}

fn run_simulation_tick(world: &mut World) {
    world.run_schedule(SimulationTick);
}
