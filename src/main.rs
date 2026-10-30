pub mod character;
pub mod grid;

use core::f32;

use bevy::prelude::*;
use bon::bon;
use rand::{RngExt, distr::Distribution, seq::WeightError};

use self::{
    character::{Herbivore, Plant, Predator},
    grid::{Facing, Grid, GridPlugin, GridPosition, GridSize, TILE_SIZE},
};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(ImagePlugin::default_nearest()))
        .insert_resource(
            DistributionConfig::builder()
                .plants_fraction(0.1)
                .herbivores_fraction(0.1)
                .predators_fraction(0.1)
                .build()
                .unwrap(),
        )
        .add_plugins(GridPlugin)
        .insert_resource(GridSize {
            width: 100,
            height: 100,
        })
        .add_systems(Startup, (setup_camera, setup_world))
        // .add_systems(Update, (animate_movement,))
        .add_systems(FixedUpdate, (update_world,))
        .insert_resource(Time::<Fixed>::from_seconds(0.5))
        .run();
}

#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct DistributionConfig {
    plants_fraction: f32,
    herbivores_fraction: f32,
    predators_fraction: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum DistributionError {
    #[error("negative distribution fraction")]
    NegativeFraction,
    #[error("sum of the distribution fractions is greater than one")]
    SumGtOne,
}

#[bon]
impl DistributionConfig {
    #[builder]
    pub fn new(
        #[builder(default = 0.0)] plants_fraction: f32,
        #[builder(default = 0.0)] herbivores_fraction: f32,
        #[builder(default = 0.0)] predators_fraction: f32,
    ) -> Result<Self, DistributionError> {
        if plants_fraction < 0.0 || herbivores_fraction < 0.0 || predators_fraction < 0.0 {
            return Err(DistributionError::NegativeFraction);
        }

        if plants_fraction + herbivores_fraction + predators_fraction > 1.0 {
            return Err(DistributionError::SumGtOne);
        }

        Ok(Self {
            plants_fraction,
            herbivores_fraction,
            predators_fraction,
        })
    }
}

impl DistributionConfig {
    fn plants_fraction(&self) -> f32 {
        self.plants_fraction
    }

    fn herbivores_fraction(&self) -> f32 {
        self.herbivores_fraction
    }

    fn predators_fraction(&self) -> f32 {
        self.predators_fraction
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, States, Default)]
enum GameState {
    #[default]
    World,
}

/// Dark-grassy green (#337c37)
const BACKGROUND_COLOR: Color = Color::oklch(0.5221, 0.1258, 144.53);

fn setup_camera(mut commands: Commands) {
    commands.spawn((Camera2d,));
}

fn setup_world(
    mut commands: Commands,
    field_size: Res<GridSize>,
    config: Res<DistributionConfig>,
    asset_server: Res<AssetServer>,
) {
    commands
        .spawn((
            DespawnOnExit(GameState::World),
            Grid,
            Transform::from_translation(Vec3 {
                x: -TILE_SIZE.x * field_size.width as f32 / 2.0,
                y: -TILE_SIZE.y * field_size.height as f32 / 2.0,
                z: 0.0,
            }),
        ))
        .with_children(|parent| {
            let plant_image: Handle<Image> = asset_server.load("sprites/plant.png");
            let herbivore_image: Handle<Image> = asset_server.load("sprites/herbivore.png");
            let predator_image: Handle<Image> = asset_server.load("sprites/predator.png");

            let tiles_total = field_size.width as u32 * field_size.height as u32;

            let mut plants_total = (tiles_total as f32 * config.plants_fraction()) as u32;
            let mut herbivores_total = (tiles_total as f32 * config.herbivores_fraction()) as u32;
            let mut predators_total = (tiles_total as f32 * config.predators_fraction()) as u32;
            let mut rest = tiles_total - plants_total - herbivores_total - predators_total;

            let mut dist = match rand::distr::weighted::WeightedIndex::new([
                rest,
                plants_total,
                herbivores_total,
                predators_total,
            ]) {
                Ok(dist) => dist,
                Err(WeightError::InsufficientNonZero) => return,
                Err(_) => unreachable!(),
            };

            let mut rng = rand::rng();

            for y in 0..field_size.height {
                for x in 0..field_size.width {
                    match dist.sample(&mut rng) {
                        0 => {
                            rest -= 1;
                            match dist.update_weights(&[(0, &rest)]) {
                                Ok(()) => {}
                                Err(WeightError::InsufficientNonZero) => return,
                                Err(_) => unreachable!(),
                            }
                        }
                        1 => {
                            parent.spawn((
                                Plant,
                                GridPosition::new(x, y),
                                Sprite {
                                    image: plant_image.clone(),
                                    custom_size: Some(TILE_SIZE),
                                    ..default()
                                },
                            ));
                            plants_total -= 1;
                            match dist.update_weights(&[(1, &plants_total)]) {
                                Ok(()) => {}
                                Err(WeightError::InsufficientNonZero) => return,
                                Err(_) => unreachable!(),
                            }
                        }
                        idx @ (2 | 3) => {
                            let pos = GridPosition::new(x, y);
                            let dir = rng.random::<Facing>();
                            match idx {
                                2 => {
                                    parent.spawn((
                                        Herbivore,
                                        pos,
                                        dir,
                                        Sprite {
                                            image: herbivore_image.clone(),
                                            custom_size: Some(TILE_SIZE),
                                            ..default()
                                        },
                                    ));
                                    herbivores_total -= 1;
                                    match dist.update_weights(&[(2, &herbivores_total)]) {
                                        Ok(()) => {}
                                        Err(WeightError::InsufficientNonZero) => return,
                                        Err(_) => unreachable!(),
                                    }
                                }
                                3 => {
                                    parent.spawn((
                                        Predator,
                                        pos,
                                        dir,
                                        Sprite {
                                            image: predator_image.clone(),
                                            custom_size: Some(TILE_SIZE),
                                            ..default()
                                        },
                                    ));
                                    predators_total -= 1;
                                    match dist.update_weights(&[(3, &predators_total)]) {
                                        Ok(()) => {}
                                        Err(WeightError::InsufficientNonZero) => return,
                                        Err(_) => unreachable!(),
                                    }
                                }
                                _ => unreachable!(),
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        });
}

fn update_world(query: Query<(&mut GridPosition, &Facing)>, field_size: Res<GridSize>) {
    for (mut pos, facing) in query {
        pos.add_assign_wrapped_direction(*facing, *field_size);
    }
}
