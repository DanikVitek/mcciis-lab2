pub mod config;
pub mod inspector;

use bevy::prelude::*;

use self::{config::SimConfigUiPlugin, inspector::BrainInspectorUiPlugin};

/// System sets for UI update logic (`Update`).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiSystems {
    Config,
    Inspector,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((SimConfigUiPlugin, BrainInspectorUiPlugin));
    }
}
