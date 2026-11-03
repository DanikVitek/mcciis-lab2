use core::num::NonZeroU64;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};

use crate::simulation::{
    SimulationTickCount,
    history::{SampleRate, SimulationHistory},
    spatial::SpatialGrid,
};

use super::{HistoryPlotOpen, VIEWPORT_MARGIN};

pub struct HistoryPlotUiPlugin;

impl Plugin for HistoryPlotUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, render_history_plot_ui);
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

fn render_history_plot_ui(
    mut contexts: EguiContexts,
    mut plot_open: ResMut<HistoryPlotOpen>,
    history: Option<Res<SimulationHistory>>,
    mut sample_rate: Option<ResMut<SampleRate>>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
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
            let totals = spatial_grid.total_counts();

            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("Population History")
                        .color(egui::Color32::from_rgb(210, 235, 255))
                        .strong(),
                );

                ui.separator();

                ui.label(
                    egui::RichText::new(format!("Tick: {}", tick_count.0))
                        .color(egui::Color32::from_rgb(180, 205, 230))
                        .monospace(),
                );
                ui.separator();
                ui.colored_label(
                    egui::Color32::from_rgb(90, 220, 120),
                    format!("Algae: {}", totals.plants),
                );
                ui.colored_label(
                    egui::Color32::from_rgb(240, 215, 70),
                    format!("Herbivores: {}", totals.herbivores),
                );
                ui.colored_label(
                    egui::Color32::from_rgb(240, 85, 85),
                    format!("Predators: {}", totals.predators),
                );

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
            let counts = history.counts_history();
            let last_tick = history.last_tick();

            // let plant_points = build_series_points(ticks, counts.plants, last_tick);
            let herbivore_points = build_series_points(ticks, counts.herbivores, last_tick);
            let predator_points = build_series_points(ticks, counts.predators, last_tick);

            Plot::new("ecosystem_history_plot")
                .legend(Legend::default().position(Corner::LeftTop))
                .x_axis_label("Simulation Tick")
                .y_axis_label("Organism Count")
                .include_y(0.0)
                .allow_scroll(false)
                .show(ui, |plot_ui| {
                    // plot_ui.line(
                    //     Line::new("Algae (Plants)", plant_points)
                    //         .color(egui::Color32::from_rgb(90, 220, 120))
                    //         .width(2.0),
                    // );
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
        });

    Ok(())
}
