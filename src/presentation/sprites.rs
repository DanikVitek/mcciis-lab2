use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;

use crate::{
    grid::{Facing, GridPosition, GridSize},
    simulation::organism::{Herbivore, Plant, Predator},
};

pub const TILE_SIZE: Vec2 = Vec2::splat(32.0);

/// World-space translation of tile `(0, 0)` so the entire [`GridSize`] field is centered at `(0, 0)`.
pub fn grid_origin_offset(grid_size: GridSize) -> Vec2 {
    -TILE_SIZE * (grid_size.as_vec2() - Vec2::ONE) / 2.0
}

/// Local translation of a [`GridPosition`] relative to [`GridVisualRoot`].
pub fn grid_to_local_xyz(pos: GridPosition, z: f32) -> Vec3 {
    Vec3 {
        x: f32::from(pos.x) * TILE_SIZE.x,
        y: f32::from(pos.y) * TILE_SIZE.y,
        z,
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

impl HasSprite for Plant {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.plant.clone()
    }
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

pub struct SpritesPlugin;

impl Plugin for SpritesPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SpriteMaterialPlugin::<PaletteSwapMaterial>::default())
            .add_systems(PreStartup, setup_visual_root_and_sprites)
            .add_observer(attach_visuals::<Plant>)
            .add_observer(attach_visuals_remapped::<Herbivore>)
            .add_observer(attach_visuals_remapped::<Predator>)
            .add_systems(
                PostStartup,
                (sync_translation_with_grid, sync_rotation_with_grid)
                    .chain_weak()
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_grid_root_centering.run_if(resource_changed::<GridSize>),
                    sync_translation_with_grid,
                    sync_rotation_with_grid,
                )
                    .chain_weak()
                    .before(TransformSystems::Propagate),
            );
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

    let herbivore_colors = [
        LinearRgba::new(0.85, 0.80, 0.2, 1.0),
        LinearRgba::new(0.4, 0.4, 0.0, 1.0),
        LinearRgba::new(0.0, 0.0, 0.0, 1.0),
    ];
    let herbivore_sh = shaders.add(PaletteSwapMaterial {
        colors: herbivore_colors,
    });

    let predator_colors = [
        LinearRgba::new(0.85, 0.15, 0.15, 1.0),
        LinearRgba::new(0.45, 0.1, 0.1, 1.0),
        LinearRgba::new(0.0, 0.0, 0.0, 1.0),
    ];
    let predator_sh = shaders.add(PaletteSwapMaterial {
        colors: predator_colors,
    });

    commands.insert_resource(OrganismSpriteRemaps {
        herbivore: herbivore_sh,
        predator: predator_sh,
    });
}

fn attach_visuals<T: HasSprite>(
    add: On<Add<T>>,
    mut commands: Commands,
    sprites: Res<OrganismSprites>,
    grid_root: Res<GridRootEntity>,
) {
    commands.entity(add.entity).insert((
        ChildOf(grid_root.0),
        Sprite {
            image: T::sprite_handle(&sprites),
            custom_size: Some(TILE_SIZE),
            ..default()
        },
    ));
}

fn attach_visuals_remapped<T: HasRemappedSprite>(
    add: On<Add<T>>,
    mut commands: Commands,
    sprites: Res<OrganismSprites>,
    shaders: Res<OrganismSpriteRemaps>,
    grid_root: Res<GridRootEntity>,
) {
    commands.entity(add.entity).insert((
        ChildOf(grid_root.0),
        Sprite {
            image: T::sprite_handle(&sprites),
            custom_size: Some(TILE_SIZE),
            ..default()
        },
        SpriteMaterial(T::shader_handle(&shaders)),
    ));
}

fn sync_grid_root_centering(
    field_size: Res<GridSize>,
    mut root_transform: Single<&mut Transform, With<GridVisualRoot>>,
) {
    root_transform.translation = grid_origin_offset(*field_size).extend(0.0);
}

fn sync_translation_with_grid(
    query: Query<(&GridPosition, &mut Transform), Changed<GridPosition>>,
) {
    for (grid_position, mut transform) in query {
        let z = transform.translation.z;
        transform.translation = grid_to_local_xyz(*grid_position, z);
    }
}

fn sync_rotation_with_grid(query: Query<(&Facing, &mut Transform), Changed<Facing>>) {
    for (facing_direction, mut transform) in query {
        transform.rotation = facing_direction.to_quat();
    }
}
