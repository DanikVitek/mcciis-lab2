use bevy::prelude::*;
use itertools::Itertools;
use rand::RngExt;
use rand::distr::Distribution;
use rand::seq::WeightError;

use super::organism::{
    Energy,
    EntityKind,
    FoodValues,
    Herbivore,
    InitialEnergy,
    Plant,
    Predator,
    ValueAsFood,
};
use super::spatial::SpatialGrid;
pub use super::{
    DistributionConfig,
    DistributionError,
};
use super::{
    ResetSimulation,
    SimulationTickCount,
    WorldSeeded,
    spawn_herbivore,
    spawn_plant,
    spawn_predator,
};
use crate::grid::{
    Facing,
    GridPosition,
    GridSize,
};

pub(super) fn seed_world(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
    config: Res<DistributionConfig>,
    initial_energy: Res<InitialEnergy>,
    food_values: Option<Res<FoodValues>>,
) {
    let food = food_values.as_deref().copied().unwrap_or_default();
    spatial_grid.resize_and_clear(*field_size);
    populate_grid(
        &mut commands,
        &mut spatial_grid,
        *field_size,
        *config,
        *initial_energy,
        food,
    );
    commands.trigger(WorldSeeded);
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(super) fn on_reset_simulation(
    _reset: On<ResetSimulation>,
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    mut tick_count: ResMut<SimulationTickCount>,
    field_size: Res<GridSize>,
    config: Res<DistributionConfig>,
    initial_energy: Res<InitialEnergy>,
    food_values: Option<Res<FoodValues>>,
) {
    let food = food_values.as_deref().copied().unwrap_or_default();
    commands.despawn_all::<Or<(With<Plant>, With<Herbivore>, With<Predator>)>>();
    tick_count.0 = 0;
    spatial_grid.resize_and_clear(*field_size);
    populate_grid(
        &mut commands,
        &mut spatial_grid,
        *field_size,
        *config,
        *initial_energy,
        food,
    );
    commands.trigger(WorldSeeded);
}

fn populate_grid(
    commands: &mut Commands,
    spatial_grid: &mut SpatialGrid,
    field_size: GridSize,
    config: DistributionConfig,
    initial_energy: InitialEnergy,
    food_values: FoodValues,
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
                let entity = spawn_plant(commands, pos, ValueAsFood(food_values.plant));

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
                let energy = Energy::from(initial_energy);
                match idx {
                    HERBIVORE => {
                        let entity = spawn_herbivore(commands)
                            .pos(pos)
                            .facing(dir)
                            .energy(energy)
                            .value_as_food(food_values.herbivore)
                            .call();
                        spatial_grid.insert(pos, entity, EntityKind::Herbivore);
                        herbivores_total -= 1;
                        match dist.update_weights(&[(2, &herbivores_total)]) {
                            Ok(()) => {}
                            Err(WeightError::InsufficientNonZero) => return,
                            Err(_) => unreachable!(),
                        }
                    }
                    PREDATOR => {
                        let entity = spawn_predator(commands)
                            .pos(pos)
                            .facing(dir)
                            .energy(energy)
                            .call();
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
