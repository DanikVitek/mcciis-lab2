use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use life_sim::{
    grid::GridSize,
    presentation::ui::inspector::{BrainInspectorUiPlugin, CharacterInspector, InspectedCharacter},
    simulation::{
        DistributionConfig, HeadlessRunnerPlugin, ResetSimulation, SimulationPlugin,
        SimulationTick, SimulationTickCount, StepSimulationOnce,
        organism::{Herbivore, Plant, Predator},
        spatial::SpatialGrid,
    },
};

#[test]
fn headless_simulation_runs_without_fixed_update_or_graphics() {
    let dist = DistributionConfig::builder()
        .plants_fraction(0.15)
        .herbivores_fraction(0.10)
        .predators_fraction(0.05)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins(SimulationPlugin::new(GridSize::new(20, 20), dist));

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
        SimulationPlugin::new(GridSize::new(16, 16), dist),
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
    app.add_plugins(SimulationPlugin::new(GridSize::new(10, 10), dist));
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
fn brain_inspector_reads_snapshot_and_clears_on_character_death() {
    let dist = DistributionConfig::builder()
        .plants_fraction(0.1)
        .herbivores_fraction(0.2)
        .predators_fraction(0.0)
        .build()
        .unwrap();

    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::new(GridSize::new(10, 10), dist),
        BrainInspectorUiPlugin,
    ));
    app.update();

    let herbivore = app
        .world_mut()
        .query_filtered::<Entity, With<Herbivore>>()
        .iter(app.world())
        .next()
        .expect("seeded herbivore");

    app.world_mut().resource_mut::<InspectedCharacter>().0 = Some(herbivore);

    let has_snapshot = app
        .world_mut()
        .run_system_once(move |inspector: CharacterInspector| {
            let snap = inspector.snapshot()?;
            assert_eq!(snap.entity, herbivore);
            assert_eq!(snap.energy.current(), 50);
            assert_eq!(snap.generation.number(), 1);
            Some(())
        })
        .unwrap();
    assert!(has_snapshot.is_some());

    // Despawning the inspected character must automatically clear `InspectedCharacter`.
    app.world_mut().despawn(herbivore);
    app.world_mut().flush();

    assert_eq!(app.world().resource::<InspectedCharacter>().0, None);
}
