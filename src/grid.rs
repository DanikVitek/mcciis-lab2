use core::f32::consts::{FRAC_PI_2, PI};

use bevy::math::{I16Vec2, U16Vec2};
use bevy::prelude::*;
use nalgebra::Complex;
use rand::{
    Rng, RngExt,
    distr::{Distribution, StandardUniform},
};

pub const TILE_SIZE: Vec2 = Vec2::splat(16.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, Deref, DerefMut)]
#[repr(transparent)]
#[require(Transform)]
pub struct GridPosition(pub U16Vec2);

impl GridPosition {
    pub fn new(x: u16, y: u16) -> Self {
        Self(U16Vec2 { x, y })
    }

    pub fn to_world_xyz(self, z: f32) -> Vec3 {
        Vec3 {
            x: self.x as f32 * TILE_SIZE.x,
            y: self.y as f32 * TILE_SIZE.y,
            z,
        }
    }

    pub fn add_assign_wrapping_direction(&mut self, dir: Facing, bounds: GridSize) {
        match dir {
            Facing::North => {
                if self.y == bounds.height() - 1 {
                    self.y = 0;
                } else {
                    self.y += 1;
                }
            }
            Facing::East => {
                if self.x == bounds.width() - 1 {
                    self.x = 0;
                } else {
                    self.x += 1;
                }
            }
            Facing::South => {
                if self.y == 0 {
                    self.y = bounds.height() - 1;
                } else {
                    self.y -= 1;
                }
            }
            Facing::West => {
                if self.x == 0 {
                    self.x = bounds.width() - 1;
                } else {
                    self.x -= 1;
                }
            }
        }
    }

    pub fn add_wrapping_rotated_offset(
        self,
        offset: I16Vec2,
        dir: Facing,
        bounds: GridSize,
    ) -> Self {
        let rotated_offset = dir.rotate_offset(offset);
        let raw = self.as_ivec2() + rotated_offset.as_ivec2();
        let wrapped = U16Vec2::new(
            raw.x.rem_euclid(i32::from(bounds.x)) as u16,
            raw.y.rem_euclid(i32::from(bounds.y)) as u16,
        );
        Self(wrapped)
    }

    pub fn add_wrapping_signed(self, x: I16Vec2, bounds: GridSize) -> Self {
        let raw = self.as_ivec2() + x.as_ivec2();
        let wrapped = U16Vec2::new(
            raw.x.rem_euclid(i32::from(bounds.x)) as u16,
            raw.y.rem_euclid(i32::from(bounds.y)) as u16,
        );
        Self(wrapped)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Resource, Deref, DerefMut)]
pub struct GridSize(pub U16Vec2);

impl GridSize {
    pub const fn new(width: u16, height: u16) -> Self {
        Self(U16Vec2 {
            x: width,
            y: height,
        })
    }

    pub const fn width(self) -> u16 {
        self.0.x
    }

    pub const fn height(self) -> u16 {
        self.0.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(Transform, Visibility)]
pub struct Grid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
pub enum Facing {
    #[default]
    North,
    East,
    South,
    West,
}

impl Facing {
    pub const fn clockwise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    pub const fn counterclockwise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::East => Self::North,
            Self::South => Self::East,
            Self::West => Self::South,
        }
    }

    pub const fn to_angle(self) -> f32 {
        match self {
            Self::North => 0.0,
            Self::East => -FRAC_PI_2,
            Self::South => PI,
            Self::West => FRAC_PI_2,
        }
    }

    pub const fn to_complex(self) -> Complex<i16> {
        match self {
            Self::North => Complex::new(1, 0),
            Self::East => Complex::new(0, -1),
            Self::South => Complex::new(-1, 0),
            Self::West => Complex::new(0, 1),
        }
    }

    pub fn to_quat(self) -> Quat {
        Quat::from_rotation_z(self.to_angle())
    }

    pub fn to_step_delta(self) -> I16Vec2 {
        match self {
            Self::North => I16Vec2::Y,
            Self::East => I16Vec2::X,
            Self::South => I16Vec2::NEG_Y,
            Self::West => I16Vec2::NEG_X,
        }
    }

    pub fn rotate_offset(self, offset: I16Vec2) -> I16Vec2 {
        let Complex { re, im } = Complex::new(offset.x, offset.y) * self.to_complex();
        I16Vec2 { x: re, y: im }
    }
}

impl Distribution<Facing> for StandardUniform {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Facing {
        match rng.random_range(0..4u8) {
            0 => Facing::North,
            1 => Facing::East,
            2 => Facing::South,
            3 => Facing::West,
            _ => unreachable!(),
        }
    }
}

pub mod system {
    use super::*;

    pub fn sync_translation_with_grid(
        query: Query<(&GridPosition, &mut Transform), Changed<GridPosition>>,
    ) {
        for (grid_position, mut transform) in query {
            let z = transform.translation.z;
            transform.translation = grid_position.to_world_xyz(z);
        }
    }

    pub fn sync_rotation_with_grid(query: Query<(&Facing, &mut Transform), Changed<Facing>>) {
        for (facing_direction, mut transform) in query {
            transform.rotation = facing_direction.to_quat();
        }
    }
}
