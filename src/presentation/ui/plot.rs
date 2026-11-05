use core::num::NonZeroU64;
use std::collections::BTreeSet;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};

use crate::{
    presentation::pacing::AppState,
    simulation::{
        SimulationTickCount,
        history::{GenerationSample, SampleRate, SimulationHistory},
        spatial::SpatialGrid,
    },
};

use super::{HistoryPlotOpen, VIEWPORT_MARGIN};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryPlotTab {
    #[default]
    Population,
    Generations,
}

pub struct HistoryPlotUiPlugin;

impl Plugin for HistoryPlotUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            EguiPrimaryContextPass,
            render_history_plot_ui.run_if(in_state(AppState::Simulation)),
        );
    }
}

fn build_series_points<'a>(
    ticks: &[u64],
    values: &[u32],
    last_tick: Option<u64>,
) -> PlotPoints<'a> {
    let mut pts: Vec<[f64; 2]> = Vec::with_capacity(ticks.len() + 1);
    for (&t, &c) in ticks.iter().zip(values.iter()) {
        pts.push([t as f64, f64::from(c)]);
    }
    if let (Some(lt), Some(&last_t), Some(&last_c)) = (last_tick, ticks.last(), values.last())
        && lt > last_t
    {
        pts.push([lt as f64, f64::from(last_c)]);
    }
    PlotPoints::from(pts)
}

fn build_optional_series_points<'a>(
    ticks: &[u64],
    values: impl IntoIterator<Item = Option<u32>>,
    last_tick: Option<u64>,
) -> PlotPoints<'a> {
    let mut pts: Vec<[f64; 2]> = Vec::new();
    let mut last_valid: Option<f64> = None;
    for (&t, val) in ticks.iter().zip(values) {
        if let Some(v) = val {
            let f = f64::from(v);
            pts.push([t as f64, f]);
            last_valid = Some(f);
        }
    }
    if let (Some(lt), Some(&last_t), Some(last_val)) = (last_tick, ticks.last(), last_valid)
        && lt > last_t
    {
        pts.push([lt as f64, last_val]);
    }
    PlotPoints::from(pts)
}

fn build_series_points_per_gen<'a>(
    ticks: &[u64],
    samples: &[GenerationSample],
    gen_num: u32,
    last_tick: Option<u64>,
) -> PlotPoints<'a> {
    let mut pts: Vec<[f64; 2]> = Vec::with_capacity(ticks.len() + 1);
    let mut last_val: f64 = 0.0;
    for (&t, sample) in ticks.iter().zip(samples.iter()) {
        let count = sample
            .counts
            .iter()
            .find_map(|&(g, c)| if g == gen_num { Some(c) } else { None })
            .unwrap_or(0);
        let f = f64::from(count);
        pts.push([t as f64, f]);
        last_val = f;
    }
    if let (Some(lt), Some(&last_t)) = (last_tick, ticks.last())
        && lt > last_t
    {
        pts.push([lt as f64, last_val]);
    }
    PlotPoints::from(pts)
}

fn generation_color(generation_num: u32) -> egui::Color32 {
    let golden_ratio = 0.618_034_f32;
    let hue = ((generation_num as f32) * golden_ratio).fract() * 360.0;
    egui::epaint::Hsva::new(hue / 360.0, 0.75, 0.90, 1.0).into()
}

#[allow(clippy::too_many_lines)]
fn render_history_plot_ui(
    mut contexts: EguiContexts,
    mut plot_open: ResMut<HistoryPlotOpen>,
    history: Option<Res<SimulationHistory>>,
    mut sample_rate: Option<ResMut<SampleRate>>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
    mut active_tab: Local<HistoryPlotTab>,
) -> Result {
    if !plot_open.0 {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;
    let viewport = ctx.content_rect();
    let plot_rect = viewport.shrink(VIEWPORT_MARGIN * 1.5);

    egui::Window::new("Population History")
        .id(egui::Id::new("history_plot_window"))
        .fixed_rect(plot_rect)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
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
            let totals = spatial_grid.character_counts();

            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("Ecosystem History")
                        .color(egui::Color32::from_rgb(210, 235, 255))
                        .strong(),
                );

                ui.separator();

                ui.selectable_value(
                    &mut *active_tab,
                    HistoryPlotTab::Population,
                    "👥 Population",
                );
                ui.selectable_value(
                    &mut *active_tab,
                    HistoryPlotTab::Generations,
                    "🧬 Generations",
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format!("Tick: {}", tick_count.0))
                        .color(egui::Color32::from_rgb(180, 205, 230))
                        .monospace(),
                );
                ui.separator();
                ui.colored_label(
                    egui::Color32::from_rgb(240, 215, 70),
                    format!("Herbivores: {}", totals.herbivores),
                );
                ui.colored_label(
                    egui::Color32::from_rgb(240, 85, 85),
                    format!("Predators: {}", totals.predators),
                );

                if let Some(history_res) = &history
                    && let Some(gen_sample) = history_res.last_generation()
                {
                    ui.separator();
                    let oldest_text = gen_sample
                        .oldest_alive
                        .map_or("—".to_string(), |g| g.to_string());
                    let youngest_text = gen_sample
                        .youngest
                        .map_or("—".to_string(), |g| g.to_string());
                    ui.label(
                        egui::RichText::new(format!(
                            "Gen: oldest {oldest_text} | youngest {youngest_text}"
                        ))
                        .color(egui::Color32::from_rgb(165, 215, 255))
                        .small(),
                    );
                }

                if let Some(ref mut rate) = sample_rate {
                    ui.separator();
                    ui.label("Sample every:");
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
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(egui::RichText::new("✖ Close Plot (Tab)").strong())
                        .clicked()
                    {
                        plot_open.0 = false;
                    }
                });
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(6.0);

            let Some(history) = history else {
                ui.centered_and_justified(|ui| {
                    ui.label("HistoryRecorderPlugin is not active.");
                });
                return;
            };

            let ticks = history.tick_history();
            let last_tick = history.last_tick();

            match *active_tab {
                HistoryPlotTab::Population => {
                    let counts = history.counts_history();
                    let herbivore_points = build_series_points(ticks, counts.herbivores, last_tick);
                    let predator_points = build_series_points(ticks, counts.predators, last_tick);

                    Plot::new("ecosystem_history_plot")
                        .legend(Legend::default().position(Corner::LeftTop))
                        .x_axis_label("Simulation Tick")
                        .y_axis_label("Organism Count")
                        .include_y(0.0)
                        .allow_scroll(false)
                        .show(ui, |plot_ui| {
                            plot_ui.line(
                                Line::new("Herbivores", herbivore_points)
                                    .color(egui::Color32::from_rgb(240, 215, 70))
                                    .width(2.0),
                            );
                            plot_ui.line(
                                Line::new("Predators", predator_points)
                                    .color(egui::Color32::from_rgb(240, 85, 85))
                                    .width(2.0),
                            );
                        });
                }
                HistoryPlotTab::Generations => {
                    let gen_history = history.generation_history();
                    let oldest_points = build_optional_series_points(
                        ticks,
                        gen_history.iter().map(|g| g.oldest_alive),
                        last_tick,
                    );
                    let youngest_points = build_optional_series_points(
                        ticks,
                        gen_history.iter().map(|g| g.youngest),
                        last_tick,
                    );

                    // Collect all generation numbers present across all recorded samples
                    let mut all_gens = BTreeSet::new();
                    for s in gen_history {
                        for &(gen_num, _) in &s.counts {
                            all_gens.insert(gen_num);
                        }
                    }

                    Plot::new("generations_history_plot")
                        .legend(Legend::default().position(Corner::LeftTop))
                        .x_axis_label("Simulation Tick")
                        .y_axis_label("Generation Number / Individual Count")
                        .include_y(0.0)
                        .allow_scroll(false)
                        .show(ui, |plot_ui| {
                            plot_ui.line(
                                Line::new("Youngest Generation", youngest_points)
                                    .color(egui::Color32::from_rgb(100, 215, 255))
                                    .width(2.5),
                            );
                            plot_ui.line(
                                Line::new("Oldest Alive Generation", oldest_points)
                                    .color(egui::Color32::from_rgb(255, 175, 75))
                                    .width(2.5),
                            );

                            for gen_num in all_gens {
                                let pts = build_series_points_per_gen(
                                    ticks,
                                    gen_history,
                                    gen_num,
                                    last_tick,
                                );
                                plot_ui.line(
                                    Line::new(format!("Gen {gen_num} Individuals"), pts)
                                        .color(generation_color(gen_num))
                                        .width(1.4),
                                );
                            }
                        });
                }
            }
        });

    Ok(())
}
