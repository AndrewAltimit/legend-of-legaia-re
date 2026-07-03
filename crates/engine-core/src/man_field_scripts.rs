//! Opcode-aware walk of a scene MAN's field-VM scripts.
//!
//! [`walk_partition1_scripts`] surveys partition 1 (the encounter hunt);
//! [`walk_partition_gflag_sites`] is the partition-agnostic companion that
//! collects global-flag writes (used for the opening prologue's partition-2
//! `GFLAG_SET 26` hand-off arm), both via the same [`LinearWalker`] decode.
//!
//! The record **header** is partition-specific. Partitions 0/1 use the
//! `[u8 N][N*2 locals][4-byte header]` prefix below. Partition 2 (the
//! cutscene-timeline records) instead opens with a Shift-JIS name and three
//! condition-list gates - see `partition2_record_script_offset` and
//! [`partition_record_span`], decoded from the dispatcher `FUN_8003BDE0`.
//!
//! Partition 1 of a scene MAN (the "actor-placement / scripts" partition)
//! holds one field-VM script per record:
//!
//! - record `0` is the scene-entry **system script** - the one
//!   [`crate::scene::Scene::field_man_entry_script`] resolves and
//!   `enter_field_scene` loads via `load_field_script_at`;
//! - records `1..` are per-actor **interaction scripts**, dispatched when
//!   the player interacts with the placed actor.
//!
//! Each record opens with the same `[u8 N][N*2 locals][4-byte header]`
//! prefix as the entry script, so the first opcode sits `1 + N*2 + 4`
//! bytes in (see [`legaia_asset::man_section::ManFile::scene_entry_script`]).
//!
//! This module pairs the MAN partition walk with the field-VM disassembler
//! ([`legaia_engine_vm::field_disasm`]) so callers get a faithful,
//! opcode-aware instruction stream per record instead of a byte scan. The
//! distinction matters for the scripted-encounter hunt: a naive search for
//! a "yield" byte (`0x37` / `0x41`) hits every yield opcode **and** every
//! operand / SJIS byte that happens to equal `0x37` / `0x41`. Walking the
//! opcode stream means an [`ArmSite`] is reported only at a real `Yield`
//! instruction boundary, and the inline record bytes are decoded with
//! [`EncounterRecord::parse`] - the same `+0x3` count / `+0x4` ids layout
//! the retail reader at `0x801DA620` consumes.
//!
//! ## What this can and cannot conclude
//!
//! Per [`crate::field::step`]'s own commentary there is **no dedicated
//! encounter opcode**: the arm ops (`0x37`/`0x41`, `0x38`, `0x43`, `0x47`,
//! `0x4C`) all share the yield-and-forward shape, and the *discriminator*
//! is the consuming entity-SM state, not the opcode. So a single
//! [`ArmSite`] whose inline window decodes as a valid `[count][ids]` record
//! is a *candidate*, not a proof. The value here is empirical: it surfaces
//! whether any P1 script carries an inline `[count=1][id=0x4F]` Tetsu
//! literal at a real yield boundary - which adjudicates the inline-literal
//! hypothesis against the indexed-formation-table hypothesis
//! (see [`crate::encounter_record::RIM_ELM_TRAINING_FORMATION_ID`]).

use legaia_asset::man_section::{ActorPlacement, ManFile};
use legaia_engine_vm::field_disasm::{
    EffectKind, FlagKind, InsnInfo, LinearWalker, MenuCtrlKind, YieldKind, scene_change_name,
};

use crate::encounter_record::EncounterRecord;
use crate::world::FieldCarrierConfig;

/// Inclusive `op0` range a genuine field-VM warp (`scene_transition`) uses.
///
/// The WARP opcode is `0x3E` with `op0 = map_id + 100`, and only **7** door-warp
/// destinations exist - `map_id 0..=6` (each selects a scene-*type* code overlay
/// at PROT `0x4d + map_id`; see [`crate::scene::DefaultMapIdResolver`] and
/// `docs/subsystems/asset-loader.md`). So a real warp's `op0` is `100..=106`.
///
/// This range matters for [`classify_placement`]: the per-actor walk is an
/// over-approximating linear disassembly that *desyncs* inside embedded message
/// text, and a desynced read can land on a `0x3E` whose following byte happens
/// to be `>= 100` - a phantom warp. Every observed phantom carries an `op0` far
/// outside this range (175 / 179 / 200, i.e. SJIS or dialog bytes) and rides the
/// `0x80` cross-context prefix, while every genuine corpus warp is the *base*
/// `0x3E` with `op0` in `100..=106`. So the kind decision requires both signals.
const WARP_OP0_RANGE: std::ops::RangeInclusive<u8> = 100..=106;

/// `true` when a decoded `WarpOrInteract` instruction is a *genuine* door-warp
/// (not a text-desync phantom): the base `0x3E` opcode (no `0x80` cross-context
/// prefix) carrying `op0` in [`WARP_OP0_RANGE`]. `op0` is the raw operand byte
/// (`map_id + 100`); `extended` is the disassembler's cross-context-target field
/// (`Some` iff the `0x80` prefix bit was set on the leading opcode byte).
fn is_genuine_warp(op0: u8, extended: Option<u8>) -> bool {
    extended.is_none() && WARP_OP0_RANGE.contains(&op0)
}

/// Compute the tightest upper byte bound for a record body that starts at
/// `start`: the smallest record offset (across all three partitions) or
/// section start that is strictly greater than `start`, clamped to the MAN
/// length. This stops a record's walk from spilling into the next record's
/// or the encounter section's bytes.
fn record_end_bound(man_file: &ManFile, man_len: usize, start: usize) -> usize {
    let mut bound = man_len;
    let data = man_file.data_region_offset;
    for partition in &man_file.partitions {
        for &off in partition {
            let abs = data + off as usize;
            if abs > start && abs < bound {
                bound = abs;
            }
        }
    }
    // The encounter section (and its siblings) live in the same data region;
    // their length-prefix offsets are a hard ceiling for script bytes.
    for section in &man_file.sections {
        if section.offset > start && section.offset < bound {
            bound = section.offset;
        }
    }
    bound.min(man_len)
}

mod carriers;
mod npc_motion;
mod partitions;
mod placements;
mod records;
mod scene_triggers;

pub use carriers::*;
pub use npc_motion::*;
pub use partitions::*;
pub use placements::*;
pub use records::*;
pub use scene_triggers::*;

#[cfg(test)]
mod tests;
