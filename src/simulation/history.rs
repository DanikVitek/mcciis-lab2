use core::{iter::FusedIterator, num::NonZeroU64, slice};

use bevy::prelude::*;

use super::{
    SimulationSystems, SimulationTick, SimulationTickCount, WorldSeeded,
    spatial::{CellCounts, SpatialGrid},
};

/// Single recorded sample of population and evolutionary statistics at a discrete `tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HistorySample {
    pub tick: u64,
    pub counts: CellCounts<u32>,
    // TODO(history): Add evolutionary metrics (e.g. mean/max `Generation`, `Age`, `Energy` per species).
}

/// Time-series log of [`HistorySample`]s across the current simulation run (stored in SoA form).
///
/// Cleared and seeded with a `tick = 0` baseline whenever [`WorldSeeded`] fires
/// (both on initial startup and on [`super::ResetSimulation`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Resource)]
pub struct SimulationHistory {
    tick: Vec<u64>,
    counts: CellCounts<Vec<u32>>,
}

impl SimulationHistory {
    pub fn tick_history(&self) -> &[u64] {
        &self.tick
    }

    pub fn counts_history(&self) -> CellCounts<&[u32]> {
        self.counts.as_ref().map(Vec::as_slice)
    }

    pub fn first_tick(&self) -> Option<u64> {
        self.tick.first().copied()
    }

    pub fn first_counts(&self) -> Option<CellCounts<u32>> {
        self.counts
            .as_ref()
            .map(|col| col.first().copied())
            .transpose()
    }

    pub fn last_tick(&self) -> Option<u64> {
        self.tick.last().copied()
    }

    pub fn last_counts(&self) -> Option<CellCounts<u32>> {
        self.counts
            .as_ref()
            .map(|col| col.last().copied())
            .transpose()
    }

    pub fn first(&self) -> Option<HistorySample> {
        match (self.first_tick(), self.first_counts()) {
            (Some(tick), Some(counts)) => Some(HistorySample { tick, counts }),
            (None, None) => None,
            _ => unreachable!("SimulationHistory tick and counts columns have mismatched lengths"),
        }
    }

    pub fn last(&self) -> Option<HistorySample> {
        match (self.last_tick(), self.last_counts()) {
            (Some(tick), Some(counts)) => Some(HistorySample { tick, counts }),
            (None, None) => None,
            _ => unreachable!("SimulationHistory tick and counts columns have mismatched lengths"),
        }
    }

    pub fn get(&self, index: usize) -> Option<HistorySample> {
        match (
            self.tick.get(index).copied(),
            self.counts
                .as_ref()
                .map(|col| col.get(index).copied())
                .transpose(),
        ) {
            (Some(tick), Some(counts)) => Some(HistorySample { tick, counts }),
            (None, None) => None,
            _ => unreachable!("SimulationHistory tick and counts columns have mismatched lengths"),
        }
    }

    pub fn clear(&mut self) {
        self.tick.clear();
        self.counts.as_mut().for_each(Vec::clear);
    }

    pub fn push(&mut self, HistorySample { tick, counts }: HistorySample) {
        self.tick.push(tick);
        self.counts
            .as_mut()
            .zip(counts)
            .for_each(|(col, val)| col.push(val));
    }

    pub fn len(&self) -> usize {
        let len = self.tick.len();
        self.counts
            .as_ref()
            .for_each(|col| debug_assert_eq!(len, col.len()));
        len
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> HistoryIter<'_> {
        HistoryIter {
            tick: self.tick.iter(),
            counts: self.counts.as_ref().map(|col| col.iter()),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HistoryIter<'h> {
    tick: slice::Iter<'h, u64>,
    counts: CellCounts<slice::Iter<'h, u32>>,
}

impl<'h> IntoIterator for &'h SimulationHistory {
    type Item = HistorySample;

    type IntoIter = HistoryIter<'h>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl Iterator for HistoryIter<'_> {
    type Item = HistorySample;

    fn next(&mut self) -> Option<Self::Item> {
        match (
            self.tick.next().copied(),
            self.counts
                .as_mut()
                .map(|it| it.next().copied())
                .transpose(),
        ) {
            (Some(tick), Some(counts)) => Some(HistorySample { tick, counts }),
            (None, None) => None,
            _ => unreachable!("SimulationHistory tick and counts columns have mismatched lengths"),
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}

impl ExactSizeIterator for HistoryIter<'_> {
    fn len(&self) -> usize {
        let len = self.tick.len();
        debug_assert!(
            self.counts
                .as_ref()
                .into_array()
                .iter()
                .all(|it| len == it.len())
        );
        len
    }
}

impl DoubleEndedIterator for HistoryIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match (
            self.tick.next_back().copied(),
            self.counts
                .as_mut()
                .map(|it| it.next_back().copied())
                .transpose(),
        ) {
            (Some(tick), Some(counts)) => Some(HistorySample { tick, counts }),
            (None, None) => None,
            _ => unreachable!("SimulationHistory tick and counts columns have mismatched lengths"),
        }
    }
}

impl FusedIterator for HistoryIter<'_> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Resource)]
pub struct SampleRate {
    pub once_every: NonZeroU64,
}

impl Default for SampleRate {
    fn default() -> Self {
        Self {
            once_every: NonZeroU64::MIN,
        }
    }
}

impl SampleRate {
    #[inline]
    pub const fn every(ticks: NonZeroU64) -> Self {
        Self { once_every: ticks }
    }
}

/// Optional plugin that records per-tick population & evolution history into [`SimulationHistory`].
///
/// Usable in both headless batch runs and visual runs (where UI charts can read `Res<SimulationHistory>`).
#[derive(Default)]
pub struct HistoryRecorderPlugin {
    pub sample_rate: SampleRate,
}

impl HistoryRecorderPlugin {
    #[inline]
    pub const fn every(ticks: NonZeroU64) -> Self {
        Self {
            sample_rate: SampleRate::every(ticks),
        }
    }
}

impl Plugin for HistoryRecorderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimulationHistory>()
            .insert_resource(self.sample_rate)
            .add_observer(on_world_seeded)
            .add_systems(
                SimulationTick,
                record_tick_sample
                    .run_if(is_sample_tick)
                    .in_set(SimulationSystems::Record),
            );
    }
}

fn on_world_seeded(
    _: On<WorldSeeded>,
    mut history: ResMut<SimulationHistory>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
) {
    history.clear();
    history.push(HistorySample {
        tick: tick_count.0,
        counts: spatial_grid.total_counts(),
    });
}

fn is_sample_tick(sample_rate: Res<SampleRate>, tick_count: Res<SimulationTickCount>) -> bool {
    tick_count.0.is_multiple_of(sample_rate.once_every.get())
}

fn record_tick_sample(
    mut history: ResMut<SimulationHistory>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
    // TODO(history): Query `(&Generation, &Age, &Energy, Has<Herbivore>)` here to record evolution stats.
    // All deferred spawns/despawns from `Step` and `Reproduce` are already flushed before `SimulationSystems::Record`.
) {
    history.push(HistorySample {
        tick: tick_count.0,
        counts: spatial_grid.total_counts(),
    });
}
