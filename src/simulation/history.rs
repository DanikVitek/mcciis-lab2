use core::iter::FusedIterator;
use core::num::NonZeroU64;
use core::slice;

use bevy::prelude::*;

use super::organism::{
    Age,
    CharacterFilter,
    EntityKind,
    Generation,
    Herbivore,
};
use super::spatial::{
    CellCounts,
    CharacterCounts,
    SpatialGrid,
};
use super::{
    CharacterAged,
    CharacterDied,
    OrganismReproduced,
    SimulationSystems,
    SimulationTick,
    SimulationTickCount,
    WorldSeeded,
};

/// Generation statistics at a discrete sample tick.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct GenerationSample {
    /// Earliest generation still surviving among living organisms (`None` if extinct).
    pub oldest_alive: Option<u32>,
    /// Highest (most recent) generation among living organisms (`None` if extinct).
    pub youngest: Option<u32>,
    /// Distribution of living individuals per generation: `(generation_number, count)`.
    pub counts: Vec<(u32, u32)>,
}

/// Single recorded sample of population and evolutionary statistics at a discrete `tick`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HistorySample {
    pub tick: u64,
    pub counts: CellCounts<u32>,
    pub generation: GenerationSample,
}

/// Time-series log of [`HistorySample`]s and cumulative run metrics across the current simulation run (stored in SoA form).
///
/// Cleared and seeded with a `tick = 0` baseline whenever [`WorldSeeded`] fires
/// (both on initial startup and on [`super::ResetSimulation`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Resource)]
pub struct SimulationHistory {
    epoch: u64,
    last_tick: Option<u64>,
    tick: Vec<u64>,
    counts: CellCounts<Vec<u32>>,
    generations: Vec<GenerationSample>,
    max_age: CharacterCounts<u32>,
    reproductions: CellCounts<u64>,
    max_age_history: CharacterCounts<Vec<u32>>,
    reproductions_history: CellCounts<Vec<u64>>,
}

impl SimulationHistory {
    /// Monotonically increasing run counter incremented on every [`Self::clear`] call.
    pub const fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Maximum age (in ticks) reached by any character of each type (`herbivores`, `predators`) during the run.
    pub const fn max_age(&self) -> CharacterCounts<u32> {
        self.max_age
    }

    /// Total number of reproductions performed by each entity type (`plants`, `herbivores`, `predators`) during the run.
    pub const fn reproductions(&self) -> CellCounts<u64> {
        self.reproductions
    }

    pub fn tick_history(&self) -> &[u64] {
        &self.tick
    }

    pub fn counts_history(&self) -> CellCounts<&[u32]> {
        self.counts.as_ref().map(Vec::as_slice)
    }

    pub fn generation_history(&self) -> &[GenerationSample] {
        &self.generations
    }

    pub fn max_age_history(&self) -> CharacterCounts<&[u32]> {
        self.max_age_history.as_ref().map(Vec::as_slice)
    }

    pub fn reproductions_history(&self) -> CellCounts<&[u64]> {
        self.reproductions_history.as_ref().map(Vec::as_slice)
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

    pub fn first_generation(&self) -> Option<&GenerationSample> {
        self.generations.first()
    }

    pub fn last_tick(&self) -> Option<u64> {
        self.last_tick
    }

    pub fn last_counts(&self) -> Option<CellCounts<u32>> {
        self.counts
            .as_ref()
            .map(|col| col.last().copied())
            .transpose()
    }

    pub fn last_generation(&self) -> Option<&GenerationSample> {
        self.generations.last()
    }

    pub fn first(&self) -> Option<HistorySample> {
        match (
            self.first_tick(),
            self.first_counts(),
            self.first_generation().cloned(),
        ) {
            (Some(tick), Some(counts), Some(generation)) => Some(HistorySample {
                tick,
                counts,
                generation,
            }),
            (None, None, None) => None,
            _ => unreachable!(
                "SimulationHistory tick, counts, and generation columns have mismatched lengths"
            ),
        }
    }

    pub fn last(&self) -> Option<HistorySample> {
        match (
            self.last_tick(),
            self.last_counts(),
            self.last_generation().cloned(),
        ) {
            (Some(tick), Some(counts), Some(generation)) => Some(HistorySample {
                tick,
                counts,
                generation,
            }),
            (None, None, None) => None,
            _ => unreachable!(
                "SimulationHistory tick, counts, and generation columns have mismatched lengths"
            ),
        }
    }

    pub fn get(&self, index: usize) -> Option<HistorySample> {
        match (
            self.tick.get(index).copied(),
            self.counts
                .as_ref()
                .map(|col| col.get(index).copied())
                .transpose(),
            self.generations.get(index).cloned(),
        ) {
            (Some(tick), Some(counts), Some(generation)) => Some(HistorySample {
                tick,
                counts,
                generation,
            }),
            (None, None, None) => None,
            _ => unreachable!(
                "SimulationHistory tick, counts, and generation columns have mismatched lengths"
            ),
        }
    }

    pub fn clear(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.last_tick = None;
        self.tick.clear();
        self.counts.as_mut().for_each(Vec::clear);
        self.generations.clear();
        self.max_age = CharacterCounts::default();
        self.reproductions = CellCounts::default();
        self.max_age_history.as_mut().for_each(Vec::clear);
        self.reproductions_history.as_mut().for_each(Vec::clear);
    }

    pub fn record_reproduction(&mut self, kind: EntityKind) {
        self.reproductions[kind] += 1;
        if let Some(last) = self.reproductions_history[kind].last_mut() {
            *last = self.reproductions[kind];
        }
    }

    pub fn record_age(&mut self, kind: EntityKind, ticks: u32) {
        match kind {
            EntityKind::Herbivore => {
                if ticks > self.max_age.herbivores {
                    self.max_age.herbivores = ticks;
                    if let Some(last) = self.max_age_history.herbivores.last_mut() {
                        *last = ticks;
                    }
                }
            }
            EntityKind::Predator => {
                if ticks > self.max_age.predators {
                    self.max_age.predators = ticks;
                    if let Some(last) = self.max_age_history.predators.last_mut() {
                        *last = ticks;
                    }
                }
            }
            EntityKind::Plant => {}
        }
    }

    pub fn push(
        &mut self,
        HistorySample {
            tick,
            counts,
            generation,
        }: HistorySample,
    ) {
        self.last_tick = Some(tick);
        if self.last_counts().is_some_and(|it| it == counts)
            && self.generations.last().is_some_and(|g| g == &generation)
        {
            self.max_age_history
                .as_mut()
                .zip(self.max_age)
                .for_each(|(col, val)| {
                    if let Some(last) = col.last_mut() {
                        *last = val;
                    }
                });
            self.reproductions_history
                .as_mut()
                .zip(self.reproductions)
                .for_each(|(col, val)| {
                    if let Some(last) = col.last_mut() {
                        *last = val;
                    }
                });
            return;
        }
        self.tick.push(tick);
        self.counts
            .as_mut()
            .zip(counts)
            .for_each(|(col, val)| col.push(val));
        self.generations.push(generation);
        self.max_age_history
            .as_mut()
            .zip(self.max_age)
            .for_each(|(col, val)| col.push(val));
        self.reproductions_history
            .as_mut()
            .zip(self.reproductions)
            .for_each(|(col, val)| col.push(val));
    }

    pub fn len(&self) -> usize {
        let len = self.tick.len();
        self.counts
            .as_ref()
            .for_each(|col| debug_assert_eq!(len, col.len()));
        debug_assert_eq!(len, self.generations.len());
        self.max_age_history
            .as_ref()
            .for_each(|col| debug_assert_eq!(len, col.len()));
        self.reproductions_history
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
            generations: self.generations.iter(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HistoryIter<'h> {
    tick: slice::Iter<'h, u64>,
    counts: CellCounts<slice::Iter<'h, u32>>,
    generations: slice::Iter<'h, GenerationSample>,
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
            self.generations.next().cloned(),
        ) {
            (Some(tick), Some(counts), Some(generation)) => Some(HistorySample {
                tick,
                counts,
                generation,
            }),
            (None, None, None) => None,
            _ => unreachable!(
                "SimulationHistory tick, counts, and generation columns have mismatched lengths"
            ),
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
        debug_assert_eq!(len, self.generations.len());
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
            self.generations.next_back().cloned(),
        ) {
            (Some(tick), Some(counts), Some(generation)) => Some(HistorySample {
                tick,
                counts,
                generation,
            }),
            (None, None, None) => None,
            _ => unreachable!(
                "SimulationHistory tick, counts, and generation columns have mismatched lengths"
            ),
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

/// Incremental generation histogram maintained from [`WorldSeeded`], [`OrganismReproduced`], and [`CharacterDied`].
#[derive(Debug, Clone, Default, Resource)]
struct GenerationTracker {
    table: Vec<u32>,
}

impl GenerationTracker {
    fn reset<'a>(&mut self, generations: impl Iterator<Item = &'a Generation>) {
        self.table.fill(0);
        for generation in generations {
            self.increment(*generation);
        }
    }

    #[inline]
    fn increment(&mut self, generation: Generation) {
        let idx = generation.number() as usize;
        if idx >= self.table.len() {
            self.table.resize(idx + 1, 0);
        }
        self.table[idx] += 1;
    }

    #[inline]
    fn decrement(&mut self, generation: Generation) {
        let idx = generation.number() as usize;
        if let Some(slot) = self.table.get_mut(idx) {
            *slot = slot.saturating_sub(1);
        }
    }

    fn sample(&self) -> GenerationSample {
        let mut oldest_alive = None;
        let mut youngest = None;
        let mut counts = Vec::new();

        for (idx, &count) in self.table.iter().enumerate() {
            if count > 0 {
                let gen_num = idx as u32;
                if oldest_alive.is_none() {
                    oldest_alive = Some(gen_num);
                }
                youngest = Some(gen_num);
                counts.push((gen_num, count));
            }
        }

        GenerationSample {
            oldest_alive,
            youngest,
            counts,
        }
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
        app.add_message::<OrganismReproduced>()
            .add_message::<CharacterDied>()
            .add_message::<CharacterAged>()
            .init_resource::<SimulationHistory>()
            .init_resource::<GenerationTracker>()
            .insert_resource(self.sample_rate)
            .add_observer(on_world_seeded)
            .add_systems(
                SimulationTick,
                (
                    ingest_lifecycle_messages,
                    record_tick_sample.run_if(is_sample_tick),
                )
                    .chain()
                    .in_set(SimulationSystems::Record),
            );
    }
}

#[allow(clippy::too_many_arguments)]
fn on_world_seeded(
    _: On<WorldSeeded>,
    mut history: ResMut<SimulationHistory>,
    mut tracker: ResMut<GenerationTracker>,
    mut reproduced: ResMut<Messages<OrganismReproduced>>,
    mut died: ResMut<Messages<CharacterDied>>,
    mut aged: ResMut<Messages<CharacterAged>>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
    characters: Query<(&Generation, &Age, Has<Herbivore>), CharacterFilter>,
) {
    reproduced.clear();
    died.clear();
    aged.clear();

    tracker.reset(characters.iter().map(|(generation, _, _)| generation));
    history.clear();
    for (_, age, is_herbivore) in &characters {
        let kind = if is_herbivore {
            EntityKind::Herbivore
        } else {
            EntityKind::Predator
        };
        history.record_age(kind, age.ticks());
    }
    history.push(HistorySample {
        tick: tick_count.0,
        counts: spatial_grid.total_counts(),
        generation: tracker.sample(),
    });
}

fn is_sample_tick(sample_rate: Res<SampleRate>, tick_count: Res<SimulationTickCount>) -> bool {
    tick_count.0.is_multiple_of(sample_rate.once_every.get())
}

fn ingest_lifecycle_messages(
    mut history: ResMut<SimulationHistory>,
    mut tracker: ResMut<GenerationTracker>,
    mut reproduced_reader: MessageReader<OrganismReproduced>,
    mut died_reader: MessageReader<CharacterDied>,
    mut aged_reader: MessageReader<CharacterAged>,
) {
    let history = history.bypass_change_detection();

    for &OrganismReproduced {
        kind,
        child_generation,
    } in reproduced_reader.read()
    {
        history.record_reproduction(kind);
        if let Some(child_gen) = child_generation {
            tracker.increment(child_gen);
        }
    }

    for &CharacterDied {
        kind,
        age,
        generation,
        ..
    } in died_reader.read()
    {
        history.record_age(kind, age.ticks());
        tracker.decrement(generation);
    }

    for &CharacterAged { kind, max_age } in aged_reader.read() {
        history.record_age(kind, max_age.ticks());
    }
}

fn record_tick_sample(
    mut history: ResMut<SimulationHistory>,
    tracker: Res<GenerationTracker>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
) {
    history.push(HistorySample {
        tick: tick_count.0,
        counts: spatial_grid.total_counts(),
        generation: tracker.sample(),
    });
}
