use bevy::math::I16Vec2;
use bevy::prelude::*;
use itertools::Itertools;
use nalgebra::{SMatrix, SVector};
use rand::{
    Rng, RngExt,
    distr::{Distribution, StandardUniform},
};

use crate::grid::{Facing, GridPosition, GridSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
pub struct Plant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(Brain = rand::random::<Brain>())]
pub struct Herbivore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(Brain = rand::random::<Brain>())]
pub struct Predator;

/// Base offsets for the sensors, as if the agent for [facing](Facing) [North](Facing::North)
#[derive(Debug, Component)]
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

impl Default for SensorOffsets {
    fn default() -> Self {
        let front = (-2..=2).map(|x| I16Vec2 { x, y: 2 }).collect();
        let left = Box::new([I16Vec2 { x: 0, y: -2 }, I16Vec2 { x: 1, y: -2 }]);
        let right = Box::new([I16Vec2 { x: 0, y: 2 }, I16Vec2 { x: 1, y: 2 }]);
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
#[require(SensorOffsets)]
pub struct Brain {
    weights: BrainWeights,
    biases: BrainBiases,
}

pub const INPUTS: usize = 12;
pub const OUTPUTS: usize = 4;
type BrainWeights = SMatrix<f32, OUTPUTS, INPUTS>;
type BrainBiases = SVector<f32, OUTPUTS>;
pub type BrainInputs = SVector<f32, INPUTS>;
pub type BrainOutputs = SVector<f32, OUTPUTS>;

impl Distribution<Brain> for StandardUniform {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> Brain {
        Brain {
            weights: rng.random(),
            biases: rng.random(),
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

impl Brain {
    pub fn compute(&self, inputs: BrainInputs) -> BrainOutputs {
        // o_j = b_j + \sum_{i=0}^{12}{u_i\cdot w_{ij}}
        self.weights * inputs + self.biases
    }

    pub fn act_on(&self, inputs: BrainInputs) -> Action {
        let outputs = self.compute(inputs).data.0[0];
        let argmax = outputs
            .into_iter()
            .position_max_by(|a, b| a.total_cmp(b))
            .unwrap();
        match argmax {
            0 => Action::TurnLeft,
            1 => Action::TurnRight,
            2 => Action::GoForward,
            3 => Action::Consume,
            _ => unreachable!(),
        }
    }
}

pub fn update_positions(
    query: Query<(&mut GridPosition, &Facing), With<Brain>>,
    field_size: Res<GridSize>,
) {
    for (mut pos, facing) in query {
        pos.add_assign_wrapped_direction(*facing, *field_size);
    }
}
