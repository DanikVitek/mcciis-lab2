use core::{any::Any, fmt};

use bevy::math::I16Vec2;
use bevy::prelude::*;
use rand::{RngExt, seq::IndexedRandom};

use crate::grid::{Facing, GridPosition, GridSize};

use super::{
    SimulationSystems, SimulationTick,
    brain::{Brain, INPUTS, OUTPUTS},
    organism::{Energy, EntityKind, Generation, Herbivore, MaxEnergy, Predator},
    spatial::SpatialGrid,
};

pub trait Mutation: fmt::Debug + Any + Sync + Send {
    fn apply(&self, old: &Brain) -> Brain;
}

#[derive(Debug, Clone, Copy, PartialEq, Resource)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
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

#[derive(Debug, Clone, Copy, PartialEq, Resource)]
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
            Self::Relative(fraction) => Energy((max.0 as f32 * fraction) as u8),
        }
    }

    /// Checks whether this threshold is within valid bounds for the given `max_energy`
    /// (`0.0 <= fraction <= 1.0` for [`Self::Relative`], or `1 <= threshold <= max` for [`Self::Absolute`]).
    #[must_use]
    pub const fn is_valid_for(self, MaxEnergy(max): MaxEnergy) -> bool {
        match self {
            Self::Absolute(threshold) => threshold.0 >= 1 && threshold.0 <= max.0,
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

/// Registers the energy-threshold asexual [`reproduction`] system in [`SimulationSystems::Reproduce`]
/// within [`SimulationTick`] without inserting configuration [`Resource`]s upfront.
///
/// Used in visual mode where [`ReproductionThreshold`] and [`MutationStrategy`] are inserted
/// when entering the active simulation state, and included automatically by [`ReproductionPlugin`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DeferredReproductionPlugin;

impl Plugin for DeferredReproductionPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SimulationTick,
            reproduction.in_set(SimulationSystems::Reproduce),
        );
    }
}

/// Swappable reproduction plugin implementing asexual energy-threshold fission with brain mutation.
///
/// Inserts [`ReproductionThreshold`] and [`MutationStrategy`] resources and registers
/// the reproduction system in [`SimulationSystems::Reproduce`] within [`SimulationTick`].
#[derive(Debug, Clone, Copy, Default, bon::Builder)]
pub struct ReproductionPlugin {
    #[builder(into, default)]
    pub reproduction_threshold: ReproductionThreshold,
    #[builder(into, default)]
    pub mutation_strategy: MutationStrategy,
}

impl Plugin for ReproductionPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.reproduction_threshold)
            .insert_resource(self.mutation_strategy)
            .add_plugins(DeferredReproductionPlugin);
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn reproduction(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
    repro_threshold: Res<ReproductionThreshold>,
    max_energy: Res<MaxEnergy>,
    mutation_strategy: Res<MutationStrategy>,
    query: Query<(
        Entity,
        &mut Energy,
        &Brain,
        &GridPosition,
        &Facing,
        &Generation,
        Has<Herbivore>,
    )>,
) {
    let mut rng = rand::rng();
    for (entity, mut energy, brain, pos, facing, generation, is_herbivore) in query {
        if !energy.is_enough_to_reproduce(*repro_threshold, *max_energy) {
            continue;
        }

        let entity_kind = if is_herbivore {
            EntityKind::Herbivore
        } else {
            EntityKind::Predator
        };

        let offset = *[
            [-1, -1],
            [-1, 0],
            [-1, 1],
            [0, -1],
            [0, 1],
            [1, -1],
            [1, 0],
            [1, 1],
        ]
        .choose(&mut rng)
        .unwrap();

        let child_pos = pos.add_wrapping(I16Vec2::from_array(offset), *field_size);
        let child_brain = Mutation::apply(&*mutation_strategy, brain);
        let child_gen = generation.next();

        let res = energy.distribute();

        if res.is_deceased() {
            commands.entity(entity).despawn();
            spatial_grid.remove(*pos, entity, entity_kind);
        }
        let child_energy = res.into_value();

        let child = if is_herbivore {
            commands
                .spawn((
                    Herbivore,
                    child_pos,
                    *facing,
                    child_brain,
                    child_gen,
                    child_energy,
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
                ))
                .id()
        };
        spatial_grid.insert(child_pos, child, entity_kind);
    }
}
