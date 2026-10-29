//! The site's expansion workbench: build, validate, preview and export an
//! expansion pack ([`legaia_expansion_format`]) in the browser, against the
//! user's own disc, as `#[wasm_bindgen]` exports.
//!
//! The disc is read once for what the editors pick from - retail monsters
//! with their base stats, item and spell names, scene blocks, music cues, a
//! base scene's geometry pack - and for the disc half of validation
//! ([`legaia_expansion_format::refs::DiscResolver`]). Nothing from it is
//! written into a pack: a picker records a coordinate, and the validator's
//! no-disc-bytes rule runs on every export. Nothing is uploaded.
//!
//! An rlib: the wasm module the site loads is `legaia-web-viewer`'s cdylib,
//! which links this crate. It sits in the engine layer (it reads scene
//! geometry through `legaia-engine-scene` and the level curve through
//! `legaia-engine-expansion`), above every kernel crate. Module by module,
//! see `docs/subsystems/browser-host.md` and
//! `docs/tooling/expansion-workbench.md`.

pub mod catalog;
pub mod session;
pub mod wasm;
