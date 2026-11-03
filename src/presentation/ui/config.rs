use core::num::NonZeroU64;
use std::time::Duration;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use crate::{
    grid::GridSize,
    presentation::pacing::{SimulationPlaybackState, SimulationSpeedMode},
    simulation::{
        DistributionConfig, DistributionError, ResetSimulation, SimulationTickCount,
        StepSimulationOnce, history::SampleRate, spatial::SpatialGrid,
    },
};

use super::{HistoryPlotOpen, VIEWPORT_MARGIN};

/// Staged form state edited by the configuration UI before applying changes to live resources.
#[derive(Debug, Clone, Copy, PartialEq, Resource)]
pub struct SimConfigDraft {
    pub grid_size: GridSize,
    pub plants_fraction: f32,
    pub herbivores_fraction: f32,
    pub predators_fraction: f32,
    pub tick_interval: Duration,
    pub speed_mode: SimulationSpeedMode,
}

impl FromWorld for SimConfigDraft {
    fn from_world(world: &mut World) -> Self {
        let grid_size = *world.resource::<GridSize>();
        let dist = *world.resource::<DistributionConfig>();
        let tick_interval = world.resource::<Time<Fixed>>().timestep();
        let speed_mode = *world.resource::<SimulationSpeedMode>();

        Self {
            grid_size,
            plants_fraction: dist.plants_fraction(),
            herbivores_fraction: dist.herbivores_fraction(),
            predators_fraction: dist.predators_fraction(),
            tick_interval,
            speed_mode,
        }
    }
}

impl SimConfigDraft {
    /// Applies live playback pacing changes (`tick_interval` and `speed_mode`) without resetting the world.
    pub fn apply_pacing(&self, fixed_time: &mut Time<Fixed>, speed_mode: &mut SimulationSpeedMode) {
        fixed_time.set_timestep(self.tick_interval);
        *speed_mode = self.speed_mode;
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
        app.init_resource::<SimConfigDraft>()
            .add_systems(EguiPrimaryContextPass, render_config_ui);
    }
}

#[allow(clippy::too_many_arguments)]
fn render_config_ui(
    mut contexts: EguiContexts,
    mut draft: ResMut<SimConfigDraft>,
    playback_state: Res<State<SimulationPlaybackState>>,
    mut next_playback_state: ResMut<NextState<SimulationPlaybackState>>,
    mut fixed_time: ResMut<Time<Fixed>>,
    mut speed_mode: ResMut<SimulationSpeedMode>,
    mut grid_size: ResMut<GridSize>,
    mut distribution: ResMut<DistributionConfig>,
    mut sample_rate: Option<ResMut<SampleRate>>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
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
            let totals = spatial_grid.total_counts();
            let has_organisms = totals.herbivores > 0 || totals.predators > 0;
            let is_paused = *playback_state.get() == SimulationPlaybackState::Paused;

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
                        &mut draft.speed_mode,
                        SimulationSpeedMode::FixedStep,
                        "Fixed Step",
                    )
                    .clicked()
                {
                    pacing_changed = true;
                }
                if ui
                    .selectable_value(
                        &mut draft.speed_mode,
                        SimulationSpeedMode::Uncapped,
                        "Uncapped",
                    )
                    .clicked()
                {
                    pacing_changed = true;
                }
            });

            if draft.speed_mode == SimulationSpeedMode::FixedStep {
                let mut interval_ms = draft.tick_interval.as_secs_f64() * 1000.0;
                if ui
                    .add(
                        egui::Slider::new(&mut interval_ms, 10.0..=2000.0)
                            .logarithmic(true)
                            .text("Tick (ms)"),
                    )
                    .changed()
                {
                    draft.tick_interval = Duration::from_secs_f64(interval_ms / 1000.0);
                    pacing_changed = true;
                }
            }

            if pacing_changed {
                draft.apply_pacing(&mut fixed_time, &mut speed_mode);
            }

            ui.add_space(6.0);
            ui.separator();

            // --- 2. Live Ecosystem Telemetry ---
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

            // --- 3. World & Seeding Configuration ---
            ui.label(
                egui::RichText::new("World & Seeding")
                    .color(egui::Color32::from_rgb(170, 215, 255))
                    .strong(),
            );
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Grid W × H:");
                let mut w = draft.grid_size.width();
                let mut h = draft.grid_size.height();
                let w_changed = ui
                    .add(egui::DragValue::new(&mut w).range(10..=500).prefix("W: "))
                    .changed();
                let h_changed = ui
                    .add(egui::DragValue::new(&mut h).range(10..=500).prefix("H: "))
                    .changed();
                if w_changed || h_changed {
                    draft.grid_size = GridSize::new(w.max(10), h.max(10));
                }
            });

            ui.add(
                egui::Slider::new(&mut draft.plants_fraction, 0.0..=1.0)
                    .text("Algae")
                    .fixed_decimals(2),
            );
            ui.add(
                egui::Slider::new(&mut draft.herbivores_fraction, 0.0..=1.0)
                    .text("Herbivores")
                    .fixed_decimals(2),
            );
            ui.add(
                egui::Slider::new(&mut draft.predators_fraction, 0.0..=1.0)
                    .text("Predators")
                    .fixed_decimals(2),
            );

            let total_fraction =
                draft.plants_fraction + draft.herbivores_fraction + draft.predators_fraction;
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
                        u32::from(draft.grid_size.width()) * u32::from(draft.grid_size.height())
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
                    draft.apply_world_and_reset(&mut commands, &mut grid_size, &mut distribution);
            }
        });

    Ok(())
}
