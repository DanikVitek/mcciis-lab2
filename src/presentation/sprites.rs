use bevy::curve::easing;
use bevy::ecs::query::QueryFilter;
use bevy::math::I16Vec2;
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
use crate::simulation::brain::{
    SensorOffsets,
    sense_brain_inputs,
};
use crate::simulation::organism::{
    CharacterFilter,
    EntityKind,
    Herbivore,
    Plant,
    Predator,
};
use crate::simulation::spatial::SpatialGrid;
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

/// RGB outline colors for the 4 sensor zones: `[Front, Left, Right, Near]`.
pub const POV_ZONE_OUTLINE_COLORS: [[u8; 3]; 4] = [
    [130, 225, 255], // Front
    [255, 195, 110], // Left
    [215, 150, 255], // Right
    [255, 245, 155], // Near
];

const GRID_BACKGROUND_Z: f32 = -1.0;
const POV_FILL_WORLD_Z: f32 = -0.5;
const POV_OUTLINE_WORLD_Z: f32 = 2.5;
const INSPECTED_CHARACTER_Z: f32 = 3.0;
const SELECTION_HIGHLIGHT_Z: f32 = 3.5;
const SELECTION_HIGHLIGHT_SCALE: f32 = 1.6;

const POV_FILL_LOCAL_Z: f32 = POV_FILL_WORLD_Z - INSPECTED_CHARACTER_Z;
const POV_OUTLINE_LOCAL_Z: f32 = POV_OUTLINE_WORLD_Z - INSPECTED_CHARACTER_Z;
const POV_OUTLINE_INSET: f32 = 2.0;
const POV_OUTLINE_THICKNESS: f32 = 2.0;

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

/// Marker component for the child entity of a character that renders its in-world POV sensor outlines and activation fills.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Component)]
pub struct CharacterPovOverlay;

/// Marker component on each zone fill quad inside a [`CharacterPovOverlay`], storing the zone index (`0..4`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component)]
struct PovZoneFill(usize);

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
        GRID_BACKGROUND_Z < POV_FILL_WORLD_Z
            && POV_FILL_WORLD_Z < Plant::Z
            && Plant::Z < Herbivore::Z
            && Herbivore::Z < Predator::Z
            && Predator::Z < POV_OUTLINE_WORLD_Z
            && POV_OUTLINE_WORLD_Z < INSPECTED_CHARACTER_Z
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
                (sync_selection_highlight, ApplyDeferred).chain(),
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

/// Computes the translucent RGB activation fill color (`R = Predator, G = Algae/Plant, B = Herbivore`)
/// for a sensor zone, returning `None` when all three channels are `0.0`.
fn zone_activation_fill_color(activations: [f32; 3]) -> Option<Color> {
    let [plants_act, herbivores_act, predators_act] = activations;
    let max_act = predators_act
        .max(plants_act)
        .max(herbivores_act)
        .clamp(0.0, 1.0);
    if max_act <= 0.0 {
        return None;
    }
    let r = predators_act.clamp(0.0, 1.0);
    let g = plants_act.clamp(0.0, 1.0);
    let b = herbivores_act.clamp(0.0, 1.0);
    let alpha = (65.0 + 150.0 * max_act) / 255.0;
    Some(Color::srgba(r, g, b, alpha))
}

/// Spawns a [`CharacterPovOverlay`] child entity attached to `character_entity` with zone fill sprites
/// and inset perimeter outline sprites in character-local coordinates.
fn spawn_character_pov_overlay(
    commands: &mut Commands,
    character_entity: Entity,
    sensors: &SensorOffsets,
    zone_fills: [Option<Color>; 4],
) {
    let zones = [
        sensors.front(),
        sensors.left(),
        sensors.right(),
        sensors.near(),
    ];

    commands
        .spawn((
            CharacterPovOverlay,
            Transform::default(),
            Visibility::Inherited,
            ChildOf(character_entity),
        ))
        .with_children(|parent| {
            let fill_size = TILE_SIZE - Vec2::splat(POV_OUTLINE_INSET);
            let inset = POV_OUTLINE_INSET;
            let thickness = POV_OUTLINE_THICKNESS;
            let half_t = thickness * 0.5;

            for (zone_idx, &offsets) in zones.iter().enumerate() {
                let fill_z = POV_FILL_LOCAL_Z + zone_idx as f32 * 0.01;
                let outline_z = POV_OUTLINE_LOCAL_Z + zone_idx as f32 * 0.01;
                let [r, g, b] = POV_ZONE_OUTLINE_COLORS[zone_idx];
                let outline_color = Color::srgb_u8(r, g, b);
                let (fill_color, fill_vis) = match zone_fills[zone_idx] {
                    Some(c) => (c, Visibility::Inherited),
                    None => (Color::NONE, Visibility::Hidden),
                };

                let contains = |x: i16, y: i16| offsets.contains(&I16Vec2::new(x, y));

                for &offset in offsets {
                    let center = offset.as_vec2() * TILE_SIZE;
                    parent.spawn((
                        PovZoneFill(zone_idx),
                        Sprite {
                            color: fill_color,
                            custom_size: Some(fill_size),
                            ..default()
                        },
                        Transform::from_xyz(center.x, center.y, fill_z),
                        fill_vis,
                    ));

                    let left = center.x - TILE_SIZE.x * 0.5;
                    let right = center.x + TILE_SIZE.x * 0.5;
                    let bottom = center.y - TILE_SIZE.y * 0.5;
                    let top = center.y + TILE_SIZE.y * 0.5;

                    let has_top = contains(offset.x, offset.y + 1);
                    let has_bottom = contains(offset.x, offset.y - 1);
                    let has_left = contains(offset.x - 1, offset.y);
                    let has_right = contains(offset.x + 1, offset.y);

                    // Top edge (local_y + 1)
                    if !has_top {
                        let x_min = if !has_left {
                            left + inset - half_t
                        } else if contains(offset.x - 1, offset.y + 1) {
                            left - inset - half_t
                        } else {
                            left
                        };
                        let x_max = if !has_right {
                            right - inset + half_t
                        } else if contains(offset.x + 1, offset.y + 1) {
                            right + inset + half_t
                        } else {
                            right
                        };
                        let width = (x_max - x_min).max(0.0);
                        let x_mid = (x_min + x_max) * 0.5;
                        let y_mid = top - inset;
                        parent.spawn((
                            Sprite {
                                color: outline_color,
                                custom_size: Some(Vec2::new(width, thickness)),
                                ..default()
                            },
                            Transform::from_xyz(x_mid, y_mid, outline_z),
                        ));
                    }

                    // Bottom edge (local_y - 1)
                    if !has_bottom {
                        let x_min = if !has_left {
                            left + inset - half_t
                        } else if contains(offset.x - 1, offset.y - 1) {
                            left - inset - half_t
                        } else {
                            left
                        };
                        let x_max = if !has_right {
                            right - inset + half_t
                        } else if contains(offset.x + 1, offset.y - 1) {
                            right + inset + half_t
                        } else {
                            right
                        };
                        let width = (x_max - x_min).max(0.0);
                        let x_mid = (x_min + x_max) * 0.5;
                        let y_mid = bottom + inset;
                        parent.spawn((
                            Sprite {
                                color: outline_color,
                                custom_size: Some(Vec2::new(width, thickness)),
                                ..default()
                            },
                            Transform::from_xyz(x_mid, y_mid, outline_z),
                        ));
                    }

                    // Left edge (local_x - 1)
                    if !has_left {
                        let y_min = if !has_bottom {
                            bottom + inset - half_t
                        } else if contains(offset.x - 1, offset.y - 1) {
                            bottom - inset - half_t
                        } else {
                            bottom
                        };
                        let y_max = if !has_top {
                            top - inset + half_t
                        } else if contains(offset.x - 1, offset.y + 1) {
                            top + inset + half_t
                        } else {
                            top
                        };
                        let height = (y_max - y_min).max(0.0);
                        let x_mid = left + inset;
                        let y_mid = (y_min + y_max) * 0.5;
                        parent.spawn((
                            Sprite {
                                color: outline_color,
                                custom_size: Some(Vec2::new(thickness, height)),
                                ..default()
                            },
                            Transform::from_xyz(x_mid, y_mid, outline_z),
                        ));
                    }

                    // Right edge (local_x + 1)
                    if !has_right {
                        let y_min = if !has_bottom {
                            bottom + inset - half_t
                        } else if contains(offset.x + 1, offset.y - 1) {
                            bottom - inset - half_t
                        } else {
                            bottom
                        };
                        let y_max = if !has_top {
                            top - inset + half_t
                        } else if contains(offset.x + 1, offset.y + 1) {
                            top + inset + half_t
                        } else {
                            top
                        };
                        let height = (y_max - y_min).max(0.0);
                        let x_mid = right - inset;
                        let y_mid = (y_min + y_max) * 0.5;
                        parent.spawn((
                            Sprite {
                                color: outline_color,
                                custom_size: Some(Vec2::new(thickness, height)),
                                ..default()
                            },
                            Transform::from_xyz(x_mid, y_mid, outline_z),
                        ));
                    }
                }
            }
        });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_selection_highlight(
    inspected: Option<Res<InspectedCharacter>>,
    spatial_grid: Res<SpatialGrid>,
    mut characters: Query<
        (
            &mut Transform,
            &GridPosition,
            &Facing,
            &SensorOffsets,
            Has<Herbivore>,
        ),
        (
            CharacterFilter,
            Without<SelectionHighlight>,
            Without<CharacterPovOverlay>,
            Without<PovZoneFill>,
        ),
    >,
    highlight_query: Single<
        (&mut Transform, &mut Visibility),
        (
            With<SelectionHighlight>,
            Without<CharacterPovOverlay>,
            Without<PovZoneFill>,
        ),
    >,
    mut pov_overlays: Query<
        (&ChildOf, &mut Visibility, Option<&Children>),
        (
            With<CharacterPovOverlay>,
            Without<SelectionHighlight>,
            Without<PovZoneFill>,
        ),
    >,
    mut fill_sprites: Query<
        (&PovZoneFill, &mut Sprite, &mut Visibility),
        (Without<CharacterPovOverlay>, Without<SelectionHighlight>),
    >,
    mut prev_inspected: Local<Option<Entity>>,
    mut commands: Commands,
) {
    let selected = inspected.and_then(|i| i.0);

    // Restore Z and hide POV overlay of previously inspected character if selection changed
    if *prev_inspected != selected {
        if let Some(prev_entity) = *prev_inspected {
            if let Ok((mut prev_transform, .., is_herbivore)) = characters.get_mut(prev_entity) {
                prev_transform.translation.z = if is_herbivore {
                    Herbivore::Z
                } else {
                    Predator::Z
                };
            }
            for (child_of, mut pov_vis, _) in &mut pov_overlays {
                if child_of.parent() == prev_entity && *pov_vis != Visibility::Hidden {
                    *pov_vis = Visibility::Hidden;
                }
            }
        }
        *prev_inspected = selected;
    }

    let (mut highlight_transform, mut highlight_vis) = highlight_query.into_inner();

    if let Some(entity) = selected
        && let Ok((mut char_transform, &pos, &facing, sensors, is_herbivore)) =
            characters.get_mut(entity)
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

        let kind = if is_herbivore {
            EntityKind::Herbivore
        } else {
            EntityKind::Predator
        };
        let sensed = sense_brain_inputs(&spatial_grid, pos, facing, kind, sensors);
        let inputs = sensed.as_slice();
        let zone_fills = [
            zone_activation_fill_color([inputs[0], inputs[1], inputs[2]]),
            zone_activation_fill_color([inputs[3], inputs[4], inputs[5]]),
            zone_activation_fill_color([inputs[6], inputs[7], inputs[8]]),
            zone_activation_fill_color([inputs[9], inputs[10], inputs[11]]),
        ];

        let mut existing_overlay = None;
        for (child_of, pov_vis, pov_children) in &mut pov_overlays {
            if child_of.parent() == entity {
                existing_overlay = Some((pov_vis, pov_children));
                break;
            }
        }

        if let Some((mut pov_vis, pov_children)) = existing_overlay {
            if *pov_vis != Visibility::Inherited {
                *pov_vis = Visibility::Inherited;
            }
            if let Some(children) = pov_children {
                for &child in children {
                    if let Ok((zone_fill, mut sprite, mut vis)) = fill_sprites.get_mut(child) {
                        match zone_fills[zone_fill.0] {
                            Some(color) => {
                                if sprite.color != color {
                                    sprite.color = color;
                                }
                                if *vis != Visibility::Inherited {
                                    *vis = Visibility::Inherited;
                                }
                            }
                            None => {
                                if *vis != Visibility::Hidden {
                                    *vis = Visibility::Hidden;
                                }
                            }
                        }
                    }
                }
            }
        } else {
            spawn_character_pov_overlay(&mut commands, entity, sensors, zone_fills);
        }
    } else if *highlight_vis != Visibility::Hidden {
        *highlight_vis = Visibility::Hidden;
    }
}
