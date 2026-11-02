use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

use crate::{
    grid::{Facing, GridPosition},
    simulation::{
        brain::{Action, Brain, BrainInputs, BrainOutputs, SensorOffsets, sense_brain_inputs},
        organism::{Age, Energy, EntityKind, Generation, Herbivore, MetabolismRate, Predator},
        spatial::SpatialGrid,
    },
};

use super::UiSystems;

/// Root marker component for the individual character & neural-network inspection UI panel.
#[derive(Debug, Clone, Copy, Default, Component)]
pub struct BrainInspectorPanel;

/// Tracks which character entity (`Herbivore` or `Predator`), if any, is currently selected for inspection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Resource)]
pub struct InspectedCharacter(pub Option<Entity>);

/// Read-only view-model snapshot of an inspected character's vitals and live [`Brain`] activations.
#[derive(Debug)]
pub struct CharacterBrainSnapshot<'a> {
    pub entity: Entity,
    pub kind: EntityKind,
    pub position: GridPosition,
    pub facing: Facing,
    pub age: Age,
    pub energy: Energy,
    pub metabolism: MetabolismRate,
    pub generation: Generation,
    pub brain: &'a Brain,
    pub sensors: &'a SensorOffsets,
    pub sensed_inputs: BrainInputs,
    pub raw_outputs: BrainOutputs,
    pub chosen_action: Action,
}

/// Custom [`SystemParam`] encapsulating all queries needed by the brain inspector UI or sensor gizmos.
#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
pub struct CharacterInspector<'w, 's> {
    pub inspected: Res<'w, InspectedCharacter>,
    spatial_grid: Res<'w, SpatialGrid>,
    characters: Query<
        'w,
        's,
        (
            &'static GridPosition,
            &'static Facing,
            &'static Age,
            &'static Energy,
            &'static MetabolismRate,
            &'static Generation,
            &'static Brain,
            &'static SensorOffsets,
            Has<Herbivore>,
        ),
        Or<(With<Herbivore>, With<Predator>)>,
    >,
}

impl<'w, 's> CharacterInspector<'w, 's> {
    pub fn selected_entity(&self) -> Option<Entity> {
        self.inspected.0
    }

    /// Finds the first inspectable character (`Herbivore` or `Predator`) standing on `tile`.
    /// Pair with [`crate::presentation::sprites::world_to_grid_pos`] to implement click-to-inspect.
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
            &position,
            &facing,
            &age,
            &energy,
            &metabolism,
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
            metabolism,
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
            .add_systems(Startup, setup_inspector_ui)
            .add_systems(Update, update_inspector_ui.in_set(UiSystems::Inspector));
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

fn setup_inspector_ui(_commands: Commands) {
    // TODO(ui): Spawn `BrainInspectorPanel` node hierarchy for displaying character vitals
    // and visualizing the 12x4 weight matrix, 4 biases, inputs, and action outputs.
}

fn update_inspector_ui(_inspector: CharacterInspector) {
    // TODO(ui): Handle tile click selection via `world_to_grid_pos` + `inspector.find_character_at(tile)`,
    // and refresh `BrainInspectorPanel` from `inspector.snapshot()`.
}
