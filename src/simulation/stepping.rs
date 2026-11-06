use bevy::ecs::entity::EntityHashSet;
use bevy::ecs::relationship::RelationshipSourceCollection;
use bevy::prelude::*;

use super::brain::{
    Action,
    Brain,
    PreparedAction,
    SensorOffsets,
    sense_brain_inputs,
};
use super::organism::{
    ActionCosts,
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
        if !app.is_plugin_added::<TaskPoolPlugin>() {
            app.add_plugins(TaskPoolPlugin::default());
        }

        app.add_systems(
            SimulationTick,
            (decide_actions, apply_actions)
                .chain()
                .in_set(SimulationSystems::Step),
        );
    }
}

/// Phase 1 (Parallel): Senses the [`SpatialGrid`] snapshot, evaluates each character's [`Brain`]
/// across all worker threads on Bevy's [`ComputeTaskPool`](bevy::tasks::ComputeTaskPool), and
/// immediately applies non-lethal [`Action::TurnLeft`] / [`Action::TurnRight`] updates in parallel.
#[allow(clippy::type_complexity)]
fn decide_actions(
    spatial_grid: Res<SpatialGrid>,
    action_costs: Res<ActionCosts>,
    mut characters: Query<
        (
            &GridPosition,
            &mut Facing,
            &mut Energy,
            &mut Age,
            &Brain,
            &SensorOffsets,
            &mut PreparedAction,
            Has<Herbivore>,
        ),
        Without<Plant>,
    >,
) {
    let turn_cost = action_costs.turn;

    characters.par_iter_mut().for_each(
        |(pos, mut facing, mut energy, mut age, brain, sensors, mut action, is_herbivore)| {
            let self_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };
            let inputs = sense_brain_inputs(&spatial_grid, *pos, *facing, self_kind, sensors);
            let chosen = brain.act_on(inputs);

            // Apply non-lethal in-place turns directly in parallel so `apply_actions` only needs
            // to process movements, consumption, and lethal energy depletion.
            match chosen {
                Action::TurnLeft if energy.0 > turn_cost.0 => {
                    let _ = energy.decrement(turn_cost);
                    *facing = facing.counterclockwise();
                    age.increment();
                    action.0 = None;
                }
                Action::TurnRight if energy.0 > turn_cost.0 => {
                    let _ = energy.decrement(turn_cost);
                    *facing = facing.clockwise();
                    age.increment();
                    action.0 = None;
                }
                _ => {
                    action.0 = Some(chosen);
                }
            }
        },
    );
}

/// Phase 2 (Sequential): Resolves movements, consumption, and lethal energy depletion that mutate
/// [`SpatialGrid`] or other entities.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_actions(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    field_size: Res<GridSize>,
    max_energy: Res<MaxEnergy>,
    action_costs: Res<ActionCosts>,
    characters: Query<
        (
            Entity,
            &mut GridPosition,
            &Facing,
            &mut Energy,
            &mut Age,
            &mut PreparedAction,
            &SensorOffsets,
            Has<Herbivore>,
        ),
        Without<Plant>,
    >,
    food: Query<&ValueAsFood, Without<Predator>>,
    mut plant_positions: Query<&mut GridPosition, With<Plant>>,
) {
    let mut consumed_herbivores = EntityHashSet::new();

    for (entity, mut pos, facing, mut energy, mut age, mut action, sensors, is_herbivore) in
        characters
    {
        let Some(action) = action.0.take() else {
            continue;
        };
        let self_kind = if is_herbivore {
            if consumed_herbivores.remove(entity) {
                continue;
            }
            EntityKind::Herbivore
        } else {
            EntityKind::Predator
        };

        match action {
            Action::TurnLeft | Action::TurnRight => {
                // Non-lethal turns (`energy.0 > turn_cost.0`) were already applied in `decide_actions`.
                // Only organisms whose remaining energy could not survive the turn cost reach here.
                if energy.decrement(action_costs.turn).is_deceased() {
                    spatial_grid.remove(*pos, entity, self_kind);
                    commands.entity(entity).despawn();
                }
            }
            Action::GoForward => {
                if energy.decrement(action_costs.go_forward).is_deceased() {
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
                }

                if energy.decrement(action_costs.consume).is_deceased() {
                    spatial_grid.remove(*pos, entity, self_kind);
                    commands.entity(entity).despawn();
                    continue;
                }
                age.increment();
            }
        }
    }
}
