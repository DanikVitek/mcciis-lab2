This project uses bevy v0.20.1 and `cargo-make`.

To run the simulation use `cargo make run`.
To run the tests use `cargo make test`.

The app has the headless mode used to quickly collect run data without the overhead and controls of the GUI. It performs grid traversal (combinatorial product) over the provided simulation parameter ranges (filtering for allowed constraints, e.g. initial energy being <= max energy).

This project aims to use the best practices for file structure and plugin-based scoping of systems and comonents, so that it would be easier to add new functionality without the need to untangle the dependecy graph. In this sense, sibling modules must not have access to each-other's systems and components and must not be concerned about them—composition is done at the higher (parent) level.
