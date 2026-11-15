pub mod brain;
pub mod history;
pub mod organism;
pub mod reproduction;
pub mod seed;
pub mod spatial;
pub mod stepping;

use bevy::app::AppExit;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bon::bon;
use serde::{
    Deserialize,
    Serialize,
};

use self::brain::{
    Brain,
    SENSORS_HERBIVORE,
    SENSORS_PREDATOR,
};
pub use self::history::{
    GenerationSample,
    HistoryRecorderPlugin,
    HistorySample,
    SampleRate,
    SimulationHistory,
};
pub use self::organism::{
    ActionCosts,
    FoodValues,
};
use self::organism::{
    Age,
    CharacterFilter,
    Energy,
    EntityKind,
    Generation,
    Herbivore,
    InitialEnergy,
    MaxEnergy,
    Plant,
    Predator,
    ValueAsFood,
};
pub use self::reproduction::{
    DeferredReproductionPlugin,
    DistributedMutation,
    Mutation,
    MutationStrategy,
    PlantGrowthRate,
    ReproductionPlugin,
    ReproductionThreshold,
    SingleNeuronMutation,
};
use self::spatial::SpatialGrid;
use self::stepping::SteppingPlugin;
use crate::grid::{
    Facing,
    GridPosition,
    GridSize,
};
use crate::name;

#[derive(Debug, Clone, Copy, PartialEq, Resource, Serialize, Deserialize)]
#[serde(rename = "distribution_config", rename_all = "snake_case")]
pub struct DistributionConfig {
    plants_fraction: f32,
    herbivores_fraction: f32,
    predators_fraction: f32,
}

impl Default for DistributionConfig {
    fn default() -> Self {
        Self {
            plants_fraction: 0.1,
            herbivores_fraction: 0.1,
            predators_fraction: 0.1,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DistributionError {
    #[error("negative distribution fraction")]
    NegativeFraction,
    #[error("sum of the distribution fractions is greater than one")]
    SumGtOne,
}

#[bon]
impl DistributionConfig {
    #[builder(const)]
    pub const fn new(
        #[builder(default = 0.0)] plants_fraction: f32,
        #[builder(default = 0.0)] herbivores_fraction: f32,
        #[builder(default = 0.0)] predators_fraction: f32,
    ) -> Result<Self, DistributionError> {
        if plants_fraction < 0.0 || herbivores_fraction < 0.0 || predators_fraction < 0.0 {
            return Err(DistributionError::NegativeFraction);
        }

        if plants_fraction + herbivores_fraction + predators_fraction > 1.0 {
            return Err(DistributionError::SumGtOne);
        }

        Ok(Self {
            plants_fraction,
            herbivores_fraction,
            predators_fraction,
        })
    }
}

impl DistributionConfig {
    pub const fn plants_fraction(&self) -> f32 {
        self.plants_fraction
    }

    pub const fn herbivores_fraction(&self) -> f32 {
        self.herbivores_fraction
    }

    pub const fn predators_fraction(&self) -> f32 {
        self.predators_fraction
    }
}

/// Cause of a character organism's death during a simulation tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeathCause {
    Eaten,
    Starved,
}

/// Message emitted whenever a character organism (`Herbivore` or `Predator`) dies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Message)]
pub struct CharacterDied {
    pub kind: EntityKind,
    pub age: Age,
    pub generation: Generation,
    pub cause: DeathCause,
}

/// Message emitted per worker batch in `SimulationSystems::Step` reporting the maximum [`Age`]
/// reached among surviving characters of `kind` in that batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Message)]
pub struct CharacterAged {
    pub kind: EntityKind,
    pub max_age: Age,
}

/// Message emitted whenever an organism (`Plant`, `Herbivore`, or `Predator`) reproduces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Message)]
pub struct OrganismReproduced {
    pub kind: EntityKind,
    pub child_generation: Option<Generation>,
}

pub(crate) fn spawn_plant(commands: &mut Commands, pos: GridPosition, food: ValueAsFood) -> Entity {
    let mut entity_commands = commands.spawn_empty();
    let entity = entity_commands.id();
    entity_commands.insert((name!(format!("Plant {entity}")), Plant, pos, food));
    entity
}

#[bon::builder]
pub(crate) fn spawn_herbivore<'w, 's>(
    #[builder(start_fn)] commands: &mut Commands<'w, 's>,
    pos: GridPosition,
    facing: Facing,
    energy: Energy,
    #[builder(into)] value_as_food: ValueAsFood,
    brain: Option<Brain>,
    generation: Option<Generation>,
) -> Entity {
    let mut entity_commands = commands.spawn_empty();
    let entity = entity_commands.id();
    if let Some(brain) = brain {
        entity_commands.insert(brain);
    }
    if let Some(generation) = generation {
        entity_commands.insert(generation);
    }
    entity_commands.insert((
        name!(format!("Herbivore {entity}")),
        Herbivore,
        pos,
        facing,
        energy,
        value_as_food,
        SENSORS_HERBIVORE.to_owned(),
    ));
    entity
}

#[bon::builder]
pub(crate) fn spawn_predator<'w, 's>(
    #[builder(start_fn)] commands: &mut Commands<'w, 's>,
    pos: GridPosition,
    facing: Facing,
    energy: Energy,
    brain: Option<Brain>,
    generation: Option<Generation>,
) -> Entity {
    let mut entity_commands = commands.spawn_empty();
    let entity = entity_commands.id();
    if let Some(brain) = brain {
        entity_commands.insert(brain);
    }
    if let Some(generation) = generation {
        entity_commands.insert(generation);
    }
    entity_commands.insert((
        name!(format!("Predator {entity}")),
        Predator,
        pos,
        facing,
        energy,
        SENSORS_PREDATOR.to_owned(),
    ));
    entity
}

/// Run condition returning `true` while at least one character organism (`Herbivore` or `Predator`) is alive.
pub fn has_living_organisms(organisms: Query<(), CharacterFilter>) -> bool {
    !organisms.is_empty()
}

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
    /// Reproduction phase between [`Step`](Self::Step) and [`Record`](Self::Record),
    /// populated by a swappable reproduction plugin such as [`ReproductionPlugin`] or [`DeferredReproductionPlugin`].
    Reproduce,
    /// End-of-tick telemetry / history recording phase.
    ///
    /// Runs after all [`Step`](Self::Step) and [`Reproduce`](Self::Reproduce) deferred commands
    /// have been applied and after [`SimulationTickCount`] has been incremented for the completed tick.
    Record,
}

/// Number of completed [`SimulationTick`] steps since initial startup or the last [`ResetSimulation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Resource)]
pub struct SimulationTickCount(pub u64);

/// Global event emitted once all initial organisms have been spawned into the [`World`]
/// (on initial `Startup` and after every [`ResetSimulation`]).
#[derive(Debug, Clone, Copy, Default, Event)]
pub struct WorldSeeded;

/// Global event that clears all existing organisms, resizes [`SpatialGrid`] to [`GridSize`],
/// resets [`SimulationTickCount`], and seeds a fresh population from [`DistributionConfig`].
#[derive(Debug, Clone, Copy, Default, Event)]
pub struct ResetSimulation;

/// Global event that advances [`SimulationTick`] by a single discrete step on demand
/// (useful for a "Step 1 Tick" UI button while visual playback is paused).
#[derive(Debug, Clone, Copy, Default, Event)]
pub struct StepSimulationOnce;

/// Registers the [`SimulationTick`] schedule, system sets, stepping system, and reset/step observers
/// without inserting configuration resources or seeding the world automatically on `Startup`.
///
/// Used directly in visual mode where configuration [`Resource`]s are inserted only after the user
/// accepts the initial setup screen, and included automatically by [`SimulationPlugin`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DeferredSimulationPlugin;

impl Plugin for DeferredSimulationPlugin {
    fn build(&self, app: &mut App) {
        app.init_schedule(SimulationTick)
            .configure_sets(
                SimulationTick,
                (
                    SimulationSystems::Step,
                    SimulationSystems::Reproduce,
                    SimulationSystems::Record,
                )
                    .chain(),
            )
            .add_plugins(SteppingPlugin)
            .add_systems(
                SimulationTick,
                (
                    advance_tick_counter
                        .after_ignore_deferred(SimulationSystems::Reproduce)
                        .before_ignore_deferred(SimulationSystems::Record),
                    (update_simulation_messages,).after(SimulationSystems::Record),
                ),
            )
            .add_observer(seed::on_reset_simulation)
            .add_observer(on_step_simulation_once);
    }
}

/// Schedule-agnostic, headless-ready ecosystem simulation plugin that immediately inserts
/// world and energy [`Resource`]s and seeds the world on `Startup`.
///
/// Pair with [`ReproductionPlugin`] (or a custom reproduction plugin) to enable reproduction
/// during [`SimulationSystems::Reproduce`].
#[derive(Debug, Clone, bon::Builder)]
pub struct SimulationPlugin {
    pub grid_size: GridSize,
    pub distribution: DistributionConfig,
    #[builder(into)]
    pub initial_energy: InitialEnergy,
    #[builder(into)]
    pub max_energy: MaxEnergy,
    #[builder(default)]
    pub action_costs: ActionCosts,
    #[builder(default)]
    pub food_values: FoodValues,
}

impl Plugin for SimulationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.grid_size)
            .insert_resource(self.distribution)
            .insert_resource(SpatialGrid::new(self.grid_size))
            .insert_resource(self.initial_energy)
            .insert_resource(self.max_energy)
            .insert_resource(self.action_costs)
            .insert_resource(self.food_values)
            .init_resource::<SimulationTickCount>()
            .add_plugins(DeferredSimulationPlugin)
            .add_systems(Startup, seed::seed_world);
    }
}

fn advance_tick_counter(mut ticks: ResMut<SimulationTickCount>) {
    ticks.0 += 1;
}

fn update_simulation_messages(
    mut reproduced: Option<ResMut<Messages<OrganismReproduced>>>,
    mut died: Option<ResMut<Messages<CharacterDied>>>,
    mut aged: Option<ResMut<Messages<CharacterAged>>>,
) {
    if let Some(messages) = reproduced.as_deref_mut() {
        messages.update();
    }
    if let Some(messages) = died.as_deref_mut() {
        messages.update();
    }
    if let Some(messages) = aged.as_deref_mut() {
        messages.update();
    }
}

fn on_step_simulation_once(
    _step: On<StepSimulationOnce>,
    organisms: Query<(), CharacterFilter>,
    mut commands: Commands,
) {
    if organisms.is_empty() {
        return;
    }
    commands.queue(|world: &mut World| {
        world.run_schedule(SimulationTick);
    });
}

/// Optional uncapped runner plugin for headless `App::run()` executions (e.g. with `MinimalPlugins`).
///
/// Runs [`SimulationTick`] every `Update` cycle without any `FixedUpdate` sleep/timer delay,
/// exiting automatically when all organisms die out or when `max_ticks` is reached.
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
            if world
                .resource::<SpatialGrid>()
                .character_counts()
                .is_empty()
            {
                world.write_message(AppExit::Success);
                return;
            }
            world.run_schedule(SimulationTick);
        });
    }
}
