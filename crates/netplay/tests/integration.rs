//! The crate's integration tests as one test binary.
//!
//! Each `tests/<name>.rs` is a module here rather than its own binary
//! (`autotests = false` in Cargo.toml). Run a single file's tests with
//! `cargo test -p legaia-netplay --test integration <name>::`.

mod authority_rooms;
mod codec_roundtrip;
mod harness;
mod lockstep_loopback;
mod relay_lobby;
#[cfg(not(target_arch = "wasm32"))]
mod socket_smoke;
