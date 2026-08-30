//! Per-vertex **packet colour** streams for the browser renderer's
//! `a_flat_rgba` vertex attribute.
//!
//! The kernel lives in [`legaia_engine_core::packet_color`] (shared with the
//! native `.glb` exporters - see `docs/tooling/host-drift.md` for why the two
//! hosts must read one implementation); this module is the crate-local name
//! the viewer modules import it under.

pub(crate) use legaia_engine_core::packet_color::{hybrid, textured};
