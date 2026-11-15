use bevy::prelude::*;
use parking_lot::Mutex;

use super::brain::{
    Action,
    Brain,
    ConsumeTarget,
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
    Generation,
    Herbivore,
    MaxEnergy,
    Plant,
    ValueAsFood,
};
use super::spatial::SpatialGrid;
use super::{
    CharacterAged,
    CharacterDied,
    DeathCause,
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

        app.add_message::<CharacterDied>()
            .add_message::<CharacterAged>()
            .add_systems(
                SimulationTick,
                (decide_actions, apply_actions)
                    .chain()
                    .in_set(SimulationSystems::Step),
            );
    }
}

/// Phase 1 (Parallel, Order-Independent): Every character simultaneously senses the immutable
/// start-of-tick [`SpatialGrid`] snapshot, evaluates its [`Brain`], and stages its chosen action
/// in [`PreparedAction`] (atomically registering food consumption targets in [`ConsumerCount`]).
#[allow(clippy::type_complexity)]
fn decide_actions(
    spatial_grid: Res<SpatialGrid>,
    field_size: Res<GridSize>,
    mut characters: Query<(
        Entity,
        &GridPosition,
        &Facing,
        &Brain,
        &SensorOffsets,
        &mut PreparedAction,
        Has<Herbivore>,
    )>,
    consumers_count: Query<&ConsumerCount>,
) {
    characters.par_iter_mut().for_each(
        |(entity, pos, facing, brain, sensors, mut prepared, is_herbivore)| {
            let self_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };
            let inputs = sense_brain_inputs(&spatial_grid, *pos, *facing, self_kind, sensors);
            let chosen = brain.act_on(inputs);

            *prepared = match chosen {
                Action::TurnLeft => PreparedAction::TurnLeft,
                Action::TurnRight => PreparedAction::TurnRight,
                Action::GoForward => {
                    let mut next_pos = *pos;
                    next_pos.add_assign_wrapping_direction(*facing, *field_size);
                    PreparedAction::MoveTo(next_pos)
                }
                Action::Consume => {
                    let target_kind = if is_herbivore {
                        EntityKind::Plant
                    } else {
                        EntityKind::Herbivore
                    };
                    let target = spatial_grid
                        .find_target_in_near(*pos, *facing, sensors.near(), target_kind, entity)
                        .map(|(target_entity, target_pos)| {
                            let first_consumer = consumers_count
                                .get(target_entity)
                                .is_ok_and(ConsumerCount::increment);
                            ConsumeTarget {
                                entity: target_entity,
                                pos: target_pos,
                                first_consumer,
                            }
                        });
                    PreparedAction::Consume(target)
                }
            };
        },
    );
}

#[derive(Default)]
struct SharedStepTelemetry {
    died: Vec<CharacterDied>,
    max_herbivore_age: Option<Age>,
    max_predator_age: Option<Age>,
}

struct ThreadStepBatch<'a> {
    died: Vec<CharacterDied>,
    max_herbivore_age: Option<Age>,
    max_predator_age: Option<Age>,
    shared: &'a Mutex<SharedStepTelemetry>,
}

impl ThreadStepBatch<'_> {
    #[inline]
    fn record_age(&mut self, kind: EntityKind, age: Age) {
        let slot = match kind {
            EntityKind::Herbivore => &mut self.max_herbivore_age,
            EntityKind::Predator => &mut self.max_predator_age,
            EntityKind::Plant => return,
        };
        *slot = Some(slot.map_or(age, |prev| prev.max(age)));
    }
}

impl Drop for ThreadStepBatch<'_> {
    fn drop(&mut self) {
        if self.died.is_empty()
            && self.max_herbivore_age.is_none()
            && self.max_predator_age.is_none()
        {
            return;
        }
        let mut shared = self.shared.lock();
        shared.died.append(&mut self.died);
        if let Some(age) = self.max_herbivore_age {
            shared.max_herbivore_age =
                Some(shared.max_herbivore_age.map_or(age, |prev| prev.max(age)));
        }
        if let Some(age) = self.max_predator_age {
            shared.max_predator_age =
                Some(shared.max_predator_age.map_or(age, |prev| prev.max(age)));
        }
    }
}

/// Phase 2 (Parallel, Order-Independent): Splits targeted [`ValueAsFood`] evenly among all
/// simultaneous consumers (`food_value / consumer_count`), deducts action energy costs,
/// removes and despawns eaten or starved entities, and applies turns/movements and age increments
/// for surviving characters.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_actions(
    par_commands: ParallelCommands,
    spatial_grid: Res<SpatialGrid>,
    max_energy: Res<MaxEnergy>,
    action_costs: Res<ActionCosts>,
    mut died_writer: MessageWriter<CharacterDied>,
    mut aged_writer: MessageWriter<CharacterAged>,
    mut characters: Query<
        (
            Entity,
            &mut GridPosition,
            &mut Facing,
            &mut Energy,
            &mut Age,
            &Generation,
            &mut PreparedAction,
            Option<&ConsumerCount>,
            Has<Herbivore>,
        ),
        Without<Plant>,
    >,
    food: Query<(&ValueAsFood, &ConsumerCount)>,
) {
    let shared_telemetry: Mutex<SharedStepTelemetry> = Mutex::new(SharedStepTelemetry::default());

    characters.par_iter_mut().for_each_init(
        || ThreadStepBatch {
            died: Vec::new(),
            max_herbivore_age: None,
            max_predator_age: None,
            shared: &shared_telemetry,
        },
        |batch,
         (
            entity,
            mut pos,
            mut facing,
            mut energy,
            mut age,
            generation,
            mut prepared,
            my_consumers,
            is_herbivore,
        )| {
            let self_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };

            let staged = core::mem::take(&mut *prepared);
            let cost = match staged {
                PreparedAction::None => Energy(0.0),
                PreparedAction::TurnLeft | PreparedAction::TurnRight => action_costs.turn,
                PreparedAction::MoveTo(_) => action_costs.go_forward,
                PreparedAction::Consume(maybe_target) => {
                    if let Some(target) = maybe_target {
                        let (food_value, target_consumers) = food.get(target.entity).unwrap();
                        let count = target_consumers.get();
                        debug_assert!(count > 0);
                        let share = ValueAsFood(Energy(food_value.0.0 / count as f32));
                        energy.increment(share, *max_energy);

                        if is_herbivore && target.first_consumer {
                            spatial_grid.remove(target.pos, target.entity, EntityKind::Plant);
                            par_commands.command_scope(|mut commands| {
                                commands.entity(target.entity).despawn();
                            });
                        }
                    }
                    action_costs.consume
                }
            };

            let starved = energy.decrement(cost).is_deceased();
            let was_eaten = my_consumers.is_some_and(|c| c.get() > 0);
            if was_eaten || starved {
                let cause = if was_eaten {
                    DeathCause::Eaten
                } else {
                    DeathCause::Starved
                };
                batch.died.push(CharacterDied {
                    kind: self_kind,
                    age: *age,
                    generation: *generation,
                    cause,
                });
                spatial_grid.remove(*pos, entity, self_kind);
                par_commands.command_scope(|mut commands| {
                    commands.entity(entity).despawn();
                });
                return;
            }

            match staged {
                PreparedAction::TurnLeft => {
                    *facing = facing.counterclockwise();
                }
                PreparedAction::TurnRight => {
                    *facing = facing.clockwise();
                }
                PreparedAction::MoveTo(next_pos) => {
                    let old_pos = *pos;
                    *pos = next_pos;
                    spatial_grid.move_entity(old_pos, next_pos, entity, self_kind);
                }
                PreparedAction::Consume(_) | PreparedAction::None => {}
            }

            age.increment();
            batch.record_age(self_kind, *age);
        },
    );

    let telemetry = shared_telemetry.into_inner();
    if !telemetry.died.is_empty() {
        died_writer.write_batch(telemetry.died);
    }
    if let Some(max_age) = telemetry.max_herbivore_age {
        aged_writer.write(CharacterAged {
            kind: EntityKind::Herbivore,
            max_age,
        });
    }
    if let Some(max_age) = telemetry.max_predator_age {
        aged_writer.write(CharacterAged {
            kind: EntityKind::Predator,
            max_age,
        });
    }
}
