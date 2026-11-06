use std::sync::{
    Arc,
    LazyLock,
};

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
#[derive(Debug, Clone, Component)]
#[component(immutable)]
pub struct SensorOffsets {
    /// Front far sight sensors
    front: Arc<[I16Vec2]>,
    /// Left far sight sensors
    left: Arc<[I16Vec2]>,
    /// Right far sight sensors
    right: Arc<[I16Vec2]>,
    /// Near sight sensors.
    ///
    /// Agent can interact with the near zone
    near: Arc<[I16Vec2]>,
}

/// ```text
/// FFFFFFFFF
/// FFFFFFFFF
/// LFFFFFFFR
/// LLFFFFFRR
/// LLLFFFRRR
/// LLLNNNRRR
/// LLLNONRRR
/// ```
pub static SENSORS_PREDATOR: LazyLock<SensorOffsets> = LazyLock::new(|| {
    #[rustfmt::skip]
    let front = Arc::new(
        [
            [-4, 6], [-3, 6], [-2, 6], [-1, 6], [0, 6], [1, 6], [2, 6], [3, 6], [4, 6],
            [-4, 5], [-3, 5], [-2, 5], [-1, 5], [0, 5], [1, 5], [2, 5], [3, 5], [4, 5],
                     [-3, 4], [-2, 4], [-1, 4], [0, 4], [1, 4], [2, 4], [3, 4],
                              [-2, 3], [-1, 3], [0, 3], [1, 3], [2, 3],
                                       [-1, 2], [0, 2], [1, 2],
        ]
        .map(I16Vec2::from_array),
    );

    #[rustfmt::skip]
    let left = Arc::new(
        [
            [-4, 4],
            [-4, 3], [-3, 3],
            [-4, 2], [-3, 2], [-2, 2],
            [-4, 1], [-3, 1], [-2, 1],
            [-4, 0], [-3, 0], [-2, 0],
        ]
        .map(I16Vec2::from_array),
    );

    #[rustfmt::skip]
    let right = Arc::new(
        [
                            [4, 4],
                    [3, 3], [4, 3],
            [2, 2], [3, 2], [4, 2],
            [2, 1], [3, 1], [4, 1],
            [2, 0], [3, 0], [4, 0],
        ]
        .map(I16Vec2::from_array),
    );

    #[rustfmt::skip]
    let near = Arc::new(
        [
            [-1, 1], [0, 1], [1, 1],
            [-1, 0], [0, 0], [1, 0],
        ]
        .map(I16Vec2::from_array),
    );

    SensorOffsets {
        front,
        left,
        right,
        near,
    }
});

/// ```text
/// LLFFXXXXXFFRR
/// LLLFFXXXFFRRR
/// LLLLFFFFFRRRR
/// LLLLLFFFRRRRR
/// LLLLLNNNRRRRR
/// LLLLLNONRRRRR
/// LLLLLXXXRRRRR
/// LLLLXXXXXRRRR
/// LLLXXXXXXXRRR
/// ```
pub static SENSORS_HERBIVORE: LazyLock<SensorOffsets> = LazyLock::new(|| {
    #[rustfmt::skip]
    let front = Arc::new(
        [
            [-4, 5], [-3, 5],                                           [3, 5], [4, 5],
                     [-3, 4], [-2, 4],                          [2, 4], [3, 4],
                              [-2, 3], [-1, 3], [0, 3], [1, 3], [2, 3],
                                       [-1, 2], [0, 2], [1, 2],
        ]
        .map(I16Vec2::from_array),
    );

    #[rustfmt::skip]
    let left = Arc::new(
        [
            [-6,  5], [-5,  5],
            [-6,  4], [-5,  4], [-4,  4],
            [-6,  3], [-5,  3], [-4,  3], [-3,  3],
            [-6,  2], [-5,  2], [-4,  2], [-3,  2], [-2,  2],
            [-6,  1], [-5,  1], [-4,  1], [-3,  1], [-2,  1],
            [-6,  0], [-5,  0], [-4,  0], [-3,  0], [-2,  0],
            [-6, -1], [-5, -1], [-4, -1], [-3, -1], [-2, -1],
            [-6, -2], [-5, -2], [-4, -2], [-3, -2],
            [-6, -3], [-5, -3], [-4, -3],
        ]
        .map(I16Vec2::from_array),
    );

    #[rustfmt::skip]
    let right = Arc::new(
        [
                                       [5,  5], [6,  5],
                              [4,  4], [5,  4], [6,  4],
                     [3,  3], [4,  3], [5,  3], [6,  3],
            [2,  2], [3,  2], [4,  2], [5,  2], [6,  2],
            [2,  1], [3,  1], [4,  1], [5,  1], [6,  1],
            [2,  0], [3,  0], [4,  0], [5,  0], [6,  0],
            [2, -1], [3, -1], [4, -1], [5, -1], [6, -1],
                     [3, -2], [4, -2], [5, -2], [6, -2],
                              [4, -3], [5, -3], [6, -3],
        ]
        .map(I16Vec2::from_array),
    );

    #[rustfmt::skip]
    let near = Arc::new(
        [
            [-1, 1], [0, 1], [1, 1],
            [-1, 0], [0, 0], [1, 0],
        ]
        .map(I16Vec2::from_array),
    );

    SensorOffsets {
        front,
        left,
        right,
        near,
    }
});

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

    /// Returns the inclusive `(min, max)` ego-centric tile coordinates covering `(0, 0)` and all sensor zones.
    pub fn bounds(&self) -> (I16Vec2, I16Vec2) {
        self.front
            .iter()
            .chain(self.left.iter())
            .chain(self.right.iter())
            .chain(self.near.iter())
            .fold((I16Vec2::ZERO, I16Vec2::ZERO), |(min, max), &offset| {
                (min.min(offset), max.max(offset))
            })
    }
}

impl Default for SensorOffsets {
    fn default() -> Self {
        let front = (-2..=2).map(|x| I16Vec2 { x, y: 2 }).collect();
        let left = Arc::new([I16Vec2 { y: 0, x: -2 }, I16Vec2 { y: 1, x: -2 }]);
        let right = Arc::new([I16Vec2 { y: 0, x: 2 }, I16Vec2 { y: 1, x: 2 }]);
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
