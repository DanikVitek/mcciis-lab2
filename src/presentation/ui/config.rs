use core::num::NonZeroU64;
use std::path::{
    Path,
    PathBuf,
};
use std::time::Duration;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::egui::emath::Numeric;
use bevy_egui::egui::{
    self,
};
use bevy_egui::{
    EguiContexts,
    EguiPrimaryContextPass,
};
use serde::{
    Deserialize,
    Serialize,
};

use super::inspector::{
    CharacterInspector,
    InspectedCharacter,
};
use super::{
    HistoryPlotOpen,
    VIEWPORT_MARGIN,
};
use crate::grid::GridSize;
use crate::presentation::pacing::{
    AppState,
    SimulationPlaybackState,
    SimulationSpeedMode,
};
use crate::simulation::history::SampleRate;
use crate::simulation::organism::{
    ActionCosts,
    Energy,
    FoodValues,
    Herbivore,
    InitialEnergy,
    MaxEnergy,
    Plant,
    ValueAsFood,
};
use crate::simulation::spatial::SpatialGrid;
use crate::simulation::{
    DistributedMutation,
    DistributionConfig,
    DistributionError,
    MutationStrategy,
    ReproductionThreshold,
    ResetSimulation,
    SimulationTickCount,
    SingleNeuronMutation,
    StepSimulationOnce,
};

/// Optional resource holding the filesystem path where [`SimConfig`] is automatically saved on change.
#[derive(Debug, Clone, PartialEq, Eq, Resource)]
pub struct AutoSaveSettingsPath(pub PathBuf);

/// Staged form state edited by the configuration UI before applying changes to live resources.
#[derive(Debug, Clone, Copy, PartialEq, Resource, Serialize, Deserialize)]
#[serde(
    rename = "sim_config",
    rename_all = "snake_case",
    default = "SimConfig::default_values"
)]
pub struct SimConfig {
    pub grid_size: GridSize,
    pub plants_fraction: f32,
    pub herbivores_fraction: f32,
    pub predators_fraction: f32,
    pub initial_energy: InitialEnergy,
    pub max_energy: MaxEnergy,
    pub reproduction_threshold: ReproductionThreshold,
    pub action_costs: ActionCosts,
    pub food_values: FoodValues,
    pub tick_interval: Duration,
    pub speed_mode: SimulationSpeedMode,
    pub mutation: MutationStrategy,
}

impl Numeric for Energy {
    const INTEGRAL: bool = false;

    const MIN: Self = Self(0.0);

    const MAX: Self = Self(f32::MAX);

    fn to_f64(self) -> f64 {
        f64::from(self.0)
    }

    fn from_f64(num: f64) -> Self {
        Self(num as f32)
    }
}

impl Numeric for InitialEnergy {
    const INTEGRAL: bool = Energy::INTEGRAL;

    const MIN: Self = Self(Energy::MIN);

    const MAX: Self = Self(Energy::MAX);

    fn to_f64(self) -> f64 {
        self.0.to_f64()
    }

    fn from_f64(num: f64) -> Self {
        Self(Energy::from_f64(num))
    }
}

impl Numeric for MaxEnergy {
    const INTEGRAL: bool = Energy::INTEGRAL;

    const MIN: Self = Self(Energy::MIN);

    const MAX: Self = Self(Energy::MAX);

    fn to_f64(self) -> f64 {
        self.0.to_f64()
    }

    fn from_f64(num: f64) -> Self {
        Self(Energy::from_f64(num))
    }
}

impl FromWorld for SimConfig {
    fn from_world(world: &mut World) -> Self {
        let grid_size = world
            .get_resource::<GridSize>()
            .copied()
            .unwrap_or_default();
        let dist = world
            .get_resource::<DistributionConfig>()
            .copied()
            .unwrap_or_default();
        let tick_interval = world
            .get_resource::<Time<Fixed>>()
            .map_or(Duration::from_secs_f64(0.5), Time::<Fixed>::timestep);
        let speed_mode = world
            .get_resource::<SimulationSpeedMode>()
            .copied()
            .unwrap_or_default();
        let mutation = world
            .get_resource::<MutationStrategy>()
            .copied()
            .unwrap_or_default();
        let initial_energy = world
            .get_resource::<InitialEnergy>()
            .copied()
            .unwrap_or_default();
        let max_energy = world
            .get_resource::<MaxEnergy>()
            .copied()
            .unwrap_or_default();
        let reproduction_threshold = world
            .get_resource::<ReproductionThreshold>()
            .copied()
            .unwrap_or_default();
        let action_costs = world
            .get_resource::<ActionCosts>()
            .copied()
            .unwrap_or_default();
        let food_values = world
            .get_resource::<FoodValues>()
            .copied()
            .unwrap_or_default();

        Self {
            grid_size,
            plants_fraction: dist.plants_fraction(),
            herbivores_fraction: dist.herbivores_fraction(),
            predators_fraction: dist.predators_fraction(),
            initial_energy,
            max_energy,
            reproduction_threshold,
            action_costs,
            food_values,
            tick_interval,
            speed_mode,
            mutation,
        }
    }
}

impl SimConfig {
    /// Returns the baseline default configuration values without requiring a [`World`].
    #[must_use]
    pub fn default_values() -> Self {
        let dist = DistributionConfig::default();
        Self {
            grid_size: GridSize::default(),
            plants_fraction: dist.plants_fraction(),
            herbivores_fraction: dist.herbivores_fraction(),
            predators_fraction: dist.predators_fraction(),
            initial_energy: InitialEnergy::default(),
            max_energy: MaxEnergy::default(),
            reproduction_threshold: ReproductionThreshold::default(),
            action_costs: ActionCosts::default(),
            food_values: FoodValues::default(),
            tick_interval: Duration::from_secs_f64(0.5),
            speed_mode: SimulationSpeedMode::default(),
            mutation: MutationStrategy::default(),
        }
    }

    /// Returns the default OS configuration file path for persisting settings across restarts.
    #[must_use]
    pub fn default_settings_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("life-sim").join("settings.toml"))
    }

    /// Serializes and writes this configuration as TOML to `path`, creating parent directories if needed.
    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)?;
        }
        let contents =
            toml::to_string_pretty(self).map_err(|err| std::io::Error::other(err.to_string()))?;
        std::fs::write(path, contents)
    }

    /// Reads and deserializes a [`SimConfig`] from a TOML file at `path`.
    pub fn load_from_path(path: &Path) -> std::io::Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        toml::from_str(&contents)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))
    }

    /// Loads the saved configuration from [`Self::default_settings_path`], or falls back to [`Self::default_values`].
    #[must_use]
    pub fn load_system_or_default() -> Self {
        Self::default_settings_path()
            .and_then(|path| Self::load_from_path(&path).ok())
            .unwrap_or_else(Self::default_values)
    }

    /// Applies live playback pacing changes (`tick_interval` and `speed_mode`) without resetting the world.
    pub fn apply_pacing(&self, fixed_time: &mut Time<Fixed>, speed_mode: &mut SimulationSpeedMode) {
        fixed_time.set_timestep(self.tick_interval);
        *speed_mode = self.speed_mode;
    }

    /// Validates staged distribution fractions, inserts all initial simulation [`Resource`]s into the [`World`],
    /// and triggers [`ResetSimulation`] to seed the initial population.
    pub fn insert_resources_and_start(
        &self,
        commands: &mut Commands,
        fixed_time: &mut Time<Fixed>,
        speed_mode: &mut SimulationSpeedMode,
    ) -> Result<(), DistributionError> {
        let validated = DistributionConfig::builder()
            .plants_fraction(self.plants_fraction)
            .herbivores_fraction(self.herbivores_fraction)
            .predators_fraction(self.predators_fraction)
            .build()?;

        self.apply_pacing(fixed_time, speed_mode);
        commands.insert_resource(self.grid_size);
        commands.insert_resource(validated);
        commands.insert_resource(SpatialGrid::new(self.grid_size));
        commands.insert_resource(self.mutation);
        commands.insert_resource(self.initial_energy);
        commands.insert_resource(self.max_energy);
        commands.insert_resource(self.reproduction_threshold);
        commands.insert_resource(self.action_costs);
        commands.insert_resource(self.food_values);
        commands.insert_resource(SimulationTickCount::default());
        commands.trigger(ResetSimulation);
        Ok(())
    }

    /// Validates staged distribution fractions, updates [`GridSize`] and [`DistributionConfig`],
    /// and triggers [`ResetSimulation`] to re-seed the world.
    pub fn apply_world_and_reset(
        &self,
        commands: &mut Commands,
        grid_size: &mut GridSize,
        distribution: &mut DistributionConfig,
    ) -> Result<(), DistributionError> {
        let validated = DistributionConfig::builder()
            .plants_fraction(self.plants_fraction)
            .herbivores_fraction(self.herbivores_fraction)
            .predators_fraction(self.predators_fraction)
            .build()?;

        *grid_size = self.grid_size;
        *distribution = validated;
        commands.trigger(ResetSimulation);
        Ok(())
    }
}

#[derive(SystemParam)]
struct LiveLifecycleResources<'w> {
    initial_energy: ResMut<'w, InitialEnergy>,
    max_energy: ResMut<'w, MaxEnergy>,
    reproduction_threshold: Option<ResMut<'w, ReproductionThreshold>>,
    mutation_strategy: Option<ResMut<'w, MutationStrategy>>,
    action_costs: Option<ResMut<'w, ActionCosts>>,
    food_values: Option<ResMut<'w, FoodValues>>,
}

impl LiveLifecycleResources<'_> {
    fn sync_food_values(&mut self, food_values: FoodValues) {
        if let Some(ref mut res) = self.food_values {
            **res = food_values;
        }
    }

    fn sync_all_from_config(&mut self, config: &SimConfig) {
        *self.initial_energy = config.initial_energy;
        *self.max_energy = config.max_energy;
        if let Some(ref mut repro) = self.reproduction_threshold {
            **repro = config.reproduction_threshold;
        }
        if let Some(ref mut mutation) = self.mutation_strategy {
            **mutation = config.mutation;
        }
        if let Some(ref mut costs) = self.action_costs {
            **costs = config.action_costs;
        }
        self.sync_food_values(config.food_values);
    }
}

/// Helper commands for playback controls (Pause / Resume / Step Once) from UI widgets or hotkeys.
pub fn toggle_playback_pause(
    current: &State<SimulationPlaybackState>,
    next: &mut NextState<SimulationPlaybackState>,
) {
    match current.get() {
        SimulationPlaybackState::Running => next.set(SimulationPlaybackState::Paused),
        SimulationPlaybackState::Paused => next.set(SimulationPlaybackState::Running),
    }
}

pub fn trigger_single_step(commands: &mut Commands) {
    commands.trigger(StepSimulationOnce);
}

pub struct SimConfigUiPlugin;

impl Plugin for SimConfigUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SimConfig>()
            .init_resource::<InspectedCharacter>()
            .add_systems(
                OnEnter(AppState::Simulation),
                (setup_simulation_from_config, ApplyDeferred).chain(),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    render_initial_setup_ui.run_if(in_state(AppState::Setup)),
                    render_config_ui.run_if(in_state(AppState::Simulation)),
                ),
            )
            .add_systems(
                PostUpdate,
                (
                    sync_food_values_to_entities.run_if(in_state(AppState::Simulation)),
                    auto_save_settings_system,
                ),
            );
    }
}

fn sync_food_values_to_entities(
    food_values: Option<Res<FoodValues>>,
    mut plants: Query<&mut ValueAsFood, (With<Plant>, Without<Herbivore>)>,
    mut herbivores: Query<&mut ValueAsFood, (With<Herbivore>, Without<Plant>)>,
) {
    if let Some(values) = food_values
        && values.is_changed()
    {
        for mut food in &mut plants {
            food.0 = values.plant;
        }
        for mut food in &mut herbivores {
            food.0 = values.herbivore;
        }
    }
}

fn auto_save_settings_system(
    config: Res<SimConfig>,
    auto_save_path: Option<Res<AutoSaveSettingsPath>>,
) {
    if let Some(path) = auto_save_path
        && config.is_changed()
        && let Err(err) = config.save_to_path(&path.0)
    {
        warn!(
            "Failed to auto-save settings to {}: {err}",
            path.0.display()
        );
    }
}

/// Runs on [`OnEnter(AppState::Simulation)`] to insert all simulation [`Resource`]s from [`SimConfig`]
/// and seed the initial world.
pub fn setup_simulation_from_config(
    config: Res<SimConfig>,
    mut fixed_time: ResMut<Time<Fixed>>,
    mut speed_mode: ResMut<SimulationSpeedMode>,
    mut commands: Commands,
) {
    config
        .insert_resources_and_start(&mut commands, &mut fixed_time, &mut speed_mode)
        .expect("validated SimConfig before entering AppState::Simulation");
}

fn render_reproduction_threshold_controls(ui: &mut egui::Ui, config: &mut SimConfig) -> bool {
    let mut changed = false;
    let is_relative = matches!(
        config.reproduction_threshold,
        ReproductionThreshold::Relative(_)
    );

    ui.horizontal(|ui| {
        ui.label("Repro threshold:");
        if ui.selectable_label(is_relative, "Relative").clicked() && !is_relative {
            config.reproduction_threshold = ReproductionThreshold::default();
            changed = true;
        }
        if ui.selectable_label(!is_relative, "Absolute").clicked() && is_relative {
            let abs = config.reproduction_threshold.min_energy(config.max_energy);
            config.reproduction_threshold = ReproductionThreshold::Absolute(Energy(abs.0.max(0.1)));
            changed = true;
        }
    });

    match config.reproduction_threshold {
        ReproductionThreshold::Relative(ref mut fraction) => {
            if ui
                .add(
                    egui::Slider::new(fraction, 0.0..=1.0)
                        .text("Fraction")
                        .fixed_decimals(2),
                )
                .changed()
            {
                changed = true;
            }
        }
        ReproductionThreshold::Absolute(ref mut energy) => {
            if ui
                .add(
                    egui::DragValue::new(energy)
                        .range(0.1..=10_000.0)
                        .speed(0.1)
                        .fixed_decimals(1)
                        .prefix("Energy ≥ "),
                )
                .changed()
            {
                changed = true;
            }
        }
    }

    changed
}

fn render_action_economy_controls(ui: &mut egui::Ui, config: &mut SimConfig) -> (bool, bool) {
    let mut costs_changed = false;
    let mut food_changed = false;

    ui.label(
        egui::RichText::new("Action Economy & Food")
            .color(egui::Color32::from_rgb(170, 215, 255))
            .strong(),
    );
    ui.add_space(4.0);

    egui::Grid::new(ui.id().with("action_economy_grid"))
        .num_columns(2)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            ui.label("Turn cost:");
            if ui
                .add(
                    egui::DragValue::new(&mut config.action_costs.turn)
                        .range(0.0..=1000.0)
                        .speed(0.1)
                        .fixed_decimals(2),
                )
                .changed()
            {
                costs_changed = true;
            }
            ui.end_row();

            ui.label("Move forward cost:");
            if ui
                .add(
                    egui::DragValue::new(&mut config.action_costs.go_forward)
                        .range(0.0..=1000.0)
                        .speed(0.1)
                        .fixed_decimals(2),
                )
                .changed()
            {
                costs_changed = true;
            }
            ui.end_row();

            ui.label("Consume cost:");
            if ui
                .add(
                    egui::DragValue::new(&mut config.action_costs.consume)
                        .range(0.0..=1000.0)
                        .speed(0.1)
                        .fixed_decimals(2),
                )
                .changed()
            {
                costs_changed = true;
            }
            ui.end_row();

            ui.label("Algae food value:");
            if ui
                .add(
                    egui::DragValue::new(&mut config.food_values.plant)
                        .range(0.0..=1000.0)
                        .speed(0.1)
                        .fixed_decimals(2),
                )
                .changed()
            {
                food_changed = true;
            }
            ui.end_row();

            ui.label("Herbivore food value:");
            if ui
                .add(
                    egui::DragValue::new(&mut config.food_values.herbivore)
                        .range(0.0..=1000.0)
                        .speed(0.1)
                        .fixed_decimals(2),
                )
                .changed()
            {
                food_changed = true;
            }
            ui.end_row();
        });

    (costs_changed, food_changed)
}

fn configured_file_dialog() -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new()
        .add_filter("TOML Settings", &["toml"])
        .set_file_name("settings.toml");
    if let Some(default_path) = SimConfig::default_settings_path()
        && let Some(parent) = default_path.parent()
    {
        let _ = std::fs::create_dir_all(parent);
        if parent.exists() {
            return dialog.set_directory(parent);
        }
    }
    dialog
}

fn render_settings_persistence_controls(ui: &mut egui::Ui, config: &mut SimConfig) -> bool {
    let mut loaded_new_config = false;
    ui.horizontal(|ui| {
        if ui.button("💾 Save Settings...").clicked()
            && let Some(path) = configured_file_dialog().save_file()
            && let Err(err) = config.save_to_path(&path)
        {
            warn!("Failed to save settings to {}: {err}", path.display());
        }

        if ui.button("📂 Load Settings...").clicked()
            && let Some(path) = configured_file_dialog().pick_file()
        {
            match SimConfig::load_from_path(&path) {
                Ok(loaded) => {
                    *config = loaded;
                    loaded_new_config = true;
                }
                Err(err) => {
                    warn!("Failed to load settings from {}: {err}", path.display());
                }
            }
        }
    });
    loaded_new_config
}

fn render_initial_setup_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<SimConfig>,
    mut next_app_state: ResMut<NextState<AppState>>,
    mut sample_rate: Option<ResMut<SampleRate>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Initial Simulation Setup")
        .id(egui::Id::new("sim_initial_setup_modal"))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .default_width(340.0)
        .resizable(false)
        .collapsible(false)
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(egui::Color32::from_rgba_unmultiplied(12, 24, 38, 242))
                .stroke(egui::Stroke::new(
                    1.5,
                    egui::Color32::from_rgb(56, 108, 156),
                ))
                .corner_radius(10.0)
                .inner_margin(16.0),
        )
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Configure parameters for the initial run before starting.")
                    .color(egui::Color32::from_rgb(175, 205, 235)),
            );
            ui.add_space(6.0);
            render_settings_persistence_controls(ui, &mut config);
            ui.add_space(6.0);
            ui.separator();

            // --- 1. World & Seeding Configuration ---
            ui.label(
                egui::RichText::new("World & Seeding")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Grid W × H:");
                let mut w = config.grid_size.width();
                let mut h = config.grid_size.height();
                let w_changed = ui
                    .add(egui::DragValue::new(&mut w).range(10..=500).prefix("W: "))
                    .changed();
                let h_changed = ui
                    .add(egui::DragValue::new(&mut h).range(10..=500).prefix("H: "))
                    .changed();
                if w_changed || h_changed {
                    config.grid_size = GridSize::new(w.max(10), h.max(10));
                }
            });

            ui.add(
                egui::Slider::new(&mut config.plants_fraction, 0.0..=1.0)
                    .text("Algae")
                    .fixed_decimals(2),
            );
            ui.add(
                egui::Slider::new(&mut config.herbivores_fraction, 0.0..=1.0)
                    .text("Herbivores")
                    .fixed_decimals(2),
            );
            ui.add(
                egui::Slider::new(&mut config.predators_fraction, 0.0..=1.0)
                    .text("Predators")
                    .fixed_decimals(2),
            );

            let total_fraction =
                config.plants_fraction + config.herbivores_fraction + config.predators_fraction;
            let fractions_valid = total_fraction <= 1.0;

            if !fractions_valid {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 100, 100),
                    format!(
                        "Total density ({:.0}%) exceeds 100%!",
                        total_fraction * 100.0
                    ),
                );
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "Initial occupancy: {:.0}% of {} tiles",
                        total_fraction * 100.0,
                        u32::from(config.grid_size.width()) * u32::from(config.grid_size.height())
                    ))
                    .small()
                    .color(egui::Color32::from_rgb(155, 180, 205)),
                );
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 2. Lifecycle & Mutation ---
            ui.label(
                egui::RichText::new("Lifecycle & Evolution")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Initial energy:");
                ui.add(
                    egui::DragValue::new(&mut config.initial_energy)
                        .range(0.1..=10_000.0)
                        .speed(0.1)
                        .fixed_decimals(1),
                );
            });
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                ui.label("Max energy:");
                ui.add(
                    egui::DragValue::new(&mut config.max_energy)
                        .range(0.1..=10_000.0)
                        .speed(0.1)
                        .fixed_decimals(1),
                );
            });
            ui.add_space(2.0);

            render_reproduction_threshold_controls(ui, &mut config);

            let initial_valid = config.initial_energy.0 <= config.max_energy.0;
            if !initial_valid {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 100, 100),
                    "Initial energy must not exceed Max energy!",
                );
            }

            let repro_valid = config
                .reproduction_threshold
                .is_valid_for(config.max_energy);
            if !repro_valid {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 100, 100),
                    "Reproduction threshold must not exceed Max energy!",
                );
            }
            let energy_valid = initial_valid && repro_valid;
            ui.add_space(4.0);

            let current_kind = match config.mutation {
                MutationStrategy::SingleNeuron { .. } => "Single Neuron",
                MutationStrategy::Distributed { .. } => "Distributed",
            };

            egui::ComboBox::from_label("Mutation Strategy")
                .selected_text(current_kind)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            matches!(config.mutation, MutationStrategy::SingleNeuron { .. }),
                            "Single Neuron",
                        )
                        .clicked()
                        && !matches!(config.mutation, MutationStrategy::SingleNeuron { .. })
                    {
                        config.mutation = MutationStrategy::SingleNeuron(SingleNeuronMutation);
                    }

                    if ui
                        .selectable_label(
                            matches!(config.mutation, MutationStrategy::Distributed { .. }),
                            "Distributed",
                        )
                        .clicked()
                        && !matches!(config.mutation, MutationStrategy::Distributed { .. })
                    {
                        config.mutation =
                            MutationStrategy::Distributed(DistributedMutation::default());
                    }
                });

            if let MutationStrategy::Distributed(DistributedMutation { ref mut offset }) =
                config.mutation
            {
                ui.add_space(2.0);
                ui.add(
                    egui::Slider::new(offset, 0.01..=2.0)
                        .text("Offset")
                        .fixed_decimals(2),
                );
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 2b. Action Economy & Food ---
            render_action_economy_controls(ui, &mut config);

            ui.add_space(6.0);
            ui.separator();

            // --- 3. Pacing & History Recording ---
            ui.label(
                egui::RichText::new("Pacing & Recording")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Speed Mode:");
                ui.selectable_value(
                    &mut config.speed_mode,
                    SimulationSpeedMode::FixedStep,
                    "Fixed Step",
                );
                ui.selectable_value(
                    &mut config.speed_mode,
                    SimulationSpeedMode::Uncapped,
                    "Uncapped",
                );
            });

            if config.speed_mode == SimulationSpeedMode::FixedStep {
                let mut interval_ms = config.tick_interval.as_secs_f64() * 1000.0;
                if ui
                    .add(
                        egui::Slider::new(&mut interval_ms, 10.0..=2000.0)
                            .logarithmic(true)
                            .text("Tick (ms)"),
                    )
                    .changed()
                {
                    config.tick_interval = Duration::from_secs_f64(interval_ms / 1000.0);
                }
            }

            if let Some(ref mut rate) = sample_rate {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.label("Record every:");
                    let mut ticks = rate.once_every.get();
                    if ui
                        .add(
                            egui::DragValue::new(&mut ticks)
                                .range(1..=10_000)
                                .suffix(" ticks"),
                        )
                        .changed()
                        && let Some(nz) = NonZeroU64::new(ticks.max(1))
                    {
                        rate.once_every = nz;
                    }
                });
            }

            ui.add_space(10.0);
            let can_start = fractions_valid && energy_valid;
            let enter_pressed = can_start && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let button_clicked = ui
                .add_enabled_ui(can_start, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(
                            egui::RichText::new("▶ Accept & Start Simulation (Enter)").strong(),
                        ),
                    )
                })
                .inner
                .clicked();

            if button_clicked || enter_pressed {
                next_app_state.set(AppState::Simulation);
            }
        });

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn render_config_ui(
    mut contexts: EguiContexts,
    mut config: ResMut<SimConfig>,
    playback_state: Res<State<SimulationPlaybackState>>,
    mut next_playback_state: ResMut<NextState<SimulationPlaybackState>>,
    mut fixed_time: ResMut<Time<Fixed>>,
    mut speed_mode: ResMut<SimulationSpeedMode>,
    mut lifecycle: LiveLifecycleResources,
    mut grid_size: ResMut<GridSize>,
    mut distribution: ResMut<DistributionConfig>,
    mut sample_rate: Option<ResMut<SampleRate>>,
    tick_count: Res<SimulationTickCount>,
    inspector: CharacterInspector,
    mut plot_open: ResMut<HistoryPlotOpen>,
    mut commands: Commands,
) -> Result {
    if plot_open.0 {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Simulation Controls")
        .id(egui::Id::new("sim_config_left_panel"))
        .anchor(
            egui::Align2::LEFT_TOP,
            egui::vec2(VIEWPORT_MARGIN, VIEWPORT_MARGIN),
        )
        .default_width(280.0)
        .resizable(false)
        .collapsible(true)
        .frame(
            egui::Frame::window(&ctx.global_style())
                .fill(egui::Color32::from_rgba_unmultiplied(12, 24, 38, 232))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgb(56, 108, 156),
                ))
                .corner_radius(8.0)
                .inner_margin(12.0),
        )
        .show(ctx, |ui| {
            let totals = inspector.spatial_grid.total_counts();
            let has_organisms = totals.herbivores > 0 || totals.predators > 0;
            let is_paused = *playback_state.get() == SimulationPlaybackState::Paused;

            if render_settings_persistence_controls(ui, &mut config) {
                config.apply_pacing(&mut fixed_time, &mut speed_mode);
                lifecycle.sync_all_from_config(&config);
            }
            ui.add_space(4.0);
            ui.separator();

            // --- 1. Playback & Pacing ---
            ui.label(
                egui::RichText::new("Playback & Pacing")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let pause_label = if is_paused {
                    "▶ Resume (Space)"
                } else {
                    "⏸ Pause (Space)"
                };
                if ui
                    .add_enabled(has_organisms, egui::Button::new(pause_label))
                    .clicked()
                {
                    toggle_playback_pause(&playback_state, &mut next_playback_state);
                }

                if ui
                    .add_enabled(is_paused && has_organisms, egui::Button::new("⏭ Step (S)"))
                    .clicked()
                {
                    trigger_single_step(&mut commands);
                }
            });

            if !has_organisms {
                ui.add_space(2.0);
                ui.colored_label(
                    egui::Color32::from_rgb(245, 165, 95),
                    egui::RichText::new("No organisms left — reset world to continue.").small(),
                );
            }

            ui.add_space(6.0);

            let mut pacing_changed = false;
            ui.horizontal(|ui| {
                ui.label("Speed Mode:");
                if ui
                    .selectable_value(
                        &mut config.speed_mode,
                        SimulationSpeedMode::FixedStep,
                        "Fixed Step",
                    )
                    .clicked()
                {
                    pacing_changed = true;
                }
                if ui
                    .selectable_value(
                        &mut config.speed_mode,
                        SimulationSpeedMode::Uncapped,
                        "Uncapped",
                    )
                    .clicked()
                {
                    pacing_changed = true;
                }
            });

            if config.speed_mode == SimulationSpeedMode::FixedStep {
                let mut interval_ms = config.tick_interval.as_secs_f64() * 1000.0;
                if ui
                    .add(
                        egui::Slider::new(&mut interval_ms, 10.0..=2000.0)
                            .logarithmic(true)
                            .text("Tick (ms)"),
                    )
                    .changed()
                {
                    config.tick_interval = Duration::from_secs_f64(interval_ms / 1000.0);
                    pacing_changed = true;
                }
            }

            if pacing_changed {
                config.apply_pacing(&mut fixed_time, &mut speed_mode);
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 2. Lifecycle ---
            ui.label(
                egui::RichText::new("Lifecycle")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            let mut initial_energy_changed = false;
            ui.horizontal(|ui| {
                ui.label("Initial energy:");
                if ui
                    .add(
                        egui::DragValue::new(&mut config.initial_energy)
                            .range(0.1..=10_000.0)
                            .speed(0.1)
                            .fixed_decimals(1),
                    )
                    .changed()
                {
                    initial_energy_changed = true;
                }
            });
            if initial_energy_changed {
                *lifecycle.initial_energy = config.initial_energy;
            }
            ui.add_space(4.0);

            let mut max_energy_changed = false;
            ui.horizontal(|ui| {
                ui.label("Max energy:");
                if ui
                    .add(
                        egui::DragValue::new(&mut config.max_energy)
                            .range(0.1..=10_000.0)
                            .speed(0.1)
                            .fixed_decimals(1),
                    )
                    .changed()
                {
                    max_energy_changed = true;
                }
            });
            if max_energy_changed {
                *lifecycle.max_energy = config.max_energy;
            }
            ui.add_space(4.0);

            if render_reproduction_threshold_controls(ui, &mut config)
                && let Some(ref mut repro_threshold) = lifecycle.reproduction_threshold
            {
                **repro_threshold = config.reproduction_threshold;
            }
            ui.add_space(4.0);

            let mut mutation_changed = false;
            let current_kind = match config.mutation {
                MutationStrategy::SingleNeuron { .. } => "Single Neuron",
                MutationStrategy::Distributed { .. } => "Distributed",
            };

            egui::ComboBox::from_label("Mutation Strategy")
                .selected_text(current_kind)
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            matches!(config.mutation, MutationStrategy::SingleNeuron { .. }),
                            "Single Neuron",
                        )
                        .clicked()
                        && !matches!(config.mutation, MutationStrategy::SingleNeuron { .. })
                    {
                        config.mutation = MutationStrategy::SingleNeuron(SingleNeuronMutation);
                        mutation_changed = true;
                    }

                    if ui
                        .selectable_label(
                            matches!(config.mutation, MutationStrategy::Distributed { .. }),
                            "Distributed",
                        )
                        .clicked()
                        && !matches!(config.mutation, MutationStrategy::Distributed { .. })
                    {
                        config.mutation =
                            MutationStrategy::Distributed(DistributedMutation::default());
                        mutation_changed = true;
                    }
                });

            if let MutationStrategy::Distributed(DistributedMutation { ref mut offset }) =
                config.mutation
            {
                ui.add_space(2.0);
                if ui
                    .add(
                        egui::Slider::new(offset, 0.01..=2.0)
                            .text("Offset")
                            .fixed_decimals(2),
                    )
                    .changed()
                {
                    mutation_changed = true;
                }
            }

            if mutation_changed && let Some(ref mut mutation_strategy) = lifecycle.mutation_strategy
            {
                **mutation_strategy = config.mutation;
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 2b. Action Economy & Food ---
            let (costs_changed, food_changed) = render_action_economy_controls(ui, &mut config);
            if costs_changed && let Some(ref mut action_costs) = lifecycle.action_costs {
                **action_costs = config.action_costs;
            }
            if food_changed {
                lifecycle.sync_food_values(config.food_values);
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 3. Live Ecosystem Telemetry ---
            ui.label(
                egui::RichText::new("Live Telemetry")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            egui::Grid::new("telemetry_grid")
                .num_columns(2)
                .spacing([16.0, 4.0])
                .show(ui, |ui| {
                    ui.label("Tick:");
                    ui.label(egui::RichText::new(format!("{}", tick_count.0)).monospace());
                    ui.end_row();

                    ui.colored_label(egui::Color32::from_rgb(90, 220, 120), "Algae (Plants):");
                    ui.label(egui::RichText::new(format!("{}", totals.plants)).monospace());
                    ui.end_row();

                    ui.colored_label(egui::Color32::from_rgb(240, 215, 70), "Herbivores:");
                    ui.label(egui::RichText::new(format!("{}", totals.herbivores)).monospace());
                    ui.end_row();

                    ui.colored_label(egui::Color32::from_rgb(240, 85, 85), "Predators:");
                    ui.label(egui::RichText::new(format!("{}", totals.predators)).monospace());
                    ui.end_row();
                });

            if let Some(ref mut rate) = sample_rate {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Record every:");
                    let mut ticks = rate.once_every.get();
                    if ui
                        .add(
                            egui::DragValue::new(&mut ticks)
                                .range(1..=10_000)
                                .suffix(" ticks"),
                        )
                        .changed()
                        && let Some(nz) = NonZeroU64::new(ticks.max(1))
                    {
                        rate.once_every = nz;
                    }
                });
            }

            ui.add_space(6.0);
            if ui
                .add_enabled_ui(has_organisms, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 26.0],
                        egui::Button::new("🎲 Follow Random Character"),
                    )
                })
                .inner
                .clicked()
            {
                commands.insert_resource(InspectedCharacter(inspector.pick_random_character()));
            }

            ui.add_space(4.0);
            if ui
                .add_sized(
                    [ui.available_width(), 26.0],
                    egui::Button::new("📈 Open History Plot (Tab)"),
                )
                .clicked()
            {
                plot_open.0 = true;
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 4. World & Seeding Configuration ---
            ui.label(
                egui::RichText::new("World & Seeding")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Grid W × H:");
                let mut w = config.grid_size.width();
                let mut h = config.grid_size.height();
                let w_changed = ui
                    .add(egui::DragValue::new(&mut w).range(10..=500).prefix("W: "))
                    .changed();
                let h_changed = ui
                    .add(egui::DragValue::new(&mut h).range(10..=500).prefix("H: "))
                    .changed();
                if w_changed || h_changed {
                    config.grid_size = GridSize::new(w.max(10), h.max(10));
                }
            });

            ui.add(
                egui::Slider::new(&mut config.plants_fraction, 0.0..=1.0)
                    .text("Algae")
                    .fixed_decimals(2),
            );
            ui.add(
                egui::Slider::new(&mut config.herbivores_fraction, 0.0..=1.0)
                    .text("Herbivores")
                    .fixed_decimals(2),
            );
            ui.add(
                egui::Slider::new(&mut config.predators_fraction, 0.0..=1.0)
                    .text("Predators")
                    .fixed_decimals(2),
            );

            let total_fraction =
                config.plants_fraction + config.herbivores_fraction + config.predators_fraction;
            let fractions_valid = total_fraction <= 1.0;

            if !fractions_valid {
                ui.colored_label(
                    egui::Color32::from_rgb(255, 100, 100),
                    format!(
                        "Total density ({:.0}%) exceeds 100%!",
                        total_fraction * 100.0
                    ),
                );
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "Initial occupancy: {:.0}% of {} tiles",
                        total_fraction * 100.0,
                        u32::from(config.grid_size.width()) * u32::from(config.grid_size.height())
                    ))
                    .small()
                    .color(egui::Color32::from_rgb(155, 180, 205)),
                );
            }

            ui.add_space(6.0);
            if ui
                .add_enabled_ui(fractions_valid, |ui| {
                    ui.add_sized(
                        [ui.available_width(), 28.0],
                        egui::Button::new("🔄 Apply & Reset World"),
                    )
                })
                .inner
                .clicked()
            {
                let _ =
                    config.apply_world_and_reset(&mut commands, &mut grid_size, &mut distribution);
            }
        });

    Ok(())
}
