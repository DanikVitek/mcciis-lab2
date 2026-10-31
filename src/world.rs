use bevy::prelude::*;
use bon::bon;
use rand::{RngExt, distr::Distribution, seq::WeightError};

use crate::{
    character::{Herbivore, Plant, Predator},
    grid::{Facing, Grid, GridPosition, GridSize, TILE_SIZE},
};

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

pub mod system {
    use crate::character::{EntityKind, SpatialGrid};

    use super::*;

    pub fn setup(
        mut commands: Commands,
        mut spatial_grid: ResMut<SpatialGrid>,
        field_size: Res<GridSize>,
        config: Res<DistributionConfig>,
        asset_server: Res<AssetServer>,
    ) {
        commands
            .spawn((
                Grid,
                Transform::from_translation(
                    (-TILE_SIZE * (field_size.as_vec2() - Vec2::ONE) / 2.0).extend(0.0),
                ),
            ))
            .with_children(|parent| {
                let plant_image: Handle<Image> = asset_server.load("sprites/plant.png");
                let herbivore_image: Handle<Image> = asset_server.load("sprites/herbivore.png");
                let predator_image: Handle<Image> = asset_server.load("sprites/predator.png");

                let tiles_total = field_size.width() as u32 * field_size.height() as u32;

                let mut plants_total = (tiles_total as f32 * config.plants_fraction()) as u32;
                let mut herbivores_total =
                    (tiles_total as f32 * config.herbivores_fraction()) as u32;
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

                for y in 0..field_size.height() {
                    for x in 0..field_size.width() {
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
                                let pos = GridPosition::new(x, y);
                                let entity = parent
                                    .spawn((
                                        Plant,
                                        pos,
                                        Sprite {
                                            image: plant_image.clone(),
                                            custom_size: Some(TILE_SIZE),
                                            ..default()
                                        },
                                    ))
                                    .id();
                                spatial_grid.insert(pos, entity, EntityKind::Plant);
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
                                        let entity = parent
                                            .spawn((
                                                Herbivore,
                                                pos,
                                                dir,
                                                Sprite {
                                                    image: herbivore_image.clone(),
                                                    custom_size: Some(TILE_SIZE),
                                                    ..default()
                                                },
                                            ))
                                            .id();
                                        spatial_grid.insert(pos, entity, EntityKind::Herbivore);
                                        herbivores_total -= 1;
                                        match dist.update_weights(&[(2, &herbivores_total)]) {
                                            Ok(()) => {}
                                            Err(WeightError::InsufficientNonZero) => return,
                                            Err(_) => unreachable!(),
                                        }
                                    }
                                    3 => {
                                        let entity = parent
                                            .spawn((
                                                Predator,
                                                pos,
                                                dir,
                                                Sprite {
                                                    image: predator_image.clone(),
                                                    custom_size: Some(TILE_SIZE),
                                                    ..default()
                                                },
                                            ))
                                            .id();
                                        spatial_grid.insert(pos, entity, EntityKind::Predator);
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
}
