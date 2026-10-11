//! The `asset` command-line tool's crate, and the old import paths of the
//! format crates it was divided into.
//!
//! No format is defined here. Every `pub use` below is a transitional alias
//! that keeps `legaia_asset::<module>` resolving for crates that have not yet
//! been rewritten to name the owning crate:
//!
//! - `legaia-container-formats` - asset-type dispatcher, descriptor,
//!   DATA_FIELD stream, pack, the container-level detectors, the sound
//!   containers.
//! - `legaia-scene-formats`, `legaia-minigame-formats`, `legaia-ui-formats` -
//!   the formats by what they describe.
//! - `legaia-actor-formats`, `legaia-game-tables`, `legaia-overlay-images` -
//!   models, static data tables, code-overlay images.
//! - `legaia-disc-audit` - byte accounting, classification, catalogs.
//!
//! New code names the owning crate directly.
