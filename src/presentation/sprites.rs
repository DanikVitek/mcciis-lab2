use bevy::prelude::*;
use bevy::{curve::easing, ecs::query::QueryFilter, render::render_resource::AsBindGroup};

use crate::{
    grid::{Facing, GridPosition, GridSize},
    simulation::{
        SimulationSystems, SimulationTick, WorldSeeded,
        organism::{Herbivore, Plant, Predator},
    },
};

use super::pacing::SimulationSpeedMode;

pub const TILE_SIZE: Vec2 = Vec2::splat(32.0);

/// World-space translation of tile `(0, 0)` so the entire [`GridSize`] field is centered at `(0, 0)`.
pub fn grid_origin_offset(grid_size: GridSize) -> Vec2 {
    -TILE_SIZE * (grid_size.as_vec2() - Vec2::ONE) / 2.0
}

/// Local translation of a [`GridPosition`] relative to [`GridVisualRoot`].
pub fn grid_to_local_xy(pos: GridPosition) -> Vec2 {
    Vec2 {
        x: f32::from(pos.x) * TILE_SIZE.x,
        y: f32::from(pos.y) * TILE_SIZE.y,
    }
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

/// Marker component for the centered parent entity holding all organism sprites.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Component)]
#[require(Transform, Visibility)]
pub struct GridVisualRoot;

/// Resource storing the [`Entity`] of [`GridVisualRoot`] for fast `O(1)` parenting in observers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Resource)]
pub struct GridRootEntity(pub Entity);

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
    assert!(Plant::Z < Herbivore::Z && Herbivore::Z < Predator::Z);
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
        app.add_plugins(SpriteMaterialPlugin::<PaletteSwapMaterial>::default())
            .add_systems(PreStartup, setup_visual_root_and_sprites)
            .add_observer(attach_visuals_to_plant)
            .add_observer(attach_visuals_to_character::<Herbivore>)
            .add_observer(attach_visuals_to_character::<Predator>)
            .add_observer(on_world_seeded)
            .add_systems(
                SimulationTick,
                (
                    remember_previous_pose.before(SimulationSystems::Step),
                    reset_step_transition,
                )
                    .run_if(resource_equals(SimulationSpeedMode::FixedStep)),
            )
            .add_systems(
                Update,
                (
                    update_step_transition.run_if(resource_equals(SimulationSpeedMode::FixedStep)),
                    set_step_transitioned.run_if(resource_equals(SimulationSpeedMode::Uncapped)),
                )
                    .run_if(|it: Res<StepTransition>| it.progress < 1.0),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_grid_root_centering.run_if(resource_changed::<GridSize>),
                    sync_translation_with_grid::<With<Plant>>
                        .run_if(resource_equals(SimulationSpeedMode::FixedStep)),
                    interpolate_character_transform.run_if(resource_changed::<StepTransition>),
                    (sync_translation_with_grid::<()>, sync_rotation_with_grid)
                        .chain_weak()
                        .run_if(resource_equals(SimulationSpeedMode::Uncapped)),
                )
                    .chain_weak()
                    .before(TransformSystems::Propagate),
            )
            .init_resource::<StepTransition>();
    }
}

fn setup_visual_root_and_sprites(
    mut commands: Commands,
    field_size: Res<GridSize>,
    asset_server: Res<AssetServer>,
    mut shaders: ResMut<Assets<PaletteSwapMaterial>>,
) {
    let root = commands
        .spawn((
            GridVisualRoot,
            Transform::from_translation(grid_origin_offset(*field_size).extend(0.0)),
        ))
        .id();

    commands.insert_resource(GridRootEntity(root));

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
    grid_root: Res<GridRootEntity>,
) {
    let &pos = query.get_inner(add.entity).unwrap();
    commands.entity(add.entity).insert((
        ChildOf(grid_root.0),
        Sprite {
            image: Plant::sprite_handle(&sprites),
            custom_size: Some(TILE_SIZE),
            ..default()
        },
        Transform {
            translation: grid_to_local_xy(pos).extend(Plant::Z),
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
    grid_root: Res<GridRootEntity>,
) {
    let (&pos, &facing) = query.get_inner(add.entity).unwrap();
    commands.entity(add.entity).insert((
        ChildOf(grid_root.0),
        Sprite {
            image: T::sprite_handle(&sprites),
            custom_size: Some(TILE_SIZE),
            ..default()
        },
        Transform {
            translation: grid_to_local_xy(pos).extend(T::Z),
            rotation: facing.to_quat(),
            ..default()
        },
        SpriteMaterial(T::shader_handle(&shaders)),
        PreviousPose { pos, facing },
    ));
}

fn remember_previous_pose(query: Query<(&mut PreviousPose, &GridPosition, &Facing)>) {
    for (mut prev_pose, prev_pos, prev_facing) in query {
        prev_pose.pos = *prev_pos;
        prev_pose.facing = *prev_facing;
    }
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

fn sync_grid_root_centering(
    field_size: Res<GridSize>,
    mut root_transform: Single<&mut Transform, With<GridVisualRoot>>,
) {
    root_transform.translation = grid_origin_offset(*field_size).extend(0.0);
}

fn sync_translation_with_grid<F: QueryFilter>(
    query: Query<(&GridPosition, &mut Transform), (Changed<GridPosition>, F)>,
) {
    for (grid_position, mut transform) in query {
        let z = transform.translation.z;
        transform.translation = grid_to_local_xy(*grid_position).extend(z);
    }
}

fn sync_rotation_with_grid(query: Query<(&Facing, &mut Transform), Changed<Facing>>) {
    for (facing_direction, mut transform) in query {
        transform.rotation = facing_direction.to_quat();
    }
}

fn interpolate_character_transform(
    query: Query<(&PreviousPose, &GridPosition, &Facing, &mut Transform)>,
    step_transition: Res<StepTransition>,
    field_size: Res<GridSize>,
) {
    let size = field_size.as_ivec2();
    let lower_pos_delta_cap = -size / 2;
    let upper_pos_delta_cap = size / 2;

    let eased_t = easing::SmootherStepCurve.sample_unchecked(step_transition.progress);

    for (prev, &curr_pos, &curr_facing, mut transform) in query {
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
            transform.translation = if eased_t < 0.5 {
                grid_to_local_xy(prev.pos) + pos_delta_world * eased_t
            } else {
                grid_to_local_xy(curr_pos) - pos_delta_world * (1.0 - eased_t)
            }
            .extend(z);
        }
    }
}
