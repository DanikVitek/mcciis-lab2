use std::time::Duration;

use bevy::prelude::*;
use serde::{
    Deserialize,
    Serialize,
};

use crate::simulation::{
    SimulationTick,
    has_living_organisms,
};

/// Top-level visual application phase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum AppState {
    /// Initial pre-run setup screen where simulation [`Resource`]s have not yet been inserted.
    #[default]
    Setup,
    /// Active simulation phase after initial configuration has been accepted and [`Resource`]s exist.
    Simulation,
}

/// Visual playback sub-state active only during [`AppState::Simulation`], controlling whether
/// [`SimulationTick`] advances automatically or is paused.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, SubStates)]
#[source(AppState = AppState::Simulation)]
pub enum SimulationPlaybackState {
    #[default]
    Running,
    Paused,
}

/// Controls how [`SimulationTick`] is paced while [`SimulationPlaybackState::Running`] is active.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Resource, Serialize, Deserialize)]
#[serde(rename = "simulation_speed_mode", rename_all = "snake_case")]
pub enum SimulationSpeedMode {
    /// Runs [`SimulationTick`] on Bevy's `FixedUpdate` schedule using [`Time<Fixed>`].
    #[default]
    FixedStep,
    /// Runs [`SimulationTick`] once per rendered `Update` frame without waiting on [`Time<Fixed>`].
    Uncapped,
}

/// Drives [`SimulationTick`] according to [`AppState`], [`SimulationPlaybackState`], and [`SimulationSpeedMode`].
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
            .init_state::<AppState>()
            .add_sub_state::<SimulationPlaybackState>()
            .init_resource::<SimulationSpeedMode>()
            .add_systems(
                FixedUpdate,
                run_simulation_tick
                    .run_if(in_state(SimulationPlaybackState::Running))
                    .run_if(resource_equals(SimulationSpeedMode::FixedStep))
                    .run_if(has_living_organisms),
            )
            .add_systems(
                Update,
                (
                    run_simulation_tick
                        .run_if(resource_equals(SimulationSpeedMode::Uncapped))
                        .run_if(has_living_organisms),
                    pause_when_no_organisms.run_if(not(has_living_organisms)),
                )
                    .chain_weak()
                    .run_if(in_state(SimulationPlaybackState::Running)),
            );
    }
}

fn run_simulation_tick(world: &mut World) {
    world.run_schedule(SimulationTick);
}

fn pause_when_no_organisms(mut next_state: ResMut<NextState<SimulationPlaybackState>>) {
    next_state.set(SimulationPlaybackState::Paused);
}
