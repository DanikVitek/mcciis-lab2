pub mod config;
pub mod inspector;
pub mod plot;

use bevy::prelude::*;
use bevy_egui::{
    EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui, input::egui_wants_any_keyboard_input,
};

use crate::simulation::organism::CharacterFilter;

use self::{
    config::{SimConfigUiPlugin, toggle_playback_pause, trigger_single_step},
    inspector::{BrainInspectorUiPlugin, InspectedCharacter},
    plot::HistoryPlotUiPlugin,
};
use super::pacing::SimulationPlaybackState;

/// Minimum margin separating all floating UI panels from the viewport edges.
pub const VIEWPORT_MARGIN: f32 = 16.0;

/// Controls whether the almost-full-viewport population history plot modal is open.
/// When `true`, the left configuration panel and right brain inspector panels are hidden.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Resource)]
pub struct HistoryPlotOpen(pub bool);

/// System sets for UI update logic (`Update`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum UiSystems {
    Config,
    Inspector,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<EguiPlugin>() {
            app.add_plugins(EguiPlugin::default());
        }

        app.init_resource::<HistoryPlotOpen>()
            .add_plugins((
                SimConfigUiPlugin,
                BrainInspectorUiPlugin,
                HistoryPlotUiPlugin,
            ))
            .add_systems(
                Update,
                handle_ui_hotkeys
                    .in_set(UiSystems::Config)
                    .run_if(not(egui_wants_any_keyboard_input)),
            )
            .add_systems(
                EguiPrimaryContextPass,
                render_pause_indicator.run_if(in_state(SimulationPlaybackState::Paused)),
            );
    }
}

fn handle_ui_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    playback_state: Res<State<SimulationPlaybackState>>,
    mut next_playback_state: ResMut<NextState<SimulationPlaybackState>>,
    mut plot_open: ResMut<HistoryPlotOpen>,
    mut inspected: ResMut<InspectedCharacter>,
    organisms: Query<(), CharacterFilter>,
    mut commands: Commands,
) {
    let has_organisms = !organisms.is_empty();

    if has_organisms && keys.just_pressed(KeyCode::Space) {
        toggle_playback_pause(&playback_state, &mut next_playback_state);
    }

    if has_organisms
        && *playback_state.get() == SimulationPlaybackState::Paused
        && (keys.just_pressed(KeyCode::KeyS) || keys.just_pressed(KeyCode::ArrowRight))
    {
        trigger_single_step(&mut commands);
    }

    if keys.just_pressed(KeyCode::Tab) {
        plot_open.0 = !plot_open.0;
    }

    if keys.just_pressed(KeyCode::Escape) {
        if plot_open.0 {
            plot_open.0 = false;
        } else {
            inspected.0 = None;
        }
    }
}

/// Renders vertical pause bars (`||`) and a `"PAUSED"` label in the center of the viewport
/// at [`egui::Order::Background`] so it stays underneath all UI panels and never blocks controls.
fn render_pause_indicator(mut contexts: EguiContexts) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Area::new(egui::Id::new("center_pause_indicator"))
        .order(egui::Order::Background)
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .interactable(false)
        .show(ctx, |ui| {
            let desired_size = egui::vec2(140.0, 120.0);
            let (rect, _) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
            let painter = ui.painter();

            // Subtle translucent backdrop pill behind the pause bars
            painter.rect(
                rect,
                egui::CornerRadius::same(18),
                egui::Color32::from_rgba_unmultiplied(6, 16, 28, 135),
                egui::Stroke::new(
                    1.5,
                    egui::Color32::from_rgba_unmultiplied(120, 185, 240, 110),
                ),
                egui::epaint::StrokeKind::Inside,
            );

            let center = rect.center() - egui::vec2(0.0, 12.0);
            let bar_width = 16.0;
            let bar_height = 52.0;
            let bar_gap = 14.0;
            let bar_color = egui::Color32::from_rgba_unmultiplied(210, 235, 255, 175);

            let left_bar = egui::Rect::from_center_size(
                center - egui::vec2((bar_width + bar_gap) * 0.5, 0.0),
                egui::vec2(bar_width, bar_height),
            );
            let right_bar = egui::Rect::from_center_size(
                center + egui::vec2((bar_width + bar_gap) * 0.5, 0.0),
                egui::vec2(bar_width, bar_height),
            );

            painter.rect_filled(left_bar, egui::CornerRadius::same(4), bar_color);
            painter.rect_filled(right_bar, egui::CornerRadius::same(4), bar_color);

            painter.text(
                egui::pos2(rect.center().x, rect.bottom() - 18.0),
                egui::Align2::CENTER_CENTER,
                "PAUSED",
                egui::FontId::proportional(15.0),
                bar_color,
            );
        });

    Ok(())
}
