use bevy::curve::easing;
use bevy::ecs::query::QueryFilter;
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;

use super::pacing::{
    AppState,
    SimulationSpeedMode,
};
use super::ui::inspector::InspectedCharacter;
use crate::grid::{
    Facing,
    GridPosition,
    GridSize,
};
use crate::simulation::organism::{
    CharacterFilter,
    Herbivore,
    Plant,
    Predator,
};
use crate::simulation::{
    SimulationSystems,
    SimulationTick,
    WorldSeeded,
};

pub const TILE_SIZE: Vec2 = Vec2::splat(32.0);

/// Darker deep-water blue used for both grid cell borders and the viewport area outside the grid.
pub const WATER_DARK_COLOR: Color = Color::oklch(0.32, 0.075, 248.0);

/// Lighter aquatic blue used for the interior of each grid cell.
pub const WATER_CELL_COLOR: Color = Color::oklch(0.44, 0.088, 242.0);

const GRID_BACKGROUND_Z: f32 = -1.0;
const INSPECTED_CHARACTER_Z: f32 = 3.0;
const SELECTION_HIGHLIGHT_Z: f32 = 3.5;
const SELECTION_HIGHLIGHT_SCALE: f32 = 1.6;

/// System set in `PostUpdate` that updates grid and organism `Transform`s before camera follow and transform propagation.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpriteTransformSystems;

/// World-space translation of tile `(0, 0)` so the entire [`GridSize`] field is centered at `(0, 0)`.
pub fn grid_origin_offset(grid_size: GridSize) -> Vec2 {
    -TILE_SIZE * (grid_size.as_vec2() - Vec2::ONE) / 2.0
}

/// Local translation of a [`GridPosition`] relative to tile `(0, 0)`.
pub fn grid_to_local_xy(pos: GridPosition) -> Vec2 {
    Vec2 {
        x: f32::from(pos.x) * TILE_SIZE.x,
        y: f32::from(pos.y) * TILE_SIZE.y,
    }
}

/// World-space translation of a [`GridPosition`] for a centered [`GridSize`] field.
pub fn grid_to_world_xy(pos: GridPosition, grid_size: GridSize) -> Vec2 {
    grid_origin_offset(grid_size) + grid_to_local_xy(pos)
}

/// Converts a 2D world-space coordinate (e.g. from a cursor click) into a [`GridPosition`],
/// returning `None` if the point lies outside the grid bounds.
pub fn world_to_grid_pos(world_xy: Vec2, grid_size: GridSize) -> Option<GridPosition> {
    let local_xy = world_xy - grid_origin_offset(grid_size);
    let tile = ((local_xy + TILE_SIZE / 2.0) / TILE_SIZE).floor();
    if tile.x < 0.0
        || tile.y < 0.0
        || tile.x >= f32::from(grid_size.width())
        || tile.y >= f32::from(grid_size.height())
    {
        return None;
    }
    Some(GridPosition::new(tile.x as u16, tile.y as u16))
}

/// Marker component for the single background quad rendering the water cells and darker cell borders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Component)]
pub struct GridBackground;

/// Marker component for the highlight ring rendered around the currently inspected character.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Component)]
pub struct SelectionHighlight;

#[derive(Debug, Clone, Resource)]
pub struct OrganismSprites {
    pub plant: Handle<Image>,
    pub character: Handle<Image>,
}

#[derive(Debug, Clone, Resource)]
pub struct OrganismSpriteRemaps {
    pub herbivore: Handle<PaletteSwapMaterial>,
    pub predator: Handle<PaletteSwapMaterial>,
}

#[derive(Debug, Clone, Asset, AsBindGroup, Reflect)]
pub struct PaletteSwapMaterial {
    #[uniform(100)]
    pub colors: [LinearRgba; 3],
}

impl MaterialExtension2d for PaletteSwapMaterial {
    fn fragment_shader() -> Option<bevy::shader::ShaderRef> {
        Some("shaders/palette_swap.wesl".into())
    }
}

#[derive(Debug, Clone, Asset, AsBindGroup, Reflect)]
pub struct GridBackgroundMaterial {
    #[uniform(100)]
    pub cell_color: LinearRgba,
    #[uniform(101)]
    pub border_color: LinearRgba,
    #[uniform(102)]
    pub grid_size: Vec2,
}

impl MaterialExtension2d for GridBackgroundMaterial {
    fn fragment_shader() -> Option<bevy::shader::ShaderRef> {
        Some("shaders/grid_background.wesl".into())
    }
}

#[derive(Debug, Clone, Asset, AsBindGroup, Reflect)]
pub struct SelectionHighlightMaterial {
    #[uniform(100)]
    pub ring_color: LinearRgba,
    #[uniform(101)]
    pub outline_color: LinearRgba,
}

impl MaterialExtension2d for SelectionHighlightMaterial {
    fn fragment_shader() -> Option<bevy::shader::ShaderRef> {
        Some("shaders/selection_highlight.wesl".into())
    }
}

trait HasSprite: Component {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image>;
}

trait HasRemappedSprite: HasSprite {
    fn shader_handle(shaders: &OrganismSpriteRemaps) -> Handle<PaletteSwapMaterial>;
}

trait HasDedicatedZ {
    const Z: f32;
}

impl HasSprite for Plant {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.plant.clone()
    }
}

impl HasDedicatedZ for Plant {
    const Z: f32 = 0.0;
}

impl HasSprite for Herbivore {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.character.clone()
    }
}

impl HasRemappedSprite for Herbivore {
    fn shader_handle(shaders: &OrganismSpriteRemaps) -> Handle<PaletteSwapMaterial> {
        shaders.herbivore.clone()
    }
}

impl HasDedicatedZ for Herbivore {
    const Z: f32 = 1.0;
}

impl HasSprite for Predator {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.character.clone()
    }
}

impl HasRemappedSprite for Predator {
    fn shader_handle(shaders: &OrganismSpriteRemaps) -> Handle<PaletteSwapMaterial> {
        shaders.predator.clone()
    }
}

impl HasDedicatedZ for Predator {
    const Z: f32 = 2.0;
}

const _: () = {
    assert!(
        GRID_BACKGROUND_Z < Plant::Z
            && Plant::Z < Herbivore::Z
            && Herbivore::Z < Predator::Z
            && Predator::Z < INSPECTED_CHARACTER_Z
            && INSPECTED_CHARACTER_Z < SELECTION_HIGHLIGHT_Z
    );
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
pub struct PreviousPose {
    pos: GridPosition,
    facing: Facing,
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Resource)]
pub struct StepTransition {
    progress: f32,
}

pub struct SpritesPlugin;

impl Plugin for SpritesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            SpriteMaterialPlugin::<PaletteSwapMaterial>::default(),
            SpriteMaterialPlugin::<GridBackgroundMaterial>::default(),
            SpriteMaterialPlugin::<SelectionHighlightMaterial>::default(),
        ))
        .add_systems(PreStartup, setup_visual_root_and_sprites)
        .add_observer(attach_visuals_to_plant)
        .add_observer(attach_visuals_to_character::<Herbivore>)
        .add_observer(attach_visuals_to_character::<Predator>)
        .add_observer(on_world_seeded)
        .add_systems(
            SimulationTick,
            (
                remember_previous_pose
                    .before(SimulationSystems::Step)
                    .run_if(resource_equals(SimulationSpeedMode::FixedStep)),
                reset_step_transition.run_if(resource_equals(SimulationSpeedMode::FixedStep)),
            ),
        )
        .add_systems(
            Update,
            (
                update_step_transition.run_if(resource_equals(SimulationSpeedMode::FixedStep)),
                set_step_transitioned.run_if(resource_equals(SimulationSpeedMode::Uncapped)),
            )
                .run_if(
                    in_state(AppState::Simulation)
                        .and_then(|it: Res<StepTransition>| it.progress < 1.0),
                ),
        )
        .configure_sets(
            PostUpdate,
            SpriteTransformSystems
                .run_if(in_state(AppState::Simulation))
                .before(TransformSystems::Propagate),
        )
        .add_systems(
            PostUpdate,
            (
                sync_grid_background.run_if(resource_exists_and_changed::<GridSize>),
                sync_translation_with_grid::<With<Plant>>
                    .run_if(resource_equals(SimulationSpeedMode::FixedStep)),
                interpolate_character_transform.run_if(resource_changed::<StepTransition>),
                (sync_translation_with_grid::<()>, sync_rotation_with_grid)
                    .chain_weak()
                    .run_if(resource_equals(SimulationSpeedMode::Uncapped)),
                sync_selection_highlight,
            )
                .chain_weak()
                .in_set(SpriteTransformSystems),
        )
        .init_resource::<StepTransition>();
    }
}

fn setup_visual_root_and_sprites(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut shaders: ResMut<Assets<PaletteSwapMaterial>>,
    mut bg_materials: ResMut<Assets<GridBackgroundMaterial>>,
    mut highlight_materials: ResMut<Assets<SelectionHighlightMaterial>>,
) {
    let placeholder_size = GridSize::default();

    let bg_material = bg_materials.add(GridBackgroundMaterial {
        cell_color: LinearRgba::from(WATER_CELL_COLOR),
        border_color: LinearRgba::from(WATER_DARK_COLOR),
        grid_size: placeholder_size.as_vec2(),
    });

    commands.spawn((
        GridBackground,
        Sprite {
            custom_size: Some(placeholder_size.as_vec2() * TILE_SIZE),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, GRID_BACKGROUND_Z),
        Visibility::Hidden,
        SpriteMaterial(bg_material),
    ));

    let highlight_material = highlight_materials.add(SelectionHighlightMaterial {
        ring_color: LinearRgba::new(0.25, 0.95, 1.0, 1.0),
        outline_color: LinearRgba::new(0.02, 0.08, 0.14, 1.0),
    });

    commands.spawn((
        SelectionHighlight,
        Sprite {
            custom_size: Some(TILE_SIZE * SELECTION_HIGHLIGHT_SCALE),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, SELECTION_HIGHLIGHT_Z),
        Visibility::Hidden,
        SpriteMaterial(highlight_material),
    ));

    commands.insert_resource(OrganismSprites {
        plant: asset_server.load("sprites/algae.png"),
        character: asset_server.load("sprites/bacterium.png"),
    });

    const HERBIVORE_COLORS: [LinearRgba; 3] = [
        LinearRgba::new(0.85, 0.80, 0.2, 1.0),
        LinearRgba::new(0.4, 0.4, 0.0, 1.0),
        LinearRgba::new(0.0, 0.0, 0.0, 1.0),
    ];
    let herbivore_sh = shaders.add(PaletteSwapMaterial {
        colors: HERBIVORE_COLORS,
    });

    const PREDATOR_COLORS: [LinearRgba; 3] = [
        LinearRgba::new(0.85, 0.15, 0.15, 1.0),
        LinearRgba::new(0.45, 0.1, 0.1, 1.0),
        LinearRgba::new(0.0, 0.0, 0.0, 1.0),
    ];
    let predator_sh = shaders.add(PaletteSwapMaterial {
        colors: PREDATOR_COLORS,
    });

    commands.insert_resource(OrganismSpriteRemaps {
        herbivore: herbivore_sh,
        predator: predator_sh,
    });
}

fn attach_visuals_to_plant(
    add: On<Add<Plant>>,
    query: Query<&GridPosition>,
    mut commands: Commands,
    sprites: Res<OrganismSprites>,
    field_size: Res<GridSize>,
) {
    let &pos = query.get_inner(add.entity).unwrap();
    commands.entity(add.entity).insert((
        Sprite {
            image: Plant::sprite_handle(&sprites),
            custom_size: Some(TILE_SIZE),
            ..default()
        },
        Transform {
            translation: grid_to_world_xy(pos, *field_size).extend(Plant::Z),
            ..default()
        },
    ));
}

fn attach_visuals_to_character<T: HasRemappedSprite + HasDedicatedZ>(
    add: On<Add<T>>,
    mut commands: Commands,
    query: Query<(&GridPosition, &Facing)>,
    sprites: Res<OrganismSprites>,
    shaders: Res<OrganismSpriteRemaps>,
    field_size: Res<GridSize>,
) {
    let (&pos, &facing) = query.get_inner(add.entity).unwrap();
    commands.entity(add.entity).insert((
        Sprite {
            image: T::sprite_handle(&sprites),
            custom_size: Some(TILE_SIZE),
            ..default()
        },
        Transform {
            translation: grid_to_world_xy(pos, *field_size).extend(T::Z),
            rotation: facing.to_quat(),
            ..default()
        },
        SpriteMaterial(T::shader_handle(&shaders)),
        PreviousPose { pos, facing },
    ));
}

fn remember_previous_pose(mut query: Query<(&mut PreviousPose, &GridPosition, &Facing)>) {
    query
        .par_iter_mut()
        .for_each(|(mut prev_pose, &prev_pos, &prev_facing)| {
            if prev_pose.pos != prev_pos {
                prev_pose.pos = prev_pos;
            }
            if prev_pose.facing != prev_facing {
                prev_pose.facing = prev_facing;
            }
        });
}

fn reset_step_transition(mut step_transition: ResMut<StepTransition>) {
    step_transition.progress = 0.0;
}

fn on_world_seeded(_: On<WorldSeeded>, step_transition: ResMut<StepTransition>) {
    set_step_transitioned(step_transition)
}

fn set_step_transitioned(mut step_transition: ResMut<StepTransition>) {
    step_transition.progress = 1.0;
}

fn update_step_transition(
    time: Res<Time>,
    fixed_time: Res<Time<Fixed>>,
    mut step_transition: ResMut<StepTransition>,
) {
    let step_duration = fixed_time.timestep().as_secs_f32();
    if step_duration > 0.0 {
        step_transition.progress = f32::min(
            step_transition.progress + time.delta_secs() / step_duration,
            1.0,
        );
    } else {
        set_step_transitioned(step_transition)
    }
}

#[allow(clippy::type_complexity)]
fn sync_grid_background(
    field_size: Res<GridSize>,
    bg_query: Single<
        (
            &mut Sprite,
            &mut Visibility,
            &SpriteMaterial<GridBackgroundMaterial>,
        ),
        With<GridBackground>,
    >,
    mut bg_materials: ResMut<Assets<GridBackgroundMaterial>>,
) {
    let (mut bg_sprite, mut bg_vis, bg_mat_handle) = bg_query.into_inner();
    *bg_vis = Visibility::Inherited;
    bg_sprite.custom_size = Some(field_size.as_vec2() * TILE_SIZE);
    if let Some(mut mat) = bg_materials.get_mut(&bg_mat_handle.0) {
        mat.grid_size = field_size.as_vec2();
    }
}

fn sync_translation_with_grid<F: QueryFilter>(
    mut query: Query<(&GridPosition, &mut Transform), (Changed<GridPosition>, F)>,
    field_size: Res<GridSize>,
) {
    let origin = grid_origin_offset(*field_size);
    query
        .par_iter_mut()
        .for_each(|(grid_position, mut transform)| {
            let z = transform.translation.z;
            transform.translation = (origin + grid_to_local_xy(*grid_position)).extend(z);
        });
}

fn sync_rotation_with_grid(mut query: Query<(&Facing, &mut Transform), Changed<Facing>>) {
    query
        .par_iter_mut()
        .for_each(|(facing_direction, mut transform)| {
            transform.rotation = facing_direction.to_quat();
        });
}

fn interpolate_character_transform(
    mut query: Query<(&PreviousPose, &GridPosition, &Facing, &mut Transform)>,
    step_transition: Res<StepTransition>,
    field_size: Res<GridSize>,
) {
    let size = field_size.as_ivec2();
    let lower_pos_delta_cap = -size / 2;
    let upper_pos_delta_cap = size / 2;
    let origin = grid_origin_offset(*field_size);

    let eased_t = easing::SmootherStepCurve.sample_unchecked(step_transition.progress);

    query
        .par_iter_mut()
        .for_each(|(prev, &curr_pos, &curr_facing, mut transform)| {
            // avoid dereferencing transform mutably to not mark it as changed for bevy

            if prev.facing != curr_facing {
                transform.rotation = prev.facing.to_quat().slerp(curr_facing.to_quat(), eased_t);
            }

            if prev.pos != curr_pos {
                let wrapped_pos_delta = {
                    let mut pos_delta = curr_pos.as_ivec2() - prev.pos.as_ivec2();
                    if pos_delta.x < lower_pos_delta_cap.x {
                        pos_delta.x += size.x;
                    } else if upper_pos_delta_cap.x < pos_delta.x {
                        pos_delta.x -= size.x;
                    }
                    if pos_delta.y < lower_pos_delta_cap.y {
                        pos_delta.y += size.y;
                    } else if upper_pos_delta_cap.y < pos_delta.y {
                        pos_delta.y -= size.y;
                    }
                    pos_delta
                };

                let pos_delta_world = wrapped_pos_delta.as_vec2() * TILE_SIZE;

                let z = transform.translation.z;
                let local_xy = if eased_t < 0.5 {
                    grid_to_local_xy(prev.pos) + pos_delta_world * eased_t
                } else {
                    grid_to_local_xy(curr_pos) - pos_delta_world * (1.0 - eased_t)
                };
                transform.translation = (origin + local_xy).extend(z);
            }
        });
}

#[allow(clippy::type_complexity)]
fn sync_selection_highlight(
    inspected: Option<Res<InspectedCharacter>>,
    mut characters: Query<
        (&mut Transform, Has<Herbivore>),
        (CharacterFilter, Without<SelectionHighlight>),
    >,
    highlight_query: Single<(&mut Transform, &mut Visibility), With<SelectionHighlight>>,
    mut prev_inspected: Local<Option<Entity>>,
) {
    let selected = inspected.and_then(|i| i.0);

    // Restore Z of previously inspected character if selection changed
    if *prev_inspected != selected {
        if let Some(prev_entity) = *prev_inspected
            && let Ok((mut prev_transform, is_herbivore)) = characters.get_mut(prev_entity)
        {
            prev_transform.translation.z = if is_herbivore {
                Herbivore::Z
            } else {
                Predator::Z
            };
        }
        *prev_inspected = selected;
    }

    let (mut highlight_transform, mut highlight_vis) = highlight_query.into_inner();

    if let Some(entity) = selected
        && let Ok((mut char_transform, _)) = characters.get_mut(entity)
    {
        if char_transform.translation.z != INSPECTED_CHARACTER_Z {
            char_transform.translation.z = INSPECTED_CHARACTER_Z;
        }
        highlight_transform.translation = char_transform
            .translation
            .truncate()
            .extend(SELECTION_HIGHLIGHT_Z);
        if *highlight_vis != Visibility::Inherited {
            *highlight_vis = Visibility::Inherited;
        }
    } else if *highlight_vis != Visibility::Hidden {
        *highlight_vis = Visibility::Hidden;
    }
}
