use core::fmt;
use core::num::NonZeroU32;
use core::ops::Add;
use core::str::FromStr;

use bevy::prelude::*;
use serde::{
    Deserialize,
    Serialize,
};

use super::brain::{
    Action,
    Brain,
};

/// Query filter matching any living character organism ([`Herbivore`] or [`Predator`]).
pub type CharacterFilter = Or<(With<Herbivore>, With<Predator>)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(ValueAsFood(1.0.into()))]
#[component(immutable)]
pub struct Plant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    ValueAsFood(2.0.into()),
    Generation,
    Brain = rand::random::<Brain>(),
)]
#[component(immutable)]
pub struct Herbivore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    Generation,
    Brain = rand::random::<Brain>(),
)]
#[component(immutable)]
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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Default, Component, Serialize, Deserialize)]
#[serde(rename = "energy")]
#[repr(transparent)]
pub struct Energy(pub f32);

impl From<f32> for Energy {
    fn from(value: f32) -> Self {
        Self(value)
    }
}

impl From<u8> for Energy {
    fn from(value: u8) -> Self {
        Self(f32::from(value))
    }
}

impl From<Energy> for f32 {
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
    type Err = <f32 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        f32::from_str(s).map(Self)
    }
}

impl Add for Energy {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Resource, Serialize, Deserialize)]
#[serde(rename = "initial_energy")]
#[repr(transparent)]
pub struct InitialEnergy(pub Energy);

impl Default for InitialEnergy {
    fn default() -> Self {
        5.0.into()
    }
}

impl From<Energy> for InitialEnergy {
    fn from(value: Energy) -> Self {
        Self(value)
    }
}

impl From<f32> for InitialEnergy {
    fn from(value: f32) -> Self {
        Self(value.into())
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

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Resource, Serialize, Deserialize)]
#[serde(rename = "max_energy")]
#[repr(transparent)]
pub struct MaxEnergy(pub Energy);

impl Default for MaxEnergy {
    fn default() -> Self {
        10.0.into()
    }
}

impl From<f32> for MaxEnergy {
    fn from(value: f32) -> Self {
        Self(value.into())
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

/// Configurable energy costs for organism actions during [`SimulationSystems::Step`](super::SimulationSystems::Step).
#[derive(
    Debug, Clone, Copy, PartialEq, PartialOrd, Resource, Serialize, Deserialize, bon::Builder,
)]
#[serde(rename = "action_costs", rename_all = "snake_case", default)]
pub struct ActionCosts {
    /// Energy cost for [`Action::TurnLeft`] and [`Action::TurnRight`].
    #[builder(into, default = Energy(1.0))]
    pub turn: Energy,
    /// Energy cost for [`Action::GoForward`].
    #[builder(into, default = Energy(1.0))]
    pub go_forward: Energy,
    /// Energy cost for [`Action::Consume`].
    #[builder(into, default = Energy(1.0))]
    pub consume: Energy,
}

impl Default for ActionCosts {
    fn default() -> Self {
        Self {
            turn: Energy(1.0),
            go_forward: Energy(1.0),
            consume: Energy(1.0),
        }
    }
}

impl ActionCosts {
    #[must_use]
    pub const fn cost_of(self, action: Action) -> Energy {
        match action {
            Action::TurnLeft | Action::TurnRight => self.turn,
            Action::GoForward => self.go_forward,
            Action::Consume => self.consume,
        }
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.turn.0 >= 0.0 && self.go_forward.0 >= 0.0 && self.consume.0 >= 0.0
    }
}

/// Configurable nutritional energy values gained when consuming [`Plant`]s or [`Herbivore`]s.
#[derive(
    Debug, Clone, Copy, PartialEq, PartialOrd, Resource, Serialize, Deserialize, bon::Builder,
)]
#[serde(rename = "food_values", rename_all = "snake_case", default)]
pub struct FoodValues {
    /// Energy gained by a [`Herbivore`] when consuming a [`Plant`].
    #[builder(into, default = Energy(1.0))]
    pub plant: Energy,
    /// Energy gained by a [`Predator`] when consuming a [`Herbivore`].
    #[builder(into, default = Energy(2.0))]
    pub herbivore: Energy,
}

impl Default for FoodValues {
    fn default() -> Self {
        Self {
            plant: Energy(1.0),
            herbivore: Energy(2.0),
        }
    }
}

impl FoodValues {
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.plant.0 >= 0.0 && self.herbivore.0 >= 0.0
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
    #[must_use]
    pub fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0)
    }

    #[must_use]
    pub fn saturating_sub(self, rhs: Self) -> Self {
        Self((self.0 - rhs.0).max(0.0))
    }

    pub fn increment(&mut self, ValueAsFood(value): ValueAsFood, MaxEnergy(max): MaxEnergy) {
        self.0 = (self.0 + value.0).min(max.0);
    }

    pub fn decrement(&mut self, cost: Energy) -> DecrementResult<()> {
        *self = self.saturating_sub(cost);

        DecrementResult {
            value: (),
            deceased: self.0 <= 0.0,
        }
    }

    pub fn distribute(&mut self) -> DecrementResult<Energy> {
        let half = self.0 * 0.5;
        self.0 = half;

        DecrementResult {
            value: Self(half),
            deceased: self.0 <= 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Component, Serialize, Deserialize)]
#[serde(rename = "value_as_food")]
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
