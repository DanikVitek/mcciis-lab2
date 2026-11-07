use core::num::NonZeroU64;
use std::collections::HashSet;

use bevy::prelude::*;
use bevy_egui::{
    EguiContexts,
    EguiPrimaryContextPass,
    egui,
};
use egui_plot::{
    Line,
    Plot,
    PlotPoints,
};

use super::{
    HistoryPlotOpen,
    VIEWPORT_MARGIN,
};
use crate::presentation::pacing::AppState;
use crate::simulation::SimulationTickCount;
use crate::simulation::history::{
    SampleRate,
    SimulationHistory,
};
use crate::simulation::spatial::SpatialGrid;

const COLOR_PLANTS: egui::Color32 = egui::Color32::from_rgb(90, 220, 120);
const COLOR_HERBIVORES: egui::Color32 = egui::Color32::from_rgb(240, 215, 70);
const COLOR_PREDATORS: egui::Color32 = egui::Color32::from_rgb(240, 85, 85);
const COLOR_YOUNGEST_GEN: egui::Color32 = egui::Color32::from_rgb(100, 215, 255);
const COLOR_OLDEST_GEN: egui::Color32 = egui::Color32::from_rgb(255, 175, 75);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HistoryPlotTab {
    #[default]
    Population,
    Generations,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SeriesKey {
    Plants,
    Herbivores,
    Predators,
    YoungestGen,
    OldestGen,
    Gen(u32),
}

struct GenSeries {
    gen_num: u32,
    label: String,
    color: egui::Color32,
    points: Vec<[f64; 2]>,
    last_sample_idx: usize,
    last_count: u32,
}

#[derive(Default)]
struct PlotDataCache {
    first_tick: Option<u64>,
    processed_samples: usize,
    plants: Vec<[f64; 2]>,
    herbivores: Vec<[f64; 2]>,
    predators: Vec<[f64; 2]>,
    youngest: Vec<[f64; 2]>,
    oldest: Vec<[f64; 2]>,
    gens: Vec<GenSeries>,
    /// Maps `gen_num as usize` -> index into `self.gens` for O(1) incremental updates.
    gen_lookup: Vec<Option<usize>>,
}

impl PlotDataCache {
    fn clear(&mut self) {
        self.first_tick = None;
        self.processed_samples = 0;
        self.plants.clear();
        self.herbivores.clear();
        self.predators.clear();
        self.youngest.clear();
        self.oldest.clear();
        self.gens.clear();
        self.gen_lookup.clear();
    }

    /// Incrementally ingests any new samples from `history` in O(new_samples) time.
    /// Returns `true` if `history` was reset since the last sync.
    fn sync(&mut self, history: &SimulationHistory) -> bool {
        let ticks = history.tick_history();
        if ticks.is_empty() {
            let was_non_empty = self.processed_samples > 0;
            self.clear();
            return was_non_empty;
        }

        let current_first = ticks.first().copied();
        let was_reset = self.first_tick != current_first || ticks.len() < self.processed_samples;
        if was_reset {
            self.clear();
            self.first_tick = current_first;
        }

        if self.processed_samples == ticks.len() {
            return was_reset;
        }

        let counts = history.counts_history();
        let gen_history = history.generation_history();
        let mut added_new_gen = false;

        for idx in self.processed_samples..ticks.len() {
            let t = ticks[idx] as f64;
            self.plants.push([t, f64::from(counts.plants[idx])]);
            self.herbivores.push([t, f64::from(counts.herbivores[idx])]);
            self.predators.push([t, f64::from(counts.predators[idx])]);

            let gen_sample = &gen_history[idx];
            if let Some(y) = gen_sample.youngest {
                self.youngest.push([t, f64::from(y)]);
            }
            if let Some(o) = gen_sample.oldest_alive {
                self.oldest.push([t, f64::from(o)]);
            }

            for &(gen_num, count) in &gen_sample.counts {
                let lookup_idx = gen_num as usize;
                if lookup_idx >= self.gen_lookup.len() {
                    self.gen_lookup.resize(lookup_idx + 1, None);
                }

                if let Some(series_idx) = self.gen_lookup[lookup_idx] {
                    let series = &mut self.gens[series_idx];
                    if series.last_sample_idx + 1 < idx {
                        // Drop to 0 right after the previous active sample, and stay at 0 until `idx - 1`.
                        let drop_t = ticks[series.last_sample_idx + 1] as f64;
                        series.points.push([drop_t, 0.0]);
                        if idx - 1 > series.last_sample_idx + 1 {
                            let rise_t = ticks[idx - 1] as f64;
                            series.points.push([rise_t, 0.0]);
                        }
                    }
                    series.points.push([t, f64::from(count)]);
                    series.last_sample_idx = idx;
                    series.last_count = count;
                } else {
                    let mut points = Vec::with_capacity(16);
                    if idx > 0 {
                        points.push([ticks[idx - 1] as f64, 0.0]);
                    }
                    points.push([t, f64::from(count)]);

                    let series_idx = self.gens.len();
                    self.gens.push(GenSeries {
                        gen_num,
                        label: format!("Gen {gen_num} Individuals"),
                        color: generation_color(gen_num),
                        points,
                        last_sample_idx: idx,
                        last_count: count,
                    });
                    self.gen_lookup[lookup_idx] = Some(series_idx);
                    added_new_gen = true;
                }
            }
        }

        if added_new_gen
            && !self
                .gens
                .windows(2)
                .all(|pair| pair[0].gen_num <= pair[1].gen_num)
        {
            self.gens.sort_by_key(|s| s.gen_num);
            for (series_idx, series) in self.gens.iter().enumerate() {
                self.gen_lookup[series.gen_num as usize] = Some(series_idx);
            }
        }

        self.processed_samples = ticks.len();
        was_reset
    }
}

#[derive(Default)]
struct PlotUiState {
    active_tab: HistoryPlotTab,
    cache: PlotDataCache,
    hidden: HashSet<SeriesKey>,
    hovered: Option<SeriesKey>,
    last_legend_rect: Option<egui::Rect>,
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

fn tail_for_dense_series(pts: &[[f64; 2]], last_tick: Option<u64>) -> Option<[f64; 2]> {
    let &[last_x, last_y] = pts.last()?;
    let lt = last_tick? as f64;
    (lt > last_x).then_some([lt, last_y])
}

fn tail_for_gen_series(
    series: &GenSeries,
    ticks: &[u64],
    last_tick: Option<u64>,
) -> Option<[f64; 2]> {
    let total_samples = ticks.len();
    if total_samples == 0 {
        return None;
    }
    if series.last_sample_idx + 1 < total_samples {
        // Generation dropped to 0 at `last_sample_idx + 1`.
        Some([ticks[series.last_sample_idx + 1] as f64, 0.0])
    } else if let (Some(lt), Some(&last_t)) = (last_tick, ticks.last())
        && lt > last_t
    {
        Some([lt as f64, f64::from(series.last_count)])
    } else {
        None
    }
}

/// Viewport-aware peak-preserving (Min-Max bucket) downsampler.
///
/// Bounds the number of tessellated vertices per line to at most ~2 points per horizontal screen pixel
/// occupied by the series while preserving exact peaks, troughs, and zoom-in fidelity.
fn build_decimated_plot_points(
    base: &[[f64; 2]],
    tail: Option<[f64; 2]>,
    view_x: (f64, f64),
    cull_offscreen: bool,
    plot_width_px: f64,
) -> Option<PlotPoints<'static>> {
    if base.is_empty() && tail.is_none() {
        return None;
    }

    let first_pt = base.first().copied().or(tail)?;
    let last_pt = tail.or_else(|| base.last().copied())?;
    let (vx_min, vx_max) = view_x;

    if cull_offscreen && (last_pt[0] < vx_min || first_pt[0] > vx_max) {
        return None;
    }

    let (vis_start, vis_end) = if cull_offscreen && !base.is_empty() {
        let raw_start = base.partition_point(|p| p[0] < vx_min);
        let raw_end = base.partition_point(|p| p[0] <= vx_max);
        (raw_start.saturating_sub(1), (raw_end + 1).min(base.len()))
    } else {
        (0, base.len())
    };

    let vis = &base[vis_start..vis_end];
    let include_tail = tail.filter(|tp| {
        !cull_offscreen || vis.last().is_none_or(|lp| lp[0] <= vx_max) || tp[0] >= vx_min
    });

    let view_span = (vx_max - vx_min).max(1.0);
    let vis_min_x = vis.first().map_or(first_pt[0], |p| p[0]).max(vx_min);
    let vis_max_x = include_tail
        .map_or_else(|| vis.last().map_or(last_pt[0], |p| p[0]), |tp| tp[0])
        .min(vx_max);
    let px_span = (((vis_max_x - vis_min_x).max(0.0) / view_span) * plot_width_px).ceil() as usize;
    let max_buckets = plot_width_px.ceil().max(16.0) as usize;
    let num_buckets = px_span.clamp(8, max_buckets);

    if vis.len() <= num_buckets * 2 {
        let mut out = Vec::with_capacity(vis.len() + usize::from(include_tail.is_some()));
        out.extend_from_slice(vis);
        if let Some(tp) = include_tail {
            out.push(tp);
        }
        return (!out.is_empty()).then(|| PlotPoints::from(out));
    }

    let mut out = Vec::with_capacity(num_buckets * 2 + 3);
    out.push(vis[0]);

    let interior = &vis[1..vis.len() - 1];
    if !interior.is_empty() {
        let n = interior.len();
        for b in 0..num_buckets {
            let start = b * n / num_buckets;
            let end = ((b + 1) * n / num_buckets).min(n);
            if start >= end {
                continue;
            }
            let slice = &interior[start..end];
            let mut min_i = 0;
            let mut max_i = 0;
            for (i, pt) in slice.iter().enumerate().skip(1) {
                if pt[1] < slice[min_i][1] {
                    min_i = i;
                }
                if pt[1] > slice[max_i][1] {
                    max_i = i;
                }
            }
            if min_i < max_i {
                out.push(slice[min_i]);
                out.push(slice[max_i]);
            } else if max_i < min_i {
                out.push(slice[max_i]);
                out.push(slice[min_i]);
            } else {
                out.push(slice[min_i]);
            }
        }
    }

    out.push(*vis.last().unwrap());
    if let Some(tp) = include_tail {
        out.push(tp);
    }

    Some(PlotPoints::from(out))
}

fn generation_color(generation_num: u32) -> egui::Color32 {
    let golden_ratio = 0.618_034_f32;
    let hue = ((generation_num as f32) * golden_ratio).fract() * 360.0;
    egui::epaint::Hsva::new(hue / 360.0, 0.75, 0.90, 1.0).into()
}

fn render_legend_item(
    ui: &mut egui::Ui,
    key: SeriesKey,
    label: &str,
    color: egui::Color32,
    hidden: &mut HashSet<SeriesKey>,
    next_hovered: &mut Option<SeriesKey>,
) {
    let is_hidden = hidden.contains(&key);
    let row_height = 18.0;

    let response = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let (swatch_rect, _) =
                ui.allocate_exact_size(egui::vec2(14.0, row_height), egui::Sense::hover());
            let y = swatch_rect.center().y;
            let stroke_color = if is_hidden {
                color.gamma_multiply(0.35)
            } else {
                color
            };
            ui.painter().line_segment(
                [
                    egui::pos2(swatch_rect.left(), y),
                    egui::pos2(swatch_rect.right(), y),
                ],
                egui::Stroke::new(if is_hidden { 1.5 } else { 2.5 }, stroke_color),
            );

            let text_color = if is_hidden {
                egui::Color32::from_rgb(120, 135, 150)
            } else {
                egui::Color32::from_rgb(220, 235, 250)
            };
            let mut rich = egui::RichText::new(label).size(12.0).color(text_color);
            if is_hidden {
                rich = rich.strikethrough();
            }
            ui.add(egui::Label::new(rich).selectable(false))
        })
        .response
        .interact(egui::Sense::click());

    if response.hovered() {
        *next_hovered = Some(key);
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if response.clicked() {
        if is_hidden {
            hidden.remove(&key);
        } else {
            hidden.insert(key);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_scrollable_legend_overlay(
    ctx: &egui::Context,
    plot_rect: egui::Rect,
    active_tab: HistoryPlotTab,
    gens: &[GenSeries],
    hidden: &mut HashSet<SeriesKey>,
    hovered: &mut Option<SeriesKey>,
) -> egui::Rect {
    let max_height = (plot_rect.height() - 24.0).max(80.0);
    let mut next_hovered = None;

    let area_response = egui::Area::new(egui::Id::new("history_plot_scrollable_legend"))
        .order(egui::Order::Foreground)
        .fixed_pos(plot_rect.left_top() + egui::vec2(10.0, 10.0))
        .show(ctx, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(10, 20, 32, 224))
                .stroke(egui::Stroke::new(
                    1.0,
                    egui::Color32::from_rgb(56, 108, 156),
                ))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.set_max_width(210.0);
                    ui.set_max_height(max_height);
                    ui.spacing_mut().item_spacing.y = 3.0;

                    match active_tab {
                        HistoryPlotTab::Population => {
                            render_legend_item(
                                ui,
                                SeriesKey::Plants,
                                "Algae (Plants)",
                                COLOR_PLANTS,
                                hidden,
                                &mut next_hovered,
                            );
                            render_legend_item(
                                ui,
                                SeriesKey::Herbivores,
                                "Herbivores",
                                COLOR_HERBIVORES,
                                hidden,
                                &mut next_hovered,
                            );
                            render_legend_item(
                                ui,
                                SeriesKey::Predators,
                                "Predators",
                                COLOR_PREDATORS,
                                hidden,
                                &mut next_hovered,
                            );

                            if !gens.is_empty() {
                                ui.add_space(2.0);
                                ui.separator();
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Generations ({})",
                                            gens.len()
                                        ))
                                        .size(11.0)
                                        .color(egui::Color32::from_rgb(160, 195, 225))
                                        .strong(),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            if ui.small_button("None").clicked() {
                                                for g in gens {
                                                    hidden.insert(SeriesKey::Gen(g.gen_num));
                                                }
                                            }
                                            if ui.small_button("All").clicked() {
                                                for g in gens {
                                                    hidden.remove(&SeriesKey::Gen(g.gen_num));
                                                }
                                            }
                                        },
                                    );
                                });

                                let remaining_h = (max_height - 108.0).max(48.0);
                                let row_height = 18.0;
                                egui::ScrollArea::vertical()
                                    .id_salt("legend_generations_scroll")
                                    .max_height(remaining_h)
                                    .auto_shrink([false, true])
                                    .show_rows(ui, row_height, gens.len(), |ui, row_range| {
                                        for gen_series in &gens[row_range] {
                                            render_legend_item(
                                                ui,
                                                SeriesKey::Gen(gen_series.gen_num),
                                                &gen_series.label,
                                                gen_series.color,
                                                hidden,
                                                &mut next_hovered,
                                            );
                                        }
                                    });
                            }
                        }
                        HistoryPlotTab::Generations => {
                            render_legend_item(
                                ui,
                                SeriesKey::YoungestGen,
                                "Youngest Generation",
                                COLOR_YOUNGEST_GEN,
                                hidden,
                                &mut next_hovered,
                            );
                            render_legend_item(
                                ui,
                                SeriesKey::OldestGen,
                                "Oldest Alive Generation",
                                COLOR_OLDEST_GEN,
                                hidden,
                                &mut next_hovered,
                            );
                        }
                    }
                });
        });

    if *hovered != next_hovered {
        *hovered = next_hovered;
        ctx.request_repaint();
    }

    area_response.response.rect
}

#[allow(clippy::too_many_lines)]
fn render_history_plot_ui(
    mut contexts: EguiContexts,
    mut plot_open: ResMut<HistoryPlotOpen>,
    history: Option<Res<SimulationHistory>>,
    mut sample_rate: Option<ResMut<SampleRate>>,
    tick_count: Res<SimulationTickCount>,
    spatial_grid: Res<SpatialGrid>,
    mut state: Local<PlotUiState>,
) -> Result {
    if !plot_open.0 {
        state.hovered = None;
        state.last_legend_rect = None;
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;
    let viewport = ctx.content_rect();
    let window_rect = viewport.shrink(VIEWPORT_MARGIN * 1.5);

    egui::Window::new("Population History")
        .id(egui::Id::new("history_plot_window"))
        .fixed_rect(window_rect)
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
                    egui::RichText::new("Ecosystem History")
                        .color(egui::Color32::from_rgb(210, 235, 255))
                        .strong(),
                );

                ui.separator();

                ui.selectable_value(
                    &mut state.active_tab,
                    HistoryPlotTab::Population,
                    "👥 Population",
                );
                ui.selectable_value(
                    &mut state.active_tab,
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
                ui.colored_label(COLOR_PLANTS, format!("Algae: {}", totals.plants));
                ui.colored_label(
                    COLOR_HERBIVORES,
                    format!("Herbivores: {}", totals.herbivores),
                );
                ui.colored_label(COLOR_PREDATORS, format!("Predators: {}", totals.predators));

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

            if state.cache.sync(&history) {
                state.hidden.retain(|key| !matches!(key, SeriesKey::Gen(_)));
            }

            let ticks = history.tick_history();
            let last_tick = history.last_tick();
            let global_min_x = ticks.first().copied().unwrap_or(0) as f64;
            let global_max_x = last_tick
                .or_else(|| ticks.last().copied())
                .unwrap_or(1)
                .max(1) as f64;

            let pointer_over_legend = state.last_legend_rect.is_some_and(|rect| {
                ui.ctx()
                    .pointer_interact_pos()
                    .is_some_and(|pos| rect.contains(pos))
            });

            let active_tab = state.active_tab;
            let hovered = state.hovered;

            let plot_canvas_rect = match active_tab {
                HistoryPlotTab::Population => {
                    Plot::new("ecosystem_history_plot")
                        .x_axis_label("Simulation Tick")
                        .y_axis_label("Organism Count")
                        .include_y(0.0)
                        .allow_scroll(false)
                        .allow_drag(!pointer_over_legend)
                        .allow_zoom(!pointer_over_legend)
                        .allow_boxed_zoom(!pointer_over_legend)
                        .allow_double_click_reset(!pointer_over_legend)
                        .show(ui, |plot_ui| {
                            let bounds = plot_ui.plot_bounds();
                            let auto = plot_ui.auto_bounds().any();
                            let cull_offscreen =
                                !auto && bounds.is_valid_x() && bounds.width() > 0.0;
                            let view_x = if cull_offscreen {
                                (bounds.min()[0], bounds.max()[0])
                            } else {
                                (global_min_x, global_max_x)
                            };
                            let plot_width_px =
                                f64::from(plot_ui.transform().frame().width().max(200.0));

                            if !state.hidden.contains(&SeriesKey::Plants)
                                && let Some(pts) = build_decimated_plot_points(
                                    &state.cache.plants,
                                    tail_for_dense_series(&state.cache.plants, last_tick),
                                    view_x,
                                    cull_offscreen,
                                    plot_width_px,
                                )
                            {
                                plot_ui.line(
                                    Line::new("Algae (Plants)", pts)
                                        .color(COLOR_PLANTS)
                                        .width(2.0)
                                        .highlight(hovered == Some(SeriesKey::Plants)),
                                );
                            }

                            if !state.hidden.contains(&SeriesKey::Herbivores)
                                && let Some(pts) = build_decimated_plot_points(
                                    &state.cache.herbivores,
                                    tail_for_dense_series(&state.cache.herbivores, last_tick),
                                    view_x,
                                    cull_offscreen,
                                    plot_width_px,
                                )
                            {
                                plot_ui.line(
                                    Line::new("Herbivores", pts)
                                        .color(COLOR_HERBIVORES)
                                        .width(2.0)
                                        .highlight(hovered == Some(SeriesKey::Herbivores)),
                                );
                            }

                            if !state.hidden.contains(&SeriesKey::Predators)
                                && let Some(pts) = build_decimated_plot_points(
                                    &state.cache.predators,
                                    tail_for_dense_series(&state.cache.predators, last_tick),
                                    view_x,
                                    cull_offscreen,
                                    plot_width_px,
                                )
                            {
                                plot_ui.line(
                                    Line::new("Predators", pts)
                                        .color(COLOR_PREDATORS)
                                        .width(2.0)
                                        .highlight(hovered == Some(SeriesKey::Predators)),
                                );
                            }

                            for gen_series in &state.cache.gens {
                                let key = SeriesKey::Gen(gen_series.gen_num);
                                if state.hidden.contains(&key) {
                                    continue;
                                }
                                let tail = tail_for_gen_series(gen_series, ticks, last_tick);
                                if let Some(pts) = build_decimated_plot_points(
                                    &gen_series.points,
                                    tail,
                                    view_x,
                                    cull_offscreen,
                                    plot_width_px,
                                ) {
                                    plot_ui.line(
                                        Line::new(&gen_series.label, pts)
                                            .color(gen_series.color)
                                            .width(1.4)
                                            .highlight(hovered == Some(key)),
                                    );
                                }
                            }
                        })
                        .response
                        .rect
                }
                HistoryPlotTab::Generations => {
                    Plot::new("generations_history_plot")
                        .x_axis_label("Simulation Tick")
                        .y_axis_label("Generation Number")
                        .include_y(0.0)
                        .allow_scroll(false)
                        .allow_drag(!pointer_over_legend)
                        .allow_zoom(!pointer_over_legend)
                        .allow_boxed_zoom(!pointer_over_legend)
                        .allow_double_click_reset(!pointer_over_legend)
                        .show(ui, |plot_ui| {
                            let bounds = plot_ui.plot_bounds();
                            let auto = plot_ui.auto_bounds().any();
                            let cull_offscreen =
                                !auto && bounds.is_valid_x() && bounds.width() > 0.0;
                            let view_x = if cull_offscreen {
                                (bounds.min()[0], bounds.max()[0])
                            } else {
                                (global_min_x, global_max_x)
                            };
                            let plot_width_px =
                                f64::from(plot_ui.transform().frame().width().max(200.0));

                            if !state.hidden.contains(&SeriesKey::YoungestGen)
                                && let Some(pts) = build_decimated_plot_points(
                                    &state.cache.youngest,
                                    tail_for_dense_series(&state.cache.youngest, last_tick),
                                    view_x,
                                    cull_offscreen,
                                    plot_width_px,
                                )
                            {
                                plot_ui.line(
                                    Line::new("Youngest Generation", pts)
                                        .color(COLOR_YOUNGEST_GEN)
                                        .width(2.5)
                                        .highlight(hovered == Some(SeriesKey::YoungestGen)),
                                );
                            }

                            if !state.hidden.contains(&SeriesKey::OldestGen)
                                && let Some(pts) = build_decimated_plot_points(
                                    &state.cache.oldest,
                                    tail_for_dense_series(&state.cache.oldest, last_tick),
                                    view_x,
                                    cull_offscreen,
                                    plot_width_px,
                                )
                            {
                                plot_ui.line(
                                    Line::new("Oldest Alive Generation", pts)
                                        .color(COLOR_OLDEST_GEN)
                                        .width(2.5)
                                        .highlight(hovered == Some(SeriesKey::OldestGen)),
                                );
                            }
                        })
                        .response
                        .rect
                }
            };

            let PlotUiState {
                active_tab,
                ref cache,
                ref mut hidden,
                ref mut hovered,
                ref mut last_legend_rect,
            } = *state;

            *last_legend_rect = Some(render_scrollable_legend_overlay(
                ui.ctx(),
                plot_canvas_rect,
                active_tab,
                &cache.gens,
                hidden,
                hovered,
            ));
        });

    Ok(())
}
