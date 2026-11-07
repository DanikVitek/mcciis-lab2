use core::num::NonZeroU64;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{
    AtomicU64,
    Ordering,
};

use bevy::math::I16Vec2;
use bevy::prelude::*;
use life_sim::grid::{
    Facing,
    GridPosition,
    GridSize,
};
use life_sim::simulation::brain::{
    Action,
    Brain,
    INPUTS,
    OUTPUTS,
    SENSORS_HERBIVORE,
    SENSORS_PREDATOR,
};
use life_sim::simulation::organism::{
    ActionCosts,
    Energy,
    EntityKind,
    FoodValues,
    Generation,
    Herbivore,
    Plant,
    Predator,
    ValueAsFood,
};
use life_sim::simulation::spatial::{
    CellCounts,
    SpatialGrid,
};
use life_sim::simulation::{
    DistributionConfig,
    HeadlessRunnerPlugin,
    HistoryRecorderPlugin,
    ReproductionPlugin,
    ResetSimulation,
    SampleRate,
    SimulationHistory,
    SimulationPlugin,
    SimulationSystems,
    SimulationTick,
    SimulationTickCount,
    StepSimulationOnce,
    WorldSeeded,
};

/// Deterministic mock replacement for [`SimulationPlugin`] used to test [`HistoryRecorderPlugin`]
/// without any randomness from organism brains, movement, or reproduction.
struct MockSimulationPlugin {
    grid_size: GridSize,
    initial_counts: CellCounts<u32>,
    /// Predicate deciding whether the mock world mutates its population on a given 1-based tick.
    mutate_on_tick: fn(u64) -> bool,
}

#[derive(Resource, Clone, Copy)]
struct MockSimConfig {
    grid_size: GridSize,
    initial_counts: CellCounts<u32>,
    mutate_on_tick: fn(u64) -> bool,
}

impl MockSimulationPlugin {
    const fn new(initial_counts: CellCounts<u32>) -> Self {
        Self {
            grid_size: GridSize::new(10, 10),
            initial_counts,
            mutate_on_tick: |_| true,
        }
    }

    const fn with_tick_predicate(
        initial_counts: CellCounts<u32>,
        mutate_on_tick: fn(u64) -> bool,
    ) -> Self {
        Self {
            grid_size: GridSize::new(10, 10),
            initial_counts,
            mutate_on_tick,
        }
    }
}

impl Plugin for MockSimulationPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.grid_size)
            .insert_resource(SpatialGrid::new(self.grid_size))
            .insert_resource(MockSimConfig {
                grid_size: self.grid_size,
                initial_counts: self.initial_counts,
                mutate_on_tick: self.mutate_on_tick,
            })
            .init_resource::<SimulationTickCount>()
            .init_schedule(SimulationTick)
            .configure_sets(
                SimulationTick,
                (
                    SimulationSystems::Step,
                    SimulationSystems::Reproduce,
                    SimulationSystems::Record,
                )
                    .chain(),
            )
            .add_systems(Startup, mock_seed_world)
            .add_systems(
                SimulationTick,
                (
                    mock_step_world.in_set(SimulationSystems::Step),
                    mock_advance_tick_counter
                        .after_ignore_deferred(SimulationSystems::Reproduce)
                        .before_ignore_deferred(SimulationSystems::Record),
                ),
            )
            .add_observer(mock_on_reset_simulation);
    }
}

fn populate_mock_grid(spatial_grid: &SpatialGrid, counts: CellCounts<u32>) {
    let origin = GridPosition::new(0, 0);
    for _ in 0..counts.plants {
        spatial_grid.insert(origin, Entity::PLACEHOLDER, EntityKind::Plant);
    }
    for _ in 0..counts.herbivores {
        spatial_grid.insert(origin, Entity::PLACEHOLDER, EntityKind::Herbivore);
    }
    for _ in 0..counts.predators {
        spatial_grid.insert(origin, Entity::PLACEHOLDER, EntityKind::Predator);
    }
}

fn mock_seed_world(
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    config: Res<MockSimConfig>,
) {
    spatial_grid.resize_and_clear(config.grid_size);
    populate_mock_grid(&spatial_grid, config.initial_counts);
    commands.trigger(WorldSeeded);
}

fn mock_on_reset_simulation(
    _reset: On<ResetSimulation>,
    mut commands: Commands,
    mut spatial_grid: ResMut<SpatialGrid>,
    mut tick_count: ResMut<SimulationTickCount>,
    config: Res<MockSimConfig>,
) {
    tick_count.0 = 0;
    spatial_grid.resize_and_clear(config.grid_size);
    populate_mock_grid(&spatial_grid, config.initial_counts);
    commands.trigger(WorldSeeded);
}

fn mock_step_world(
    spatial_grid: Res<SpatialGrid>,
    tick_count: Res<SimulationTickCount>,
    config: Res<MockSimConfig>,
) {
    let upcoming_tick = tick_count.0 + 1;
    if (config.mutate_on_tick)(upcoming_tick) {
        spatial_grid.insert(
            GridPosition::new(0, 0),
            Entity::PLACEHOLDER,
            EntityKind::Herbivore,
        );
    }
}

fn mock_advance_tick_counter(mut ticks: ResMut<SimulationTickCount>) {
    ticks.0 += 1;
}

#[test]
fn headless_simulation_runs_without_fixed_update_or_graphics() {
    let dist = DistributionConfig::builder()
        .plants_fraction(0.15)
        .herbivores_fraction(0.10)
        .predators_fraction(0.05)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::builder()
            .grid_size(GridSize::new(20, 20))
            .distribution(dist)
            .initial_energy(50)
            .max_energy(100)
            .build(),
        ReproductionPlugin::default(),
    ));

    // Runs `Startup` (`seed_world`) once.
    app.update();

    assert_eq!(app.world().resource::<SimulationTickCount>().0, 0);
    let initial_totals = app.world().resource::<SpatialGrid>().total_counts();
    assert_eq!(initial_totals.plants, 60);
    assert_eq!(initial_totals.herbivores, 40);
    assert_eq!(initial_totals.predators, 20);

    // Run 100 discrete simulation ticks directly with zero timer / rendering overhead.
    for _ in 0..100 {
        app.world_mut().run_schedule(SimulationTick);
    }

    assert_eq!(app.world().resource::<SimulationTickCount>().0, 100);

    // Verify `SpatialGrid` counts remain strictly consistent with live ECS entities.
    let grid_totals = app.world().resource::<SpatialGrid>().total_counts();
    let world = app.world_mut();
    let ecs_plants = world
        .query_filtered::<Entity, With<Plant>>()
        .iter(world)
        .count() as u32;
    let ecs_herbivores = world
        .query_filtered::<Entity, With<Herbivore>>()
        .iter(world)
        .count() as u32;
    let ecs_predators = world
        .query_filtered::<Entity, With<Predator>>()
        .iter(world)
        .count() as u32;

    assert_eq!(grid_totals.plants, ecs_plants);
    assert_eq!(grid_totals.herbivores, ecs_herbivores);
    assert_eq!(grid_totals.predators, ecs_predators);
}

#[test]
fn simulation_runs_with_swapped_reproduction_strategy() {
    #[derive(Resource, Default)]
    struct CustomReproductionRuns(u64);

    struct CustomReproductionPlugin;
    impl Plugin for CustomReproductionPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<CustomReproductionRuns>().add_systems(
                SimulationTick,
                (|mut runs: ResMut<CustomReproductionRuns>| {
                    runs.0 += 1;
                })
                .in_set(SimulationSystems::Reproduce),
            );
        }
    }

    let dist = DistributionConfig::builder()
        .plants_fraction(0.1)
        .herbivores_fraction(0.1)
        .predators_fraction(0.05)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::builder()
            .grid_size(GridSize::new(10, 10))
            .distribution(dist)
            .initial_energy(50)
            .max_energy(100)
            .build(),
        CustomReproductionPlugin,
    ));
    app.update();

    for _ in 0..10 {
        app.world_mut().run_schedule(SimulationTick);
    }

    assert_eq!(app.world().resource::<SimulationTickCount>().0, 10);
    assert_eq!(app.world().resource::<CustomReproductionRuns>().0, 10);
}

#[test]
fn headless_runner_plugin_exits_at_target_ticks() {
    let dist = DistributionConfig::builder()
        .plants_fraction(0.1)
        .herbivores_fraction(0.1)
        .predators_fraction(0.05)
        .build()
        .unwrap();

    let observed_ticks = Arc::new(AtomicU64::new(0));
    let observed_ticks_clone = Arc::clone(&observed_ticks);

    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        SimulationPlugin::builder()
            .grid_size(GridSize::new(16, 16))
            .distribution(dist)
            .initial_energy(50)
            .max_energy(100)
            .build(),
        ReproductionPlugin::default(),
        HeadlessRunnerPlugin::for_ticks(25),
    ))
    .add_systems(Last, move |ticks: Res<SimulationTickCount>| {
        observed_ticks_clone.store(ticks.0, Ordering::Relaxed);
    });

    // Note: `App::run` consumes the internal `App` via `mem::take`.
    let exit = app.run();
    assert_eq!(exit, AppExit::Success);
    assert_eq!(observed_ticks.load(Ordering::Relaxed), 25);
}

#[test]
fn reset_and_step_once_events_reconfigure_and_advance_world() {
    let dist = DistributionConfig::builder()
        .plants_fraction(0.1)
        .herbivores_fraction(0.1)
        .predators_fraction(0.1)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::builder()
            .grid_size(GridSize::new(10, 10))
            .distribution(dist)
            .initial_energy(50)
            .max_energy(100)
            .build(),
        ReproductionPlugin::default(),
    ));
    app.update();

    app.world_mut().trigger(StepSimulationOnce);
    app.world_mut().flush();
    assert_eq!(app.world().resource::<SimulationTickCount>().0, 1);

    // Reconfigure grid dimensions & fractions and trigger `ResetSimulation`.
    *app.world_mut().resource_mut::<GridSize>() = GridSize::new(20, 15);
    *app.world_mut().resource_mut::<DistributionConfig>() = DistributionConfig::builder()
        .plants_fraction(0.2)
        .herbivores_fraction(0.05)
        .predators_fraction(0.0)
        .build()
        .unwrap();

    app.world_mut().trigger(ResetSimulation);
    app.world_mut().flush();

    assert_eq!(app.world().resource::<SimulationTickCount>().0, 0);
    let spatial = app.world().resource::<SpatialGrid>();
    assert_eq!(spatial.size(), GridSize::new(20, 15));
    let totals = spatial.total_counts();
    assert_eq!(totals.plants, 60);
    assert_eq!(totals.herbivores, 15);
    assert_eq!(totals.predators, 0);
}

#[test]
fn history_recorder_tracks_baseline_ticks_and_resets() {
    let mut app = App::new();
    app.add_plugins((
        MockSimulationPlugin::new(CellCounts {
            plants: 20,
            herbivores: 10,
            predators: 5,
        }),
        HistoryRecorderPlugin::default(),
    ));
    app.update();

    // `WorldSeeded` records the `tick = 0` baseline immediately after seeding.
    {
        let history = app.world().resource::<SimulationHistory>();
        assert_eq!(history.len(), 1);
        assert_eq!(history.tick_history()[0], 0);
        let counts = history.counts_history();
        assert_eq!(counts.plants[0], 20);
        assert_eq!(counts.herbivores[0], 10);
        assert_eq!(counts.predators[0], 5);
    }

    for _ in 0..10 {
        app.world_mut().run_schedule(SimulationTick);
    }

    {
        let history = app.world().resource::<SimulationHistory>();
        assert_eq!(history.tick_history(), &(0..=10).collect::<Vec<u64>>());
        assert_eq!(
            history.last().unwrap().counts,
            app.world().resource::<SpatialGrid>().total_counts()
        );

        // Verify forward and reverse (`DoubleEndedIterator`) iteration match indexed `get()`.
        let forward: Vec<_> = history.iter().collect();
        let mut reversed: Vec<_> = history.iter().rev().collect();
        reversed.reverse();
        assert_eq!(forward.len(), 11);
        assert_eq!(forward, reversed);
        for (i, sample) in forward.into_iter().enumerate() {
            assert_eq!(Some(sample), history.get(i));
        }
    }

    // `ResetSimulation` clears history and records a fresh `tick = 0` baseline.
    app.world_mut().trigger(ResetSimulation);
    app.world_mut().flush();

    let history = app.world().resource::<SimulationHistory>();
    assert_eq!(history.len(), 1);
    assert_eq!(history.first_tick(), Some(0));
}

#[test]
fn sample_rate_skips_non_multiple_ticks_and_preserves_rate_across_reset() {
    let mut app = App::new();
    app.add_plugins((
        MockSimulationPlugin::new(CellCounts {
            plants: 15,
            herbivores: 10,
            predators: 5,
        }),
        HistoryRecorderPlugin::every(NonZeroU64::new(5).unwrap()),
    ));
    app.update();

    // Run 13 ticks (not a multiple of 5): only baseline 0 and multiples 5, 10 should be recorded.
    for _ in 0..13 {
        app.world_mut().run_schedule(SimulationTick);
    }

    assert_eq!(app.world().resource::<SimulationTickCount>().0, 13);
    assert_eq!(
        app.world().resource::<SimulationHistory>().tick_history(),
        &[0, 5, 10]
    );

    // Resetting clears history to `[0]` while preserving `SampleRate::every(5)`.
    app.world_mut().trigger(ResetSimulation);
    app.world_mut().flush();

    for _ in 0..7 {
        app.world_mut().run_schedule(SimulationTick);
    }

    assert_eq!(
        app.world().resource::<SimulationHistory>().tick_history(),
        &[0, 5]
    );
}

#[test]
fn sample_rate_exceeding_run_length_keeps_only_baseline_sample() {
    let mut app = App::new();
    app.add_plugins((
        MockSimulationPlugin::new(CellCounts {
            plants: 10,
            herbivores: 10,
            predators: 0,
        }),
        HistoryRecorderPlugin::every(NonZeroU64::new(50).unwrap()),
    ));
    app.update();

    for _ in 0..12 {
        app.world_mut().run_schedule(SimulationTick);
    }

    let history = app.world().resource::<SimulationHistory>();
    assert_eq!(history.len(), 1);
    assert_eq!(history.tick_history(), &[0]);
}

#[test]
fn sample_rate_can_change_at_runtime_and_only_marks_history_changed_on_sample_ticks() {
    #[derive(Resource, Default)]
    struct ChangedTicks(Vec<u64>);

    let mut app = App::new();
    app.init_resource::<ChangedTicks>()
        .add_plugins((
            MockSimulationPlugin::new(CellCounts {
                plants: 15,
                herbivores: 10,
                predators: 5,
            }),
            HistoryRecorderPlugin::every(NonZeroU64::new(2).unwrap()),
        ))
        .add_systems(
            SimulationTick,
            (|history: Res<SimulationHistory>,
              tick: Res<SimulationTickCount>,
              mut changed: ResMut<ChangedTicks>| {
                if history.is_changed() {
                    changed.0.push(tick.0);
                }
            })
            .after(SimulationSystems::Record),
        );
    app.update();

    // Ticks 1..=6 at `every(2)` -> samples at 2, 4, 6.
    // Note: on tick 1, the observer system runs for the first time and sees the `tick = 0` baseline mutation,
    // so clear `ChangedTicks` right after tick 1 to isolate per-tick change detection.
    app.world_mut().run_schedule(SimulationTick);
    app.world_mut().resource_mut::<ChangedTicks>().0.clear();

    for _ in 2..=6 {
        app.world_mut().run_schedule(SimulationTick);
    }

    // Switch to `every(5)` at runtime for ticks 7..=16 -> samples at 10, 15.
    *app.world_mut().resource_mut::<SampleRate>() = SampleRate::every(NonZeroU64::new(5).unwrap());
    for _ in 7..=16 {
        app.world_mut().run_schedule(SimulationTick);
    }

    // Switch to `SampleRate::default()` (`every(1)`) for ticks 17..=19 -> samples at 17, 18, 19.
    *app.world_mut().resource_mut::<SampleRate>() = SampleRate::default();
    for _ in 17..=19 {
        app.world_mut().run_schedule(SimulationTick);
    }

    assert_eq!(
        app.world().resource::<SimulationHistory>().tick_history(),
        &[0, 2, 4, 6, 10, 15, 17, 18, 19]
    );
    assert_eq!(
        app.world().resource::<ChangedTicks>().0,
        &[2, 4, 6, 10, 15, 17, 18, 19]
    );
}

#[test]
fn history_recorder_deduplicates_unchanged_samples_while_updating_last_tick() {
    let mut app = App::new();
    app.add_plugins((
        MockSimulationPlugin::with_tick_predicate(
            CellCounts {
                plants: 10,
                herbivores: 5,
                predators: 2,
            },
            |tick| matches!(tick, 2 | 5),
        ),
        HistoryRecorderPlugin::default(),
    ));
    app.update();

    for _ in 0..7 {
        app.world_mut().run_schedule(SimulationTick);
    }

    let history = app.world().resource::<SimulationHistory>();
    assert_eq!(history.tick_history(), &[0, 2, 5]);
    assert_eq!(history.last_tick(), Some(7));
    assert_eq!(history.counts_history().herbivores, &[5, 6, 7]);
}

fn deterministic_action_brain(target: Action) -> Brain {
    for idx in 0..OUTPUTS {
        let mut brain: Brain = rand::random();
        brain.weights_mut().fill(0.0);
        brain.biases_mut().fill(-10.0);
        brain.biases_mut()[idx] = 10.0;
        if brain.act_on([0.0; INPUTS].into()) == target {
            return brain;
        }
    }
    panic!("no output neuron produced {target:?}");
}

fn position_with_target_in_near(
    target: GridPosition,
    facing: Facing,
    near_offsets: &[I16Vec2],
    size: GridSize,
) -> GridPosition {
    let offset = near_offsets
        .iter()
        .copied()
        .find(|&o| o != I16Vec2::ZERO)
        .unwrap_or(I16Vec2::ZERO);
    target.add_wrapping(-facing.rotate_offset(offset), size)
}

#[test]
fn simultaneous_consumers_split_food_evenly_and_handle_chained_predation() {
    let size = GridSize::new(10, 10);
    let dist = DistributionConfig::builder()
        .plants_fraction(0.0)
        .herbivores_fraction(0.0)
        .predators_fraction(0.0)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins(
        SimulationPlugin::builder()
            .grid_size(size)
            .distribution(dist)
            .initial_energy(10.0)
            .max_energy(100.0)
            .action_costs(ActionCosts {
                turn: Energy(0.0),
                go_forward: Energy(0.0),
                consume: Energy(1.0),
            })
            .food_values(FoodValues {
                plant: Energy(9.0),
                herbivore: Energy(12.0),
            })
            .build(),
    );
    app.update();

    let consume_brain = deterministic_action_brain(Action::Consume);
    let plant_pos = GridPosition::new(5, 5);

    let (plant, h1, h2, h3, p1, p2) = {
        let world = app.world_mut();
        let plant = world
            .spawn((Plant, plant_pos, ValueAsFood(Energy(9.0))))
            .id();
        world
            .resource::<SpatialGrid>()
            .insert(plant_pos, plant, EntityKind::Plant);

        let h1_pos =
            position_with_target_in_near(plant_pos, Facing::East, SENSORS_HERBIVORE.near(), size);
        let h2_pos =
            position_with_target_in_near(plant_pos, Facing::West, SENSORS_HERBIVORE.near(), size);
        let h3_pos =
            position_with_target_in_near(plant_pos, Facing::North, SENSORS_HERBIVORE.near(), size);

        let h1 = world
            .spawn((
                Herbivore,
                h1_pos,
                Facing::East,
                consume_brain.clone(),
                Generation::default(),
                Energy(10.0),
                ValueAsFood(Energy(12.0)),
                SENSORS_HERBIVORE.to_owned(),
            ))
            .id();
        let h2 = world
            .spawn((
                Herbivore,
                h2_pos,
                Facing::West,
                consume_brain.clone(),
                Generation::default(),
                Energy(10.0),
                ValueAsFood(Energy(12.0)),
                SENSORS_HERBIVORE.to_owned(),
            ))
            .id();
        let h3 = world
            .spawn((
                Herbivore,
                h3_pos,
                Facing::North,
                consume_brain.clone(),
                Generation::default(),
                Energy(10.0),
                ValueAsFood(Energy(12.0)),
                SENSORS_HERBIVORE.to_owned(),
            ))
            .id();

        for (pos, entity) in [(h1_pos, h1), (h2_pos, h2), (h3_pos, h3)] {
            world
                .resource::<SpatialGrid>()
                .insert(pos, entity, EntityKind::Herbivore);
        }

        // Two predators simultaneously target `h1` on the same tick `h1` consumes `plant`.
        let p1_pos =
            position_with_target_in_near(h1_pos, Facing::North, SENSORS_PREDATOR.near(), size);
        let p2_pos =
            position_with_target_in_near(h1_pos, Facing::South, SENSORS_PREDATOR.near(), size);

        let p1 = world
            .spawn((
                Predator,
                p1_pos,
                Facing::North,
                consume_brain.clone(),
                Generation::default(),
                Energy(10.0),
                SENSORS_PREDATOR.to_owned(),
            ))
            .id();
        let p2 = world
            .spawn((
                Predator,
                p2_pos,
                Facing::South,
                consume_brain,
                Generation::default(),
                Energy(10.0),
                SENSORS_PREDATOR.to_owned(),
            ))
            .id();

        for (pos, entity) in [(p1_pos, p1), (p2_pos, p2)] {
            world
                .resource::<SpatialGrid>()
                .insert(pos, entity, EntityKind::Predator);
        }

        (plant, h1, h2, h3, p1, p2)
    };

    app.world_mut().run_schedule(SimulationTick);

    let world = app.world();
    let totals = world.resource::<SpatialGrid>().total_counts();
    assert_eq!(totals.plants, 0);
    assert_eq!(totals.herbivores, 2);
    assert_eq!(totals.predators, 2);

    assert!(world.get_entity(plant).is_err());
    assert!(world.get_entity(h1).is_err());

    // 3 herbivores split `9.0` evenly (`3.0` each) minus `1.0` consume cost = `+2.0` -> `12.0`.
    assert_eq!(*world.get::<Energy>(h2).unwrap(), Energy(12.0));
    assert_eq!(*world.get::<Energy>(h3).unwrap(), Energy(12.0));

    // 2 predators split `12.0` evenly (`6.0` each) minus `1.0` consume cost = `+5.0` -> `15.0`.
    assert_eq!(*world.get::<Energy>(p1).unwrap(), Energy(15.0));
    assert_eq!(*world.get::<Energy>(p2).unwrap(), Energy(15.0));
}

#[test]
fn plants_bud_locally_up_to_capacity_and_stay_extinct_at_zero() {
    let size = GridSize::new(10, 10);
    let dist = DistributionConfig::builder()
        .plants_fraction(0.20)
        .herbivores_fraction(0.0)
        .predators_fraction(0.0)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::builder()
            .grid_size(size)
            .distribution(dist)
            .initial_energy(10.0)
            .max_energy(100.0)
            .build(),
        ReproductionPlugin::builder().plant_growth_rate(1.0).build(),
    ));
    app.update();

    // Despawn all seeded plants and leave a single founder plant at `(5, 5)`.
    let founder_pos = GridPosition::new(5, 5);
    {
        let existing: Vec<(Entity, GridPosition)> = {
            let world = app.world_mut();
            world
                .query_filtered::<(Entity, &GridPosition), With<Plant>>()
                .iter(world)
                .map(|(e, p)| (e, *p))
                .collect()
        };
        let world = app.world_mut();
        for (entity, pos) in existing {
            world
                .resource::<SpatialGrid>()
                .remove(pos, entity, EntityKind::Plant);
            world.despawn(entity);
        }
        let founder = world
            .spawn((Plant, founder_pos, ValueAsFood(Energy(1.0))))
            .id();
        world
            .resource::<SpatialGrid>()
            .insert(founder_pos, founder, EntityKind::Plant);
    }

    assert_eq!(app.world().resource::<SpatialGrid>().plant_count(), 1);

    // Advance until the first offspring buds, and verify it appears on an adjacent Moore neighbor.
    for _ in 0..50 {
        app.world_mut().run_schedule(SimulationTick);
        if app.world().resource::<SpatialGrid>().plant_count() > 1 {
            break;
        }
    }

    {
        let world = app.world_mut();
        let positions: Vec<GridPosition> = world
            .query_filtered::<&GridPosition, With<Plant>>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(positions.len(), 2);
        let child_pos = positions.into_iter().find(|&p| p != founder_pos).unwrap();
        let dx = (i32::from(child_pos.x) - i32::from(founder_pos.x)).abs();
        let dy = (i32::from(child_pos.y) - i32::from(founder_pos.y)).abs();
        assert!(dx <= 1 && dy <= 1 && (dx != 0 || dy != 0));
    }

    // Advance further: logistic budding fills out to `max_plants = 20`, never exceeding it or stacking tiles.
    for _ in 0..400 {
        app.world_mut().run_schedule(SimulationTick);
    }

    assert_eq!(app.world().resource::<SpatialGrid>().plant_count(), 20);
    {
        let world = app.world_mut();
        let positions: HashSet<GridPosition> = world
            .query_filtered::<&GridPosition, With<Plant>>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(
            positions.len(),
            20,
            "expected 20 plants on 20 distinct tiles"
        );
    }

    // Once all plants are consumed/removed (`0` remaining), algae remain permanently extinct.
    {
        let all_plants: Vec<(Entity, GridPosition)> = {
            let world = app.world_mut();
            world
                .query_filtered::<(Entity, &GridPosition), With<Plant>>()
                .iter(world)
                .map(|(e, p)| (e, *p))
                .collect()
        };
        let world = app.world_mut();
        for (entity, pos) in all_plants {
            world
                .resource::<SpatialGrid>()
                .remove(pos, entity, EntityKind::Plant);
            world.despawn(entity);
        }
    }
    assert_eq!(app.world().resource::<SpatialGrid>().plant_count(), 0);

    for _ in 0..50 {
        app.world_mut().run_schedule(SimulationTick);
    }
    assert_eq!(app.world().resource::<SpatialGrid>().plant_count(), 0);
}
