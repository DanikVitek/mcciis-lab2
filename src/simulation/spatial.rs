use core::iter::Sum;
use core::ops::{
    AddAssign,
    Index,
    IndexMut,
};
use core::sync::atomic::{
    AtomicU16,
    AtomicU32,
    Ordering,
};

use bevy::math::I16Vec2;
use bevy::prelude::*;
use parking_lot::{
    RwLock,
    RwLockReadGuard,
};

use super::organism::EntityKind;
use crate::grid::{
    Facing,
    GridPosition,
    GridSize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Resource)]
pub struct CharacterCounts {
    pub herbivores: u32,
    pub predators: u32,
}

impl CharacterCounts {
    pub const fn total(&self) -> u32 {
        self.herbivores + self.predators
    }

    pub const fn is_empty(&self) -> bool {
        self.herbivores == 0 && self.predators == 0
    }
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
    pub const fn as_ref(&self) -> CellCounts<&T> {
        CellCounts {
            plants: &self.plants,
            herbivores: &self.herbivores,
            predators: &self.predators,
        }
    }

    pub fn as_mut(&mut self) -> CellCounts<&mut T> {
        CellCounts {
            plants: &mut self.plants,
            herbivores: &mut self.herbivores,
            predators: &mut self.predators,
        }
    }

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

    pub fn for_each<F>(self, mut f: F)
    where
        F: FnMut(T),
    {
        f(self.plants);
        f(self.herbivores);
        f(self.predators);
    }

    pub fn zip<U>(self, other: CellCounts<U>) -> CellCounts<(T, U)> {
        CellCounts {
            plants: (self.plants, other.plants),
            herbivores: (self.herbivores, other.herbivores),
            predators: (self.predators, other.predators),
        }
    }

    pub fn into_array(self) -> [T; 3] {
        [self.plants, self.herbivores, self.predators]
    }
}

impl<T> CellCounts<Option<T>> {
    pub fn transpose(self) -> Option<CellCounts<T>> {
        match (self.plants, self.herbivores, self.predators) {
            (Some(plants), Some(herbivores), Some(predators)) => Some(CellCounts {
                plants,
                herbivores,
                predators,
            }),
            (None, None, None) => None,
            _ => unreachable!("SoA columns in CellCounts have mismatched lengths"),
        }
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
    /// Running O(1) population totals across the entire grid (lock-free atomics for parallel updates).
    total_counts: CellCounts<AtomicU32>,
    /// Hot array for `Brain` inputs: `[plants, herbivores, predators]` per tile (lock-free atomics).
    counts: Vec<CellCounts<AtomicU16>>,
    /// Per-tile entity lists guarded by fine-grained per-cell reader-writer locks for concurrent access.
    entities: Vec<RwLock<Vec<(Entity, EntityKind)>>>,
}

impl SpatialGrid {
    pub fn new(size: GridSize) -> Self {
        let len = usize::from(size.x) * usize::from(size.y);
        Self {
            size,
            total_counts: CellCounts::default(),
            counts: (0..len).map(|_| CellCounts::default()).collect(),
            entities: (0..len).map(|_| RwLock::new(Vec::new())).collect(),
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
        self.total_counts.as_mut().for_each(|c| *c.get_mut() = 0);
        for cell_counts in &mut self.counts {
            cell_counts.as_mut().for_each(|c| *c.get_mut() = 0);
        }
        for cell in &mut self.entities {
            cell.get_mut().clear();
        }
    }

    pub fn resize_and_clear(&mut self, size: GridSize) {
        if self.size == size {
            self.clear();
        } else {
            *self = Self::new(size);
        }
    }

    pub fn insert(&self, pos: GridPosition, entity: Entity, kind: EntityKind) {
        let i = self.idx(pos);
        self.counts[i][kind].fetch_add(1, Ordering::Relaxed);
        self.total_counts[kind].fetch_add(1, Ordering::Relaxed);
        self.entities[i].write().push((entity, kind));
    }

    /// Atomically claims `pos` for a newborn [`EntityKind::Plant`] if `pos` currently has `0` plants
    /// and total grid plants are strictly below `max_plants`.
    ///
    /// When this returns `true`, the caller MUST register the spawned entity via [`Self::push_claimed_entity`].
    pub fn try_claim_empty_plant_tile(&self, pos: GridPosition, max_plants: u32) -> bool {
        let i = self.idx(pos);
        if self.counts[i]
            .plants
            .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        if self
            .total_counts
            .plants
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |curr| {
                (curr < max_plants).then_some(curr + 1)
            })
            .is_err()
        {
            self.counts[i].plants.store(0, Ordering::Relaxed);
            return false;
        }
        true
    }

    /// Appends `entity` to `pos`'s entity list after its cell and total counts were already incremented
    /// (e.g. via [`Self::try_claim_empty_plant_tile`]).
    pub fn push_claimed_entity(&self, pos: GridPosition, entity: Entity, kind: EntityKind) {
        let i = self.idx(pos);
        self.entities[i].write().push((entity, kind));
    }

    pub fn remove(&self, pos: GridPosition, entity: Entity, kind: EntityKind) -> bool {
        let i = self.idx(pos);
        let mut cell = self.entities[i].write();
        if let Some(pos_in_vec) = cell.iter().position(|&(e, _)| e == entity) {
            cell.swap_remove(pos_in_vec);
            drop(cell);
            self.counts[i][kind].fetch_sub(1, Ordering::Relaxed);
            self.total_counts[kind].fetch_sub(1, Ordering::Relaxed);
            true
        } else {
            false
        }
    }

    #[cfg_attr(debug_assertions, track_caller)]
    pub fn move_entity(
        &self,
        from: GridPosition,
        to: GridPosition,
        entity: Entity,
        kind: EntityKind,
    ) {
        if from == to {
            return;
        }
        let from_idx = self.idx(from);
        {
            let mut from_cell = self.entities[from_idx].write();
            let Some(pos_in_vec) = from_cell.iter().position(|&(e, _)| e == entity) else {
                debug_assert!(false, "Moved entity was not registered at `from` tile");
                return;
            };
            from_cell.swap_remove(pos_in_vec);
        }
        self.counts[from_idx][kind].fetch_sub(1, Ordering::Relaxed);

        let to_idx = self.idx(to);
        self.counts[to_idx][kind].fetch_add(1, Ordering::Relaxed);
        self.entities[to_idx].write().push((entity, kind));
    }

    /// Returns a shared read guard to all `(Entity, EntityKind)` pairs currently occupying `pos`.
    pub fn entities_at(&self, pos: GridPosition) -> RwLockReadGuard<'_, Vec<(Entity, EntityKind)>> {
        self.entities[self.idx(pos)].read()
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
                let target = origin.add_wrapping_rotated(offset, facing, self.size);
                let mut cell_counts = self.counts[self.idx(target)]
                    .as_ref()
                    .map(|c| c.load(Ordering::Relaxed));

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
            let target_pos = origin.add_wrapping_rotated(offset, facing, self.size);
            let i = self.idx(target_pos);
            if self.counts[i][target_kind].load(Ordering::Relaxed) == 0 {
                continue;
            }
            let cell = self.entities[i].read();
            for &(entity, kind) in cell.iter() {
                if kind == target_kind && entity != exclude_self {
                    return Some((entity, target_pos));
                }
            }
        }
        None
    }

    #[inline]
    pub fn plant_count(&self) -> u32 {
        self.total_counts.plants.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn character_counts(&self) -> CharacterCounts {
        CharacterCounts {
            herbivores: self.total_counts.herbivores.load(Ordering::Relaxed),
            predators: self.total_counts.predators.load(Ordering::Relaxed),
        }
    }

    #[inline]
    pub fn total_counts(&self) -> CellCounts<u32> {
        self.total_counts
            .as_ref()
            .map(|c| c.load(Ordering::Relaxed))
    }
}
