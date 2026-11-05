use core::{fmt, num::NonZeroU32};

use bevy::prelude::*;

use super::brain::Brain;

/// Query filter matching any living character organism ([`Herbivore`] or [`Predator`]).
pub type CharacterFilter = Or<(With<Herbivore>, With<Predator>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(ValueAsFood(1))]
pub struct Plant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    ValueAsFood(2),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct Energy(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Resource)]
pub struct InitialEnergy(pub u8);

impl Default for InitialEnergy {
    fn default() -> Self {
        Self(5)
    }
}

impl From<u8> for InitialEnergy {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<InitialEnergy> for Energy {
    fn from(value: InitialEnergy) -> Self {
        Self(value.0)
    }
}

impl fmt::Display for InitialEnergy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Resource)]
pub struct MaxEnergy(pub u8);

impl Default for MaxEnergy {
    fn default() -> Self {
        Self(10)
    }
}

impl From<u8> for MaxEnergy {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl fmt::Display for MaxEnergy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
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
    pub const REPRO_THRESHOLD: f32 = 0.9;

    pub const fn current(self) -> u8 {
        self.0
    }

    pub fn increment(&mut self, ValueAsFood(value): ValueAsFood, MaxEnergy(max): MaxEnergy) {
        self.0 = u8::min(max, self.0.saturating_add(value));
    }

    pub fn decrement(&mut self) -> DecrementResult<()> {
        self.0 = self.0.saturating_sub(1);

        DecrementResult {
            value: (),
            deceased: self.0 == 0,
        }
    }

    pub const fn is_enough_to_reproduce(self, MaxEnergy(max): MaxEnergy) -> bool {
        self.0 >= (max as f32 * Self::REPRO_THRESHOLD) as u8
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
pub struct ValueAsFood(pub u8);

impl ValueAsFood {
    pub const fn per_tick(self) -> u8 {
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
