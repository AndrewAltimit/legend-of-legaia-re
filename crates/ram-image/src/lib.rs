//! Emulator-neutral analysis of a PSX main-RAM image from a save state.
//!
//! Everything here takes the 2 MiB of main RAM as a byte slice (index `0` is
//! virtual address `0x80000000`) and does not care which emulator captured
//! it, nor (outside [`game_anchors`]) which game filled it:
//!
//! - [`profile`] - the [`GameProfile`] trait: the per-game parameters (anchor
//!   strings, identity globals, manifest defaults) the generic readers take,
//!   and [`profile::GenericGame`] for a title nobody has described.
//! - [`extract`] - locate main RAM in a raw payload by anchor string, and
//!   slice a virtual-address window out of it.
//! - [`game_anchors`] - Legend of Legaia's globals read out of the image
//!   (scene name, game mode, player pointer and position), state
//!   identification, and the [`game_anchors::Legaia`] profile.
//! - [`diff`], [`bisect`], [`source_hunt`] - the watchpoint-equivalent
//!   tooling: pairwise RAM diff, write-transition bisection, and the hunt
//!   for a value's source across states.
//! - [`prim_pool`], [`prim_dispatch`] - decode the GPU primitive pool and
//!   the primitive dispatch table resident in the image.
//! - [`scenarios`] - the capture-scenario manifest (`scripts/scenarios.toml`).
//!
//! The container readers `legaia-mednafen` and `legaia-pcsx-redux` sit on
//! this crate; it depends on neither.

pub mod bisect;
pub mod diff;
pub mod extract;
pub mod game_anchors;
pub mod prim_dispatch;
pub mod prim_pool;
pub mod profile;
pub mod scenarios;
pub mod source_hunt;

pub use diff::{RamDiff, RegionDiff};
pub use extract::{PSX_RAM_KSEG0, PSX_RAM_SIZE, ram_slice};
pub use game_anchors::{Legaia, StateIdentity, game_mode_label, identify};
pub use profile::{GameProfile, GenericGame, ScenarioProfile};
pub use scenarios::{Scenario, ScenarioManifest, WatchpointSpec};
