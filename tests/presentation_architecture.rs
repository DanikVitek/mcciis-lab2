use std::time::Duration;

use bevy::ecs::system::RunSystemOnce;
use bevy::input::keyboard::{
    Key,
    KeyboardInput,
};
use bevy::input::{
    ButtonState,
    InputPlugin,
};
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy::window::{
    ExitCondition,
    WindowPlugin,
};
use life_sim::grid::{
    GridPosition,
    GridSize,
};
use life_sim::presentation::sprites::{
    GridBackground,
    GridRootEntity,
    GridVisualRoot,
    PreviousPose,
    SelectionHighlight,
    TILE_SIZE,
    grid_origin_offset,
    grid_to_local_xy,
};
use life_sim::presentation::ui::HistoryPlotOpen;
use life_sim::presentation::ui::config::SimConfig;
use life_sim::presentation::ui::inspector::{
    BrainInspectorUiPlugin,
    CharacterInspector,
    InspectedCharacter,
    follow_random_character,
};
use life_sim::presentation::{
    AppState,
    PresentationPlugin,
    SimulationPlaybackState,
    SimulationSpeedMode,
};
use life_sim::simulation::organism::{
    CharacterFilter,
    Energy,
    Herbivore,
    InitialEnergy,
    MaxEnergy,
    Plant,
    Predator,
};
use life_sim::simulation::spatial::SpatialGrid;
use life_sim::simulation::{
    DeferredReproductionPlugin,
    DeferredSimulationPlugin,
    DistributionConfig,
    HistoryRecorderPlugin,
    MutationStrategy,
    ReproductionPlugin,
    ReproductionThreshold,
    SimulationHistory,
    SimulationPlugin,
    SimulationTickCount,
};

/// Builds a headless Bevy [`App`] with the full [`PresentationPlugin`] (`SimulationPacingPlugin`,
/// `SpritesPlugin`, `CameraPlugin`, and `UiPlugin`), [`DeferredSimulationPlugin`], and
/// [`DeferredReproductionPlugin`] without opening an OS window or requiring a GPU render device.
fn build_headless_presentation_app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        AssetPlugin::default(),
        InputPlugin,
        WindowPlugin {
            primary_window: None,
            exit_condition: ExitCondition::DontExit,
            ..default()
        },
    ))
    .init_asset::<Image>()
    .init_asset::<Shader>()
    .init_asset::<TextureAtlasLayout>()
    .add_plugins((
        DeferredSimulationPlugin,
        DeferredReproductionPlugin,
        HistoryRecorderPlugin::default(),
        PresentationPlugin::new(Duration::from_millis(250)),
    ));

    app
}

fn send_key_press(app: &mut App, key_code: KeyCode) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

#[test]
fn presentation_setup_state_withholds_resources_and_hides_grid_until_accepted() {
    let mut app = build_headless_presentation_app();

    // Run multiple full `Main` schedule updates (`First`, `PreUpdate`, `StateTransition`,
    // `RunFixedMainLoop`, `Update`, `EguiPrimaryContextPass`, `PostUpdate`, `Last`) while in
    // `AppState::Setup` to verify that no presentation system or run condition panics when
    // simulation resources (`GridSize`, `SpatialGrid`, etc.) have not been inserted yet.
    app.update();
    app.update();

    assert_eq!(*app.world().resource::<State<AppState>>(), AppState::Setup);
    assert!(
        !app.world()
            .contains_resource::<State<SimulationPlaybackState>>()
    );
    assert!(!app.world().contains_resource::<GridSize>());
    assert!(!app.world().contains_resource::<DistributionConfig>());
    assert!(!app.world().contains_resource::<SpatialGrid>());
    assert!(!app.world().contains_resource::<MutationStrategy>());
    assert!(!app.world().contains_resource::<InitialEnergy>());
    assert!(!app.world().contains_resource::<MaxEnergy>());
    assert!(!app.world().contains_resource::<ReproductionThreshold>());
    assert!(!app.world().contains_resource::<SimulationTickCount>());
    assert!(app.world().resource::<SimulationHistory>().is_empty());

    // `SpritesPlugin` spawns `GridBackground` and `SelectionHighlight` hidden during `AppState::Setup`.
    {
        let world = app.world_mut();
        let bg_vis = world
            .query_filtered::<&Visibility, With<GridBackground>>()
            .single(world)
            .expect("spawned GridBackground");
        assert_eq!(*bg_vis, Visibility::Hidden);

        let highlight_vis = world
            .query_filtered::<&Visibility, With<SelectionHighlight>>()
            .single(world)
            .expect("spawned SelectionHighlight");
        assert_eq!(*highlight_vis, Visibility::Hidden);
    }

    // Edit staged `SimConfig` and accept it by transitioning `AppState` to `AppState::Simulation`.
    let custom_size = GridSize::new(20, 10);
    {
        let mut config = app.world_mut().resource_mut::<SimConfig>();
        config.grid_size = custom_size;
        config.plants_fraction = 0.25;
        config.herbivores_fraction = 0.10;
        config.predators_fraction = 0.05;
        config.initial_energy = 12.into();
        config.max_energy = 24.into();
        config.reproduction_threshold = ReproductionThreshold::Absolute(Energy(18));
    }

    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Simulation);

    // This update runs `StateTransition` (`OnEnter(AppState::Simulation)` -> `setup_simulation_from_config`)
    // followed by `Update` and `PostUpdate` (`sync_grid_root_centering`, `interpolate_character_transform`, etc.).
    app.update();

    assert_eq!(
        *app.world().resource::<State<AppState>>(),
        AppState::Simulation
    );
    assert_eq!(
        *app.world().resource::<State<SimulationPlaybackState>>(),
        SimulationPlaybackState::Running
    );
    assert_eq!(*app.world().resource::<GridSize>(), custom_size);
    assert_eq!(*app.world().resource::<InitialEnergy>(), 12.into());
    assert_eq!(*app.world().resource::<MaxEnergy>(), 24.into());
    assert_eq!(
        *app.world().resource::<ReproductionThreshold>(),
        ReproductionThreshold::Absolute(Energy(18))
    );
    assert_eq!(app.world().resource::<SimulationTickCount>().0, 0);

    let totals = app.world().resource::<SpatialGrid>().total_counts();
    assert_eq!(totals.plants, 50);
    assert_eq!(totals.herbivores, 20);
    assert_eq!(totals.predators, 10);
    assert_eq!(
        app.world().resource::<SimulationHistory>().tick_history(),
        &[0]
    );

    // Verify `SpritesPlugin` centered `GridVisualRoot`, unhid and resized `GridBackground`,
    // and attached `Sprite` + `ChildOf(grid_root)` + `PreviousPose` to seeded entities.
    let grid_root = app.world().resource::<GridRootEntity>().0;
    {
        let world = app.world_mut();
        let root_transform = world
            .query_filtered::<&Transform, With<GridVisualRoot>>()
            .single(world)
            .expect("GridVisualRoot transform");
        assert_eq!(
            root_transform.translation,
            grid_origin_offset(custom_size).extend(0.0)
        );

        let (bg_vis, bg_sprite) = world
            .query_filtered::<(&Visibility, &Sprite), With<GridBackground>>()
            .single(world)
            .expect("GridBackground components");
        assert_eq!(*bg_vis, Visibility::Inherited);
        assert_eq!(
            bg_sprite.custom_size,
            Some(custom_size.as_vec2() * TILE_SIZE)
        );

        let plant_sprites = world
            .query_filtered::<&ChildOf, (With<Plant>, With<Sprite>)>()
            .iter(world)
            .filter(|c| c.0 == grid_root)
            .count();
        assert_eq!(plant_sprites, 50);

        let character_sprites = world
            .query_filtered::<&ChildOf, (
                Or<(With<Herbivore>, With<Predator>)>,
                With<Sprite>,
                With<PreviousPose>,
            )>()
            .iter(world)
            .filter(|c| c.0 == grid_root)
            .count();
        assert_eq!(character_sprites, 30);
    }

    // Place a character at the corner of the grid `(0, 0)` and follow it:
    // Verify `Camera2d` centers directly on the edge character and `SelectionHighlight` becomes visible.
    let (edge_entity, edge_pos) = {
        let world = app.world_mut();
        let entity = world
            .query_filtered::<Entity, With<Herbivore>>()
            .iter(world)
            .next()
            .expect("seeded herbivore");
        let corner = GridPosition::new(0, 0);
        *world.get_mut::<GridPosition>(entity).unwrap() = corner;
        world.get_mut::<Transform>(entity).unwrap().translation =
            grid_to_local_xy(corner).extend(1.0);
        (entity, corner)
    };

    app.world_mut().resource_mut::<InspectedCharacter>().0 = Some(edge_entity);
    app.update();

    {
        let world = app.world_mut();
        let camera_transform = world
            .query_filtered::<&Transform, With<Camera2d>>()
            .single(world)
            .expect("Camera2d transform");
        let expected_world_xy = grid_origin_offset(custom_size) + grid_to_local_xy(edge_pos);
        assert_eq!(camera_transform.translation.truncate(), expected_world_xy);

        let (highlight_transform, highlight_vis) = world
            .query_filtered::<(&Transform, &Visibility), With<SelectionHighlight>>()
            .single(world)
            .expect("SelectionHighlight");
        assert_eq!(*highlight_vis, Visibility::Inherited);
        assert_eq!(
            highlight_transform.translation.truncate(),
            grid_to_local_xy(edge_pos)
        );
    }

    // Pressing `Escape` unfollows the character and hides `SelectionHighlight`.
    send_key_press(&mut app, KeyCode::Escape);
    app.update();

    assert_eq!(app.world().resource::<InspectedCharacter>().0, None);
    {
        let world = app.world_mut();
        let highlight_vis = world
            .query_filtered::<&Visibility, With<SelectionHighlight>>()
            .single(world)
            .expect("SelectionHighlight visibility");
        assert_eq!(*highlight_vis, Visibility::Hidden);
    }
}

#[test]
fn presentation_hotkeys_and_playback_pacing_control_simulation_ticks() {
    let mut app = build_headless_presentation_app();
    app.update();

    // Accept initial setup with uncapped speed mode so each `Update` advances 1 tick while `Running`.
    {
        let mut config = app.world_mut().resource_mut::<SimConfig>();
        config.grid_size = GridSize::new(16, 16);
        config.plants_fraction = 0.2;
        config.herbivores_fraction = 0.2;
        config.predators_fraction = 0.0;
        config.initial_energy = 50.into();
        config.max_energy = 100.into();
        config.speed_mode = SimulationSpeedMode::Uncapped;
    }
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Simulation);

    // Frame 1: enters `AppState::Simulation` and `SimulationPlaybackState::Running`, and executes tick 1 in `Update`.
    app.update();
    assert_eq!(app.world().resource::<SimulationTickCount>().0, 1);

    // Frame 2: advances to tick 2 in `Uncapped` mode.
    app.update();
    assert_eq!(app.world().resource::<SimulationTickCount>().0, 2);

    // Pressing `Space` queues transition to `SimulationPlaybackState::Paused`.
    send_key_press(&mut app, KeyCode::Space);
    app.update();
    // Next frame's `StateTransition` applies `SimulationPlaybackState::Paused` before `Update` runs.
    app.update();

    assert_eq!(
        *app.world().resource::<State<SimulationPlaybackState>>(),
        SimulationPlaybackState::Paused
    );
    let paused_tick = app.world().resource::<SimulationTickCount>().0;

    // While paused, additional `app.update()` frames do not advance `SimulationTickCount`.
    app.update();
    assert_eq!(app.world().resource::<SimulationTickCount>().0, paused_tick);

    // Pressing `S` while paused triggers `StepSimulationOnce`, advancing `SimulationTickCount` by 1.
    send_key_press(&mut app, KeyCode::KeyS);
    app.update();
    assert_eq!(
        app.world().resource::<SimulationTickCount>().0,
        paused_tick + 1
    );

    // Pressing `Tab` toggles `HistoryPlotOpen`, and `Escape` closes it.
    send_key_press(&mut app, KeyCode::Tab);
    app.update();
    assert!(app.world().resource::<HistoryPlotOpen>().0);

    send_key_press(&mut app, KeyCode::Escape);
    app.update();
    assert!(!app.world().resource::<HistoryPlotOpen>().0);
}

#[test]
fn presentation_auto_pauses_when_all_organisms_die() {
    let mut app = build_headless_presentation_app();
    app.update();

    // Start with zero herbivores and zero predators so the world has no living characters.
    {
        let mut config = app.world_mut().resource_mut::<SimConfig>();
        config.grid_size = GridSize::new(10, 10);
        config.plants_fraction = 0.2;
        config.herbivores_fraction = 0.0;
        config.predators_fraction = 0.0;
    }
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Simulation);

    // Frame 1 enters `AppState::Simulation` and `pause_when_no_organisms` queues `SimulationPlaybackState::Paused`.
    app.update();
    // Frame 2 applies the sub-state transition to `SimulationPlaybackState::Paused`.
    app.update();

    assert_eq!(
        *app.world().resource::<State<SimulationPlaybackState>>(),
        SimulationPlaybackState::Paused
    );
    assert_eq!(app.world().resource::<SimulationTickCount>().0, 0);
}

#[test]
fn brain_inspector_reads_snapshot_and_clears_on_character_death() {
    let dist = DistributionConfig::builder()
        .plants_fraction(0.1)
        .herbivores_fraction(0.02)
        .predators_fraction(0.0)
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
        BrainInspectorUiPlugin,
    ));
    app.update();

    // Follow a random character when none is currently selected (2 herbivores seeded).
    app.world_mut()
        .run_system_once(
            |characters: Query<Entity, CharacterFilter>,
             mut inspected: ResMut<InspectedCharacter>| {
                follow_random_character(&characters, &mut inspected);
            },
        )
        .unwrap();

    let first_selected = app
        .world()
        .resource::<InspectedCharacter>()
        .0
        .expect("randomly selected character");

    // Calling `follow_random_character` again with 2 living characters must switch to the other character.
    app.world_mut()
        .run_system_once(
            |characters: Query<Entity, CharacterFilter>,
             mut inspected: ResMut<InspectedCharacter>| {
                follow_random_character(&characters, &mut inspected);
            },
        )
        .unwrap();

    let second_selected = app
        .world()
        .resource::<InspectedCharacter>()
        .0
        .expect("switched random character");
    assert_ne!(first_selected, second_selected);

    let has_snapshot = app
        .world_mut()
        .run_system_once(move |inspector: CharacterInspector| {
            let snap = inspector.snapshot()?;

            assert_eq!(snap.entity, second_selected);
            assert_eq!(snap.energy, Energy(50));
            assert_eq!(snap.generation.number(), 1);
            assert_eq!(inspector.pick_random_character(), Some(first_selected));
            Some(())
        })
        .unwrap();
    assert!(has_snapshot.is_some());

    // Despawning the inspected character must automatically clear `InspectedCharacter`.
    app.world_mut().despawn(second_selected);
    app.world_mut().flush();

    assert_eq!(app.world().resource::<InspectedCharacter>().0, None);
}
