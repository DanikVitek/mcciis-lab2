use bevy::math::I16Vec2;
use bevy::prelude::*;
use itertools::Itertools;
use nalgebra::{
    ArrayStorage,
    Const,
    ReshapableStorage,
    SMatrix,
    SVector,
};
use rand::Rng;
use rand::distr::{
    Distribution,
    StandardUniform,
    Uniform,
};

use super::organism::EntityKind;
use super::spatial::SpatialGrid;
use crate::grid::{
    Facing,
    GridPosition,
};

/// Base offsets for the sensors, as if the agent is [facing](Facing) [North](Facing::North)
#[derive(Debug, Component)]
#[component(immutable)]
pub struct SensorOffsets {
    /// Front far sight sensors
    front: Box<[I16Vec2]>,
    /// Left far sight sensors
    left: Box<[I16Vec2]>,
    /// Right far sight sensors
    right: Box<[I16Vec2]>,
    /// Near sight sensors.
    ///
    /// Agent can interact with the near zone
    near: Box<[I16Vec2]>,
}

impl SensorOffsets {
    pub fn front(&self) -> &[I16Vec2] {
        &self.front
    }

    pub fn left(&self) -> &[I16Vec2] {
        &self.left
    }

    pub fn right(&self) -> &[I16Vec2] {
        &self.right
    }

    pub fn near(&self) -> &[I16Vec2] {
        &self.near
    }
}

impl Default for SensorOffsets {
    fn default() -> Self {
        let front = (-2..=2).map(|x| I16Vec2 { x, y: 2 }).collect();
        let left = Box::new([I16Vec2 { y: 0, x: -2 }, I16Vec2 { y: 1, x: -2 }]);
        let right = Box::new([I16Vec2 { y: 0, x: 2 }, I16Vec2 { y: 1, x: 2 }]);
        let near = (-1..=1)
            .cartesian_product(0..=1)
            .map(|(x, y)| I16Vec2 { x, y })
            .collect();
        Self {
            front,
            left,
            right,
            near,
        }
    }
}

#[derive(Debug, Clone, Component)]
#[component(immutable)]
#[require(SensorOffsets, PreparedAction)]
pub struct Brain {
    weights: BrainWeights,
    biases: BrainBiases,
}

pub const INPUTS: usize = 12;
pub const OUTPUTS: usize = 4;
pub type BrainWeights = SMatrix<f32, OUTPUTS, INPUTS>;
pub type BrainBiases = SVector<f32, OUTPUTS>;
pub type BrainInputs = SVector<f32, INPUTS>;
pub type BrainOutputs = SVector<f32, OUTPUTS>;

impl Distribution<Brain> for StandardUniform {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Brain {
        let distr = Uniform::new_inclusive(-1.0, 1.0).unwrap();
        Brain {
            weights: BrainWeights::from_distribution(&distr, rng),
            biases: BrainBiases::from_distribution(&distr, rng),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    TurnLeft,
    TurnRight,
    GoForward,
    Consume,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
pub struct PreparedAction(pub Option<Action>);

impl Action {
    pub const ALL: [Self; OUTPUTS] = [
        Self::TurnLeft,
        Self::TurnRight,
        Self::GoForward,
        Self::Consume,
    ];

    pub fn from_outputs(outputs: BrainOutputs) -> Self {
        let argmax = outputs.data.0[0]
            .into_iter()
            .position_max_by(|a, b| a.total_cmp(b))
            .unwrap();
        Self::ALL[argmax]
    }
}

impl Brain {
    pub const fn weights(&self) -> &BrainWeights {
        &self.weights
    }

    pub const fn weights_mut(&mut self) -> &mut BrainWeights {
        &mut self.weights
    }

    pub const fn biases(&self) -> &BrainBiases {
        &self.biases
    }

    pub const fn biases_mut(&mut self) -> &mut BrainBiases {
        &mut self.biases
    }

    pub fn compute(&self, inputs: BrainInputs) -> BrainOutputs {
        // o_j = b_j + \sum_{i=0}^{12}{u_i\cdot w_{ij}}
        self.weights * inputs + self.biases
    }

    pub fn act_on(&self, inputs: BrainInputs) -> Action {
        Action::from_outputs(self.compute(inputs))
    }
}

/// Computes all 12 normalized inputs (4 zones × 3 entity kinds) for [`Brain::act_on`].
pub fn sense_brain_inputs(
    spatial_grid: &SpatialGrid,
    origin: GridPosition,
    facing: Facing,
    self_kind: EntityKind,
    sensors: &SensorOffsets,
) -> BrainInputs {
    #[inline]
    fn normalized_count(n: u32, len: usize) -> f32 {
        let res = f64::from(n) / f64::from(u16::try_from(len).unwrap());
        res as f32
    }

    let front = sensors.front();
    let left = sensors.left();
    let right = sensors.right();
    let near = sensors.near();

    let f = spatial_grid
        .count_zone(origin, facing, self_kind, front)
        .map(|n| normalized_count(n, front.len()))
        .into_array();
    let l = spatial_grid
        .count_zone(origin, facing, self_kind, left)
        .map(|n| normalized_count(n, left.len()))
        .into_array();
    let r = spatial_grid
        .count_zone(origin, facing, self_kind, right)
        .map(|n| normalized_count(n, right.len()))
        .into_array();
    let n = spatial_grid
        .count_zone(origin, facing, self_kind, near)
        .map(|n| normalized_count(n, near.len()))
        .into_array();

    BrainInputs::from_array_storage(ArrayStorage([f, l, r, n]).reshape_generic(Const, Const))
}
