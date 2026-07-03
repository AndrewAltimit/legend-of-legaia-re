//! Per-actor animation runtime - wraps the actor-tick anim dispatch.
//!
//! PORT: FUN_80024CFC, FUN_8004AD80, FUN_80047430, FUN_80048A08
//! PORT: FUN_80049348, FUN_8004998C, FUN_8004E13C
//!
//! ## Background
//!
//! `FUN_80024CFC` in `SCUS_942.54` is the only static-binary entry point
//! that touches an animation record. It stows the per-record byte pointer
//! in `actor[+0x4C]`, sets `actor[+0x56] = 0xB` and `actor[+0x68] = 100`,
//! and returns. The thing that actually consumes those fields is the
//! per-actor tick at `FUN_80021DF4` (also in `SCUS_942.54`, 4732 bytes,
//! 1183 instructions). The tick reads `actor[+0x5A]` as the dispatch
//! byte and ladders through opcodes `0x01..=0x07` (see
//! [`DispatchByte`]).
//!
//! The dispatch byte selects a layered set of side-effects:
//!
//! - The **keyframe pose decoder** for opcode `0x06` is ported in
//!   [`legaia_anm::AnimPlayer`] - that's the per-bone interpolation that
//!   writes the renderer-consumed pose buffer at `actor[+0x4C]`.
//! - The **per-actor physics tick** that wraps the keyframe decoder is
//!   ported in [`crate::actor_tick`]. It models the position / velocity /
//!   acceleration math for every dispatch byte (`0x01..=0x07`), the
//!   positional SFX emitter (`0x05`), and the per-arm render submissions
//!   (line draws for `0x04`, scene-graph triangle for `0x07`). Audio
//!   cues and render submissions surface via
//!   [`actor_tick::TickEvent`](crate::actor_tick::TickEvent).
//!
//! For the bulk of retail ANM data (records the runtime calls "opcode 6")
//! the per-record body is a per-bone keyframe table, and the interpolation
//! math is statically reachable in `FUN_80021DF4`. That algorithm is
//! already ported in [`legaia_anm::AnimPlayer`].
//!
//! For everything else - records with header `a` field other than `0x06`
//! or `0x0A` - the per-record body shape is opaque. The scaffold below
//! lets engines wire the runtime they need *now*: the dispatcher's `Host`
//! trait exposes a single hook (`on_opaque_record`) for record-level
//! side-effects (sprite swaps, voice cues), and the keyframe-driven case
//! is fully handled by delegating to `AnimPlayer`. Per-actor physics -
//! the part that's the same for every record kind - is in
//! [`crate::actor_tick`].
//!
//! ## What this scaffold provides
//!
//! - A typed `RecordKind` derived from the header `a` field, populated
//!   from real-data observations in `crates/anm`.
//! - `AnimSlot`: per-actor playback state (record bytes, bone count,
//!   factor, finished flag).
//! - `AnimRuntime`: a fixed-size pool of `AnimSlot`s indexed by actor id,
//!   with `play(actor, record, bone_count, kind)` / `tick(actor)` /
//!   `stop(actor)` / `reset(actor)` operations.
//! - `Host`: callbacks the runtime makes when it sees a record kind it
//!   can't handle on its own (the eventual overlay-resident dispatcher).
//! - `AnimEvent`: stream surface so engines can react without polling
//!   per-actor state.
//!
//! When the overlay capture lands, the only change required is to fill
//! the `Host::on_opaque_record` body with the real per-kind dispatch -
//! every other piece of plumbing (per-actor pool, frame stepping,
//! lifecycle hooks, event stream) stays as-is.
//! REF: FUN_80021DF4, FUN_80056798

mod dispatch;
mod keyframe;
mod record;
mod runtime;

pub use dispatch::*;
pub use keyframe::*;
pub use record::*;
pub use runtime::*;

#[cfg(test)]
mod tests;
