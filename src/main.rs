use core::iter::{
    self,
    FusedIterator,
};
use core::num::NonZeroU64;
use std::fs::{
    self,
    File,
};
use std::io::{
    self,
    BufWriter,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};
use std::time::{
    Duration,
    Instant,
};

use bevy::prelude::*;
use clap::{
    Args,
    Parser,
    Subcommand,
};
use itertools::iproduct;
use life_sim::grid::GridSize;
use life_sim::presentation::PresentationPlugin;
use life_sim::presentation::ui::config::{
    AutoSaveSettingsPath,
    SimConfig,
};
use life_sim::simulation::organism::{
    ActionCosts,
    Energy,
    FoodValues,
    InitialEnergy,
    MaxEnergy,
};
use life_sim::simulation::spatial::{
    CellCounts,
    CharacterCounts,
    SpatialGrid,
};
use life_sim::simulation::{
    DeferredReproductionPlugin,
    DeferredSimulationPlugin,
    DistributedMutation,
    DistributionConfig,
    HistoryRecorderPlugin,
    MutationStrategy,
    PlantGrowthRate,
    ReproductionPlugin,
    ReproductionThreshold,
    SimulationHistory,
    SimulationPlugin,
    SimulationTick,
    SimulationTickCount,
    SingleNeuronMutation,
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
    Headless(Box<HeadlessArgs>),
}

#[derive(Debug, Clone, Default, Args)]
struct VisualArgs {
    /// Grid width in tiles.
    #[arg(long)]
    width: Option<u16>,

    /// Grid height in tiles.
    #[arg(long)]
    height: Option<u16>,

    /// Initial fraction of tiles populated with plants (algae) [0.0..=1.0].
    #[arg(long)]
    plants: Option<f32>,

    /// Initial fraction of tiles populated with herbivores [0.0..=1.0].
    #[arg(long)]
    herbivores: Option<f32>,

    /// Initial fraction of tiles populated with predators [0.0..=1.0].
    #[arg(long)]
    predators: Option<f32>,

    /// Initial energy of newly seeded organisms.
    #[arg(long)]
    initial_energy: Option<InitialEnergy>,

    /// Maximum energy capacity of organisms.
    #[arg(long)]
    max_energy: Option<MaxEnergy>,

    /// Seconds per simulation tick in FixedStep mode.
    #[arg(long)]
    tick_interval_secs: Option<f64>,
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
        num_args = 0..,
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

    /// Comma-separated list of logistic plant (algae) growth rates to sweep [0.0..=1.0].
    #[arg(long, value_delimiter = ',', default_values_t = [PlantGrowthRate::default()])]
    plant_growth_rate: Vec<PlantGrowthRate>,

    /// Comma-separated list of initial organism energies to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [5_u8, 8_u8].map(InitialEnergy::from))]
    initial_energy: Vec<InitialEnergy>,

    /// Comma-separated list of maximum organism energy capacities to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [10_u8, 20_u8].map(MaxEnergy::from))]
    max_energy: Vec<MaxEnergy>,

    /// Comma-separated list of relative reproduction energy thresholds (fractions of `max_energy`) to sweep [0.0..=1.0].
    #[arg(
        long,
        value_delimiter = ',',
        num_args = 0..,
        default_values_t = [0.9_f32]
    )]
    reproduction_threshold_relative: Vec<f32>,

    /// Comma-separated list of absolute reproduction energy thresholds to sweep (0.0 < threshold <= max_energy].
    #[arg(long, value_delimiter = ',', num_args = 0..)]
    reproduction_threshold_absolute: Vec<Energy>,

    /// Comma-separated list of energy costs for turning (`TurnLeft` / `TurnRight`) to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [ActionCosts::default().turn])]
    turn_cost: Vec<Energy>,

    /// Comma-separated list of energy costs for moving forward (`GoForward`) to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [ActionCosts::default().go_forward])]
    go_forward_cost: Vec<Energy>,

    /// Comma-separated list of energy costs for consuming (`Consume`) to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [ActionCosts::default().consume])]
    consume_cost: Vec<Energy>,

    /// Comma-separated list of nutritional energy values gained from consuming a plant (algae) to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [FoodValues::default().plant])]
    plant_food_value: Vec<Energy>,

    /// Comma-separated list of nutritional energy values gained from consuming a herbivore to sweep.
    #[arg(long, value_delimiter = ',', default_values_t = [FoodValues::default().herbivore])]
    herbivore_food_value: Vec<Energy>,

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

#[derive(Debug, Clone, Copy, PartialEq)]
struct GridPoint {
    mutation_strategy: MutationStrategy,
    distribution: DistributionConfig,
    plant_growth_rate: PlantGrowthRate,
    initial_energy: InitialEnergy,
    max_energy: MaxEnergy,
    reproduction_threshold: ReproductionThreshold,
    action_costs: ActionCosts,
    food_values: FoodValues,
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
    final_counts: CellCounts<u32>,
    peak_counts: CellCounts<u32>,
    max_age: CharacterCounts<u32>,
    reproductions: CellCounts<u64>,
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
        CliCommand::Headless(args) => run_headless_sweep(*args),
    }
}

/// Resolves the `assets` folder across `cargo run`, standalone binaries, and
/// installed OS bundles (`.msi`, `.deb`, `.rpm`, `.AppImage`).
fn resolve_asset_folder() -> String {
    if std::env::var_os("BEVY_ASSET_ROOT").is_some()
        || std::env::var_os("CARGO_MANIFEST_DIR").is_some()
    {
        return "assets".to_owned();
    }

    if let Ok(exe_path) = std::env::current_exe()
        && let Some(exe_dir) = exe_path.parent()
    {
        if exe_dir.join("assets").is_dir() {
            return "assets".to_owned();
        }

        for rel in [
            "../lib/life-sim/assets",
            "../lib/LifeSim/assets",
            "../share/life-sim/assets",
        ] {
            let candidate = exe_dir.join(rel);
            if candidate.is_dir() {
                let resolved = fs::canonicalize(&candidate).unwrap_or(candidate);
                return resolved.to_string_lossy().into_owned();
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        let cwd_assets = cwd.join("assets");
        if cwd_assets.is_dir() {
            return cwd_assets.to_string_lossy().into_owned();
        }
    }

    "assets".to_owned()
}

fn run_visual(args: VisualArgs) {
    let mut prefilled_config = SimConfig::load_system_or_default();
    if args.width.is_some() || args.height.is_some() {
        let w = args
            .width
            .unwrap_or_else(|| prefilled_config.grid_size.width());
        let h = args
            .height
            .unwrap_or_else(|| prefilled_config.grid_size.height());
        prefilled_config.grid_size = GridSize::new(w, h);
    }
    if let Some(plants) = args.plants {
        prefilled_config.plants_fraction = plants;
    }
    if let Some(herbivores) = args.herbivores {
        prefilled_config.herbivores_fraction = herbivores;
    }
    if let Some(predators) = args.predators {
        prefilled_config.predators_fraction = predators;
    }
    if let Some(initial_energy) = args.initial_energy {
        prefilled_config.initial_energy = initial_energy;
    }
    if let Some(max_energy) = args.max_energy {
        prefilled_config.max_energy = max_energy;
    }
    if let Some(secs) = args.tick_interval_secs {
        prefilled_config.tick_interval = Duration::from_secs_f64(secs);
    }

    let tick_interval = prefilled_config.tick_interval;
    let mut app = App::new();
    if let Some(settings_path) = SimConfig::default_settings_path() {
        app.insert_resource(AutoSaveSettingsPath(settings_path));
    }
    app.insert_resource(prefilled_config)
        .add_plugins((
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: resolve_asset_folder(),
                    ..default()
                })
                .set(ImagePlugin::default_nearest())
                .set(WindowPlugin {
                    primary_window: Some({
                        let mut w = Window::default();
                        w.set_maximized(true);
                        w
                    }),
                    ..default()
                }),
            DeferredSimulationPlugin,
            DeferredReproductionPlugin,
            HistoryRecorderPlugin::default(),
            PresentationPlugin::new(tick_interval),
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
    plant_growth_rate: &'a [PlantGrowthRate],
    initial_energy: &'a [InitialEnergy],
    max_energy: &'a [MaxEnergy],
    reproduction_threshold_relative: &'a [f32],
    reproduction_threshold_absolute: &'a [Energy],
    turn_cost: &'a [Energy],
    go_forward_cost: &'a [Energy],
    consume_cost: &'a [Energy],
    plant_food_value: &'a [Energy],
    herbivore_food_value: &'a [Energy],
}

impl Grid<'_> {
    fn mutation_strategies(
        &self,
    ) -> impl DoubleEndedIterator<Item = MutationStrategy> + FusedIterator + Clone + use<'_> {
        iter::chain(
            self.single_neuron_mutation
                .then_some(MutationStrategy::SingleNeuron(SingleNeuronMutation)),
            self.distributed_mutation
                .iter()
                .copied()
                .map(|offset| MutationStrategy::Distributed(DistributedMutation { offset })),
        )
    }

    fn distributions(&self) -> impl Iterator<Item = DistributionConfig> + Clone + use<'_> {
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
        })
    }

    fn plant_growth_rates(&self) -> impl Iterator<Item = PlantGrowthRate> + Clone + use<'_> {
        self.plant_growth_rate
            .iter()
            .copied()
            .filter(|rate| rate.is_valid())
    }

    fn reproduction_thresholds(
        &self,
    ) -> impl Iterator<Item = ReproductionThreshold> + Clone + use<'_> {
        iter::chain(
            self.reproduction_threshold_relative
                .iter()
                .copied()
                .map(ReproductionThreshold::Relative),
            self.reproduction_threshold_absolute
                .iter()
                .copied()
                .map(ReproductionThreshold::Absolute),
        )
    }

    fn energy_and_thresholds(
        &self,
    ) -> impl Iterator<Item = (InitialEnergy, MaxEnergy, ReproductionThreshold)> + Clone + use<'_>
    {
        iproduct!(
            self.initial_energy.iter().copied(),
            self.max_energy.iter().copied(),
            self.reproduction_thresholds(),
        )
        .filter(|&(InitialEnergy(initial), max_energy, threshold)| {
            initial.0 > 0.0
                && max_energy.0.0 > 0.0
                && initial <= max_energy.0
                && threshold.is_valid_for(max_energy)
        })
    }

    fn action_costs(&self) -> impl Iterator<Item = ActionCosts> + Clone + use<'_> {
        iproduct!(
            self.turn_cost.iter().copied(),
            self.go_forward_cost.iter().copied(),
            self.consume_cost.iter().copied(),
        )
        .map(|(turn, go_forward, consume)| ActionCosts {
            turn,
            go_forward,
            consume,
        })
        .filter(|costs| costs.is_valid())
    }

    fn food_values(&self) -> impl Iterator<Item = FoodValues> + Clone + use<'_> {
        iproduct!(
            self.plant_food_value.iter().copied(),
            self.herbivore_food_value.iter().copied(),
        )
        .map(|(plant, herbivore)| FoodValues { plant, herbivore })
        .filter(|food| food.is_valid())
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }

    #[must_use]
    pub fn len(&self) -> u64 {
        (self.mutation_strategies().count() as u64)
            * (self.distributions().count() as u64)
            * (self.plant_growth_rates().count() as u64)
            * (self.energy_and_thresholds().count() as u64)
            * (self.action_costs().count() as u64)
            * (self.food_values().count() as u64)
    }

    fn iter(&self) -> impl Iterator<Item = GridPoint> + use<'_> {
        iproduct!(
            self.mutation_strategies(),
            self.distributions(),
            self.plant_growth_rates(),
            self.energy_and_thresholds(),
            self.action_costs(),
            self.food_values(),
        )
        .map(
            |(
                mutation_strategy,
                distribution,
                plant_growth_rate,
                (initial_energy, max_energy, reproduction_threshold),
                action_costs,
                food_values,
            )| GridPoint {
                mutation_strategy,
                distribution,
                plant_growth_rate,
                initial_energy,
                max_energy,
                reproduction_threshold,
                action_costs,
                food_values,
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
            plant_growth_rate: &args.plant_growth_rate,
            initial_energy: &args.initial_energy,
            max_energy: &args.max_energy,
            reproduction_threshold_relative: &args.reproduction_threshold_relative,
            reproduction_threshold_absolute: &args.reproduction_threshold_absolute,
            turn_cost: &args.turn_cost,
            go_forward_cost: &args.go_forward_cost,
            consume_cost: &args.consume_cost,
            plant_food_value: &args.plant_food_value,
            herbivore_food_value: &args.herbivore_food_value,
        }
    }
}

fn run_headless_sweep(args: HeadlessArgs) -> io::Result<()> {
    let grid = Grid::from(&args);
    if grid.is_empty() {
        eprintln!(
            "Parameter grid is empty. Check mutation strategies, distribution fractions, plant growth rates, energy/reproduction ranges, action costs, and food values."
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
            let peak_counts = counts_history
                .zip(final_counts)
                .map(|(col, final_val)| col.iter().copied().max().unwrap_or(final_val));
            let max_age = history.max_age();
            let reproductions = history.reproductions();
            let max_generation_reached = history
                .generation_history()
                .iter()
                .filter_map(|g| g.youngest)
                .max()
                .unwrap_or(0);
            let final_gen = history.last_generation();
            let extinct = final_counts.herbivores == 0 && final_counts.predators == 0;

            println!(
                "[{run_id}/{total_runs}] {label} (rep {repetition}) -> {total_ticks} ticks ({status}), max gen {max_generation_reached}, max age (H:{}/P:{}), repro (Pl:{}/H:{}/P:{}), {samples} samples in {elapsed:.2?} -> {history_filename}",
                max_age.herbivores,
                max_age.predators,
                reproductions.plants,
                reproductions.herbivores,
                reproductions.predators,
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
                final_counts,
                peak_counts,
                max_age,
                reproductions,
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
) -> (SimulationHistory, u64, CellCounts<u32>) {
    let mut app = App::new();
    app.add_plugins((
        SimulationPlugin::builder()
            .grid_size(grid_size)
            .distribution(point.distribution)
            .initial_energy(point.initial_energy)
            .max_energy(point.max_energy)
            .action_costs(point.action_costs)
            .food_values(point.food_values)
            .build(),
        ReproductionPlugin::builder()
            .mutation_strategy(point.mutation_strategy)
            .reproduction_threshold(point.reproduction_threshold)
            .plant_growth_rate(point.plant_growth_rate)
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
    let repro = match point.reproduction_threshold {
        ReproductionThreshold::Relative(frac) => format!("Rel({frac:.2})"),
        ReproductionThreshold::Absolute(energy) => format!("Abs({energy})"),
    };
    format!(
        "{strategy}, dist=({:.2}/{:.2}/{:.2}), growth={:.2}, energy={}/{}, repro={repro}, costs=({}/{}/{}), food=({}/{})",
        point.distribution.plants_fraction(),
        point.distribution.herbivores_fraction(),
        point.distribution.predators_fraction(),
        point.plant_growth_rate.0,
        point.initial_energy,
        point.max_energy,
        point.action_costs.turn,
        point.action_costs.go_forward,
        point.action_costs.consume,
        point.food_values.plant,
        point.food_values.herbivore,
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
        "tick,plants,herbivores,predators,max_age_herbivores,max_age_predators,reproductions_plants,reproductions_herbivores,reproductions_predators,oldest_generation_alive,youngest_generation_alive,generation_counts"
    )?;

    let max_age_hist = history.max_age_history();
    let repro_hist = history.reproductions_history();

    for (idx, sample) in history.iter().enumerate() {
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
            "{},{},{},{},{},{},{},{},{},{},{},\"{}\"",
            sample.tick,
            sample.counts.plants,
            sample.counts.herbivores,
            sample.counts.predators,
            max_age_hist.herbivores[idx],
            max_age_hist.predators[idx],
            repro_hist.plants[idx],
            repro_hist.herbivores[idx],
            repro_hist.predators[idx],
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
                "run_id,strategy,mutation_offset,repetition,grid_width,grid_height,plants_fraction,herbivores_fraction,predators_fraction,plant_growth_rate,initial_energy,max_energy,reproduction_threshold_mode,reproduction_threshold_value,turn_cost,go_forward_cost,consume_cost,plant_food_value,herbivore_food_value,max_ticks_limit,total_ticks_survived,extinct,final_plants,final_herbivores,final_predators,peak_plants,peak_herbivores,peak_predators,max_age_herbivores,max_age_predators,reproductions_plants,reproductions_herbivores,reproductions_predators,max_generation_reached,final_oldest_generation_alive,final_youngest_generation_alive,recorded_samples,history_file"
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
        let (repro_mode, repro_val) = match summary.point.reproduction_threshold {
            ReproductionThreshold::Relative(frac) => ("Relative", format!("{frac:.4}")),
            ReproductionThreshold::Absolute(energy) => ("Absolute", energy.to_string()),
        };
        let final_oldest = summary
            .final_oldest_generation
            .map_or_else(String::new, |v| v.to_string());
        let final_youngest = summary
            .final_youngest_generation
            .map_or_else(String::new, |v| v.to_string());

        writeln!(
            self.writer,
            "{},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
            summary.run_id,
            strategy_name,
            offset_str,
            summary.repetition,
            summary.width,
            summary.height,
            summary.point.distribution.plants_fraction(),
            summary.point.distribution.herbivores_fraction(),
            summary.point.distribution.predators_fraction(),
            summary.point.plant_growth_rate.0,
            summary.point.initial_energy,
            summary.point.max_energy,
            repro_mode,
            repro_val,
            summary.point.action_costs.turn,
            summary.point.action_costs.go_forward,
            summary.point.action_costs.consume,
            summary.point.food_values.plant,
            summary.point.food_values.herbivore,
            summary.max_ticks,
            summary.total_ticks,
            summary.extinct,
            summary.final_counts.plants,
            summary.final_counts.herbivores,
            summary.final_counts.predators,
            summary.peak_counts.plants,
            summary.peak_counts.herbivores,
            summary.peak_counts.predators,
            summary.max_age.herbivores,
            summary.max_age.predators,
            summary.reproductions.plants,
            summary.reproductions.herbivores,
            summary.reproductions.predators,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_filters_invalid_thresholds_costs_food_and_growth_rates() {
        let grid = Grid {
            single_neuron_mutation: true,
            distributed_mutation: &[],
            plants_fraction: &[0.2],
            herbivores_fraction: &[0.1],
            predators_fraction: &[0.05],
            // 0.1 is valid, -0.2 and 1.5 are filtered out
            plant_growth_rate: &[
                PlantGrowthRate(-0.2),
                PlantGrowthRate(0.1),
                PlantGrowthRate(1.5),
            ],
            // (5, 10) and (5, 20) are valid; (25, 10) and (25, 20) are filtered out (initial > max)
            initial_energy: &[InitialEnergy(Energy(5.0)), InitialEnergy(Energy(25.0))],
            max_energy: &[MaxEnergy(Energy(10.0)), MaxEnergy(Energy(20.0))],
            // 0.9 is valid for both max_energy=10 and max_energy=20; 1.2 is invalid
            reproduction_threshold_relative: &[0.9, 1.2],
            // 15.0 is invalid for max_energy=10 (15 > 10), but valid for max_energy=20
            reproduction_threshold_absolute: &[Energy(0.0), Energy(15.0)],
            // Negative cost is filtered out
            turn_cost: &[Energy(-1.0), Energy(1.0)],
            go_forward_cost: &[Energy(1.5)],
            consume_cost: &[Energy(0.5)],
            // Negative food value is filtered out
            plant_food_value: &[Energy(1.0), Energy(-0.5)],
            herbivore_food_value: &[Energy(2.0)],
        };

        assert!(!grid.is_empty());
        // Valid combinations:
        // - 1 mutation strategy (SingleNeuron)
        // - 1 distribution (0.2, 0.1, 0.05)
        // - 1 plant growth rate (0.1)
        // - 3 (initial_energy, max_energy, reproduction_threshold) tuples:
        //   1. (5.0, 10.0, Relative(0.9))
        //   2. (5.0, 20.0, Relative(0.9))
        //   3. (5.0, 20.0, Absolute(15.0))
        // - 1 ActionCosts (1.0, 1.5, 0.5)
        // - 1 FoodValues (1.0, 2.0)
        assert_eq!(grid.len(), 3);
        let points: Vec<_> = grid.iter().collect();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].plant_growth_rate, PlantGrowthRate(0.1));
        assert_eq!(
            points[0].action_costs,
            ActionCosts {
                turn: Energy(1.0),
                go_forward: Energy(1.5),
                consume: Energy(0.5),
            }
        );
        assert_eq!(
            points[0].food_values,
            FoodValues {
                plant: Energy(1.0),
                herbivore: Energy(2.0),
            }
        );
        assert_eq!(
            points[0].reproduction_threshold,
            ReproductionThreshold::Relative(0.9)
        );
        assert_eq!(
            points[2].reproduction_threshold,
            ReproductionThreshold::Absolute(Energy(15.0))
        );
    }
}
