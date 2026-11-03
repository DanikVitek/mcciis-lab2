use bevy::ecs::entity::EntityHashSet;
use bevy::math::I16Vec2;
use bevy::prelude::*;
use rand::seq::IndexedRandom;

use crate::grid::{Facing, GridPosition, GridSize};

use super::{
    brain::{Action, Brain, Mutation, MutationStrategy, SensorOffsets, sense_brain_inputs},
    organism::{Age, Energy, EntityKind, Generation, Herbivore, Plant, Predator, ValueAsFood},
    spatial::SpatialGrid,
};

#[allow(clippy::type_complexity)]
pub(super) fn step_organisms(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
    characters: Query<
        (
            Entity,
            &mut GridPosition,
            &mut Facing,
            &mut Energy,
            &mut Age,
            &Brain,
            &SensorOffsets,
            Has<Herbivore>,
        ),
        Without<Plant>,
    >,
    food: Query<&ValueAsFood, Without<Predator>>,
    mut plant_positions: Query<&mut GridPosition, With<Plant>>,
) {
    let mut consumed_herbivores = EntityHashSet::new();

    for (entity, mut pos, mut facing, mut energy, mut age, brain, sensors, is_herbivore) in
        characters
    {
        let self_kind = if is_herbivore {
            if consumed_herbivores.contains(&entity) {
                continue;
            }
            EntityKind::Herbivore
        } else {
            EntityKind::Predator
        };

        let inputs = sense_brain_inputs(&spatial_grid, *pos, *facing, self_kind, sensors);

        match brain.act_on(inputs) {
            Action::TurnLeft => {
                if energy.decrement().is_deceased() {
                    spatial_grid.remove(*pos, entity, self_kind);
                    commands.entity(entity).despawn();
                } else {
                    *facing = facing.counterclockwise();
                    age.increment();
                }
            }
            Action::TurnRight => {
                if energy.decrement().is_deceased() {
                    spatial_grid.remove(*pos, entity, self_kind);
                    commands.entity(entity).despawn();
                } else {
                    *facing = facing.clockwise();
                    age.increment();
                }
            }
            Action::GoForward => {
                if energy.decrement().is_deceased() {
                    spatial_grid.remove(*pos, entity, self_kind);
                    commands.entity(entity).despawn();
                } else {
                    let old_pos = *pos;
                    pos.add_assign_wrapping_direction(*facing, *field_size);
                    spatial_grid.move_entity(old_pos, *pos, entity, self_kind);
                    age.increment();
                }
            }
            Action::Consume => {
                let target_kind = if is_herbivore {
                    EntityKind::Plant
                } else {
                    EntityKind::Herbivore
                };
                if let Some((target_entity, target_pos)) = spatial_grid.find_target_in_near(
                    *pos,
                    *facing,
                    sensors.near(),
                    target_kind,
                    entity,
                ) {
                    if target_kind == EntityKind::Plant {
                        let new_pos = GridPosition::new(
                            rand::random_range(0..field_size.width()),
                            rand::random_range(0..field_size.height()),
                        );
                        *plant_positions.get_mut(target_entity).unwrap() = new_pos;
                        spatial_grid.move_entity(target_pos, new_pos, target_entity, target_kind);
                    } else {
                        commands.entity(target_entity).despawn();
                        spatial_grid.remove(target_pos, target_entity, target_kind);
                        consumed_herbivores.insert(target_entity);
                    }

                    let value = food.get(target_entity).unwrap();
                    energy.increment(*value);
                } else if energy.decrement().is_deceased() {
                    spatial_grid.remove(*pos, entity, self_kind);
                    commands.entity(entity).despawn();
                    continue;
                }
                age.increment();
            }
        }
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn reproduction(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
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
        if !energy.is_enough_to_reproduce() {
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

        let child_pos = pos.add_wrapping_signed(I16Vec2::from_array(offset), *field_size);
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
