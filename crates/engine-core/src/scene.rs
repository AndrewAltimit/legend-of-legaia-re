//! Scene-loading shell: PROT-resident asset indexing + per-CDNAME-block
//! bundle resolution + BGM lookup that mirrors the runtime field-VM
//! `0x35` opcode chain in [`docs/subsystems/script-vm.md`].
//!
//! Built on top of `legaia-prot` (TOC walker + CDNAME map) and the asset
//! categorizer (`legaia-asset::categorize`). Engines call:
//!
//! 1. [`ProtIndex::open_extracted`] once at startup.
//! 2. [`Scene::load`] when the scene name changes (resolves the CDNAME block
//!    and lazy-classifies every entry).
//! 3. [`Scene::find_bgm`] to resolve a BGM ID inside the active scene to a
//!    PROT entry (the BGM is the SEQ-shaped streaming container).
//!
//! See [`docs/subsystems/asset-loader.md`] for the per-mode layout the
//! retail engine uses.
//!
//! No Sony bytes - this is plumbing over the format crates.
//!
//! This module is split into cohesive submodules (Rust 2018 style - the file
//! stays at `scene.rs` with children under `scene/`); every public item is
//! re-exported here so external paths (`legaia_engine_core::scene::<Item>`)
//! keep resolving unchanged.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use legaia_asset::categorize::{Class, classify};
use legaia_prot::Region;
use legaia_prot::archive::{Archive, Entry};
use legaia_prot::cdname;

mod cutscene;
mod host;
mod prot_index;
mod resolvers;
mod scene_ty;

pub use cutscene::*;
pub use host::*;
pub use prot_index::*;
pub use resolvers::*;
pub use scene_ty::*;

#[cfg(test)]
mod tests;
