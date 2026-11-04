use core::{iter, num::NonZeroU64};
use std::{
    fs::{self, File},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use bevy::prelude::*;
use clap::{Args, Parser, Subcommand};
use itertools::iproduct;
use life_sim::{
    grid::GridSize,
    presentation::PresentationPlugin,
    simulation::{
        DistributedMutation, DistributionConfig, HistoryRecorderPlugin, MutationStrategy,
        SimulationHistory, SimulationPlugin, SimulationTick, SimulationTickCount,
        SingleNeuronMutation,
        organism::{InitialEnergy, MaxEnergy},
        spatial::SpatialGrid,
    },
};

#[derive(Debug, Parser)]
#[command(name = "life-sim", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<CliCommand>,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Run the interactive 2D visual simulation (default when no subcommand is given).
    Visual(VisualArgs),
    /// Run headless batch experiments across a parameter grid and export CSVs for the lab protocol.
    Headless(HeadlessArgs),
}

#[derive(Debug, Clone, Args)]
struct VisualArgs {
    /// Grid width in tiles.
    #[arg(long, default_value_t = 100)]
    width: u16,

    /// Grid height in tiles.
    #[arg(long, default_value_t = 100)]
    height: u16,

    /// Initial fraction of tiles populated with plants (algae) [0.0..=1.0].
    #[arg(long, default_value_t = 0.1)]
    plants: f32,

    /// Initial fraction of tiles populated with herbivores [0.0..=1.0].
    #[arg(long, default_value_t = 0.1)]
    herbivores: f32,

    /// Initial fraction of tiles populated with predators [0.0..=1.0].
    #[arg(long, default_value_t = 0.1)]
    predators: f32,

    /// Initial energy of newly seeded organisms.
    #[arg(long, default_value_t = 5)]
    initial_energy: u8,

    /// Maximum energy capacity of organisms.
    #[arg(long, default_value_t = 10)]
    max_energy: u8,

    /// Seconds per simulation tick in FixedStep mode.
    #[arg(long, default_value_t = 0.5)]
    tick_interval_secs: f64,
}

impl Default for VisualArgs {
    fn default() -> Self {
        Self {
            width: 100,
            height: 100,
            plants: 0.1,
            herbivores: 0.1,
            predators: 0.1,
            initial_energy: 5,
            max_energy: 10,
            tick_interval_secs: 0.5,
        }
    }
}

#[derive(Debug, Clone, Args)]
struct HeadlessArgs {
    /// Include the `SingleNeuron` mutation strategy in the parameter sweep.
    #[arg(
        long,
        default_value_t = true,
        action = clap::ArgAction::Set,
        num_args = 0..=1,
        default_missing_value = "true"
    )]
    include_single_neuron: bool,

    /// Comma-separated list of offsets to sweep for the `Distributed` mutation strategy.
    #[arg(
        short = 'o',
        long = "distributed-offsets",
        value_delimiter = ',',
        default_values_t = [0.1_f32, 0.25_f32, 0.5_f32, 1.0_f32]
    )]
    distributed_offsets: Vec<f32>,

    /// Comma-separated list of initial plant (algae) fractions to sweep [0.0..=1.0].
    #[arg(long, value_delimiter = ',', default_values_t = [0.1_f32, 0.2, 0.3])]
    plants: Vec<f32>,

    /// Comma-separated list of initial herbivore fractions to sweep [0.0..=1.0].
    #[arg(long, value_delimiter = ',', default_values_t = [0.1_f32, 0.2, 0.3])]
    herbivores: Vec<f32>,

    /// Comma-separated list of initial predator fractions to sweep [0.0..=1.0].
    #[arg(long, value_delimiter = ',', default_values_t = [0.1_f32, 0.2, 0.3])]
    predators: Vec<f32>,

    /// Comma-separated list of initial organism energies to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [5_u8, 8_u8])]
    initial_energy: Vec<u8>,

    /// Comma-separated list of maximum organism energy capacities to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [10_u8, 20_u8])]
    max_energy: Vec<u8>,

    /// Number of independent repetitions to execute per parameter configuration.
    #[arg(short = 'r', long, default_value_t = 1)]
    runs: u32,

    /// Maximum simulation ticks per run before stopping if organisms are still alive.
    #[arg(short = 't', long, default_value_t = 5_000)]
    max_ticks: u64,

    /// Record a history sample every N ticks.
    #[arg(long, default_value_t = NonZeroU64::MIN)]
    sample_rate: NonZeroU64,

    /// Grid width in tiles.
    #[arg(long, default_value_t = 100)]
    width: u16,

    /// Grid height in tiles.
    #[arg(long, default_value_t = 100)]
    height: u16,

    /// Output directory where `summary.csv` and per-run time-series CSV files will be written.
    #[arg(long, default_value = "protocol_results")]
    output_dir: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct GridPoint {
    mutation_strategy: MutationStrategy,
    distribution: DistributionConfig,
    initial_energy: InitialEnergy,
    max_energy: MaxEnergy,
}

struct RunSummary {
    run_id: usize,
    point: GridPoint,
    repetition: u32,
    width: u16,
    height: u16,
    max_ticks: u64,
    total_ticks: u64,
    extinct: bool,
    final_herbivores: u32,
    final_predators: u32,
    peak_herbivores: u32,
    peak_predators: u32,
    max_generation_reached: u32,
    final_oldest_generation: Option<u32>,
    final_youngest_generation: Option<u32>,
    recorded_samples: usize,
    history_filename: String,
}

fn main() -> io::Result<()> {
    let cli = Cli::parse();

    match cli
        .command
        .unwrap_or(CliCommand::Visual(VisualArgs::default()))
    {
        CliCommand::Visual(args) => {
            run_visual(args);
            Ok(())
        }
        CliCommand::Headless(args) => run_headless_sweep(args),
    }
}

fn run_visual(args: VisualArgs) {
    let distribution = DistributionConfig::builder()
        .plants_fraction(args.plants)
        .herbivores_fraction(args.herbivores)
        .predators_fraction(args.predators)
        .build()
        .expect("invalid initial species distribution fractions");

    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            SimulationPlugin::builder()
                .grid_size(GridSize::new(args.width, args.height))
                .distribution(distribution)
                .initial_energy(args.initial_energy)
                .max_energy(args.max_energy)
                .mutation_strategy(DistributedMutation::default())
                .build(),
            HistoryRecorderPlugin::default(),
            PresentationPlugin::new(Duration::from_secs_f64(args.tick_interval_secs)),
        ))
        .run();
}

#[derive(Debug, Clone, PartialEq, PartialOrd, Default)]
struct Grid<'a> {
    single_neuron_mutation: bool,
    distributed_mutation: &'a [f32],
    plants_fraction: &'a [f32],
    herbivores_fraction: &'a [f32],
    predators_fraction: &'a [f32],
    initial_energy: &'a [u8],
    max_energy: &'a [u8],
}

impl Grid<'_> {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        (!self.single_neuron_mutation && self.distributed_mutation.is_empty())
            || self.plants_fraction.is_empty()
            || self.herbivores_fraction.is_empty()
            || self.predators_fraction.is_empty()
            || self.initial_energy.is_empty()
            || self.max_energy.is_empty()
            || iproduct!(
                self.initial_energy.iter().copied(),
                self.max_energy.iter().copied()
            )
            .all(|(initial, max)| initial > max)
            || iproduct!(
                self.plants_fraction.iter().copied(),
                self.herbivores_fraction.iter().copied(),
                self.predators_fraction.iter().copied(),
            )
            .all(|(pl, h, pr)| {
                DistributionConfig::builder()
                    .plants_fraction(pl)
                    .herbivores_fraction(h)
                    .predators_fraction(pr)
                    .build()
                    .is_err()
            })
    }

    #[must_use]
    pub fn len(&self) -> u64 {
        (u64::from(self.single_neuron_mutation) + self.distributed_mutation.len() as u64)
            * iproduct!(
                self.plants_fraction.iter().copied(),
                self.herbivores_fraction.iter().copied(),
                self.predators_fraction.iter().copied(),
            )
            .filter_map(|(pl, h, pr)| {
                DistributionConfig::builder()
                    .plants_fraction(pl)
                    .herbivores_fraction(h)
                    .predators_fraction(pr)
                    .build()
                    .ok()
            })
            .fold(0_u64, |acc, _| acc + 1)
            * iproduct!(
                self.initial_energy.iter().copied(),
                self.max_energy.iter().copied(),
            )
            .filter(|&(initial, max)| initial <= max)
            .fold(0_u64, |acc, _| acc + 1)
    }

    fn iter(&self) -> impl Iterator<Item = GridPoint> + use<'_> {
        iproduct!(
            iter::chain(
                self.single_neuron_mutation
                    .then_some(MutationStrategy::SingleNeuron(SingleNeuronMutation)),
                self.distributed_mutation
                    .iter()
                    .copied()
                    .map(|offset| MutationStrategy::Distributed(DistributedMutation { offset }))
            ),
            iproduct!(
                self.plants_fraction.iter().copied(),
                self.herbivores_fraction.iter().copied(),
                self.predators_fraction.iter().copied(),
            )
            .filter_map(|(pl, h, pr)| {
                DistributionConfig::builder()
                    .plants_fraction(pl)
                    .herbivores_fraction(h)
                    .predators_fraction(pr)
                    .build()
                    .ok()
            }),
            iproduct!(
                self.initial_energy.iter().copied().map(InitialEnergy),
                self.max_energy.iter().copied().map(MaxEnergy),
            )
            .filter(|&(InitialEnergy(initial), MaxEnergy(max))| initial <= max)
        )
        .map(
            |(mutation_strategy, distribution, (initial_energy, max_energy))| GridPoint {
                mutation_strategy,
                distribution,
                initial_energy,
                max_energy,
            },
        )
    }
}

impl<'a> From<&'a HeadlessArgs> for Grid<'a> {
    fn from(args: &'a HeadlessArgs) -> Self {
        Self {
            single_neuron_mutation: args.include_single_neuron,
            distributed_mutation: &args.distributed_offsets,
            plants_fraction: &args.plants,
            herbivores_fraction: &args.herbivores,
            predators_fraction: &args.predators,
            initial_energy: &args.initial_energy,
            max_energy: &args.max_energy,
        }
    }
}

fn run_headless_sweep(args: HeadlessArgs) -> io::Result<()> {
    let grid = Grid::from(&args);
    if grid.is_empty() {
        eprintln!(
            "Parameter grid is empty. Check mutation strategies, distribution fractions, and energy ranges (initial_energy <= max_energy)."
        );
        return Ok(());
    }

    fs::create_dir_all(&args.output_dir)?;

    let repetitions = args.runs.max(1);
    let grid_len = grid.len();
    let total_runs = grid_len * u64::from(repetitions);
    println!(
        "Starting headless parameter sweep: {grid_len} grid configuration(s) × {repetitions} repetition(s) = {total_runs} total run(s)",
    );

    let summary_path = args.output_dir.join("summary.csv");
    let mut summary_writer = SummaryWriter::new(&summary_path)?;
    let mut run_id = 0usize;

    for point in grid.iter() {
        for repetition in 1..=repetitions {
            run_id += 1;
            let start = Instant::now();

            let (history, total_ticks, final_counts) = execute_single_headless_run(
                GridSize::new(args.width, args.height),
                point,
                args.sample_rate,
                args.max_ticks,
            );

            let history_filename = format_run_filename(run_id, point, repetition);
            let history_path = args.output_dir.join(&history_filename);
            write_run_history_csv(&history_path, &history)?;

            let counts_history = history.counts_history();
            let peak_herbivores = counts_history
                .herbivores
                .iter()
                .copied()
                .max()
                .unwrap_or(final_counts.herbivores);
            let peak_predators = counts_history
                .predators
                .iter()
                .copied()
                .max()
                .unwrap_or(final_counts.predators);
            let max_generation_reached = history
                .generation_history()
                .iter()
                .filter_map(|g| g.youngest)
                .max()
                .unwrap_or(0);
            let final_gen = history.last_generation();
            let extinct = final_counts.herbivores == 0 && final_counts.predators == 0;

            println!(
                "[{run_id}/{total_runs}] {label} (rep {repetition}) -> {total_ticks} ticks ({status}), max gen {max_generation_reached}, {samples} samples in {elapsed:.2?} -> {history_filename}",
                label = format_grid_point_label(point),
                status = if extinct { "extinct" } else { "survived" },
                samples = history.len(),
                elapsed = start.elapsed(),
            );

            summary_writer.write(RunSummary {
                run_id,
                point,
                repetition,
                width: args.width,
                height: args.height,
                max_ticks: args.max_ticks,
                total_ticks,
                extinct,
                final_herbivores: final_counts.herbivores,
                final_predators: final_counts.predators,
                peak_herbivores,
                peak_predators,
                max_generation_reached,
                final_oldest_generation: final_gen.and_then(|g| g.oldest_alive),
                final_youngest_generation: final_gen.and_then(|g| g.youngest),
                recorded_samples: history.len(),
                history_filename,
            })?;
        }
    }

    summary_writer.flush()?;
    println!("Exported protocol summary to {}", summary_path.display());

    Ok(())
}

fn execute_single_headless_run(
    grid_size: GridSize,
    point: GridPoint,
    sample_rate: NonZeroU64,
    max_ticks: u64,
) -> (
    SimulationHistory,
    u64,
    life_sim::simulation::spatial::CellCounts<u32>,
) {
    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::builder()
            .grid_size(grid_size)
            .distribution(point.distribution)
            .initial_energy(point.initial_energy)
            .max_energy(point.max_energy)
            .mutation_strategy(point.mutation_strategy)
            .build(),
        HistoryRecorderPlugin::every(sample_rate),
    ));

    // Execute `Startup` schedule (`seed_world` -> `WorldSeeded` -> baseline tick 0 sample).
    app.update();

    while app.world().resource::<SimulationTickCount>().0 < max_ticks {
        if app
            .world()
            .resource::<SpatialGrid>()
            .character_counts()
            .is_empty()
        {
            break;
        }
        app.world_mut().run_schedule(SimulationTick);
    }

    let total_ticks = app.world().resource::<SimulationTickCount>().0;
    let final_counts = app.world().resource::<SpatialGrid>().total_counts();
    let history = app.world().resource::<SimulationHistory>().clone();

    (history, total_ticks, final_counts)
}

fn format_grid_point_label(point: GridPoint) -> String {
    let strategy = match point.mutation_strategy {
        MutationStrategy::SingleNeuron(_) => "SingleNeuron".to_string(),
        MutationStrategy::Distributed(d) => format!("Distributed({:.3})", d.offset),
    };
    format!(
        "{strategy}, dist=({:.2}/{:.2}/{:.2}), energy={}/{}",
        point.distribution.plants_fraction(),
        point.distribution.herbivores_fraction(),
        point.distribution.predators_fraction(),
        point.initial_energy,
        point.max_energy,
    )
}

fn format_run_filename(run_id: usize, point: GridPoint, repetition: u32) -> String {
    let strategy_slug = match point.mutation_strategy {
        MutationStrategy::SingleNeuron(_) => "single_neuron".to_string(),
        MutationStrategy::Distributed(d) => {
            let offset_slug = format!("{:.3}", d.offset).replace('.', "_");
            format!("distributed_{offset_slug}")
        }
    };
    format!(
        "run_{run_id:03}_{strategy_slug}_e{}_{}_rep{repetition}.csv",
        point.initial_energy, point.max_energy
    )
}

fn write_run_history_csv(path: &Path, history: &SimulationHistory) -> io::Result<()> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);

    writeln!(
        writer,
        "tick,herbivores,predators,oldest_generation_alive,youngest_generation_alive,generation_counts"
    )?;

    for sample in history {
        let oldest = sample
            .generation
            .oldest_alive
            .map_or_else(String::new, |v| v.to_string());
        let youngest = sample
            .generation
            .youngest
            .map_or_else(String::new, |v| v.to_string());
        let gen_counts = sample
            .generation
            .counts
            .iter()
            .map(|(gen_num, count)| format!("{gen_num}:{count}"))
            .collect::<Vec<_>>()
            .join(";");

        writeln!(
            writer,
            "{},{},{},{},{},\"{}\"",
            sample.tick,
            sample.counts.herbivores,
            sample.counts.predators,
            oldest,
            youngest,
            gen_counts
        )?;
    }

    writer.flush()
}

struct SummaryWriter {
    writer: BufWriter<File>,
}

impl SummaryWriter {
    #[inline]
    pub fn new(path: impl AsRef<Path>) -> io::Result<Self> {
        fn inner(path: &Path) -> io::Result<SummaryWriter> {
            let file = File::create(path)?;
            let mut writer = BufWriter::new(file);
            writeln!(
                writer,
                "run_id,strategy,mutation_offset,repetition,grid_width,grid_height,plants_fraction,herbivores_fraction,predators_fraction,initial_energy,max_energy,max_ticks_limit,total_ticks_survived,extinct,final_herbivores,final_predators,peak_herbivores,peak_predators,max_generation_reached,final_oldest_generation_alive,final_youngest_generation_alive,recorded_samples,history_file"
            )?;
            Ok(SummaryWriter { writer })
        }
        inner(path.as_ref())
    }

    pub fn write(&mut self, summary: RunSummary) -> io::Result<()> {
        let (strategy_name, offset_str) = match summary.point.mutation_strategy {
            MutationStrategy::SingleNeuron(_) => ("SingleNeuron", String::new()),
            MutationStrategy::Distributed(DistributedMutation { offset }) => {
                ("Distributed", format!("{offset:.4}"))
            }
        };
        let final_oldest = summary
            .final_oldest_generation
            .map_or_else(String::new, |v| v.to_string());
        let final_youngest = summary
            .final_youngest_generation
            .map_or_else(String::new, |v| v.to_string());

        writeln!(
            self.writer,
            "{},{},{},{},{},{},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            summary.run_id,
            strategy_name,
            offset_str,
            summary.repetition,
            summary.width,
            summary.height,
            summary.point.distribution.plants_fraction(),
            summary.point.distribution.herbivores_fraction(),
            summary.point.distribution.predators_fraction(),
            summary.point.initial_energy,
            summary.point.max_energy,
            summary.max_ticks,
            summary.total_ticks,
            summary.extinct,
            summary.final_herbivores,
            summary.final_predators,
            summary.peak_herbivores,
            summary.peak_predators,
            summary.max_generation_reached,
            final_oldest,
            final_youngest,
            summary.recorded_samples,
            summary.history_filename
        )
    }

    pub fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
