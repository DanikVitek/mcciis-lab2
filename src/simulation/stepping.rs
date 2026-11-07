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
    ConsumerCount,
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

/// Phase 1 (Parallel, Order-Independent): Every character simultaneously senses the immutable
/// start-of-tick [`SpatialGrid`] snapshot, evaluates its [`Brain`], applies in-place turns, and
/// stages movements or atomically registers food consumption targets in [`ConsumerCount`].
#[allow(clippy::type_complexity)]
fn decide_actions(
    spatial_grid: Res<SpatialGrid>,
    field_size: Res<GridSize>,
    action_costs: Res<ActionCosts>,
    mut characters: Query<
        (
            Entity,
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
    food_consumers: Query<&ConsumerCount, Without<Predator>>,
) {
    let turn_cost = action_costs.turn;
    let go_forward_cost = action_costs.go_forward;

    characters.par_iter_mut().for_each(
        |(
            entity,
            pos,
            mut facing,
            mut energy,
            mut age,
            brain,
            sensors,
            mut prepared,
            is_herbivore,
        )| {
            let self_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };
            let inputs = sense_brain_inputs(&spatial_grid, *pos, *facing, self_kind, sensors);
            let chosen = brain.act_on(inputs);

            match chosen {
                Action::TurnLeft => {
                    if !energy.decrement(turn_cost).is_deceased() {
                        *facing = facing.counterclockwise();
                        age.increment();
                    }
                    *prepared = PreparedAction::None;
                }
                Action::TurnRight => {
                    if !energy.decrement(turn_cost).is_deceased() {
                        *facing = facing.clockwise();
                        age.increment();
                    }
                    *prepared = PreparedAction::None;
                }
                Action::GoForward => {
                    if !energy.decrement(go_forward_cost).is_deceased() {
                        let mut next_pos = *pos;
                        next_pos.add_assign_wrapping_direction(*facing, *field_size);
                        *prepared = PreparedAction::MoveTo(next_pos);
                    } else {
                        *prepared = PreparedAction::None;
                    }
                }
                Action::Consume => {
                    let target_kind = if is_herbivore {
                        EntityKind::Plant
                    } else {
                        EntityKind::Herbivore
                    };
                    let target = spatial_grid
                        .find_target_in_near(*pos, *facing, sensors.near(), target_kind, entity)
                        .map(|(target_entity, _)| target_entity);
                    if let Some(target_entity) = target
                        && let Ok(consumers) = food_consumers.get(target_entity)
                    {
                        consumers.increment();
                    }
                    *prepared = PreparedAction::Consume(target);
                }
            }
        },
    );
}

/// Phase 2 (Parallel, Order-Independent): Splits targeted [`ValueAsFood`] evenly among all
/// simultaneous consumers (`food_value / consumer_count`), applies staged movements to
/// [`SpatialGrid`], and queues despawns via [`ParallelCommands`] for all eaten or starved entities.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_actions(
    par_commands: ParallelCommands,
    spatial_grid: Res<SpatialGrid>,
    max_energy: Res<MaxEnergy>,
    action_costs: Res<ActionCosts>,
    mut characters: Query<
        (
            Entity,
            &mut GridPosition,
            &mut Energy,
            &mut Age,
            &mut PreparedAction,
            Option<&ConsumerCount>,
            Has<Herbivore>,
        ),
        Without<Plant>,
    >,
    food: Query<(&ValueAsFood, &ConsumerCount), Without<Predator>>,
    plants: Query<(Entity, &GridPosition, &ConsumerCount), With<Plant>>,
) {
    let consume_cost = action_costs.consume;

    characters.par_iter_mut().for_each(
        |(entity, mut pos, mut energy, mut age, mut prepared, my_consumers, is_herbivore)| {
            let self_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };

            let staged = core::mem::take(&mut *prepared);
            if let PreparedAction::Consume(maybe_target) = staged {
                if let Some(target_entity) = maybe_target
                    && let Ok((food_value, target_consumers)) = food.get(target_entity)
                {
                    let count = target_consumers.get();
                    debug_assert!(count > 0);
                    let share = ValueAsFood(Energy(food_value.0.0 / count as f32));
                    energy.increment(share, *max_energy);
                }
                if !energy.decrement(consume_cost).is_deceased() {
                    age.increment();
                }
            }

            let was_eaten = my_consumers.is_some_and(|c| c.get() > 0);
            if was_eaten || energy.0 <= 0.0 {
                spatial_grid.remove(*pos, entity, self_kind);
                par_commands.command_scope(|mut commands| {
                    commands.entity(entity).despawn();
                });
                return;
            }

            if let PreparedAction::MoveTo(next_pos) = staged {
                let old_pos = *pos;
                *pos = next_pos;
                spatial_grid.move_entity(old_pos, next_pos, entity, self_kind);
                age.increment();
            }
        },
    );

    plants
        .par_iter()
        .for_each(|(plant_entity, pos, consumers)| {
            if consumers.get() > 0 {
                spatial_grid.remove(*pos, plant_entity, EntityKind::Plant);
                par_commands.command_scope(|mut commands| {
                    commands.entity(plant_entity).despawn();
                });
            }
        });
}
