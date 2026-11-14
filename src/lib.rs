pub mod grid;
pub mod presentation;
pub mod simulation;

macro_rules! name {
    ($s:expr) => {
        cfg_select! {
            feature = "dev" => ::bevy::ecs::name::Name::new($s),
            _ => (),
        }
    };
}

pub(crate) use name;
