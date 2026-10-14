//! The world exporter behind `legaia-engine export-glb`: per-scene textured
//! `.glb`s and their placement manifest for Unity / VRChat world building.
//!
//! It sits above `legaia-engine-core`: baking a scene's animation and its
//! traversal data steps a scratch `World`, and nothing in the simulation
//! names the exporter back. No wgpu / windowing / audio dependencies.

pub mod glb_export;
