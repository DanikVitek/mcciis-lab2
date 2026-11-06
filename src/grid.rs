use core::f32::consts::{
    FRAC_PI_2,
    PI,
};

use bevy::math::{
    I16Vec2,
    U16Vec2,
};
use bevy::prelude::*;
use nalgebra::Complex;
use rand::distr::{
    Distribution,
    StandardUniform,
};
use rand::{
    Rng,
    RngExt,
};
use serde::{
    Deserialize,
    Serialize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, Deref, DerefMut)]
#[repr(transparent)]
pub struct GridPosition(pub U16Vec2);

impl GridPosition {
    #[must_use]
    pub const fn new(x: u16, y: u16) -> Self {
        Self(U16Vec2 { x, y })
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

    #[must_use]
    pub fn add_wrapping_rotated(self, offset: I16Vec2, dir: Facing, bounds: GridSize) -> Self {
        let rotated_offset = dir.rotate_offset(offset);
        self.add_wrapping(rotated_offset, bounds)
    }

    #[must_use]
    pub fn add_wrapping(self, offset: I16Vec2, bounds: GridSize) -> Self {
        let raw = self.as_ivec2() + offset.as_ivec2();
        let wrapped = U16Vec2::new(
            raw.x.rem_euclid(i32::from(bounds.x)) as u16,
            raw.y.rem_euclid(i32::from(bounds.y)) as u16,
        );
        Self(wrapped)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename = "grid_size", rename_all = "snake_case")]
struct GridSizeRepr {
    width: u16,
    height: u16,
}

impl From<GridSize> for GridSizeRepr {
    fn from(size: GridSize) -> Self {
        Self {
            width: size.width(),
            height: size.height(),
        }
    }
}

impl From<GridSizeRepr> for GridSize {
    fn from(repr: GridSizeRepr) -> Self {
        Self::new(repr.width, repr.height)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Resource, Deref, DerefMut, Serialize, Deserialize,
)]
#[serde(from = "GridSizeRepr", into = "GridSizeRepr")]
pub struct GridSize(pub U16Vec2);

impl Default for GridSize {
    fn default() -> Self {
        Self::new(100, 100)
    }
}

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
pub enum Facing {
    #[default]
    North,
    East,
    South,
    West,
}

impl Facing {
    #[must_use]
    pub const fn clockwise(self) -> Self {
        match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
        }
    }

    #[must_use]
    pub const fn counterclockwise(self) -> Self {
        match self {
            Self::North => Self::West,
            Self::East => Self::North,
            Self::South => Self::East,
            Self::West => Self::South,
        }
    }

    #[must_use]
    pub const fn to_angle(self) -> f32 {
        match self {
            Self::North => 0.0,
            Self::East => -FRAC_PI_2,
            Self::South => PI,
            Self::West => FRAC_PI_2,
        }
    }

    #[must_use]
    pub const fn to_complex(self) -> Complex<i8> {
        match self {
            Self::North => Complex::new(1, 0),
            Self::East => Complex::new(0, -1),
            Self::South => Complex::new(-1, 0),
            Self::West => Complex::new(0, 1),
        }
    }

    #[must_use]
    pub fn to_quat(self) -> Quat {
        Quat::from_rotation_z(self.to_angle())
    }

    #[must_use]
    pub const fn to_step_delta(self) -> I16Vec2 {
        match self {
            Self::North => I16Vec2::Y,
            Self::East => I16Vec2::X,
            Self::South => I16Vec2::NEG_Y,
            Self::West => I16Vec2::NEG_X,
        }
    }

    #[must_use]
    pub const fn rotate_offset(self, offset: I16Vec2) -> I16Vec2 {
        match self {
            Self::North => offset,
            Self::East => I16Vec2::new(offset.y, -offset.x),
            Self::South => I16Vec2::new(-offset.y, -offset.x), // -offset,
            Self::West => I16Vec2::new(-offset.y, offset.x),
        }
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
