use bevy::ecs::entity::EntityHashSet;
use bevy::prelude::*;

use super::brain::{
    Action,
    Brain,
    SensorOffsets,
    sense_brain_inputs,
};
use super::organism::{
    Age,
    Energy,
    EntityKind,
    Herbivore,
    MaxEnergy,
    Plant,
    Predator,
    ValueAsFood,
};
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

pub(super) struct SteppingPlugin;

impl Plugin for SteppingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            SimulationTick,
            step_organisms.in_set(SimulationSystems::Step),
        );
    }
}

#[allow(clippy::type_complexity)]
fn step_organisms(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
    max_energy: Res<MaxEnergy>,
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
                    energy.increment(*value, *max_energy);
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
