use bevy::prelude::*;

use crate::{
    grid::{Facing, GridPosition, GridSize},
    simulation::organism::{Herbivore, Plant, Predator},
};

pub const TILE_SIZE: Vec2 = Vec2::splat(16.0);

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
    pub herbivore: Handle<Image>,
    pub predator: Handle<Image>,
}

trait HasOrganismSprite: Component {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image>;
}

impl HasOrganismSprite for Plant {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.plant.clone()
    }
}

impl HasOrganismSprite for Herbivore {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.herbivore.clone()
    }
}

impl HasOrganismSprite for Predator {
    fn sprite_handle(sprites: &OrganismSprites) -> Handle<Image> {
        sprites.predator.clone()
    }
}

pub struct SpritesPlugin;

impl Plugin for SpritesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, setup_visual_root_and_sprites)
            .add_observer(attach_organism_visuals::<Plant>)
            .add_observer(attach_organism_visuals::<Herbivore>)
            .add_observer(attach_organism_visuals::<Predator>)
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
) {
    let root = commands
        .spawn((
            GridVisualRoot,
            Transform::from_translation(grid_origin_offset(*field_size).extend(0.0)),
        ))
        .id();

    commands.insert_resource(GridRootEntity(root));
    commands.insert_resource(OrganismSprites {
        plant: asset_server.load("sprites/plant.png"),
        herbivore: asset_server.load("sprites/herbivore.png"),
        predator: asset_server.load("sprites/predator.png"),
    });
}

fn attach_organism_visuals<T: HasOrganismSprite>(
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
