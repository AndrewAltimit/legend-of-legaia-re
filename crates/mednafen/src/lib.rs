//! Mednafen save-state parser + watchpoint-style automation toolkit.
//!
//! Mednafen save states (`.mc{0..9}`) are gzipped streams whose decompressed
//! body is a `MDFNSVST` container with named top-level sections. Each section
//! holds typed sub-entries - for the PSX module, the `MAIN` section carries a
//! `MainRAM.data8` sub-entry containing the full 2 MB of main RAM.
//!
//! This crate provides [`SaveState`], which parses the gzip wrapper + the
//! section/subsection table and exposes typed accessors (`main_ram()`,
//! `cpu_pc()`, `gte_state()`), plus the GPU and SPU section decoders.
//!
//! The emulator-neutral analysis over the main RAM it yields is
//! `legaia-ram-image`:
//!
//! 1. `legaia_ram_image::extract::ram_slice` - slices a PSX virtual-address
//!    window (`0x801C0000..0x801F0000` for overlay capture, etc.) out of
//!    main RAM.
//! 2. `legaia_ram_image::diff` - pairwise byte/word diff between two save
//!    states' main RAM, surfacing addresses whose value changed (the
//!    watchpoint-equivalent output that engine RE work needs).
//! 3. `legaia_ram_image::scenarios` - declarative scenario manifest
//!    (`scripts/scenarios.toml`) mapping each `.mc{0..9}` to a labelled
//!    scenario with watchpoint regions, overlay slices, and downstream
//!    artefact paths.
//! 4. `legaia_ram_image::bisect` - given a known-good and known-bad state,
//!    suggests the intermediate states that would isolate when a write
//!    occurred.
//!
//! Save states never carry copyright Sony bytes by themselves - they're a
//! capture of the user's runtime memory. The crate ships with no fixtures;
//! all integration tests gate on `LEGAIA_MEDNAFEN_DIR` (the user's mednafen
//! `mcs/` directory) and pass-skip when unset, matching the project-wide
//! `LEGAIA_DISC_BIN` convention.

pub mod container;
pub mod gpu;
pub mod psx;
pub mod spu;

pub use container::{SaveState, Section, SubEntry};
pub use gpu::{
    GpuRegs, PsxGpu, VRAM_BYTES, VRAM_HEIGHT, VRAM_WIDTH, bgr555_to_rgba8, vram_to_rgba8,
};
pub use psx::{CpuRegs, PsxMain};
pub use spu::{NUM_VOICES as SPU_NUM_VOICES, PsxSpu, SPU_RAM_BYTES, SpuVoiceState};
