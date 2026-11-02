pub mod brain;
mod lifecycle;
pub mod organism;
pub mod seed;
pub mod spatial;

use bevy::app::AppExit;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use crate::grid::GridSize;

pub use self::seed::{DistributionConfig, DistributionError};
use self::spatial::SpatialGrid;

/// Custom schedule executing one discrete tick of the simulation.
///
/// Keeping simulation logic inside its own schedule (instead of hardcoding `FixedUpdate`)
/// lets visual runners pace ticks on a timer while headless runners execute `SimulationTick`
/// back-to-back at maximum CPU speed.
#[derive(ScheduleLabel, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SimulationTick;

/// Ordered system sets within [`SimulationTick`].
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SimulationSystems {
    Step,
    Reproduce,
}

/// Number of completed [`SimulationTick`] steps since initial startup or the last [`ResetSimulation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Resource)]
pub struct SimulationTickCount(pub u64);

/// Global event that clears all existing organisms, resizes [`SpatialGrid`] to [`GridSize`],
/// resets [`SimulationTickCount`], and seeds a fresh population from [`DistributionConfig`].
#[derive(Debug, Clone, Copy, Default, Event)]
pub struct ResetSimulation;

/// Global event that advances [`SimulationTick`] by a single discrete step on demand
/// (useful for a "Step 1 Tick" UI button while visual playback is paused).
#[derive(Debug, Clone, Copy, Default, Event)]
pub struct StepSimulationOnce;

/// Schedule-agnostic, headless-ready ecosystem simulation plugin.
pub struct SimulationPlugin {
    pub grid_size: GridSize,
    pub distribution: DistributionConfig,
}

impl SimulationPlugin {
    pub const fn new(grid_size: GridSize, distribution: DistributionConfig) -> Self {
        Self {
            grid_size,
            distribution,
        }
    }
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.grid_size)
            .insert_resource(self.distribution)
            .insert_resource(SpatialGrid::new(self.grid_size))
            .init_resource::<SimulationTickCount>()
            .init_schedule(SimulationTick)
            .configure_sets(
                SimulationTick,
                (SimulationSystems::Step, SimulationSystems::Reproduce).chain(),
            )
            .add_systems(Startup, seed::seed_world)
            .add_systems(
                SimulationTick,
                (
                    lifecycle::step_organisms.in_set(SimulationSystems::Step),
                    lifecycle::reproduction.in_set(SimulationSystems::Reproduce),
                    advance_tick_counter.after_ignore_deferred(SimulationSystems::Reproduce),
                ),
            )
            .add_observer(seed::on_reset_simulation)
            .add_observer(on_step_simulation_once);
    }
}

fn advance_tick_counter(mut ticks: ResMut<SimulationTickCount>) {
    ticks.0 += 1;
}

fn on_step_simulation_once(_step: On<StepSimulationOnce>, mut commands: Commands) {
    commands.queue(|world: &mut World| {
        world.run_schedule(SimulationTick);
    });
}

/// Optional uncapped runner plugin for headless `App::run()` executions (e.g. with `MinimalPlugins`).
///
/// Runs [`SimulationTick`] every `Update` cycle without any `FixedUpdate` sleep/timer delay,
/// optionally exiting after `max_ticks`.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlessRunnerPlugin {
    pub max_ticks: Option<u64>,
}

impl HeadlessRunnerPlugin {
    pub const fn uncapped() -> Self {
        Self { max_ticks: None }
    }

    pub const fn for_ticks(ticks: u64) -> Self {
        Self {
            max_ticks: Some(ticks),
        }
    }
}

impl Plugin for HeadlessRunnerPlugin {
    fn build(&self, app: &mut App) {
        let max_ticks = self.max_ticks;
        app.add_systems(Update, move |world: &mut World| {
            if let Some(limit) = max_ticks
                && world.resource::<SimulationTickCount>().0 >= limit
            {
                world.write_message(AppExit::Success);
                return;
            }
            world.run_schedule(SimulationTick);
        });
    }
}
