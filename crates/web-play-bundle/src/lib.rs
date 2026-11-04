//! The wasm module the play, minigames and modes pages load: the play host's
//! exports without the inspectors, the ROM patcher or the expansion workbench.
//!
//! Nothing here but the link. rustc links only the crates a build references and
//! this crate references no item of either, so it names them: without these lines
//! their `#[wasm_bindgen]` exports would drop out of the cdylib.

extern crate legaia_web_minigames;
extern crate legaia_web_play;

#[cfg(feature = "bench-alloc")]
mod bench_alloc;
