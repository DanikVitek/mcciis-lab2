use core::{
    iter::Sum,
    num::NonZeroU32,
    ops::{AddAssign, Index, IndexMut},
};

use bevy::math::I16Vec2;
use bevy::prelude::*;
use itertools::Itertools;
use nalgebra::{ArrayStorage, Const, ReshapableStorage, SMatrix, SVector};
use rand::{
    Rng, RngExt,
    distr::{Distribution, StandardUniform},
};

use crate::grid::{Facing, GridPosition, GridSize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
pub struct Plant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    Energy,
    MetabolismRate(2),
    Generation,
    Brain = rand::random::<Brain>(),
)]
pub struct Herbivore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
#[require(
    Age,
    Energy,
    MetabolismRate(1),
    Generation,
    Brain = rand::random::<Brain>(),
)]
pub struct Predator;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Component)]
pub struct Age(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct Energy(u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct Generation(NonZeroU32);

impl Default for Generation {
    fn default() -> Self {
        Self(NonZeroU32::MIN)
    }
}

impl Generation {
    pub fn next(self) -> Self {
        Self(NonZeroU32::new(self.0.get() + 1).unwrap())
    }
}

impl Default for Energy {
    fn default() -> Self {
        Self(50)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DecrementResult<T> {
    CarriesOn(T),
    Deceased(T),
}

impl<T> DecrementResult<T> {
    pub fn into_value(self) -> T {
        match self {
            Self::CarriesOn(v) => v,
            Self::Deceased(v) => v,
        }
    }
}

impl Energy {
    const MAX: Self = Self(100);

    pub fn increment(&mut self) {
        self.0 = Self::MAX.0.min(self.0 + 1);
    }

    /// # Returns
    /// Returns `true` if reached zero
    pub fn decrement(&mut self, rate: MetabolismRate) -> DecrementResult<()> {
        self.0 = self.0.saturating_sub(rate.0);
        if self.0 == 0 {
            DecrementResult::Deceased(())
        } else {
            DecrementResult::CarriesOn(())
        }
    }

    pub fn is_enough_to_reproduce(self) -> bool {
        self.0 >= 90
    }

    pub fn distribute(&mut self) -> DecrementResult<Energy> {
        let start = self.0;
        self.0 /= 2;
        let child_energy = Self(start - self.0);
        if self.0 == 0 {
            DecrementResult::Deceased(child_energy)
        } else {
            DecrementResult::CarriesOn(child_energy)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Component)]
pub struct MetabolismRate(u8);

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

    pub fn mutated<R: Rng>(&self, rng: &mut R) -> Self {
        let what_to_mutate = rng.random_range(0..((INPUTS + 1) * OUTPUTS));
        if (0..(INPUTS * OUTPUTS)).contains(&what_to_mutate) {
            let mut weights = self.weights;

            weights[what_to_mutate] = rng.random();

            Self {
                weights,
                biases: self.biases,
            }
        } else {
            let mut biases = self.biases;

            biases[what_to_mutate - INPUTS * OUTPUTS] = rng.random();

            Self {
                weights: self.weights,
                biases,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EntityKind {
    Plant,
    Herbivore,
    Predator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Resource)]
pub struct CharacterCounts {
    pub herbivores: u32,
    pub predators: u32,
}

impl<T> From<CellCounts<T>> for CharacterCounts
where
    u32: From<T>,
{
    fn from(value: CellCounts<T>) -> Self {
        Self {
            herbivores: value.herbivores.into(),
            predators: value.predators.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Resource)]
struct CellCounts<T> {
    plants: T,
    herbivores: T,
    predators: T,
}

impl<T> CellCounts<T> {
    pub fn map<F, U>(self, mut f: F) -> CellCounts<U>
    where
        F: FnMut(T) -> U,
    {
        CellCounts {
            plants: f(self.plants),
            herbivores: f(self.herbivores),
            predators: f(self.predators),
        }
    }

    pub fn into_array(self) -> [T; 3] {
        [self.plants, self.herbivores, self.predators]
    }
}

impl<T> Index<EntityKind> for CellCounts<T> {
    type Output = T;

    fn index(&self, index: EntityKind) -> &Self::Output {
        match index {
            EntityKind::Plant => &self.plants,
            EntityKind::Herbivore => &self.herbivores,
            EntityKind::Predator => &self.predators,
        }
    }
}

impl<T> IndexMut<EntityKind> for CellCounts<T> {
    fn index_mut(&mut self, index: EntityKind) -> &mut Self::Output {
        match index {
            EntityKind::Plant => &mut self.plants,
            EntityKind::Herbivore => &mut self.herbivores,
            EntityKind::Predator => &mut self.predators,
        }
    }
}

impl<T, U> AddAssign<CellCounts<U>> for CellCounts<T>
where
    T: From<U>,
    T: AddAssign,
{
    fn add_assign(&mut self, rhs: CellCounts<U>) {
        self.plants += T::from(rhs.plants);
        self.herbivores += T::from(rhs.herbivores);
        self.predators += T::from(rhs.predators);
    }
}

impl<T, U> Sum<CellCounts<U>> for CellCounts<T>
where
    Self: AddAssign<CellCounts<U>> + Default,
{
    fn sum<I: Iterator<Item = CellCounts<U>>>(iter: I) -> Self {
        iter.fold(Self::default(), |mut acc, item| {
            acc += item;
            acc
        })
    }
}

impl<T> AddAssign<CellCounts<T>> for CharacterCounts
where
    u32: From<T>,
{
    fn add_assign(&mut self, rhs: CellCounts<T>) {
        self.herbivores += u32::from(rhs.herbivores);
        self.predators += u32::from(rhs.predators);
    }
}

impl<T> Sum<CellCounts<T>> for CharacterCounts
where
    Self: AddAssign<CellCounts<T>> + Default,
{
    fn sum<I: Iterator<Item = CellCounts<T>>>(iter: I) -> Self {
        iter.fold(Self::default(), |mut acc, item| {
            acc += item;
            acc
        })
    }
}

#[derive(Resource)]
pub struct SpatialGrid {
    size: GridSize,
    /// Hot array for `Brain` inputs: `[plants, herbivores, predators]` per tile
    counts: Vec<CellCounts<u16>>,
    /// Cold array for `Action::Consume`: entities currently standing on each tile
    entities: Vec<Vec<(Entity, EntityKind)>>,
}

impl SpatialGrid {
    pub fn new(size: GridSize) -> Self {
        let len = usize::from(size.x) * usize::from(size.y);
        Self {
            size,
            counts: vec![CellCounts::default(); len],
            entities: vec![Vec::new(); len],
        }
    }

    #[inline]
    fn idx(&self, pos: GridPosition) -> usize {
        usize::from(pos.y) * usize::from(self.size.x) + usize::from(pos.x)
    }

    pub fn clear(&mut self) {
        self.counts.fill(CellCounts::default());
        for cell in &mut self.entities {
            cell.clear();
        }
    }

    pub fn insert(&mut self, pos: GridPosition, entity: Entity, kind: EntityKind) {
        let i = self.idx(pos);
        self.counts[i][kind] += 1;
        self.entities[i].push((entity, kind));
    }

    pub fn remove(&mut self, pos: GridPosition, entity: Entity, kind: EntityKind) -> bool {
        let i = self.idx(pos);
        let cell = &mut self.entities[i];
        if let Some(pos_in_vec) = cell.iter().position(|&(e, _)| e == entity) {
            cell.swap_remove(pos_in_vec);
            self.counts[i][kind] -= 1;
            true
        } else {
            false
        }
    }

    #[track_caller]
    pub fn move_entity(
        &mut self,
        from: GridPosition,
        to: GridPosition,
        entity: Entity,
        kind: EntityKind,
    ) {
        if from == to {
            return;
        }
        let removed = self.remove(from, entity, kind);
        debug_assert!(removed, "Moved entity was not registered at `from` tile");
        self.insert(to, entity, kind);
    }

    /// Sums `[Plant, Herbivore, Predator]` counts across a slice of local sensor offsets.
    /// Excludes the querying agent itself if an offset is `(0, 0)`.
    fn count_zone(
        &self,
        origin: GridPosition,
        facing: Facing,
        self_kind: EntityKind,
        offsets: &[I16Vec2],
    ) -> CellCounts<u32> {
        offsets
            .iter()
            .copied()
            .map(|offset| {
                let target = origin.add_wrapping_rotated_offset(offset, facing, self.size);
                let mut cell_counts = self.counts[self.idx(target)];

                // Do not count the sensing character itself on its own tile (0, 0) in `near`
                if offset == I16Vec2::ZERO {
                    cell_counts[self_kind] = cell_counts[self_kind].saturating_sub(1);
                }

                cell_counts
            })
            .sum()
    }

    /// Computes all 12 inputs (4 zones × 3 entity kinds) for `Brain::act_on`.
    pub fn sense_brain_inputs(
        &self,
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

        let f = self
            .count_zone(origin, facing, self_kind, &sensors.front)
            .map(|n| normalized_count(n, sensors.front.len()))
            .into_array();
        let l = self
            .count_zone(origin, facing, self_kind, &sensors.left)
            .map(|n| normalized_count(n, sensors.left.len()))
            .into_array();
        let r = self
            .count_zone(origin, facing, self_kind, &sensors.right)
            .map(|n| normalized_count(n, sensors.right.len()))
            .into_array();
        let n = self
            .count_zone(origin, facing, self_kind, &sensors.near)
            .map(|n| normalized_count(n, sensors.near.len()))
            .into_array();

        BrainInputs::from_array_storage(ArrayStorage([f, l, r, n]).reshape_generic(Const, Const))
    }

    /// Finds the first entity of `target_kind` in the agent's `near` zone for `Action::Consume`.
    pub fn find_target_in_near(
        &self,
        origin: GridPosition,
        facing: Facing,
        near_offsets: &[I16Vec2],
        target_kind: EntityKind,
        exclude_self: Entity,
    ) -> Option<(Entity, GridPosition)> {
        for &offset in near_offsets {
            let target_pos = origin.add_wrapping_rotated_offset(offset, facing, self.size);
            let i = self.idx(target_pos);
            if self.counts[i][target_kind] == 0 {
                continue;
            }
            for &(entity, kind) in &self.entities[i] {
                if kind == target_kind && entity != exclude_self {
                    return Some((entity, target_pos));
                }
            }
        }
        None
    }

    pub fn character_counts(&self) -> CharacterCounts {
        self.counts.iter().copied().sum()
    }
}

#[allow(clippy::type_complexity)]
pub mod system {
    use bevy::ecs::entity::EntityHashSet;
    use rand::seq::IndexedRandom;

    use super::*;

    pub fn step_characters(
        mut commands: Commands,
        mut spatial_grid: ResMut<SpatialGrid>,
        field_size: Res<GridSize>,
        characters: Query<
            (
                Entity,
                &mut GridPosition,
                &mut Facing,
                &mut Energy,
                &mut Age,
                &Brain,
                &SensorOffsets,
                &MetabolismRate,
                Has<Herbivore>,
            ),
            Without<Plant>,
        >,
        mut plant_positions: Query<&mut GridPosition, With<Plant>>,
    ) {
        let mut consumed_herbivores = EntityHashSet::new();

        for (
            entity,
            mut pos,
            mut facing,
            mut energy,
            mut age,
            brain,
            sensors,
            metabolism,
            is_herbivore,
        ) in characters
        {
            let self_kind = if is_herbivore {
                if consumed_herbivores.contains(&entity) {
                    continue;
                }
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };

            let inputs = spatial_grid.sense_brain_inputs(*pos, *facing, self_kind, sensors);

            match brain.act_on(inputs) {
                Action::TurnLeft => {
                    if let DecrementResult::Deceased(()) = energy.decrement(*metabolism) {
                        spatial_grid.remove(*pos, entity, self_kind);
                        commands.entity(entity).despawn();
                    } else {
                        *facing = facing.counterclockwise();
                        age.0 += 1;
                    }
                }
                Action::TurnRight => {
                    if let DecrementResult::Deceased(()) = energy.decrement(*metabolism) {
                        spatial_grid.remove(*pos, entity, self_kind);
                        commands.entity(entity).despawn();
                    } else {
                        *facing = facing.clockwise();
                        age.0 += 1;
                    }
                }
                Action::GoForward => {
                    if let DecrementResult::Deceased(()) = energy.decrement(*metabolism) {
                        spatial_grid.remove(*pos, entity, self_kind);
                        commands.entity(entity).despawn();
                    } else {
                        let old_pos = *pos;
                        pos.add_assign_wrapping_direction(*facing, *field_size);
                        spatial_grid.move_entity(old_pos, *pos, entity, self_kind);
                        age.0 += 1;
                    }
                }
                Action::Consume => {
                    let target_kind = if is_herbivore {
                        EntityKind::Plant
                    } else {
                        EntityKind::Herbivore
                    };
                    if let Some((target_entity, target_pos)) = spatial_grid.find_target_in_near(
                        *pos,
                        *facing,
                        &sensors.near,
                        target_kind,
                        entity,
                    ) {
                        if target_kind == EntityKind::Plant {
                            let new_pos = GridPosition::new(
                                rand::random_range(0..field_size.width()),
                                rand::random_range(0..field_size.height()),
                            );
                            *plant_positions.get_mut(target_entity).unwrap() = new_pos;
                            spatial_grid.move_entity(
                                target_pos,
                                new_pos,
                                target_entity,
                                target_kind,
                            );
                        } else {
                            commands.entity(target_entity).despawn();
                            spatial_grid.remove(target_pos, target_entity, target_kind);
                            consumed_herbivores.insert(target_entity);
                        }

                        energy.increment();
                    } else if let DecrementResult::Deceased(()) = energy.decrement(*metabolism) {
                        spatial_grid.remove(*pos, entity, self_kind);
                        commands.entity(entity).despawn();
                        continue;
                    }
                    age.0 += 1;
                }
            }
        }
    }

    pub fn reproduction(
        mut commands: Commands,
        mut spatial_grid: ResMut<SpatialGrid>,
        field_size: Res<GridSize>,
        query: Query<(
            Entity,
            &mut Energy,
            &Brain,
            &GridPosition,
            &Facing,
            &Generation,
            Has<Herbivore>,
        )>,
    ) {
        let mut rng = rand::rng();
        for (entity, mut energy, brain, pos, facing, generation, is_herbivore) in query {
            if !energy.is_enough_to_reproduce() {
                continue;
            }

            let entity_kind = if is_herbivore {
                EntityKind::Herbivore
            } else {
                EntityKind::Predator
            };

            let offset = *[
                [-1, -1],
                [-1, 0],
                [-1, 1],
                [0, -1],
                [0, 1],
                [1, -1],
                [1, 0],
                [1, 1],
            ]
            .choose(&mut rng)
            .unwrap();

            let child_pos = pos.add_wrapping_signed(I16Vec2::from_array(offset), *field_size);
            let child_brain = brain.mutated(&mut rng);
            let child_gen = generation.next();

            let res = energy.distribute();
            let child_energy = res.into_value();

            let child = if is_herbivore {
                commands
                    .spawn((
                        Herbivore,
                        child_pos,
                        *facing,
                        child_brain,
                        child_gen,
                        child_energy,
                    ))
                    .id()
            } else {
                commands
                    .spawn((
                        Predator,
                        child_pos,
                        *facing,
                        child_brain,
                        child_gen,
                        child_energy,
                    ))
                    .id()
            };
            spatial_grid.insert(child_pos, child, entity_kind);
            if let DecrementResult::Deceased(_) = res {
                commands.entity(entity).despawn();
                spatial_grid.remove(*pos, entity, entity_kind);
            }
        }
    }
}
