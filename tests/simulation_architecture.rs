use core::num::NonZeroU64;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use bevy::prelude::*;
use life_sim::{
    grid::{GridPosition, GridSize},
    simulation::{
        DistributionConfig, HeadlessRunnerPlugin, HistoryRecorderPlugin, ReproductionPlugin,
        ResetSimulation, SampleRate, SimulationHistory, SimulationPlugin, SimulationSystems,
        SimulationTick, SimulationTickCount, StepSimulationOnce, WorldSeeded,
        organism::{EntityKind, Herbivore, Plant, Predator},
        spatial::{CellCounts, SpatialGrid},
    },
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

fn populate_mock_grid(spatial_grid: &mut SpatialGrid, counts: CellCounts<u32>) {
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
    populate_mock_grid(&mut spatial_grid, config.initial_counts);
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
    populate_mock_grid(&mut spatial_grid, config.initial_counts);
    commands.trigger(WorldSeeded);
}

fn mock_step_world(
    mut spatial_grid: ResMut<SpatialGrid>,
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
