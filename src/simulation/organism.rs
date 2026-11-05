use core::{fmt, num::NonZeroU32, ops::Add, str::FromStr};

use bevy::prelude::*;

use super::brain::Brain;

/// Query filter matching any living character organism ([`Herbivore`] or [`Predator`]).
pub type CharacterFilter = Or<(With<Herbivore>, With<Predator>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(ValueAsFood(1.into()))]
pub struct Plant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    ValueAsFood(2.into()),
    Generation,
    Brain = rand::random::<Brain>(),
)]
pub struct Herbivore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    Generation,
    Brain = rand::random::<Brain>(),
)]
pub struct Predator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EntityKind {
    Plant,
    Herbivore,
    Predator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
pub struct Age(u32);

impl Age {
    pub const fn ticks(self) -> u32 {
        self.0
    }

    pub fn increment(&mut self) {
        self.0 += 1;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[repr(transparent)]
pub struct Energy(pub u8);

impl From<u8> for Energy {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<Energy> for u8 {
    fn from(value: Energy) -> Self {
        value.0
    }
}

impl fmt::Display for Energy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for Energy {
    type Err = <u8 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u8::from_str(s).map(Self)
    }
}

impl Add for Energy {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Resource)]
#[repr(transparent)]
pub struct InitialEnergy(pub Energy);

impl Default for InitialEnergy {
    fn default() -> Self {
        5.into()
    }
}

impl From<Energy> for InitialEnergy {
    fn from(value: Energy) -> Self {
        Self(value)
    }
}

impl From<u8> for InitialEnergy {
    fn from(value: u8) -> Self {
        Self(value.into())
    }
}

impl From<InitialEnergy> for Energy {
    fn from(value: InitialEnergy) -> Self {
        value.0
    }
}

impl fmt::Display for InitialEnergy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for InitialEnergy {
    type Err = <Energy as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Energy::from_str(s).map(Self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Resource)]
#[repr(transparent)]
pub struct MaxEnergy(pub Energy);

impl Default for MaxEnergy {
    fn default() -> Self {
        10.into()
    }
}

impl From<u8> for MaxEnergy {
    fn from(value: u8) -> Self {
        Self(value.into())
    }
}

impl From<Energy> for MaxEnergy {
    fn from(value: Energy) -> Self {
        Self(value)
    }
}

impl fmt::Display for MaxEnergy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for MaxEnergy {
    type Err = <Energy as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Energy::from_str(s).map(Self)
    }
}

#[derive(Debug, Clone, Copy)]
#[must_use]
pub struct DecrementResult<T> {
    value: T,
    deceased: bool,
}

impl<T> DecrementResult<T> {
    pub const fn value(&self) -> &T {
        &self.value
    }

    pub const fn value_mut(&mut self) -> &mut T {
        &mut self.value
    }

    pub const fn is_deceased(&self) -> bool {
        self.deceased
    }

    pub fn into_value(self) -> T {
        self.value
    }
}

impl Energy {
    pub fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    pub fn increment(&mut self, ValueAsFood(value): ValueAsFood, MaxEnergy(max): MaxEnergy) {
        *self = Ord::min(max, self.saturating_add(value));
    }

    pub fn decrement(&mut self) -> DecrementResult<()> {
        *self = self.saturating_sub(1.into());

        DecrementResult {
            value: (),
            deceased: self.0 == 0,
        }
    }

    pub fn distribute(&mut self) -> DecrementResult<Energy> {
        let start = self.0;
        self.0 /= 2;
        let child_energy = Self(start - self.0);

        DecrementResult {
            value: child_energy,
            deceased: self.0 == 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
#[repr(transparent)]
pub struct ValueAsFood(pub Energy);

impl ValueAsFood {
    pub const fn per_tick(self) -> Energy {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct Generation(NonZeroU32);

impl Default for Generation {
    fn default() -> Self {
        Self(NonZeroU32::MIN)
    }
}

impl Generation {
    pub const fn number(self) -> u32 {
        self.0.get()
    }

    pub fn next(self) -> Self {
        Self(NonZeroU32::new(self.0.get() + 1).unwrap())
    }
}
