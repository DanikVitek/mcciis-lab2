use std::time::Duration;

use bevy::prelude::*;
use life_sim::{
    grid::GridSize,
    presentation::PresentationPlugin,
    simulation::{DistributionConfig, HistoryRecorderPlugin, SimulationPlugin},
};

fn main() {
    let distribution = DistributionConfig::builder()
        .plants_fraction(0.1)
        .herbivores_fraction(0.1)
        .predators_fraction(0.1)
        .build()
        .unwrap();

    App::new()
        .add_plugins((
            DefaultPlugins.set(ImagePlugin::default_nearest()),
            SimulationPlugin::new(GridSize::new(100, 100), distribution),
            HistoryRecorderPlugin::default(),
            PresentationPlugin::new(Duration::from_secs_f64(0.5)),
        ))
        .run();
}
