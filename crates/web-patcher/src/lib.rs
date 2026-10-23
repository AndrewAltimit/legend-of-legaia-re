//! The site's client-side ROM patcher: the randomizer, the texture replacer
//! and the language-pack workbench, as `#[wasm_bindgen]` exports over
//! `legaia-patcher` and its layer crates. The visitor's disc image is edited
//! in WASM memory and downloaded back; nothing is uploaded.
//!
//! An rlib: the wasm module the site loads is `legaia-web-viewer`'s cdylib,
//! which links this crate, so these exports ship in the same bundle as the
//! viewer pages'. It names no engine crate. Module by module, see
//! `docs/subsystems/browser-host.md`.

pub mod rom_patcher;
pub mod texture_pack;
pub mod texture_registry;
pub mod translate_workbench;
