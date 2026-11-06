use core::fmt;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::input::{
    EguiWantsInput,
    egui_wants_any_pointer_input,
};
use bevy_egui::{
    EguiContexts,
    EguiPrimaryContextPass,
    egui,
};
use rand::seq::IteratorRandom;

use super::{
    HistoryPlotOpen,
    UiSystems,
    VIEWPORT_MARGIN,
};
use crate::grid::{
    Facing,
    GridPosition,
    GridSize,
};
use crate::presentation::pacing::AppState;
use crate::presentation::sprites::{
    POV_ZONE_OUTLINE_COLORS,
    world_to_grid_pos,
};
use crate::simulation::ReproductionThreshold;
use crate::simulation::brain::{
    Action,
    Brain,
    BrainInputs,
    BrainOutputs,
    INPUTS,
    OUTPUTS,
    SensorOffsets,
    sense_brain_inputs,
};
use crate::simulation::organism::{
    Age,
    CharacterFilter,
    Energy,
    EntityKind,
    Generation,
    Herbivore,
    MaxEnergy,
    Predator,
    ValueAsFood,
};
use crate::simulation::spatial::SpatialGrid;

const MAX_CLICK_DRAG_DISTANCE_PX: f32 = 5.0;
const INSPECTOR_PANEL_WIDTH: f32 = 350.0;

/// Tracks which character entity (`Herbivore` or `Predator`), if any, is currently selected for inspection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Resource)]
pub struct InspectedCharacter(pub Option<Entity>);

/// Selects a random living character (`Herbivore` or `Predator`), preferring an entity
/// different from `inspected.0` when more than one character is alive.
pub fn follow_random_character(
    characters: &Query<Entity, CharacterFilter>,
    inspected: &mut InspectedCharacter,
) {
    let mut rng = rand::rng();
    let current = inspected.0;
    inspected.0 = characters
        .iter()
        .filter(|&entity| Some(entity) != current)
        .choose(&mut rng)
        .or_else(|| characters.iter().choose(&mut rng));
}

/// Read-only view-model snapshot of an inspected character's vitals and live [`Brain`] activations.
#[derive(Debug)]
pub struct CharacterBrainSnapshot<'a> {
    pub entity: Entity,
    pub kind: EntityKind,
    pub position: GridPosition,
    pub facing: Facing,
    pub age: Age,
    pub energy: Energy,
    pub value_as_food: Option<ValueAsFood>,
    pub generation: Generation,
    pub brain: &'a Brain,
    pub sensors: &'a SensorOffsets,
    pub sensed_inputs: BrainInputs,
    pub raw_outputs: BrainOutputs,
    pub chosen_action: Action,
}

/// Custom [`SystemParam`] encapsulating all queries needed by the brain inspector UI.
#[derive(SystemParam)]
#[expect(clippy::type_complexity)]
pub struct CharacterInspector<'w, 's> {
    pub inspected: Res<'w, InspectedCharacter>,
    pub spatial_grid: Res<'w, SpatialGrid>,
    characters: Query<
        'w,
        's,
        (
            Entity,
            &'static GridPosition,
            &'static Facing,
            &'static Age,
            &'static Energy,
            Option<&'static ValueAsFood>,
            &'static Generation,
            &'static Brain,
            &'static SensorOffsets,
            Has<Herbivore>,
        ),
        CharacterFilter,
    >,
}

impl<'w, 's> CharacterInspector<'w, 's> {
    pub fn selected_entity(&self) -> Option<Entity> {
        self.inspected.0
    }

    /// Picks a random living character (`Herbivore` or `Predator`), preferring one different
    /// from the currently inspected character when more than one character exists.
    pub fn pick_random_character(&self) -> Option<Entity> {
        let mut rng = rand::rng();
        let current = self.inspected.0;
        self.characters
            .iter()
            .map(|(entity, ..)| entity)
            .filter(|&entity| Some(entity) != current)
            .choose(&mut rng)
            .or_else(|| {
                self.characters
                    .iter()
                    .map(|(entity, ..)| entity)
                    .choose(&mut rng)
            })
    }

    /// Finds the first inspectable character (`Herbivore` or `Predator`) standing on `tile`.
    pub fn find_character_at(&self, tile: GridPosition) -> Option<Entity> {
        self.spatial_grid
            .entities_at(tile)
            .iter()
            .find_map(|&(entity, kind)| match kind {
                EntityKind::Herbivore | EntityKind::Predator => Some(entity),
                EntityKind::Plant => None,
            })
    }

    /// Computes a full [`CharacterBrainSnapshot`] for the currently selected character, if any.
    pub fn snapshot(&self) -> Option<CharacterBrainSnapshot<'_>> {
        let entity = self.inspected.0?;
        let (
            _,
            &position,
            &facing,
            &age,
            &energy,
            value_as_food,
            &generation,
            brain,
            sensors,
            is_herbivore,
        ) = self.characters.get(entity).ok()?;

        let kind = if is_herbivore {
            EntityKind::Herbivore
        } else {
            EntityKind::Predator
        };
        let sensed_inputs = sense_brain_inputs(&self.spatial_grid, position, facing, kind, sensors);
        let raw_outputs = brain.compute(sensed_inputs);
        let chosen_action = Action::from_outputs(raw_outputs);

        Some(CharacterBrainSnapshot {
            entity,
            kind,
            position,
            facing,
            age,
            energy,
            value_as_food: value_as_food.copied(),
            generation,
            brain,
            sensors,
            sensed_inputs,
            raw_outputs,
            chosen_action,
        })
    }
}

pub struct BrainInspectorUiPlugin;

impl Plugin for BrainInspectorUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InspectedCharacter>()
            .add_observer(clear_inspected_on_character_removed)
            .add_systems(
                Update,
                select_character_on_click
                    .in_set(UiSystems::Inspector)
                    .run_if(
                        resource_exists::<EguiWantsInput>
                            .and_then(not(egui_wants_any_pointer_input)),
                    ),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (render_pov_panel, render_neural_network_panel)
                    .run_if(in_state(AppState::Simulation))
                    .run_if(resource_equals(HistoryPlotOpen(false))),
            );
    }
}

/// Automatically clears [`InspectedCharacter`] when the selected character dies or the simulation resets.
fn clear_inspected_on_character_removed(
    remove: On<Remove<(Herbivore, Predator)>>,
    mut inspected: ResMut<InspectedCharacter>,
) {
    if inspected.0 == Some(remove.entity) {
        inspected.0 = None;
    }
}

#[allow(clippy::too_many_arguments)]
fn select_character_on_click(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera_query: Single<(&Camera, &GlobalTransform), With<Camera2d>>,
    field_size: Res<GridSize>,
    inspector: CharacterInspector,
    plot_open: Option<Res<HistoryPlotOpen>>,
    mut commands: Commands,
    mut press_origin: Local<Option<Vec2>>,
) {
    if plot_open.is_some_and(|p| p.0) {
        *press_origin = None;
        return;
    }

    if mouse_buttons.just_pressed(MouseButton::Left) {
        *press_origin = window.cursor_position();
    }

    if mouse_buttons.just_released(MouseButton::Left) {
        let Some(start_pos) = press_origin.take() else {
            return;
        };
        let Some(release_pos) = window.cursor_position() else {
            return;
        };

        // Ignore camera pan drags
        if start_pos.distance(release_pos) > MAX_CLICK_DRAG_DISTANCE_PX {
            return;
        }

        let (camera, camera_global) = camera_query.into_inner();
        let Ok(world_pos) = camera.viewport_to_world_2d(camera_global, release_pos) else {
            return;
        };

        let next_selection = world_to_grid_pos(world_pos, *field_size)
            .and_then(|tile| inspector.find_character_at(tile));

        if inspector.selected_entity() != next_selection {
            commands.insert_resource(InspectedCharacter(next_selection));
        }
    }
}

fn inspector_window_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .fill(egui::Color32::from_rgba_unmultiplied(12, 24, 38, 232))
        .stroke(egui::Stroke::new(
            1.0,
            egui::Color32::from_rgb(56, 108, 156),
        ))
        .corner_radius(8.0)
        .inner_margin(12.0)
}

fn render_pov_panel(
    mut contexts: EguiContexts,
    inspector: CharacterInspector,
    max_energy: Res<MaxEnergy>,
    repro_threshold: Option<Res<ReproductionThreshold>>,
    mut commands: Commands,
) -> Result {
    let Some(snapshot) = inspector.snapshot() else {
        return Ok(());
    };

    let ctx = contexts.ctx_mut()?;
    let style = ctx.global_style();
    let mut should_unfollow = false;
    let mut should_follow_random = false;

    egui::Window::new("Organism POV & Sensors")
        .id(egui::Id::new("inspector_pov_upper_panel"))
        .anchor(
            egui::Align2::RIGHT_TOP,
            egui::vec2(-VIEWPORT_MARGIN, VIEWPORT_MARGIN),
        )
        .default_width(INSPECTOR_PANEL_WIDTH)
        .resizable(false)
        .collapsible(true)
        .frame(inspector_window_frame(&style))
        .show(ctx, |ui| {
            ui.set_width(INSPECTOR_PANEL_WIDTH);
            render_pov_panel_contents(
                ui,
                &snapshot,
                *max_energy,
                repro_threshold.as_deref().copied(),
                &mut should_unfollow,
                &mut should_follow_random,
            );
        });

    if should_unfollow {
        commands.insert_resource(InspectedCharacter(None));
    } else if should_follow_random {
        commands.insert_resource(InspectedCharacter(inspector.pick_random_character()));
    }

    Ok(())
}

fn render_neural_network_panel(
    mut contexts: EguiContexts,
    inspector: CharacterInspector,
) -> Result {
    let Some(snapshot) = inspector.snapshot() else {
        return Ok(());
    };

    let ctx = contexts.ctx_mut()?;
    let style = ctx.global_style();

    egui::Window::new("Neural Network Activity")
        .id(egui::Id::new("inspector_nn_lower_panel"))
        .anchor(
            egui::Align2::RIGHT_BOTTOM,
            egui::vec2(-VIEWPORT_MARGIN, -VIEWPORT_MARGIN),
        )
        .default_width(INSPECTOR_PANEL_WIDTH)
        .resizable(false)
        .collapsible(true)
        .frame(inspector_window_frame(&style))
        .show(ctx, |ui| {
            ui.set_width(INSPECTOR_PANEL_WIDTH);
            render_neural_network_panel_contents(ui, &snapshot);
        });

    Ok(())
}

#[derive(Clone, Copy)]
struct SensorZoneReadout {
    name: &'static str,
    outline_color: egui::Color32,
    /// `[plants_act, herbivores_act, predators_act]` (`amount / area`)
    activations: [f32; 3],
}

fn zone_outline_color32(zone_idx: usize) -> egui::Color32 {
    let [r, g, b] = POV_ZONE_OUTLINE_COLORS[zone_idx];
    egui::Color32::from_rgb(r, g, b)
}

fn render_pov_panel_contents(
    ui: &mut egui::Ui,
    snapshot: &CharacterBrainSnapshot<'_>,
    max_energy: MaxEnergy,
    repro_threshold: Option<ReproductionThreshold>,
    should_unfollow: &mut bool,
    should_follow_random: &mut bool,
) {
    let (species_name, species_color) = match snapshot.kind {
        EntityKind::Herbivore => ("Herbivore", egui::Color32::from_rgb(240, 215, 70)),
        EntityKind::Predator => ("Predator", egui::Color32::from_rgb(240, 85, 85)),
        EntityKind::Plant => ("Algae", egui::Color32::from_rgb(90, 220, 120)),
    };

    ui.horizontal(|ui| {
        ui.colored_label(
            species_color,
            egui::RichText::new(format!("● {species_name} ({})", snapshot.entity)).strong(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("✖ Unfollow (Esc)").clicked() {
                *should_unfollow = true;
            }
            if ui.small_button("🎲 Random").clicked() {
                *should_follow_random = true;
            }
        });
    });

    ui.add_space(4.0);

    // Compact vitals summary
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(egui::RichText::new(format!("Gen {}", snapshot.generation.number())).monospace());
        ui.label(egui::RichText::new(format!("Age {}", snapshot.age.ticks())).monospace());
        if let Some(food) = snapshot.value_as_food {
            ui.label(egui::RichText::new(format!("Food Val {}", food.0)).monospace());
        }
        ui.label(
            egui::RichText::new(format!(
                "Pos ({}, {}) {:?}",
                snapshot.position.x, snapshot.position.y, snapshot.facing
            ))
            .monospace(),
        );
    });

    let energy_cur = snapshot.energy;
    let energy_frac = f32::from(energy_cur.0) / f32::from(max_energy.0.0);
    ui.add_space(2.0);
    ui.add(
        egui::ProgressBar::new(energy_frac)
            .fill(egui::Color32::from_rgb(55, 165, 225))
            .text(format!(
                "Energy: {} / {} (-1/tick{})",
                energy_cur,
                max_energy,
                fmt::from_fn(|f| {
                    let Some(threshold) = repro_threshold else {
                        return Ok(());
                    };
                    write!(f, ", repro ≥ ")?;
                    match threshold {
                        ReproductionThreshold::Absolute(ref threshold) => {
                            fmt::Display::fmt(threshold, f)
                        }
                        ReproductionThreshold::Relative(fraction) => {
                            fmt::Display::fmt(&(max_energy.0.0 as f32 * fraction), f)
                        }
                    }
                })
            )),
    );

    ui.add_space(6.0);

    let inputs = snapshot.sensed_inputs.as_slice();
    let zones = [
        SensorZoneReadout {
            name: "Front",
            outline_color: zone_outline_color32(0),
            activations: [inputs[0], inputs[1], inputs[2]],
        },
        SensorZoneReadout {
            name: "Left",
            outline_color: zone_outline_color32(1),
            activations: [inputs[3], inputs[4], inputs[5]],
        },
        SensorZoneReadout {
            name: "Right",
            outline_color: zone_outline_color32(2),
            activations: [inputs[6], inputs[7], inputs[8]],
        },
        SensorZoneReadout {
            name: "Near",
            outline_color: zone_outline_color32(3),
            activations: [inputs[9], inputs[10], inputs[11]],
        },
    ];

    // Legend for RGB activation channels & zone values
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(egui::RichText::new("RGB Channels:").small().strong());
        ui.colored_label(
            egui::Color32::from_rgb(255, 95, 95),
            egui::RichText::new("R: Predator").small(),
        );
        ui.colored_label(
            egui::Color32::from_rgb(95, 235, 125),
            egui::RichText::new("G: Algae").small(),
        );
        ui.colored_label(
            egui::Color32::from_rgb(100, 175, 255),
            egui::RichText::new("B: Herbivore").small(),
        );
    });

    ui.add_space(2.0);
    egui::Grid::new("pov_zone_readout_grid")
        .num_columns(2)
        .spacing([16.0, 2.0])
        .show(ui, |ui| {
            for (idx, zone) in zones.iter().enumerate() {
                let [g, b, r] = zone.activations;
                ui.colored_label(
                    zone.outline_color,
                    egui::RichText::new(format!("{}: R{:.2} G{:.2} B{:.2}", zone.name, r, g, b))
                        .small()
                        .monospace(),
                );
                if idx % 2 == 1 {
                    ui.end_row();
                }
            }
        });
}

fn render_neural_network_panel_contents(ui: &mut egui::Ui, snapshot: &CharacterBrainSnapshot<'_>) {
    let canvas_size = egui::vec2(ui.available_width(), 245.0);
    let (rect, _) = ui.allocate_exact_size(canvas_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);

    let zone_names = ["Front", "Left", "Right", "Near"];
    let channel_meta = [
        ("G·Algae", egui::Color32::from_rgb(90, 230, 120)),
        ("B·Herb", egui::Color32::from_rgb(100, 175, 255)),
        ("R·Pred", egui::Color32::from_rgb(255, 95, 95)),
    ];
    let action_names = [
        (Action::TurnLeft, "↶ Turn Left"),
        (Action::TurnRight, "↷ Turn Right"),
        (Action::GoForward, "↑ Go Forward"),
        (Action::Consume, "🍽 Consume"),
    ];

    let left_node_x = rect.left() + 86.0;
    let right_node_x = rect.right() - 105.0;

    let mut input_positions = [egui::Pos2::ZERO; INPUTS];
    for (i, pos) in input_positions.iter_mut().enumerate() {
        let zone_idx = i / 3;
        let ch_idx = i % 3;
        let group_y = rect.top() + 14.0 + zone_idx as f32 * 58.0;
        let y = group_y + ch_idx as f32 * 16.0;
        *pos = egui::pos2(left_node_x, y);
    }

    let mut output_positions = [egui::Pos2::ZERO; OUTPUTS];
    for (j, pos) in output_positions.iter_mut().enumerate() {
        let y = rect.top() + 34.0 + j as f32 * 56.0;
        *pos = egui::pos2(right_node_x, y);
    }

    let weights = snapshot.brain.weights();
    let biases = snapshot.brain.biases();
    let inputs = snapshot.sensed_inputs.as_slice();
    let outputs = snapshot.raw_outputs.as_slice();

    // 1. Draw 48 synaptic connections (12 inputs × 4 outputs)
    for (i, &in_pos) in input_positions.iter().enumerate() {
        let u_i = inputs[i];
        for (j, &out_pos) in output_positions.iter().enumerate() {
            let w = weights[(j, i)];
            let contrib = u_i * w;

            let (color, width) = if u_i > 0.001 {
                let intensity = contrib.abs().clamp(0.05, 1.0);
                let alpha = (70.0 + 185.0 * intensity).round() as u8;
                let col = if contrib >= 0.0 {
                    egui::Color32::from_rgba_unmultiplied(70, 230, 195, alpha)
                } else {
                    egui::Color32::from_rgba_unmultiplied(255, 95, 115, alpha)
                };
                (col, 1.0 + 2.2 * intensity)
            } else {
                let alpha = (18.0 + 30.0 * w.abs().clamp(0.0, 1.0)).round() as u8;
                let col = if w >= 0.0 {
                    egui::Color32::from_rgba_unmultiplied(90, 165, 190, alpha)
                } else {
                    egui::Color32::from_rgba_unmultiplied(190, 95, 110, alpha)
                };
                (col, 0.8)
            };

            painter.line_segment([in_pos, out_pos], egui::Stroke::new(width, color));
        }
    }

    // 2. Draw 12 Input Nodes (grouped by zone)
    for (zone_idx, zone_name) in zone_names.iter().enumerate() {
        let group_top = input_positions[zone_idx * 3].y - 6.0;
        painter.text(
            egui::pos2(rect.left() + 2.0, group_top + 16.0),
            egui::Align2::LEFT_CENTER,
            *zone_name,
            egui::FontId::proportional(11.5),
            zone_outline_color32(zone_idx),
        );

        for (ch_idx, &(ch_label, ch_color)) in channel_meta.iter().enumerate() {
            let i = zone_idx * 3 + ch_idx;
            let pos = input_positions[i];
            let u_i = inputs[i].clamp(0.0, 1.0);

            let fill = egui::Color32::from_rgba_unmultiplied(
                ch_color.r(),
                ch_color.g(),
                ch_color.b(),
                (45.0 + 210.0 * u_i).round() as u8,
            );
            painter.circle_filled(pos, 5.5, fill);
            painter.circle_stroke(pos, 5.5, egui::Stroke::new(1.2, ch_color));

            painter.text(
                pos - egui::vec2(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                format!("{ch_label} {u_i:.2}"),
                egui::FontId::monospace(9.5),
                if u_i > 0.0 {
                    ch_color
                } else {
                    egui::Color32::from_rgb(125, 145, 165)
                },
            );
        }
    }

    // 3. Draw 4 Output Action Nodes
    for (j, &(action, label)) in action_names.iter().enumerate() {
        let pos = output_positions[j];
        let is_chosen = action == snapshot.chosen_action;
        let out_val = outputs[j];
        let bias_val = biases[j];

        let node_radius = if is_chosen { 9.0 } else { 6.5 };
        let fill_color = if is_chosen {
            egui::Color32::from_rgb(55, 195, 245)
        } else {
            egui::Color32::from_rgb(35, 65, 95)
        };
        let stroke = if is_chosen {
            egui::Stroke::new(2.2, egui::Color32::from_rgb(255, 235, 110))
        } else {
            egui::Stroke::new(1.2, egui::Color32::from_rgb(110, 150, 185))
        };

        painter.circle_filled(pos, node_radius, fill_color);
        painter.circle_stroke(pos, node_radius, stroke);

        let text_color = if is_chosen {
            egui::Color32::from_rgb(255, 240, 130)
        } else {
            egui::Color32::from_rgb(190, 210, 230)
        };

        painter.text(
            pos + egui::vec2(14.0, -6.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(if is_chosen { 12.5 } else { 11.5 }),
            text_color,
        );
        painter.text(
            pos + egui::vec2(14.0, 8.0),
            egui::Align2::LEFT_CENTER,
            format!("o={out_val:+.2} (b={bias_val:+.2})"),
            egui::FontId::monospace(9.5),
            egui::Color32::from_rgb(145, 175, 200),
        );
    }
}
