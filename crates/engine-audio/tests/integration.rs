//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml): one compile and one link instead of
//! one per file. Run a single file's tests with
//! `cargo test -p legaia-engine-audio --test integration <name>::`.

mod bgm_director_chain;
mod credits_bank_spu_layout_disc;
mod real_bgm_chain;
mod real_seq_expressive_events;
mod real_seq_meta_running_status;
mod real_seq_program_change_coverage;
mod real_seq_stream_integrity;
mod real_vab_program_mapping;
mod real_vab_tone_attributes;
mod seq_vab_spu_chain;
mod vab_smoke;
mod w1e_audio_session_ladder;
