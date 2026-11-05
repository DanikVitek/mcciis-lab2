use bevy::prelude::*;
use bon::bon;
use itertools::Itertools;
use rand::{RngExt, distr::Distribution, seq::WeightError};

use crate::{
    grid::{Facing, GridPosition, GridSize},
    simulation::organism::Energy,
};

use super::{
    ResetSimulation, SimulationTickCount, WorldSeeded,
    organism::{EntityKind, Herbivore, InitialEnergy, Plant, Predator},
    spatial::SpatialGrid,
};

#[derive(Debug, Clone, Copy, PartialEq, Resource)]
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

pub(super) fn seed_world(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
    config: Res<DistributionConfig>,
    initial_energy: Res<InitialEnergy>,
) {
    spatial_grid.resize_and_clear(*field_size);
    populate_grid(
        &mut commands,
        &mut spatial_grid,
        *field_size,
        *config,
        *initial_energy,
    );
    commands.trigger(WorldSeeded);
}

#[allow(clippy::type_complexity)]
pub(super) fn on_reset_simulation(
    _reset: On<ResetSimulation>,
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    mut tick_count: ResMut<SimulationTickCount>,
    field_size: Res<GridSize>,
    config: Res<DistributionConfig>,
    initial_energy: Res<InitialEnergy>,
) {
    commands.despawn_all::<Or<(With<Plant>, With<Herbivore>, With<Predator>)>>();
    tick_count.0 = 0;
    spatial_grid.resize_and_clear(*field_size);
    populate_grid(
        &mut commands,
        &mut spatial_grid,
        *field_size,
        *config,
        *initial_energy,
    );
    commands.trigger(WorldSeeded);
}

fn populate_grid(
    commands: &mut Commands,
    spatial_grid: &mut SpatialGrid,
    field_size: GridSize,
    config: DistributionConfig,
    initial_energy: InitialEnergy,
) {
    let tiles_total = u32::from(field_size.width()) * u32::from(field_size.height());

    let mut plants_total = (tiles_total as f32 * config.plants_fraction()) as u32;
    let mut herbivores_total = (tiles_total as f32 * config.herbivores_fraction()) as u32;
    let mut predators_total = (tiles_total as f32 * config.predators_fraction()) as u32;
    let mut rest = tiles_total - plants_total - herbivores_total - predators_total;

    let mut dist = match rand::distr::weighted::WeightedIndex::new([
        rest,
        plants_total,
        herbivores_total,
        predators_total,
    ]) {
        Ok(dist) => dist,
        Err(WeightError::InsufficientNonZero) => return,
        Err(_) => unreachable!(),
    };
    /// weighted index coresponding to the tile being left empty
    const EMPTY: usize = 0;
    /// weighted index coresponding to the tile being populated with a plant
    const PLANT: usize = 1;
    /// weighted index coresponding to the tile being populated with a herbivore
    const HERBIVORE: usize = 2;
    /// weighted index coresponding to the tile being populated with a predator
    const PREDATOR: usize = 3;

    let mut rng = rand::rng();

    for (x, y) in (0..field_size.width()).cartesian_product(0..field_size.height()) {
        match dist.sample(&mut rng) {
            EMPTY => {
                rest -= 1;
                match dist.update_weights(&[(0, &rest)]) {
                    Ok(()) => {}
                    Err(WeightError::InsufficientNonZero) => return,
                    Err(_) => unreachable!(),
                }
            }
            PLANT => {
                let pos = GridPosition::new(x, y);
                let entity = commands.spawn((Plant, pos)).id();
                spatial_grid.insert(pos, entity, EntityKind::Plant);
                plants_total -= 1;
                match dist.update_weights(&[(1, &plants_total)]) {
                    Ok(()) => {}
                    Err(WeightError::InsufficientNonZero) => return,
                    Err(_) => unreachable!(),
                }
            }
            idx @ (HERBIVORE | PREDATOR) => {
                let pos = GridPosition::new(x, y);
                let dir = rng.random::<Facing>();
                match idx {
                    HERBIVORE => {
                        let entity = commands
                            .spawn((Herbivore, pos, dir, Energy::from(initial_energy)))
                            .id();
                        spatial_grid.insert(pos, entity, EntityKind::Herbivore);
                        herbivores_total -= 1;
                        match dist.update_weights(&[(2, &herbivores_total)]) {
                            Ok(()) => {}
                            Err(WeightError::InsufficientNonZero) => return,
                            Err(_) => unreachable!(),
                        }
                    }
                    PREDATOR => {
                        let entity = commands
                            .spawn((Predator, pos, dir, Energy::from(initial_energy)))
                            .id();
                        spatial_grid.insert(pos, entity, EntityKind::Predator);
                        predators_total -= 1;
                        match dist.update_weights(&[(3, &predators_total)]) {
                            Ok(()) => {}
                            Err(WeightError::InsufficientNonZero) => return,
                            Err(_) => unreachable!(),
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => unreachable!(),
        }
    }
}
