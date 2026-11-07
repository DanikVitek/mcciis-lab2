use core::any::Any;
use core::fmt;

use bevy::math::I16Vec2;
use bevy::prelude::*;
use rand::RngExt;
use rand::seq::{
    IndexedRandom,
    SliceRandom,
};
use serde::{
    Deserialize,
    Serialize,
};

use super::brain::{
    Brain,
    INPUTS,
    OUTPUTS,
    SENSORS_HERBIVORE,
    SENSORS_PREDATOR,
};
use super::organism::{
    Energy,
    EntityKind,
    FoodValues,
    Generation,
    Herbivore,
    MaxEnergy,
    Plant,
    Predator,
    ValueAsFood,
};
use super::seed::DistributionConfig;
use super::spatial::SpatialGrid;
use super::{
    SimulationSystems,
    SimulationTick,
};
use crate::grid::{
    Facing,
    GridPosition,
    GridSize,
};

const MOORE_OFFSETS: [[i16; 2]; 8] = [
    [-1, -1],
    [-1, 0],
    [-1, 1],
    [0, -1],
    [0, 1],
    [1, -1],
    [1, 0],
    [1, 1],
];

pub trait Mutation: fmt::Debug + Any + Sync + Send {
    fn apply(&self, old: &Brain) -> Brain;
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "mutation_strategy", rename_all = "snake_case")]
enum MutationStrategyRepr {
    SingleNeuron,
    Distributed(DistributedMutation),
}

impl From<MutationStrategy> for MutationStrategyRepr {
    fn from(value: MutationStrategy) -> Self {
        match value {
            MutationStrategy::SingleNeuron(SingleNeuronMutation) => Self::SingleNeuron,
            MutationStrategy::Distributed(strat) => Self::Distributed(strat),
        }
    }
}

impl From<MutationStrategyRepr> for MutationStrategy {
    fn from(value: MutationStrategyRepr) -> Self {
        match value {
            MutationStrategyRepr::SingleNeuron => Self::SingleNeuron(SingleNeuronMutation),
            MutationStrategyRepr::Distributed(strat) => Self::Distributed(strat),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Resource, Serialize, Deserialize)]
#[serde(from = "MutationStrategyRepr", into = "MutationStrategyRepr")]
pub enum MutationStrategy {
    SingleNeuron(SingleNeuronMutation),
    Distributed(DistributedMutation),
}

impl Default for MutationStrategy {
    fn default() -> Self {
        DistributedMutation::default().into()
    }
}

impl Mutation for MutationStrategy {
    fn apply(&self, old: &Brain) -> Brain {
        match self {
            Self::SingleNeuron(strat) => strat.apply(old),
            Self::Distributed(strat) => strat.apply(old),
        }
    }
}

impl From<SingleNeuronMutation> for MutationStrategy {
    fn from(value: SingleNeuronMutation) -> Self {
        Self::SingleNeuron(value)
    }
}

impl From<DistributedMutation> for MutationStrategy {
    fn from(value: DistributedMutation) -> Self {
        Self::Distributed(value)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename = "single_neuron_mutation")]
pub struct SingleNeuronMutation;

impl Mutation for SingleNeuronMutation {
    fn apply(&self, old: &Brain) -> Brain {
        let mut rng = rand::rng();

        let what_to_mutate = rng.random_range(0..((INPUTS + 1) * OUTPUTS));
        let mut new = old.clone();

        if (0..(INPUTS * OUTPUTS)).contains(&what_to_mutate) {
            new.weights_mut()[what_to_mutate] = rng.random_range(-1.0..=1.0);
        } else {
            new.biases_mut()[what_to_mutate - INPUTS * OUTPUTS] = rng.random_range(-1.0..=1.0);
        }

        new
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename = "distributed_mutation", rename_all = "snake_case")]
pub struct DistributedMutation {
    pub offset: f32,
}

impl Default for DistributedMutation {
    fn default() -> Self {
        Self::new(0.5)
    }
}

impl DistributedMutation {
    pub const fn new(offset: f32) -> Self {
        Self { offset }
    }
}

impl Mutation for DistributedMutation {
    fn apply(&self, old: &Brain) -> Brain {
        let offset = self.offset.abs();

        let mut rng = rand::rng();

        let mut new = old.clone();

        for w in new.weights_mut() {
            *w += rng.random_range(-offset..=offset);
        }
        for b in new.biases_mut() {
            *b += rng.random_range(-offset..=offset);
        }

        new
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Resource, Serialize, Deserialize)]
#[serde(rename = "reproduction_threshold", rename_all = "snake_case")]
pub enum ReproductionThreshold {
    Absolute(Energy),
    Relative(f32),
}

impl Default for ReproductionThreshold {
    fn default() -> Self {
        Self::Relative(0.9)
    }
}

impl From<Energy> for ReproductionThreshold {
    fn from(value: Energy) -> Self {
        Self::Absolute(value)
    }
}

impl ReproductionThreshold {
    /// Returns the minimum [`Energy`] required to reproduce given `max_energy`.
    #[must_use]
    pub const fn min_energy(self, MaxEnergy(max): MaxEnergy) -> Energy {
        match self {
            Self::Absolute(threshold) => threshold,
            Self::Relative(fraction) => Energy(max.0 * fraction),
        }
    }

    /// Checks whether this threshold is within valid bounds for the given `max_energy`
    /// (`0.0 <= fraction <= 1.0` for [`Self::Relative`], or `0.0 < threshold <= max` for [`Self::Absolute`]).
    #[must_use]
    pub const fn is_valid_for(self, MaxEnergy(max): MaxEnergy) -> bool {
        match self {
            Self::Absolute(threshold) => threshold.0 > 0.0 && threshold.0 <= max.0,
            Self::Relative(fraction) => 0.0 <= fraction && fraction <= 1.0,
        }
    }
}

impl Energy {
    #[must_use]
    pub const fn is_enough_to_reproduce(
        self,
        threshold: ReproductionThreshold,
        max_energy: MaxEnergy,
    ) -> bool {
        self.0 >= threshold.min_energy(max_energy).0
    }
}

/// Intrinsic per-tick logistic budding rate `r` for surviving [`Plant`]s.
///
/// Each surviving plant attempts to bud onto an empty Moore-neighboring tile with probability
/// `r * (1 - current_plants / max_plants)`, naturally slowing as the population approaches
/// carrying capacity (`max_plants`) and permanently stopping if all plants go extinct (`0`).
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Resource, Serialize, Deserialize)]
#[serde(rename = "plant_growth_rate")]
pub struct PlantGrowthRate(pub f32);

impl Default for PlantGrowthRate {
    fn default() -> Self {
        Self(0.1)
    }
}

impl From<f32> for PlantGrowthRate {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

/// Registers the parallel character and plant reproduction systems in [`SimulationSystems::Reproduce`]
/// within [`SimulationTick`] without inserting configuration [`Resource`]s upfront.
///
/// Used in visual mode where [`ReproductionThreshold`], [`MutationStrategy`], and [`PlantGrowthRate`]
/// are inserted when entering the active simulation state, and included automatically by [`ReproductionPlugin`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DeferredReproductionPlugin;

impl Plugin for DeferredReproductionPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<TaskPoolPlugin>() {
            app.add_plugins(TaskPoolPlugin::default());
        }

        app.add_systems(
            SimulationTick,
            (reproduce_characters, reproduce_plants).in_set(SimulationSystems::Reproduce),
        );
    }
}

/// Swappable reproduction plugin implementing parallel character fission and logistic plant budding.
///
/// Inserts [`ReproductionThreshold`], [`MutationStrategy`], and [`PlantGrowthRate`] resources and registers
/// the reproduction systems in [`SimulationSystems::Reproduce`] within [`SimulationTick`].
#[derive(Debug, Clone, Copy, Default, bon::Builder)]
pub struct ReproductionPlugin {
    #[builder(into, default)]
    pub reproduction_threshold: ReproductionThreshold,
    #[builder(into, default)]
    pub mutation_strategy: MutationStrategy,
    #[builder(into, default)]
    pub plant_growth_rate: PlantGrowthRate,
}

impl Plugin for ReproductionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.reproduction_threshold)
            .insert_resource(self.mutation_strategy)
            .insert_resource(self.plant_growth_rate)
            .add_plugins(DeferredReproductionPlugin);
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn reproduce_characters(
    par_commands: ParallelCommands,
    spatial_grid: Res<SpatialGrid>,
    field_size: Res<GridSize>,
    repro_threshold: Res<ReproductionThreshold>,
    max_energy: Res<MaxEnergy>,
    mutation_strategy: Res<MutationStrategy>,
    food_values: Option<Res<FoodValues>>,
    mut query: Query<
        (
            Entity,
            &mut Energy,
            &Brain,
            &GridPosition,
            &Facing,
            &Generation,
            Has<Herbivore>,
        ),
        Without<Plant>,
    >,
) {
    let herbivore_food = ValueAsFood(
        food_values
            .as_deref()
            .copied()
            .unwrap_or_default()
            .herbivore,
    );

    query.par_iter_mut().for_each(
        |(entity, mut energy, brain, pos, facing, generation, is_herbivore)| {
            if !energy.is_enough_to_reproduce(*repro_threshold, *max_energy) {
                return;
            }

            let mut rng = rand::rng();
            let offset = *MOORE_OFFSETS.choose(&mut rng).unwrap();

            let child_pos = pos.add_wrapping(I16Vec2::from_array(offset), *field_size);
            let child_brain = Mutation::apply(&*mutation_strategy, brain);
            let child_gen = generation.next();
            let res = energy.distribute();
            let parent_deceased = res.is_deceased();
            let child_energy = res.into_value();

            let entity_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };

            par_commands.command_scope(|mut commands| {
                if parent_deceased {
                    commands.entity(entity).despawn();
                    spatial_grid.remove(*pos, entity, entity_kind);
                }

                let child = if is_herbivore {
                    commands
                        .spawn((
                            Herbivore,
                            child_pos,
                            *facing,
                            child_brain,
                            child_gen,
                            child_energy,
                            herbivore_food,
                            SENSORS_HERBIVORE.to_owned(),
                        ))
                        .id()
                } else {
                    commands
                        .spawn((
                            Predator,
                            child_pos,
                            *facing,
                            child_brain,
                            child_gen,
                            child_energy,
                            SENSORS_PREDATOR.to_owned(),
                        ))
                        .id()
                };
                spatial_grid.insert(child_pos, child, entity_kind);
            });
        },
    );
}

pub(super) fn reproduce_plants(
    par_commands: ParallelCommands,
    spatial_grid: Res<SpatialGrid>,
    field_size: Res<GridSize>,
    distribution: Option<Res<DistributionConfig>>,
    plant_growth_rate: Option<Res<PlantGrowthRate>>,
    food_values: Option<Res<FoodValues>>,
    plants: Query<&GridPosition, With<Plant>>,
) {
    let Some(distribution) = distribution else {
        return;
    };

    let r = plant_growth_rate.map_or_else(|| PlantGrowthRate::default().0, |rate| rate.0);
    if r <= 0.0 {
        return;
    }

    let tiles_total = u32::from(field_size.width()) * u32::from(field_size.height());
    let max_plants =
        ((tiles_total as f32 * distribution.plants_fraction()).round() as u32).min(tiles_total);
    let current_plants = spatial_grid.plant_count();
    if max_plants == 0 || current_plants == 0 || current_plants >= max_plants {
        return;
    }

    let prob = (f64::from(r) * f64::from(max_plants - current_plants) / f64::from(max_plants))
        .clamp(0.0, 1.0);
    if prob <= 0.0 {
        return;
    }

    let plant_food = ValueAsFood(food_values.as_deref().copied().unwrap_or_default().plant);

    plants.par_iter().for_each(|pos| {
        let mut rng = rand::rng();
        if !rng.random_bool(prob) {
            return;
        }

        let mut offsets = MOORE_OFFSETS;
        offsets.shuffle(&mut rng);

        for offset in offsets {
            let candidate = pos.add_wrapping(I16Vec2::from_array(offset), *field_size);
            if spatial_grid.try_claim_empty_plant_tile(candidate, max_plants) {
                par_commands.command_scope(|mut commands| {
                    let child = commands.spawn((Plant, candidate, plant_food)).id();
                    spatial_grid.push_claimed_entity(candidate, child, EntityKind::Plant);
                });
                break;
            }
        }
    });
}
