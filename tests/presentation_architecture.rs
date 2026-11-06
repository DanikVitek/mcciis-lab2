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
    CharacterPovOverlay,
    GridBackground,
    PreviousPose,
    SelectionHighlight,
    TILE_SIZE,
    grid_to_world_xy,
};
use life_sim::presentation::ui::HistoryPlotOpen;
use life_sim::presentation::ui::config::{
    AutoSaveSettingsPath,
    SimConfig,
};
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
    ActionCosts,
    CharacterFilter,
    Energy,
    FoodValues,
    Herbivore,
    InitialEnergy,
    MaxEnergy,
    Plant,
    Predator,
    ValueAsFood,
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
    SingleNeuronMutation,
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
    assert!(!app.world().contains_resource::<ActionCosts>());
    assert!(!app.world().contains_resource::<FoodValues>());
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
    let custom_costs = ActionCosts {
        turn: Energy(0.25),
        go_forward: Energy(1.5),
        consume: Energy(0.5),
    };
    let custom_food = FoodValues {
        plant: Energy(2.5),
        herbivore: Energy(6.0),
    };
    {
        let mut config = app.world_mut().resource_mut::<SimConfig>();
        config.grid_size = custom_size;
        config.plants_fraction = 0.25;
        config.herbivores_fraction = 0.10;
        config.predators_fraction = 0.05;
        config.initial_energy = 12.0.into();
        config.max_energy = 24.0.into();
        config.reproduction_threshold = ReproductionThreshold::Absolute(Energy(18.0));
        config.action_costs = custom_costs;
        config.food_values = custom_food;
    }

    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Simulation);

    // This update runs `StateTransition` (`OnEnter(AppState::Simulation)` -> `setup_simulation_from_config`)
    // followed by `Update` and `PostUpdate` (`sync_grid_background`, `interpolate_character_transform`, etc.).
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
    assert_eq!(*app.world().resource::<InitialEnergy>(), 12.0.into());
    assert_eq!(*app.world().resource::<MaxEnergy>(), 24.0.into());
    assert_eq!(
        *app.world().resource::<ReproductionThreshold>(),
        ReproductionThreshold::Absolute(Energy(18.0))
    );
    assert_eq!(*app.world().resource::<ActionCosts>(), custom_costs);
    assert_eq!(*app.world().resource::<FoodValues>(), custom_food);
    assert_eq!(app.world().resource::<SimulationTickCount>().0, 0);

    let totals = app.world().resource::<SpatialGrid>().total_counts();
    assert_eq!(totals.plants, 50);
    assert_eq!(totals.herbivores, 20);
    assert_eq!(totals.predators, 10);
    assert_eq!(
        app.world().resource::<SimulationHistory>().tick_history(),
        &[0]
    );

    // Verify `SpritesPlugin` unhid and resized `GridBackground`,
    // and attached `Sprite` + `PreviousPose` to seeded entities at root level.
    {
        let world = app.world_mut();
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
            .query_filtered::<&ValueAsFood, (With<Plant>, With<Sprite>)>()
            .iter(world)
            .inspect(|food| assert_eq!(food.0, custom_food.plant))
            .count();
        assert_eq!(plant_sprites, 50);

        let herbivore_food_count = world
            .query_filtered::<&ValueAsFood, With<Herbivore>>()
            .iter(world)
            .inspect(|food| assert_eq!(food.0, custom_food.herbivore))
            .count();
        assert_eq!(herbivore_food_count, 20);

        let character_sprites = world
            .query_filtered::<Entity, (
                Or<(With<Herbivore>, With<Predator>)>,
                With<Sprite>,
                With<PreviousPose>,
            )>()
            .iter(world)
            .count();
        assert_eq!(character_sprites, 30);
    }

    // Place a character at the corner of the grid `(0, 0)` and follow it:
    // Verify `Camera2d` centers directly on the edge character, `SelectionHighlight` becomes visible,
    // and `CharacterPovOverlay` is attached as a child entity with `Visibility::Inherited`.
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
            grid_to_world_xy(corner, custom_size).extend(1.0);
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
        let expected_world_xy = grid_to_world_xy(edge_pos, custom_size);
        assert_eq!(camera_transform.translation.truncate(), expected_world_xy);

        let (highlight_transform, highlight_vis) = world
            .query_filtered::<(&Transform, &Visibility), With<SelectionHighlight>>()
            .single(world)
            .expect("SelectionHighlight");
        assert_eq!(*highlight_vis, Visibility::Inherited);
        assert_eq!(
            highlight_transform.translation.truncate(),
            expected_world_xy
        );

        let (pov_child_of, pov_vis, pov_children) = world
            .query_filtered::<(&ChildOf, &Visibility, &Children), With<CharacterPovOverlay>>()
            .single(world)
            .expect("CharacterPovOverlay attached to inspected character");
        assert_eq!(pov_child_of.parent(), edge_entity);
        assert_eq!(*pov_vis, Visibility::Inherited);
        assert!(!pov_children.is_empty());
    }

    // Pressing `Escape` unfollows the character and hides both `SelectionHighlight` and `CharacterPovOverlay`.
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

        let pov_vis = world
            .query_filtered::<&Visibility, With<CharacterPovOverlay>>()
            .single(world)
            .expect("CharacterPovOverlay visibility");
        assert_eq!(*pov_vis, Visibility::Hidden);
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
        config.initial_energy = 50.0.into();
        config.max_energy = 100.0.into();
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
            assert_eq!(snap.energy, Energy(50.0));
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

#[test]
fn sim_config_saves_loads_and_auto_saves_on_change() {
    let temp_path = std::env::temp_dir().join(format!(
        "life_sim_test_settings_{}.toml",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&temp_path);

    let mut app = build_headless_presentation_app();
    app.insert_resource(AutoSaveSettingsPath(temp_path.clone()));
    app.update();

    let expected = {
        let mut config = app.world_mut().resource_mut::<SimConfig>();
        config.grid_size = GridSize::new(64, 48);
        config.plants_fraction = 0.3;
        config.herbivores_fraction = 0.15;
        config.predators_fraction = 0.05;
        config.initial_energy = Energy(7.5).into();
        config.max_energy = Energy(15.0).into();
        config.reproduction_threshold = ReproductionThreshold::Absolute(Energy(11.25));
        config.action_costs = ActionCosts {
            turn: Energy(0.2),
            go_forward: Energy(0.8),
            consume: Energy(1.2),
        };
        config.food_values = FoodValues {
            plant: Energy(1.75),
            herbivore: Energy(4.5),
        };
        config.speed_mode = SimulationSpeedMode::Uncapped;
        *config
    };

    // Running `app.update()` runs `PostUpdate` (`auto_save_settings_system`), persisting `SimConfig`.
    app.update();

    let raw_toml = std::fs::read_to_string(&temp_path).expect("read auto-saved TOML");
    assert!(raw_toml.contains("speed_mode = \"uncapped\""), "{raw_toml}");
    assert!(raw_toml.contains("absolute = 11.25"), "{raw_toml}");
    assert!(raw_toml.contains("[mutation.distributed]"), "{raw_toml}");
    assert!(
        !raw_toml.chars().any(char::is_uppercase),
        "expected all serialized keys and variants to be snake_case:\n{raw_toml}"
    );

    let loaded = SimConfig::load_from_path(&temp_path).expect("loaded auto-saved settings");
    assert_eq!(loaded, expected);

    // Also verify `SingleNeuron`, `Relative`, and `FixedStep` variants serialize in snake_case and round-trip.
    let mut alt = expected;
    alt.speed_mode = SimulationSpeedMode::FixedStep;
    alt.reproduction_threshold = ReproductionThreshold::Relative(0.75);
    alt.mutation = MutationStrategy::SingleNeuron(SingleNeuronMutation);
    alt.save_to_path(&temp_path)
        .expect("saved single_neuron config");

    let alt_toml = std::fs::read_to_string(&temp_path).expect("read single_neuron TOML");
    assert!(
        alt_toml.contains("speed_mode = \"fixed_step\""),
        "{alt_toml}"
    );
    assert!(alt_toml.contains("relative = 0.75"), "{alt_toml}");
    assert!(
        alt_toml.contains("mutation = \"single_neuron\""),
        "{alt_toml}"
    );
    assert!(
        !alt_toml.chars().any(char::is_uppercase),
        "expected all serialized keys and variants to be snake_case:\n{alt_toml}"
    );

    let loaded_alt = SimConfig::load_from_path(&temp_path).expect("loaded single_neuron config");
    assert_eq!(loaded_alt, alt);

    let _ = std::fs::remove_file(&temp_path);
}
