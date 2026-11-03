use core::num::NonZeroU32;

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
    Energy,
    ValueAsFood(2),
    Generation,
    Brain = rand::random::<Brain>(),
)]
pub struct Herbivore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    Energy,
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

impl Default for Energy {
    fn default() -> Self {
        Self(50)
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
    pub const MAX: Self = Self(100);
    pub const REPRODUCTION_THRESHOLD: u8 = 90;

    pub const fn current(self) -> u8 {
        self.0
    }

    pub fn increment(&mut self, ValueAsFood(value): ValueAsFood) {
        self.0 = Self::MAX.0.min(self.0 + value);
    }

    pub fn decrement(&mut self) -> DecrementResult<()> {
        self.0 = self.0.saturating_sub(1);

        DecrementResult {
            value: (),
            deceased: self.0 == 0,
        }
    }

    pub const fn is_enough_to_reproduce(self) -> bool {
        self.0 >= Self::REPRODUCTION_THRESHOLD
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
