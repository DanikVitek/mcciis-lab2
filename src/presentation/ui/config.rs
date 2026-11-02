use std::time::Duration;

use bevy::prelude::*;

use crate::{
    grid::GridSize,
    presentation::pacing::{SimulationPlaybackState, SimulationSpeedMode},
    simulation::{
        DistributionConfig, DistributionError, ResetSimulation, SimulationTickCount,
        StepSimulationOnce, spatial::SpatialGrid,
    },
};

use super::UiSystems;

/// Root marker component for the simulation configuration & playback controls UI panel.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct SimConfigPanel;

/// Staged form state edited by the configuration UI before applying changes to live resources.
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct SimConfigDraft {
    pub grid_size: GridSize,
    pub plants_fraction: f32,
    pub herbivores_fraction: f32,
    pub predators_fraction: f32,
    pub tick_interval: Duration,
    pub speed_mode: SimulationSpeedMode,
}

impl FromWorld for SimConfigDraft {
    fn from_world(world: &mut World) -> Self {
        let grid_size = *world.resource::<GridSize>();
        let dist = *world.resource::<DistributionConfig>();
        let tick_interval = world.resource::<Time<Fixed>>().timestep();
        let speed_mode = *world.resource::<SimulationSpeedMode>();

        Self {
            grid_size,
            plants_fraction: dist.plants_fraction(),
            herbivores_fraction: dist.herbivores_fraction(),
            predators_fraction: dist.predators_fraction(),
            tick_interval,
            speed_mode,
        }
    }
}

impl SimConfigDraft {
    /// Applies live playback pacing changes (`tick_interval` and `speed_mode`) without resetting the world.
    pub fn apply_pacing(&self, fixed_time: &mut Time<Fixed>, speed_mode: &mut SimulationSpeedMode) {
        fixed_time.set_timestep(self.tick_interval);
        *speed_mode = self.speed_mode;
    }

    /// Validates staged distribution fractions, updates [`GridSize`] and [`DistributionConfig`],
    /// and triggers [`ResetSimulation`] to re-seed the world.
    pub fn apply_world_and_reset(
        &self,
        commands: &mut Commands,
        grid_size: &mut GridSize,
        distribution: &mut DistributionConfig,
    ) -> Result<(), DistributionError> {
        let validated = DistributionConfig::builder()
            .plants_fraction(self.plants_fraction)
            .herbivores_fraction(self.herbivores_fraction)
            .predators_fraction(self.predators_fraction)
            .build()?;

        *grid_size = self.grid_size;
        *distribution = validated;
        commands.trigger(ResetSimulation);
        Ok(())
    }
}

/// Helper commands for playback controls (Pause / Resume / Step Once) from UI widgets or hotkeys.
pub fn toggle_playback_pause(
    current: &State<SimulationPlaybackState>,
    next: &mut NextState<SimulationPlaybackState>,
) {
    match current.get() {
        SimulationPlaybackState::Running => next.set(SimulationPlaybackState::Paused),
        SimulationPlaybackState::Paused => next.set(SimulationPlaybackState::Running),
    }
}

pub fn trigger_single_step(commands: &mut Commands) {
    commands.trigger(StepSimulationOnce);
}

pub struct SimConfigUiPlugin;

impl Plugin for SimConfigUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimConfigDraft>()
            .add_systems(Startup, setup_config_ui)
            .add_systems(Update, update_config_ui.in_set(UiSystems::Config));
    }
}

fn setup_config_ui(_commands: Commands) {
    // TODO(ui): Spawn `SimConfigPanel` node hierarchy with inputs for `SimConfigDraft`
    // (grid dimensions, distribution fractions, tick speed, pause/step/reset buttons).
}

fn update_config_ui(
    _draft: ResMut<SimConfigDraft>,
    _playback_state: Res<State<SimulationPlaybackState>>,
    _tick_count: Res<SimulationTickCount>,
    _spatial_grid: Res<SpatialGrid>,
) {
    // TODO(ui): Bind widget interactions to `SimConfigDraft`, `toggle_playback_pause`,
    // `trigger_single_step`, and display live `tick_count` / `spatial_grid.character_counts()`.
}
