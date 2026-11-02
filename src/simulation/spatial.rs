use core::{
    iter::Sum,
    ops::{AddAssign, Index, IndexMut},
};

use bevy::math::I16Vec2;
use bevy::prelude::*;

use crate::grid::{Facing, GridPosition, GridSize};

use super::organism::EntityKind;

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
pub struct CellCounts<T> {
    pub plants: T,
    pub herbivores: T,
    pub predators: T,
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
    /// Cold array for `Action::Consume` and tile inspection: entities currently standing on each tile
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

    pub const fn size(&self) -> GridSize {
        self.size
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

    pub fn resize_and_clear(&mut self, size: GridSize) {
        if self.size == size {
            self.clear();
        } else {
            *self = Self::new(size);
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

    #[cfg_attr(debug_assertions, track_caller)]
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

    /// Returns all `(Entity, EntityKind)` pairs currently occupying `pos`.
    pub fn entities_at(&self, pos: GridPosition) -> &[(Entity, EntityKind)] {
        &self.entities[self.idx(pos)]
    }

    /// Sums `[Plant, Herbivore, Predator]` counts across a slice of local sensor offsets.
    /// Excludes the querying agent itself if an offset is `(0, 0)`.
    pub fn count_zone(
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

    pub fn total_counts(&self) -> CellCounts<u32> {
        self.counts.iter().copied().sum()
    }
}
